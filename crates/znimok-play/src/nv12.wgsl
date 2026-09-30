// NV12 (BT.709, limited range 16–235 / 16–240) → RGBA8, on the GPU (ZK-92).
//
// The planes are either views of the decoder's own NV12 texture (Windows, zero copy) or of two
// textures (macOS IOSurface planes, or planes uploaded from CPU memory on the fallback path).
// Chroma is sampled bilinearly at its MPEG-2 siting: co-sited with the even luma columns,
// half-way between luma rows. The picture's tone (exposure, gamma, contrast — the same 256-entry
// table as `znimok_render::develop::tone_lut`) is applied at the end.
//
// `main` writes one output pixel per source pixel; `thumb` writes a smaller picture, each pixel
// the mean of a 4 × 4 grid of source samples (the film strip's thumbnails).

struct Params {
    // Size of the picture (the decoder may allocate more rows than it shows).
    width: u32,
    height: u32,
    // 1: apply `lut`.
    tone: u32,
    _pad: u32,
};

@group(0) @binding(0) var luma: texture_2d<f32>;
@group(0) @binding(1) var chroma: texture_2d<f32>;
@group(0) @binding(2) var dst: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(3) var<uniform> p: Params;
@group(0) @binding(4) var<storage, read> lut: array<u32, 256>;

fn chroma_at(x: f32, y: f32) -> vec2<f32> {
    let cdim = vec2<i32>(textureDimensions(chroma));
    let cx = x * 0.5;
    let cy = (y + 0.5) * 0.5 - 0.5;
    let x0 = i32(floor(cx));
    let y0 = i32(floor(cy));
    let fx = cx - f32(x0);
    let fy = cy - f32(y0);
    let hi = cdim - vec2<i32>(1, 1);
    let a = textureLoad(chroma, clamp(vec2<i32>(x0, y0), vec2<i32>(0), hi), 0).rg;
    let b = textureLoad(chroma, clamp(vec2<i32>(x0 + 1, y0), vec2<i32>(0), hi), 0).rg;
    let c = textureLoad(chroma, clamp(vec2<i32>(x0, y0 + 1), vec2<i32>(0), hi), 0).rg;
    let d = textureLoad(chroma, clamp(vec2<i32>(x0 + 1, y0 + 1), vec2<i32>(0), hi), 0).rg;
    return mix(mix(a, b, fx), mix(c, d, fx), fy);
}

fn rgb_at(ix: i32, iy: i32) -> vec3<f32> {
    let yv = textureLoad(luma, vec2<i32>(ix, iy), 0).r;
    let c = chroma_at(f32(ix), f32(iy));
    let y = (yv * 255.0 - 16.0) / 219.0;
    let cb = (c.r * 255.0 - 128.0) / 224.0;
    let cr = (c.g * 255.0 - 128.0) / 224.0;
    let rgb = vec3<f32>(y + 1.5748 * cr, y - 0.187324 * cb - 0.468124 * cr, y + 1.8556 * cb);
    return clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn toned(rgb: vec3<f32>) -> vec3<f32> {
    if (p.tone == 0u) {
        return rgb;
    }
    let i = vec3<u32>(round(rgb * 255.0));
    return vec3<f32>(f32(lut[i.x]), f32(lut[i.y]), f32(lut[i.z])) / 255.0;
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= p.width || id.y >= p.height) {
        return;
    }
    let rgb = toned(rgb_at(i32(id.x), i32(id.y)));
    textureStore(dst, vec2<i32>(id.xy), vec4<f32>(rgb, 1.0));
}

@compute @workgroup_size(8, 8)
fn thumb(@builtin(global_invocation_id) id: vec3<u32>) {
    let dim = textureDimensions(dst);
    if (id.x >= dim.x || id.y >= dim.y) {
        return;
    }
    let sx = f32(p.width) / f32(dim.x);
    let sy = f32(p.height) / f32(dim.y);
    var acc = vec3<f32>(0.0);
    for (var j = 0u; j < 4u; j++) {
        for (var i = 0u; i < 4u; i++) {
            let x = (f32(id.x) + (f32(i) + 0.5) / 4.0) * sx;
            let y = (f32(id.y) + (f32(j) + 0.5) / 4.0) * sy;
            let ix = clamp(i32(x), 0, i32(p.width) - 1);
            let iy = clamp(i32(y), 0, i32(p.height) - 1);
            acc += rgb_at(ix, iy);
        }
    }
    textureStore(dst, vec2<i32>(id.xy), vec4<f32>(toned(acc / 16.0), 1.0));
}
