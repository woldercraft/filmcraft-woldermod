//! VA-API as an Export hardware video encoder on Linux (AMD Radeon, Intel and Mesa drivers).
//!
//! Drives the system's VA-API hardware encoder (`/dev/dri/renderD*`, `h264_vaapi` and `hevc_vaapi`)
//! behind [`filmcraft_export::VideoEncoder`]:
//! - **H.264**: takes an export when `ExportSettings::hardware_encoding` is `Auto` and the GPU's
//!   VA-API encoder accepts the stream configuration; otherwise returns `None` so the built-in
//!   software encoder runs.
//! - **H.265 (HEVC Main 8-bit 4:2:0)**: makes `Format::Hevc` available on Linux GPUs with
//!   `VAProfileHEVCMain` (`VAEntrypointEncSlice`).

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Once, OnceLock};
use std::thread::JoinHandle;

use filmcraft_export::{
    BitrateMode, ColorSignal, EncodedPacket, EncoderFrame, ExportError, ExportSettings, FieldOrder, Format, H264Pass, H264Profile, HardwareEncoding, Result,
    VideoEncoder,
};
use filmcraft_isobmff::{AvcConfig, SampleEntry};
use filmcraft_time::FrameRate;

use crate::nvenc::{Codec, annex_b_nals, annex_b_to_length_prefixed_for, hevc, nal_type, split_hevc_parameter_sets, split_parameter_sets};

/// Encoder configuration validated against the export settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub device: String,
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    pub fps: (u32, u32),
    pub bitrate_kbps: u32,
    pub max_bitrate_kbps: u32,
    pub cbr: bool,
    pub keyint: u32,
    pub profile: &'static str,
    pub sar: Option<(u32, u32)>,
}

/// First writable DRM render node when `libva` is present and initialises cleanly.
pub fn render_node() -> Option<String> {
    super::va::probe().as_ref().ok()?;
    for n in 128..144 {
        let path = format!("/dev/dri/renderD{n}");
        if std::fs::OpenOptions::new().read(true).write(true).open(&path).is_ok() {
            return Some(path);
        }
    }
    None
}

static H264_AVAILABLE: OnceLock<bool> = OnceLock::new();
static HEVC_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Whether this system has a working VA-API H.264 hardware encoder.
pub fn available() -> bool {
    *H264_AVAILABLE.get_or_init(|| {
        let Some(device) = render_node() else { return false };
        let cfg = Config {
            device,
            codec: Codec::H264,
            width: 640,
            height: 360,
            fps: (30, 1),
            bitrate_kbps: 2000,
            max_bitrate_kbps: 3000,
            cbr: false,
            keyint: 30,
            profile: "main",
            sar: None,
        };
        probe_sample_entry(&cfg).is_ok()
    })
}

/// Whether this system has a working VA-API HEVC (H.265) hardware encoder.
pub fn hevc_available() -> bool {
    *HEVC_AVAILABLE.get_or_init(|| {
        let Some(device) = render_node() else { return false };
        let cfg = Config {
            device,
            codec: Codec::Hevc,
            width: 640,
            height: 360,
            fps: (30, 1),
            bitrate_kbps: 2000,
            max_bitrate_kbps: 3000,
            cbr: false,
            keyint: 30,
            profile: "main",
            sar: None,
        };
        probe_sample_entry(&cfg).is_ok()
    })
}

/// Warm the VA-API H.264 and HEVC probes on a background thread at startup.
pub fn warm_probes() {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new().name("vaapi-encode-probe".into()).spawn(|| {
            let _ = std::panic::catch_unwind(available);
            let _ = std::panic::catch_unwind(hevc_available);
        });
    });
}

