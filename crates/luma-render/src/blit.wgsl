// Scales the frame drawn at the game's resolution onto the window.
//
// One triangle covers the viewport. The texture coordinate is snapped to
// the centre of the source pixel and only blended across the last output
// pixel before the next one: whole source pixels stay sharp at any
// scale, exact at a whole multiple, without the shimmer of plain nearest
// sampling at the others.

@group(0) @binding(0) var t_color: texture_2d<f32>;
@group(0) @binding(1) var s_color: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var out: VertexOutput;
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    out.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(t_color));
    let pixel = in.uv * size;
    let seam = floor(pixel + vec2<f32>(0.5));
    let step = max(fwidth(pixel), vec2<f32>(0.0001));
    let sharp = seam + clamp((pixel - seam) / step, vec2<f32>(-0.5), vec2<f32>(0.5));
    return textureSampleLevel(t_color, s_color, sharp / size, 0.0);
}
