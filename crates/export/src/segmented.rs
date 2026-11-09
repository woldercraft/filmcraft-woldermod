//! Segmented export: render chunks of a sequence in parallel on multiple GPU/CPU pipelines,
//! then losslessly concatenate the container into the final output.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};

use filmcraft_isobmff::{Brand, Mp4Writer, TrackConfig, WriteSample, WriterOptions};
use filmcraft_media::ByteReader;
use filmcraft_project::{ItemId, Project};
use filmcraft_render::SourceProvider;
use filmcraft_time::TimeRange;

use crate::format::Format;
use crate::output::Out;
use crate::run::{export_here, export_range, frame_span};
use crate::{ExportError, ExportSettings, Progress, Report, Result};

pub(crate) fn is_segmentable(settings: &ExportSettings) -> bool {
    matches!(settings.format, Format::H264 | Format::Hevc | Format::Av1 | Format::ProRes | Format::DnxHr | Format::Mjpeg)
}

struct ByteReaderSource<'a, R: ?Sized>(&'a R);

impl<R: ByteReader + ?Sized> filmcraft_isobmff::ByteSource for ByteReaderSource<'_, R> {
    fn len(&self) -> u64 {
        self.0.len()
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
        self.0.read_at(offset, buf)
    }
}

/// Losslessly concatenate multiple MP4/MOV container parts into the target output.
pub(crate) fn concat_mp4s(parts: &[PathBuf], settings: &ExportSettings) -> Result<u64> {
    if parts.is_empty() {
        return Err(ExportError::Io("no parts to concatenate".into()));
    }
    if parts.len() == 1 {
        std::fs::rename(&parts[0], &settings.path).map_err(|e| ExportError::Io(e.to_string()))?;
        let len = std::fs::metadata(&settings.path).map(|m| m.len()).unwrap_or(0);
        return Ok(len);
    }

    let mut files = Vec::new();
    let mut readers = Vec::new();
    for p in parts {
        let reader = Arc::new(filmcraft_media::reader::FileReader::open(p).map_err(|e| ExportError::Io(e.to_string()))?);
        let mp4 = filmcraft_isobmff::open(ByteReaderSource(&*reader)).map_err(|e| ExportError::Io(e.to_string()))?;
        readers.push(reader);
        files.push(mp4);
    }

    let first = &files[0];
    if first.tracks.is_empty() {
        return Err(ExportError::Io("the first part has no tracks".into()));
    }

    let brand = if settings.format.is_mp4_with(settings.multiplexer) { Brand::Mp4 } else { Brand::Mov };
    let mut opts = WriterOptions::new(brand);
    opts.metadata = settings.metadata.udta();

    let out = Out::create(settings)?;
    let mut mux = Mp4Writer::new(out, opts).map_err(|e| ExportError::Io(e.to_string()))?;

    for t in &first.tracks {
        let entry = t.entries.first().ok_or_else(|| ExportError::Io("track has no sample entries".into()))?;
        let mut cfg = TrackConfig::new(entry.clone(), t.timescale);
        cfg.media_start = if t.edit_offset != 0 { Some(t.edit_offset) } else { None };
        mux.add_track(cfg).map_err(|e| ExportError::Io(e.to_string()))?;
    }

    for (k, (file, reader)) in files.iter().zip(&readers).enumerate() {
        if file.tracks.len() != first.tracks.len() {
            return Err(ExportError::Io(format!("part {k} track count mismatch")));
        }
        let mut sample_index = Vec::new();
        for (ti, track) in file.tracks.iter().enumerate() {
            for (si, s) in track.samples.iter().enumerate() {
                sample_index.push((ti, si, s.offset));
            }
        }
        sample_index.sort_by_key(|&(_, _, offset)| offset);

        let mut buf = Vec::new();
        for (ti, si, _) in sample_index {
            let track = &file.tracks[ti];
            let s = &track.samples[si];
            buf.resize(s.size as usize, 0);
            reader.read_at(s.offset, &mut buf).map_err(|e| ExportError::Io(e.to_string()))?;
            let composition_offset = (s.pts - s.dts) as i32;
            mux.write_sample(ti, WriteSample {
                data: &buf,
                duration: s.duration,
                composition_offset,
                is_sync: s.is_sync,
            }).map_err(|e| ExportError::Io(e.to_string()))?;
        }
    }

    let out = if brand == Brand::Mp4 {
        mux.finish_faststart()
    } else {
        mux.finish()
    }.map_err(|e| ExportError::Io(e.to_string()))?;

    let bytes = out.finish(settings)?;
    Ok(bytes)
}