fn config_for(format: Format, w: u32, h: u32, rate: FrameRate, s: &ExportSettings) -> std::result::Result<Config, String> {
    let hevc = format == Format::Hevc;
    if !(format == Format::H264 || hevc) || s.format.is_mxf() {
        return Err("only MP4 / MOV H.264 and H.265 exports".into());
    }
    if s.signal.is_hdr() {
        return Err("HDR is not written by the 8-bit VA-API path".into());
    }
    if hevc && s.signal != ColorSignal::default() {
        return Err(format!("the colour signal {:?} is not BT.709 SDR", s.signal));
    }
    if !matches!(s.h264_pass, H264Pass::Single) || s.bitrate_mode == BitrateMode::Vbr2Pass {
        return Err("two-pass VBR".into());
    }
    if s.bitrate_mode == BitrateMode::Crf {
        return Err("CRF (constant quality)".into());
    }
    if hevc && s.pixel_aspect.is_some_and(|(n, d)| n != d) {
        return Err("non-square pixels".into());
    }
    if s.field_order != FieldOrder::Progressive {
        return Err("interlaced output".into());
    }
    if w < 64 || h < 64 || !w.is_multiple_of(2) || !h.is_multiple_of(2) {
        return Err("VA-API needs even dimensions of at least 64x64".into());
    }
    if w > 4096 || h > 4096 {
        return Err(format!("{w}x{h} exceeds VA-API encoder limits"));
    }
    if rate.num <= 0 || rate.den <= 0 {
        return Err("frame rate".into());
    }
    let (Ok(num), Ok(den)) = (u32::try_from(rate.num), u32::try_from(rate.den)) else {
        return Err("frame rate".into());
    };
    let device = render_node().ok_or_else(|| "no VA-API DRM render node".to_string())?;
    let kbps = s.bitrate_kbps.max(100);
    let max_kbps = s.max_bitrate_kbps.filter(|m| *m >= kbps).unwrap_or_else(|| kbps.saturating_add(kbps / 2));
    let keyint = s
        .keyframe_distance
        .filter(|k| *k > 0)
        .unwrap_or_else(|| (f64::from(num) / f64::from(den) * 2.0).round().max(1.0) as u32);
    let profile = match (hevc, s.h264_profile) {
        (true, _) => "main",
        (false, H264Profile::Baseline) => "constrained_baseline",
        (false, H264Profile::Main) => "main",
        (false, H264Profile::High) => "high",
    };
    Ok(Config {
        device,
        codec: if hevc { Codec::Hevc } else { Codec::H264 },
        width: w,
        height: h,
        fps: (num, den),
        bitrate_kbps: kbps,
        max_bitrate_kbps: max_kbps,
        cbr: s.bitrate_mode == BitrateMode::Cbr,
        keyint,
        profile,
        sar: s.pixel_aspect.filter(|(n, d)| *n > 0 && *d > 0 && n != d),
    })
}

