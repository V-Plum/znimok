// The recording shader (ZK-87): one output pixel = one thread. The frame of the screen or
// window (FP16 scRGB, 10-bit or BGRA8) is cropped / fitted into the video size, tone mapped
// to SDR sRGB with the formulas of Little Helpers (kVidHlsl), then the click rings and the
// cursor overlay are drawn — AFTER the tone, so they are exact SDR colours on an HDR desktop.
// The result is BGRA8 (or RGBA8 on adapters without BGRA storage), what the encoder takes.
//
// Pixel choice (LH `Src()`): scale 1.0 = one source texel per output pixel, loaded 1:1 without
// filtering; otherwise a linear sample (a bigger window shrunk to the video size); outside
// `fit` — black (a smaller window is not stretched, LH: stretching blurs text).
//
// mode 0: values already gamma-encoded (BGRA8, 10-bit without PQ) — as they are;
// mode 1: scRGB linear, 1.0 = 80 nits — divided by white/80, sRGB curve, everything above
//         SDR white clipped («white stays white», right for interfaces);
// mode 2: PQ BT.2020 — nits, / white, BT.2020→709, sRGB curve.

struct Ring {
    // x, y (output pixels), radius, opacity
    geo: vec4<f32>,
    // kind: 0 click (ring + dot), 1 right click (second ring at 0.6 r), 2 hold (solid ring)
    kind: vec4<u32>,
};

struct Params {
    mode: u32,
    white: f32,
    scale: f32,
    ring_count: u32,
    src_origin: vec2<f32>,
    fit: vec2<u32>,
    // x, y, w, h of the cursor overlay in output pixels (w = 0: none)
    overlay: vec4<i32>,
    ring_color: vec4<f32>,
    // 2.5·s line, 5·s dot (LH), s = device scale
    ring_scale: vec4<f32>,
    rings: array<Ring, 8>,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var overlay_tex: texture_2d<f32>;
@group(0) @binding(3) var dst: texture_storage_2d<@FORMAT@, write>;
@group(0) @binding(4) var<uniform> p: Params;

fn finite(v: vec3<f32>) -> vec3<f32> {
    return select(v, vec3<f32>(0.0), v != v);
}

fn srgb(l: vec3<f32>) -> vec3<f32> {
    let c = clamp(finite(l), vec3<f32>(0.0), vec3<f32>(1.0));
    let lo = 12.92 * c;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn pq_to_nits(e: vec3<f32>) -> vec3<f32> {
    let m1 = 0.1593017578125;
    let m2 = 78.84375;
    let c1 = 0.8359375;
    let c2 = 18.8515625;
    let c3 = 18.6875;
    let pw = pow(max(e, vec3<f32>(0.0)), vec3<f32>(1.0 / m2));
    return 10000.0 * pow(max(pw - c1, vec3<f32>(0.0)) / (c2 - c3 * pw), vec3<f32>(1.0 / m1));
}

fn tone(c: vec3<f32>) -> vec3<f32> {
    if (p.mode == 1u) {
        return srgb(c / (p.white / 80.0));
    } else if (p.mode == 2u) {
        let n = pq_to_nits(c) / p.white;
        let r = vec3<f32>(
            dot(n, vec3<f32>(1.6605, -0.5876, -0.0728)),
            dot(n, vec3<f32>(-0.1246, 1.1329, -0.0083)),
            dot(n, vec3<f32>(-0.0182, -0.1006, 1.1187)));
        return srgb(r);
    }
    return clamp(finite(c), vec3<f32>(0.0), vec3<f32>(1.0));
}

// The source colour of output pixel `o`.
fn pick(o: vec2<u32>) -> vec3<f32> {
    if (o.x >= p.fit.x || o.y >= p.fit.y) {
        return vec3<f32>(0.0);
    }
    if (p.scale == 1.0) {
        let s = vec2<i32>(p.src_origin) + vec2<i32>(o);
        return textureLoad(src, s, 0).rgb;
    }
    let dims = vec2<f32>(textureDimensions(src));
    let s = (p.src_origin + (vec2<f32>(o) + vec2<f32>(0.5)) * p.scale) / dims;
    return textureSampleLevel(src, smp, s, 0.0).rgb;
}

fn rings(o: vec2<f32>, c: vec3<f32>) -> vec3<f32> {
    var out = c;
    let line = p.ring_scale.x;
    let dot_r = p.ring_scale.y;
    for (var i = 0u; i < p.ring_count; i = i + 1u) {
        let r = p.rings[i];
        let d = distance(o, r.geo.xy);
        var k = 0.0;
        if (r.kind.x == 2u) {
            // hold: a solid ring
            k = 1.0 - smoothstep(r.geo.z - line, r.geo.z + line * 0.5, d);
        } else {
            k = max(k, 1.0 - smoothstep(line * 0.5, line, abs(d - r.geo.z)));
            if (r.kind.x == 1u) {
                k = max(k, 1.0 - smoothstep(line * 0.5, line, abs(d - r.geo.z * 0.6)));
            }
            k = max(k, 0.75 * (1.0 - smoothstep(dot_r - 1.0, dot_r + 1.0, d)));
        }
        out = mix(out, p.ring_color.rgb, k * r.geo.w);
    }
    return out;
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(dst);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }
    var o = tone(pick(id.xy));
    if (p.ring_count > 0u) {
        o = rings(vec2<f32>(id.xy) + vec2<f32>(0.5), o);
    }
    // Cursor: «multiplier in a, addition in rgb» — an inverting (XOR) I-beam is a = −1, rgb = 1.
    let ov = vec2<i32>(id.xy) - p.overlay.xy;
    if (p.overlay.z > 0 && ov.x >= 0 && ov.y >= 0 && ov.x < p.overlay.z && ov.y < p.overlay.w) {
        let t = textureLoad(overlay_tex, ov, 0);
        o = clamp(o * t.a + t.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    textureStore(dst, vec2<i32>(id.xy), vec4<f32>(o, 1.0));
}