/// Run an export by splitting the range into parallel chunks and merging upon completion.
pub fn segmented_export(
    project: &Arc<Project>,
    seq: ItemId,
    settings: &ExportSettings,
    sources: &dyn SourceProvider,
    progress: &Progress,
) -> Result<Report> {
    let t0 = web_time::Instant::now();
    let q = project.sequence(seq).ok_or(ExportError::NoSequence)?;
    let full_range = export_range(project, seq, settings)?;
    let rate = settings.frame_rate.unwrap_or(q.settings.frame_rate);
    let (f0, f1) = frame_span(rate, full_range);
    let total_frames = (f1 - f0).max(0);

    let n = settings.segments.min(total_frames as usize).clamp(2, 4);
    if total_frames < 2 || n <= 1 {
        let mut single = settings.clone();
        single.segments = 1;
        return export_here(project, seq, &single, sources, progress);
    }

    progress.total.store(total_frames as u64, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    progress.set_status(format!("Exporting {} frames (Parallel {}x GPU)", total_frames, n));

    let mut chunk_settings_list = Vec::new();
    let mut temp_paths = Vec::new();

    for k in 0..n {
        let cf0 = f0 + (k as i64 * total_frames) / n as i64;
        let cf1 = f0 + ((k + 1) as i64 * total_frames) / n as i64;
        let c_start = rate.tick_of(cf0);
        let c_end = rate.tick_of(cf1);
        let c_range = TimeRange::new(c_start, c_end - c_start);

        let part_path = format!("{}.part{}.tmp.{}", settings.path, k, settings.extension());
        temp_paths.push(PathBuf::from(&part_path));

        let mut c_set = settings.clone();
        c_set.path = part_path;
        c_set.range = Some(c_range);
        c_set.part_of_batch = true;
        c_set.segments = 1;
        chunk_settings_list.push(c_set);
    }

    let chunk_results: Arc<Mutex<Vec<Option<Result<Report>>>>> = Arc::new(Mutex::new((0..n).map(|_| None).collect()));
    let cancel = &progress.cancel;

    std::thread::scope(|scope| {
        for (k, c_settings) in chunk_settings_list.into_iter().enumerate() {
            let chunk_results = chunk_results.clone();
            scope.spawn(move || {
                let r = export_here(project, seq, &c_settings, sources, progress);
                chunk_results.lock().unwrap_or_else(PoisonError::into_inner)[k] = Some(r);
            });
        }
    });

    let results = std::mem::take(&mut *chunk_results.lock().unwrap_or_else(PoisonError::into_inner));
    let mut reports = Vec::new();
    let mut err = None;

    for (k, res) in results.into_iter().enumerate() {
        match res {
            Some(Ok(r)) => reports.push(r),
            Some(Err(e)) => {
                if err.is_none() {
                    err = Some(e);
                }
            }
            None => {
                if err.is_none() {
                    err = Some(ExportError::Encode(format!("chunk {k} failed without result")));
                }
            }
        }
    }

    if let Some(e) = err {
        for p in &temp_paths {
            let _ = std::fs::remove_file(p);
        }
        return Err(e);
    }

    if cancel.load(Ordering::Relaxed) {
        for p in &temp_paths {
            let _ = std::fs::remove_file(p);
        }
        return Err(ExportError::Cancelled);
    }

    progress.set_status("Merging parallel parts…");

    let concat_result = concat_mp4s(&temp_paths, settings);

    for p in &temp_paths {
        let _ = std::fs::remove_file(p);
    }

    let total_bytes = concat_result?;
    let secs = t0.elapsed().as_secs_f64();
    progress.done.store(total_frames as u64, Ordering::Relaxed);
    progress.finished.store(true, Ordering::Relaxed);
    progress.set_status(format!("Done in {secs:.1}s (Parallel {}x GPU)", n));

    Ok(Report {
        path: settings.path.clone(),
        frames: total_frames as u64,
        seconds: secs,
        bytes: total_bytes,
        render_fps: total_frames as f64 / secs.max(1e-6),
        extra_files: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export;
    use crate::tests::project;

    fn tmp(name: &str) -> String {
        let d = std::env::temp_dir().join(format!("fc-seg-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d.join(name).to_string_lossy().to_string()
    }

    #[test]
    fn segmented_export_splits_runs_parallel_and_stitches() {
        let (p, seq, m) = project();
        let path = tmp("seg_test.mov");
        let settings = ExportSettings {
            format: Format::ProRes,
            path: path.clone(),
            segments: 2,
            include_audio: false,
            ..Default::default()
        };
        let rep = export(&p, seq, &settings, &m, &Progress::default()).expect("export");
        assert!(rep.frames > 0);
        let bytes = std::fs::read(&path).expect("read");
        let f = filmcraft_isobmff::open(bytes.as_slice()).expect("open mp4");
        let vt = f.track_of_kind(filmcraft_isobmff::TrackKind::Video).expect("video track");
        assert_eq!(f.tracks[vt].samples.len() as u64, rep.frames);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn segmented_export_with_audio_and_h264() {
        let (p, seq, m) = project();
        let path = tmp("seg_audio.mp4");
        let settings = ExportSettings {
            format: Format::H264,
            path: path.clone(),
            segments: 2,
            include_audio: true,
            ..Default::default()
        };
        let rep = export(&p, seq, &settings, &m, &Progress::default()).expect("export");
        assert!(rep.frames > 0);
        let bytes = std::fs::read(&path).expect("read");
        let f = filmcraft_isobmff::open(bytes.as_slice()).expect("open mp4");
        let vt = f.track_of_kind(filmcraft_isobmff::TrackKind::Video).expect("video track");
        assert_eq!(f.tracks[vt].samples.len() as u64, rep.frames);
        let at = f.track_of_kind(filmcraft_isobmff::TrackKind::Audio).expect("audio track");
        assert!(!f.tracks[at].samples.is_empty());
        let _ = std::fs::remove_file(&path);
    }
}