/// Convert straight RGBA8 (`w` × `h`, even dimensions) into BT.709 limited-range NV12.
fn rgba_to_nv12_bt709(rgba: &[u8], w: usize, h: usize, nv12: &mut Vec<u8>) {
    let luma_len = w.saturating_mul(h);
    let total_len = luma_len.saturating_add(luma_len / 2);
    nv12.resize(total_len, 0);
    let (luma, chroma) = nv12.split_at_mut(luma_len);
    let row_bytes = w.saturating_mul(4);
    if row_bytes == 0 || w < 2 || h < 2 {
        return;
    }
    let luma_pair = w.saturating_mul(2);
    let rgba_pair = row_bytes.saturating_mul(2);
    let pairs = h / 2;
    let rgba_end = rgba.len().min(pairs * rgba_pair);
    let luma_end = luma.len().min(pairs * luma_pair);
    let chroma_end = chroma.len().min(pairs * w);
    let rgba_valid = &rgba[..rgba_end];
    let luma_valid = &mut luma[..luma_end];
    let chroma_valid = &mut chroma[..chroma_end];
    for ((src_pair, y_pair), uv_row) in rgba_valid
        .chunks_exact(rgba_pair)
        .zip(luma_valid.chunks_exact_mut(luma_pair))
        .zip(chroma_valid.chunks_exact_mut(w))
    {
        let (src0, src1) = src_pair.split_at(row_bytes);
        let (y0, y1) = y_pair.split_at_mut(w);
        for (((p0, p1), (dy0, dy1)), duv) in src0
            .chunks_exact(8)
            .zip(src1.chunks_exact(8))
            .zip(y0.chunks_exact_mut(2).zip(y1.chunks_exact_mut(2)))
            .zip(uv_row.chunks_exact_mut(2))
        {
            let (r00, g00, b00) = (i32::from(p0[0]), i32::from(p0[1]), i32::from(p0[2]));
            let (r01, g01, b01) = (i32::from(p0[4]), i32::from(p0[5]), i32::from(p0[6]));
            let (r10, g10, b10) = (i32::from(p1[0]), i32::from(p1[1]), i32::from(p1[2]));
            let (r11, g11, b11) = (i32::from(p1[4]), i32::from(p1[5]), i32::from(p1[6]));

            dy0[0] = (16 + ((11966 * r00 + 40254 * g00 + 4063 * b00 + 32768) >> 16)).clamp(16, 235) as u8;
            dy0[1] = (16 + ((11966 * r01 + 40254 * g01 + 4063 * b01 + 32768) >> 16)).clamp(16, 235) as u8;
            dy1[0] = (16 + ((11966 * r10 + 40254 * g10 + 4063 * b10 + 32768) >> 16)).clamp(16, 235) as u8;
            dy1[1] = (16 + ((11966 * r11 + 40254 * g11 + 4063 * b11 + 32768) >> 16)).clamp(16, 235) as u8;

            let sr = r00 + r01 + r10 + r11;
            let sg = g00 + g01 + g10 + g11;
            let sb = b00 + b01 + b10 + b11;
            duv[0] = (128 + ((-6591 * sr - 22170 * sg + 28761 * sb + 131072) >> 18)).clamp(16, 240) as u8;
            duv[1] = (128 + ((28761 * sr - 26124 * sg - 2637 * sb + 131072) >> 18)).clamp(16, 240) as u8;
        }
    }
}

fn codec_args(cfg: &Config, nv12_input: bool) -> Vec<String> {
    let (enc_name, mux_fmt) = match cfg.codec {
        Codec::H264 => ("h264_vaapi", "h264"),
        Codec::Hevc => ("hevc_vaapi", "hevc"),
    };
    let vf = if nv12_input {
        "format=nv12,hwupload=extra_hw_frames=16"
    } else {
        "format=nv12,hwupload"
    };
    let mut args = vec![
        "-vf".into(),
        vf.into(),
        "-c:v".into(),
        enc_name.into(),
        "-async_depth".into(),
        "8".into(),
        "-profile:v".into(),
        cfg.profile.into(),
        "-rc_mode".into(),
        if cfg.cbr { "CBR" } else { "VBR" }.into(),
        "-b:v".into(),
        format!("{}k", cfg.bitrate_kbps),
        "-maxrate".into(),
        format!("{}k", if cfg.cbr { cfg.bitrate_kbps } else { cfg.max_bitrate_kbps }),
        "-bf".into(),
        "0".into(),
        "-g".into(),
        cfg.keyint.to_string(),
        "-aud".into(),
        "1".into(),
        "-color_primaries".into(),
        "bt709".into(),
        "-color_trc".into(),
        "bt709".into(),
        "-colorspace".into(),
        "bt709".into(),
    ];
    if let Some((n, d)) = cfg.sar {
        args.push("-sar".into());
        args.push(format!("{n}:{d}"));
    }
    args.extend(["-f".into(), mux_fmt.into()]);
    args
}

