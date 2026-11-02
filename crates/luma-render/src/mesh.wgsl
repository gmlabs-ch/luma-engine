// Lit, textured meshes: the Direct3D fixed-function pipeline as Wintermute
// Engine 1.x set it up (per-vertex Gouraud lighting, texture modulated by
// the lit colour, alpha from the texture, alpha test, linear vertex fog).
//
// The render states and light parameters follow `BRenderD3D.cpp`
// (`Setup3D`), `3DLight.cpp` and `XMesh.cpp` of Wintermute Engine 1.x
// (MIT License, Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.

// One light. `position.w` is the kind: 0 none, 1 directional, 2 point,
// 3 spot. `direction.w` is the range, `cone` holds the cosines of the
// half inner and half outer angles and the falloff exponent of a spot.
struct Light {
    position: vec4<f32>,
    direction: vec4<f32>,
    color: vec4<f32>,
    cone: vec4<f32>,
};

struct MeshUniforms {
    world: mat4x4<f32>,
    view_projection: mat4x4<f32>,
    // Material diffuse colour and alpha.
    diffuse: vec4<f32>,
    // Material emissive colour; `w` is 1 when a texture is bound.
    emissive: vec4<f32>,
    // Scene ambient colour; `w` is the number of lights.
    ambient: vec4<f32>,
    // Fog colour; `w` is 1 when fog is on.
    fog_color: vec4<f32>,
    // Fog start, fog end, alpha test reference.
    fog_params: vec4<f32>,
    lights: array<Light, 8>,
};

@group(0) @binding(0) var<uniform> u: MeshUniforms;
@group(1) @binding(0) var t_color: texture_2d<f32>;
@group(1) @binding(1) var s_color: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    // How much of the vertex colour survives the fog (1: no fog).
    @location(2) fog: f32,
};

// The diffuse term of one light at a world position with a unit normal:
// the light colour weighted by the cosine of the angle to the light, cut
// at the light's range and by the spot cone, with no distance
// attenuation (`Attenuation0 = 1`).
fn light_term(light: Light, world_pos: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    let kind = light.position.w;
    if kind < 0.5 {
        return vec3<f32>(0.0);
    }
    var to_light: vec3<f32>;
    var cone = 1.0;
    if kind < 1.5 {
        to_light = -normalize(light.direction.xyz);
    } else {
        let delta = light.position.xyz - world_pos;
        let distance = length(delta);
        if distance > light.direction.w || distance <= 0.0 {
            return vec3<f32>(0.0);
        }
        to_light = delta / distance;
        if kind > 2.5 {
            let axis = normalize(light.direction.xyz);
            let rho = dot(-to_light, axis);
            let cos_inner = light.cone.x;
            let cos_outer = light.cone.y;
            if rho > cos_inner {
                cone = 1.0;
            } else if rho <= cos_outer {
                return vec3<f32>(0.0);
            } else {
                cone = pow((rho - cos_outer) / max(cos_inner - cos_outer, 1e-6), light.cone.z);
            }
        }
    }
    let n_dot_l = max(dot(normal, to_light), 0.0);
    return light.color.rgb * n_dot_l * cone;
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    let world_pos = u.world * vec4<f32>(in.position, 1.0);
    out.position = u.view_projection * world_pos;
    out.uv = in.uv;

    // The world matrix of an actor is a rotation, a uniform scale and a
    // translation, so its upper 3x3 moves normals too; they are then
    // normalised like `D3DRS_NORMALIZENORMALS` does.
    let normal = normalize((u.world * vec4<f32>(in.normal, 0.0)).xyz);

    // Emissive + ambient * material ambient (= diffuse) + sum of the lights
    // * material diffuse. The lights have no ambient or specular part.
    var lit = u.emissive.rgb + u.ambient.rgb * u.diffuse.rgb;
    let count = i32(u.ambient.w + 0.5);
    for (var i = 0; i < 8; i++) {
        if i >= count {
            break;
        }
        lit += light_term(u.lights[i], world_pos.xyz, normal) * u.diffuse.rgb;
    }
    out.color = vec4<f32>(clamp(lit, vec3<f32>(0.0), vec3<f32>(1.0)), u.diffuse.a);

    // Linear vertex fog on the view depth, which is the clip `w` of a
    // Direct3D perspective projection.
    if u.fog_color.w > 0.5 {
        let start = u.fog_params.x;
        let end = u.fog_params.y;
        out.fog = clamp((end - out.position.w) / max(end - start, 1e-6), 0.0, 1.0);
    } else {
        out.fog = 1.0;
    }
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(t_color, s_color, in.uv);
    // Stage 0: colour = texture * lit colour (MODULATE), alpha = texture
    // alpha alone (SELECTARG1); without a texture the lit colour and the
    // material alpha.
    let textured = u.emissive.w > 0.5;
    let rgb = texel.rgb * in.color.rgb;
    let alpha = select(in.color.a, texel.a, textured);
    // Alpha test: `D3DCMP_GREATEREQUAL` against the reference.
    if alpha < u.fog_params.z {
        discard;
    }
    let fogged = mix(u.fog_color.rgb, rgb, in.fog);
    return vec4<f32>(fogged, alpha);
}
