// HDR → SDR tone mapping, the same maths as tone.rs (Little Helpers CapConvert / kVidHlsl).
// mode 1: scRGB linear (1.0 = 80 nits); 2: PQ BT.2020; 3: already sRGB-encoded.

struct Params {
    mode: u32,
    white: f32,
    _pad0: u32,
    _pad1: u32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> p: Params;

fn srgb(l: vec3<f32>) -> vec3<f32> {
    let c = clamp(l, vec3<f32>(0.0), vec3<f32>(1.0));
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

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(src);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }
    let c = textureLoad(src, vec2<i32>(id.xy), 0).rgb;
    var o: vec3<f32>;
    if (p.mode == 1u) {
        o = srgb(c / (p.white / 80.0));
    } else if (p.mode == 2u) {
        let n = pq_to_nits(c) / p.white;
        let r = vec3<f32>(
            dot(n, vec3<f32>(1.6605, -0.5876, -0.0728)),
            dot(n, vec3<f32>(-0.1246, 1.1329, -0.0083)),
            dot(n, vec3<f32>(-0.0182, -0.1006, 1.1187)));
        o = srgb(r);
    } else {
        o = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    textureStore(dst, vec2<i32>(id.xy), vec4<f32>(o, 1.0));
}