/// Probe 1 frame on the VA-API hardware encoder to validate the configuration and build the MP4 `SampleEntry`.
fn probe_sample_entry(cfg: &Config) -> std::result::Result<SampleEntry, String> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-v",
        "error",
        "-vaapi_device",
        &cfg.device,
        "-f",
        "lavfi",
        "-i",
        &format!("color=black:s={}x{}:r={}/{}:d=0.04", cfg.width, cfg.height, cfg.fps.0, cfg.fps.1),
    ]);
    cmd.args(codec_args(cfg, false));
    cmd.args(["-frames:v", "1", "pipe:1"]);
    let out = cmd.output().map_err(|e| format!("ffmpeg VA-API probe failed to start: {e}"))?;
    if !out.status.success() {
        return Err(format!("VA-API rejected configuration: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let (w, h) = (u16::try_from(cfg.width).unwrap_or(u16::MAX), u16::try_from(cfg.height).unwrap_or(u16::MAX));
    match cfg.codec {
        Codec::H264 => {
            let (sps, pps) = split_parameter_sets(&out.stdout)?;
            Ok(SampleEntry::avc(AvcConfig::new(vec![sps], vec![pps], 4), w, h))
        }
        Codec::Hevc => {
            let (vps, sps, pps) = split_hevc_parameter_sets(&out.stdout)?;
            let hevc_cfg = hevc::hevc_config(&vps, &sps, &pps, (cfg.width, cfg.height), 8)?;
            Ok(SampleEntry::hevc(hevc_cfg, w, h))
        }
    }
}

struct VaapiEncoder {
    cfg: Config,
    rate: FrameRate,
    entry: SampleEntry,
    child: Child,
    write_tx: Option<SyncSender<Vec<u8>>>,
    free_rx: Receiver<Vec<u8>>,
    writer: Option<JoinHandle<std::result::Result<(), String>>>,
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
}

impl VaapiEncoder {
    fn open(cfg: Config, rate: FrameRate) -> std::result::Result<Self, String> {
        let entry = probe_sample_entry(&cfg)?;
        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-v",
            "error",
            "-vaapi_device",
            &cfg.device,
            "-f",
            "rawvideo",
            "-pix_fmt",
            "nv12",
            "-color_range",
            "tv",
            "-colorspace",
            "bt709",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-s",
            &format!("{}x{}", cfg.width, cfg.height),
            "-r",
            &format!("{}/{}", cfg.fps.0, cfg.fps.1),
            "-i",
            "pipe:0",
        ]);
        cmd.args(codec_args(&cfg, true));
        cmd.args(["-flush_packets", "1", "pipe:1"]);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("failed to spawn VA-API encoder: {e}"))?;
        let mut stdin = child.stdin.take().ok_or_else(|| "VA-API encoder stdin missing".to_string())?;
        let mut stdout = child.stdout.take().ok_or_else(|| "VA-API encoder stdout missing".to_string())?;
        let (write_tx, write_rx) = mpsc::sync_channel::<Vec<u8>>(4);
        let (free_tx, free_rx) = mpsc::channel::<Vec<u8>>();
        let (cw, ch) = (cfg.width as usize, cfg.height as usize);
        let writer = std::thread::Builder::new()
            .name("vaapi-encode-writer".into())
            .spawn(move || {
                // RGBA -> NV12 happens here, off the exporting thread, so rendering the next batch
                // and converting this one overlap instead of queueing behind each other.
                let mut nv12: Vec<u8> = Vec::new();
                while let Ok(rgba) = write_rx.recv() {
                    filmcraft_export::timed(filmcraft_export::Stage::Convert, || {
                        rgba_to_nv12_bt709(&rgba, cw, ch, &mut nv12);
                    });
                    let _ = free_tx.send(rgba);
                    if let Err(e) = stdin.write_all(&nv12) {
                        drop(stdin);
                        return Err(format!("VA-API write failed: {e}"));
                    }
                }
                let res = stdin.flush().map_err(|e| format!("VA-API flush failed: {e}"));
                drop(stdin);
                res
            })
            .map_err(|e| format!("failed to spawn VA-API writer thread: {e}"))?;
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("vaapi-encode-reader".into())
            .spawn(move || {
                let mut chunk = vec![0u8; 64 * 1024];
                while let Ok(n) = stdout.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    if tx.send(chunk[..n].to_vec()).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| format!("failed to spawn VA-API reader thread: {e}"))?;
        Ok(Self {
            cfg,
            rate,
            entry,
            child,
            write_tx: Some(write_tx),
            free_rx,
            writer: Some(writer),
            rx,
            buf: Vec::new(),
        })
    }

    fn drain_packets(&mut self, flush_all: bool) -> Result<Vec<EncodedPacket>> {
        let aud_positions = find_aud_positions(&self.buf, self.cfg.codec);
        if aud_positions.is_empty() {
            return Ok(Vec::new());
        }
        let duration = u32::try_from(self.rate.den).unwrap_or(u32::MAX);
        let mut out = Vec::new();
        let count = if flush_all { aud_positions.len() } else { aud_positions.len().saturating_sub(1) };
        if count == 0 {
            return Ok(Vec::new());
        }
        for i in 0..count {
            let start = aud_positions[i];
            let end = aud_positions.get(i + 1).copied().unwrap_or(self.buf.len());
            let Some(au) = self.buf.get(start..end) else { continue };
            let data = annex_b_to_length_prefixed_for(self.cfg.codec, au)
                .map_err(|e| ExportError::Encode(format!("VA-API bitstream: {e}")))?;
            let key = is_idr(self.cfg.codec, au);
            out.push(EncodedPacket { data, key, duration, composition_offset: 0 });
        }
        let drain_end = if flush_all { self.buf.len() } else { aud_positions[count] };
        self.buf.drain(..drain_end);
        Ok(out)
    }
}

