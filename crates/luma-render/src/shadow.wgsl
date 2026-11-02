// Position-only drawing for the stencil shadows: the shadow volumes
// counted into the stencil buffer, the receivers written to the depth
// buffer only, and the viewport-wide fill masked by the stencil.
//
// Follows `3DShadowVolume.cpp` (`RenderToStencilBuffer`, `RenderToScene`)
// of Wintermute Engine 1.x (MIT License, Copyright (c) 2003-2013 Jan
// Nedoma); see `NOTICE`.

struct SimpleUniforms {
    // World times view-projection: model space to clip space.
    mvp: mat4x4<f32>,
    // The colour of the fill; unused by the other draws.
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: SimpleUniforms;

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return u.mvp * vec4<f32>(position, 1.0);
}

// One triangle covering the whole clip square, from the vertex index
// alone: (-1,-1), (3,-1), (-1,3).
@vertex
fn vs_fill(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return u.color;
}
