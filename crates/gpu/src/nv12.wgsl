// RGBA8 (sRGB-encoded, over black) -> BT.709 limited-range NV12, integer math identical to the CPU
// reference `rgba_to_nv12_bt709` (crates/platform/src/vaapi/export.rs), so both routes give the
// same bytes. Output is a tightly packed byte buffer written as u32 words: the Y plane (w*h bytes)
// followed by the interleaved CbCr plane (w*h/2 bytes). Requires w % 4 == 0 and h % 2 == 0.

struct P {
    w: u32,
    h: u32,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;
@group(0) @binding(2) var<uniform> p: P;

fn px(x: u32, y: u32) -> vec3<i32> {
    let c = textureLoad(src, vec2<i32>(i32(x), i32(y)), 0);
    return vec3<i32>(i32(round(c.r * 255.0)), i32(round(c.g * 255.0)), i32(round(c.b * 255.0)));
}

fn luma(v: vec3<i32>) -> u32 {
    return u32(clamp(16 + ((11966 * v.x + 40254 * v.y + 4063 * v.z + 32768) >> 16u), 16, 235));
}

// One invocation = 4 luma pixels of a row = one output word.
@compute @workgroup_size(16, 16)
fn y_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x * 4u;
    let y = id.y;
    if (x >= p.w || y >= p.h) {
        return;
    }
    let v = luma(px(x, y)) | (luma(px(x + 1u, y)) << 8u) | (luma(px(x + 2u, y)) << 16u) | (luma(px(x + 3u, y)) << 24u);
    dst[(y * p.w) / 4u + id.x] = v;
}

// One invocation = 2 chroma pairs (4 luma pixels wide, 2 rows) = one output word.
@compute @workgroup_size(16, 16)
fn uv_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x * 4u;
    let cy = id.y;
    if (x >= p.w || cy >= p.h / 2u) {
        return;
    }
    let y = cy * 2u;
    var word = 0u;
    for (var k = 0u; k < 2u; k = k + 1u) {
        let x0 = x + 2u * k;
        let s = px(x0, y) + px(x0 + 1u, y) + px(x0, y + 1u) + px(x0 + 1u, y + 1u);
        let u = u32(clamp(128 + ((-6591 * s.x - 22170 * s.y + 28761 * s.z + 131072) >> 18u), 16, 240));
        let v = u32(clamp(128 + ((28761 * s.x - 26124 * s.y - 2637 * s.z + 131072) >> 18u), 16, 240));
        word = word | ((u | (v << 8u)) << (16u * k));
    }
    dst[(p.w * p.h) / 4u + (cy * p.w) / 4u + id.x] = word;
}
