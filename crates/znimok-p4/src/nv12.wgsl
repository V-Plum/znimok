// NV12 (BT.709, limited range 16-235) -> RGBA8. Chroma is taken nearest (2x2 block), which is
// exact for the test pattern; the editor will want bilinear chroma, a detail for Phase 8.

@group(0) @binding(0) var luma: texture_2d<f32>;
@group(0) @binding(1) var chroma: texture_2d<f32>;
@group(0) @binding(2) var dst: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dim = textureDimensions(dst);
    if (id.x >= dim.x || id.y >= dim.y) {
        return;
    }
    let yv = textureLoad(luma, vec2<i32>(id.xy), 0).r;
    let c = textureLoad(chroma, vec2<i32>(id.xy / 2u), 0).rg;
    let y = (yv * 255.0 - 16.0) / 219.0;
    let cb = (c.r * 255.0 - 128.0) / 224.0;
    let cr = (c.g * 255.0 - 128.0) / 224.0;
    let rgb = vec3<f32>(y + 1.5748 * cr, y - 0.187324 * cb - 0.468124 * cr, y + 1.8556 * cb);
    textureStore(dst, vec2<i32>(id.xy), vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0));
}