impl Drop for VaapiEncoder {
    fn drop(&mut self) {
        let _ = self.write_tx.take();
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Start-code positions of every Access Unit Delimiter (AUD) NAL unit in `data`.
fn find_aud_positions(data: &[u8], codec: Codec) -> Vec<usize> {
    let aud_type = match codec {
        Codec::H264 => 9u8,
        Codec::Hevc => 35u8,
    };
    let mut positions = Vec::new();
    let mut i = 0usize;
    while let Some(w) = data.get(i..i.saturating_add(4)) {
        if w[0] == 0 && w[1] == 0 && w[2] == 1 {
            let sc_start = if i > 0 && data.get(i - 1) == Some(&0) { i - 1 } else { i };
            let hdr = w[3];
            let t = match codec {
                Codec::H264 => hdr & 0x1f,
                Codec::Hevc => (hdr >> 1) & 0x3f,
            };
            if t == aud_type {
                positions.push(sc_start);
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    positions
}

fn is_idr(codec: Codec, au: &[u8]) -> bool {
    let nals = annex_b_nals(au);
    match codec {
        Codec::H264 => nals.iter().any(|n| nal_type(Codec::H264, n) == Some(5)),
        Codec::Hevc => nals.iter().any(|n| matches!(nal_type(Codec::Hevc, n), Some(19..=21))),
    }
}

impl VideoEncoder for VaapiEncoder {
    fn sample_entry(&self) -> SampleEntry {
        self.entry.clone()
    }

    fn timescale(&self) -> u32 {
        u32::try_from(self.rate.num).unwrap_or(u32::MAX)
    }

    fn encode(&mut self, f: &EncoderFrame) -> Result<Vec<EncodedPacket>> {
        if f.width != self.cfg.width || f.height != self.cfg.height {
            return Err(ExportError::Encode(format!(
                "VA-API: a {}x{} picture for a {}x{} encoder",
                f.width, f.height, self.cfg.width, self.cfg.height
            )));
        }
        if f.hdr.is_some() {
            return Err(ExportError::Unsupported("VA-API 8-bit encoder does not take HDR pictures".into()));
        }
        let expected = (self.cfg.width as usize).saturating_mul(self.cfg.height as usize).saturating_mul(4);
        let rgba = f.rgba.get(..expected).ok_or_else(|| ExportError::Encode("VA-API: RGBA picture smaller than frame size".into()))?;
        let mut nv12 = self.free_rx.try_recv().unwrap_or_default();
        nv12.clear();
        nv12.extend_from_slice(rgba);
        let write_tx = self.write_tx.as_ref().ok_or_else(|| ExportError::Encode("VA-API encoder stdin closed".into()))?;
        if let Err(e) = write_tx.send(nv12) {
            let mut err = String::new();
            if let Some(mut se) = self.child.stderr.take() {
                let _ = se.read_to_string(&mut err);
            }
            let detail = err.trim();
            if detail.is_empty() {
                return Err(ExportError::Encode(format!("VA-API write failed: {e}")));
            }
            return Err(ExportError::Encode(format!("VA-API encoder failed: {detail}")));
        }
        filmcraft_export::note_hw_encode_frame();
        while let Ok(chunk) = self.rx.try_recv() {
            self.buf.extend_from_slice(&chunk);
        }
        self.drain_packets(false)
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>> {
        drop(self.write_tx.take());
        while let Ok(chunk) = self.rx.recv() {
            self.buf.extend_from_slice(&chunk);
        }
        if let Some(w) = self.writer.take() {
            match w.join() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(ExportError::Encode(e)),
                Err(_) => return Err(ExportError::Encode("VA-API writer thread panicked".into())),
            }
        }
        let status = self.child.wait().map_err(|e| ExportError::Encode(format!("VA-API wait failed: {e}")))?;
        if !status.success() {
            let mut err = String::new();
            if let Some(mut se) = self.child.stderr.take() {
                let _ = se.read_to_string(&mut err);
            }
            return Err(ExportError::Encode(format!("VA-API encoder failed: {}", err.trim())));
        }
        self.drain_packets(true)
    }
}

/// The Export encoder factory for VA-API on Linux.
pub fn factory(format: Format, w: u32, h: u32, rate: FrameRate, s: &ExportSettings) -> Option<Result<Box<dyn VideoEncoder>>> {
    if format == Format::Hevc {
        return hevc_factory(w, h, rate, s);
    }
    if s.hardware_encoding != HardwareEncoding::Auto || format != Format::H264 {
        return None;
    }
    let declined = |why: &str| {
        log::info!("VA-API hardware encoding declined: {why}");
        filmcraft_export::note_hw_encode_declined();
        None
    };
    let cfg = match config_for(format, w, h, rate, s) {
        Ok(c) => c,
        Err(why) => return declined(&why),
    };
    match VaapiEncoder::open(cfg, rate) {
        Ok(enc) => {
            log::info!("VA-API hardware H.264 encoder active ({w}x{h})");
            filmcraft_export::note_hw_encode_session();
            Some(Ok(Box::new(enc)))
        }
        Err(why) => declined(&why),
    }
}

fn hevc_factory(w: u32, h: u32, rate: FrameRate, s: &ExportSettings) -> Option<Result<Box<dyn VideoEncoder>>> {
    let declined = |why: &str| {
        log::info!("VA-API hardware H.265 encoding declined: {why}");
        filmcraft_export::note_hw_encode_declined();
        Some(Err(ExportError::Unsupported(format!("H.265 export with VA-API: {why}"))))
    };
    let cfg = match config_for(Format::Hevc, w, h, rate, s) {
        Ok(c) => c,
        Err(why) => return declined(&why),
    };
    match VaapiEncoder::open(cfg, rate) {
        Ok(enc) => {
            log::info!("VA-API hardware H.265 encoder active ({w}x{h})");
            filmcraft_export::note_hw_encode_session();
            Some(Ok(Box::new(enc)))
        }
        Err(why) if !hevc_available() => {
            log::info!("no VA-API hardware H.265 encoder: {why}");
            None
        }
        Err(why) => declined(&why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vaapi_encodes_h264_and_hevc_frames_end_to_end() {
        let (w, h) = (640u32, 360u32);
        let rgba = vec![128u8; (w * h * 4) as usize];
        for format in [Format::H264, Format::Hevc] {
            let s = ExportSettings { format, hardware_encoding: HardwareEncoding::Auto, ..Default::default() };
            let Some(res) = factory(format, w, h, FrameRate::FPS_30, &s) else {
                continue;
            };
            let mut enc = res.expect("VA-API encoder should open");
            let mut packets = Vec::new();
            for index in 0..4 {
                let frame = EncoderFrame { rgba: &rgba, hdr: None, width: w, height: h, index };
                packets.extend(enc.encode(&frame).expect("VA-API encode frame"));
            }
            packets.extend(enc.flush().expect("VA-API flush"));
            assert_eq!(packets.len(), 4, "expected 4 packets for {format:?}");
            assert!(packets[0].key, "first packet must be IDR for {format:?}");
        }
    }
}
