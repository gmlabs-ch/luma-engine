//! Renders lit meshes between quads into an offscreen target and checks
//! the pixels. Skipped when the machine has no graphics adapter.

use luma_formats::{Image, Rgba};
use luma_render::{
    DrawList, Fog, Gpu, IDENTITY, Light, Lights, Matrix, MeshDraw, MeshHandle, MeshMaterial,
    MeshVertex, Offscreen, Rect, Renderer, Texture,
};

fn renderer() -> Option<Renderer> {
    match Gpu::headless() {
        Ok(gpu) => {
            eprintln!("adapter: {}", gpu.describe());
            Some(Renderer::new(gpu))
        }
        Err(err) => {
            eprintln!("skipped: {err}");
            None
        }
    }
}

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Rgba {
    Rgba { r, g, b, a }
}

const BLACK: Rgba = Rgba {
    r: 0,
    g: 0,
    b: 0,
    a: 255,
};

fn close(actual: Rgba, expected: Rgba, what: &str) {
    for (a, e) in [
        (actual.r, expected.r),
        (actual.g, expected.g),
        (actual.b, expected.b),
        (actual.a, expected.a),
    ] {
        assert!(
            a.abs_diff(e) <= 2,
            "{what}: expected {expected:?}, got {actual:?}"
        );
    }
}

/// A 4x4 image of four 2x2 blocks: red, green / blue, white. The green
/// block is transparent when `keyed`.
fn block_image(keyed: bool) -> Image {
    let colors = [
        [255, 0, 0, 255],
        [0, 255, 0, if keyed { 0 } else { 255 }],
        [0, 0, 255, 255],
        [255, 255, 255, 255],
    ];
    let mut pixels = Vec::with_capacity(64);
    for y in 0..4 {
        for x in 0..4 {
            pixels.extend_from_slice(&colors[(y / 2) * 2 + x / 2]);
        }
    }
    Image {
        width: 4,
        height: 4,
        pixels,
        has_alpha_channel: true,
    }
}

/// A rectangle in clip space (`x0..x1`, `y0..y1`, `y` up) at depth `z`,
/// facing the camera, with the texture mapped once over it, wound
/// clockwise on screen as Direct3D front faces are.
fn rect_mesh(x0: f32, x1: f32, y0: f32, y1: f32, z: f32) -> (Vec<MeshVertex>, Vec<u32>) {
    let v = |x: f32, y: f32, u: f32, vv: f32| MeshVertex {
        position: [x, y, z],
        normal: [0.0, 0.0, -1.0],
        uv: [u, vv],
    };
    (
        vec![
            v(x0, y1, 0.0, 0.0), // top-left
            v(x1, y1, 1.0, 0.0), // top-right
            v(x1, y0, 1.0, 1.0), // bottom-right
            v(x0, y0, 0.0, 1.0), // bottom-left
        ],
        vec![0, 1, 2, 0, 2, 3],
    )
}

fn full_mesh(renderer: &Renderer, z: f32) -> MeshHandle {
    let (vertices, indices) = rect_mesh(-1.0, 1.0, -1.0, 1.0, z);
    renderer.upload_mesh(&vertices, &indices)
}

fn emissive(color: [f32; 3]) -> MeshMaterial {
    MeshMaterial {
        diffuse: [0.0, 0.0, 0.0, 1.0],
        emissive: color,
    }
}

const WHITE_MATERIAL: MeshMaterial = MeshMaterial {
    diffuse: [1.0, 1.0, 1.0, 1.0],
    emissive: [0.0, 0.0, 0.0],
};

fn draw<'a>(mesh: MeshHandle, texture: Option<&'a Texture>) -> MeshDraw<'a> {
    MeshDraw {
        texture,
        material: WHITE_MATERIAL,
        ..MeshDraw::new(mesh, IDENTITY, IDENTITY)
    }
}

fn render_one(renderer: &Renderer, target: &Offscreen, draw: MeshDraw<'_>) -> Image {
    let mut list = DrawList::new(target.target().width as f32, target.target().height as f32);
    list.push_mesh(draw);
    renderer.render(target.target(), BLACK, &list).unwrap();
    target.read(renderer.gpu()).unwrap()
}

#[test]
fn lights_a_textured_mesh_per_vertex() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let target = Offscreen::new(gpu, 32, 32);
    let texture = renderer.mesh_texture("blocks", &block_image(false));
    let mesh = full_mesh(&renderer, 0.5);

    // A half-intensity directional light shining along +z onto a mesh
    // facing -z: the texels at half brightness.
    let mut lit = draw(mesh, Some(&texture));
    lit.lights.push(Light::Directional {
        direction: [0.0, 0.0, 1.0],
        color: [0.5, 0.5, 0.5],
    });
    let out = render_one(&renderer, &target, lit.clone());
    close(out.pixel(8, 8).unwrap(), rgba(128, 0, 0, 255), "red block");
    close(
        out.pixel(24, 8).unwrap(),
        rgba(0, 128, 0, 255),
        "green block",
    );
    close(
        out.pixel(8, 24).unwrap(),
        rgba(0, 0, 128, 255),
        "blue block",
    );
    close(
        out.pixel(24, 24).unwrap(),
        rgba(128, 128, 128, 255),
        "white block",
    );

    // The light from behind the mesh gives nothing: black.
    lit.lights = Lights::from_slice(&[Light::Directional {
        direction: [0.0, 0.0, -1.0],
        color: [1.0, 1.0, 1.0],
    }]);
    let out = render_one(&renderer, &target, lit.clone());
    close(out.pixel(8, 8).unwrap(), BLACK, "lit from behind");

    // Ambient alone multiplies the material's diffuse (its ambient).
    lit.lights = Lights::NONE;
    lit.ambient = [0.5, 0.5, 0.5];
    let out = render_one(&renderer, &target, lit.clone());
    close(
        out.pixel(8, 8).unwrap(),
        rgba(128, 0, 0, 255),
        "ambient red",
    );
    close(
        out.pixel(24, 24).unwrap(),
        rgba(128, 128, 128, 255),
        "ambient white",
    );

    // No light at all: black, as the original renders a model in a scene
    // without lights.
    lit.ambient = [0.0; 3];
    let out = render_one(&renderer, &target, lit.clone());
    close(out.pixel(24, 24).unwrap(), BLACK, "unlit");

    // Untextured: the lit material colour, emissive added, its alpha.
    let mut plain = draw(mesh, None);
    plain.material = MeshMaterial {
        diffuse: [0.0, 0.0, 1.0, 1.0],
        emissive: [0.25, 0.0, 0.0],
    };
    plain.ambient = [0.5, 0.5, 0.5];
    let out = render_one(&renderer, &target, plain);
    close(
        out.pixel(16, 16).unwrap(),
        rgba(64, 0, 128, 255),
        "material colour",
    );

    // Lighting is clamped to white before the texture multiplies it.
    let mut bright = draw(mesh, Some(&texture));
    bright.ambient = [3.0, 3.0, 3.0];
    let out = render_one(&renderer, &target, bright);
    close(out.pixel(8, 8).unwrap(), rgba(255, 0, 0, 255), "clamped");
}

#[test]
fn point_and_spot_lights_have_a_range_and_a_cone() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let target = Offscreen::new(gpu, 32, 32);
    let mesh = full_mesh(&renderer, 0.5);

    // The light at (0, 0, -1) is 2.06 units from each corner of the mesh
    // and lights it at cos = 1.5 / 2.06 = 0.728, everywhere (Gouraud
    // interpolation of equal corners).
    let mut point = draw(mesh, None);
    point.lights.push(Light::Point {
        position: [0.0, 0.0, -1.0],
        range: 3.0,
        color: [1.0, 1.0, 1.0],
    });
    let out = render_one(&renderer, &target, point.clone());
    close(
        out.pixel(16, 16).unwrap(),
        rgba(186, 186, 186, 255),
        "point",
    );

    // Out of range: nothing, no gradual attenuation.
    point.lights = Lights::from_slice(&[Light::Point {
        position: [0.0, 0.0, -1.0],
        range: 2.0,
        color: [1.0, 1.0, 1.0],
    }]);
    let out = render_one(&renderer, &target, point.clone());
    close(out.pixel(16, 16).unwrap(), BLACK, "out of range");

    // A wide spot aimed at the mesh lights it like the point light.
    let spot = |direction: [f32; 3], inner_angle: f32, outer_angle: f32| Light::Spot {
        position: [0.0, 0.0, -1.0],
        direction,
        range: 3.0,
        inner_angle,
        outer_angle,
        falloff: 1.0,
        color: [1.0, 1.0, 1.0],
    };
    point.lights = Lights::from_slice(&[spot([0.0, 0.0, 1.0], 2.0, 2.5)]);
    let out = render_one(&renderer, &target, point.clone());
    close(
        out.pixel(16, 16).unwrap(),
        rgba(186, 186, 186, 255),
        "inside the cone",
    );

    // The original's cone (theta 0.5, phi 1.0) is narrower than the angle
    // to the corners (43 degrees): outside, nothing.
    point.lights = Lights::from_slice(&[spot([0.0, 0.0, 1.0], 0.5, 1.0)]);
    let out = render_one(&renderer, &target, point.clone());
    close(out.pixel(16, 16).unwrap(), BLACK, "outside the cone");

    // Between the cones the intensity fades with the falloff: at the
    // corners rho = 0.728; with cos(inner/2) = 0.8 and cos(outer/2) =
    // 0.6 the fade is (0.728 - 0.6) / 0.2 = 0.64.
    let inner = 2.0 * 0.8f32.acos();
    let outer = 2.0 * 0.6f32.acos();
    point.lights = Lights::from_slice(&[spot([0.0, 0.0, 1.0], inner, outer)]);
    let out = render_one(&renderer, &target, point.clone());
    let expected = (0.728 * 0.64 * 255.0) as u8;
    close(
        out.pixel(16, 16).unwrap(),
        rgba(expected, expected, expected, 255),
        "fading cone",
    );

    // Aimed away from the mesh: nothing.
    point.lights = Lights::from_slice(&[spot([0.0, 0.0, -1.0], 2.0, 2.5)]);
    let out = render_one(&renderer, &target, point);
    close(out.pixel(16, 16).unwrap(), BLACK, "aimed away");
}

#[test]
fn culls_counter_clockwise_faces_when_asked_and_draws_index_ranges() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let target = Offscreen::new(gpu, 32, 32);

    // Two triangles: a clockwise one (on screen) on the left half, its
    // mirror image on the right half.
    let v = |x: f32, y: f32| MeshVertex {
        position: [x, y, 0.5],
        normal: [0.0, 0.0, -1.0],
        uv: [0.0, 0.0],
    };
    let vertices = [
        v(-1.0, 1.0),
        v(0.0, 1.0),
        v(0.0, -1.0), // left: top-left, top-right, bottom: clockwise
        v(0.0, 1.0),
        v(0.0, -1.0),
        v(1.0, 1.0), // right: top-left, bottom-left, top-right: counter-clockwise
    ];
    let mesh = renderer.upload_mesh(&vertices, &[0, 1, 2, 3, 4, 5]);
    let mut both = draw(mesh, None);
    both.material = emissive([1.0, 1.0, 1.0]);
    let out = render_one(&renderer, &target, both.clone());
    close(
        out.pixel(6, 6).unwrap(),
        rgba(255, 255, 255, 255),
        "left, no culling",
    );
    close(
        out.pixel(26, 6).unwrap(),
        rgba(255, 255, 255, 255),
        "right, no culling",
    );

    both.cull_backfaces = true;
    let out = render_one(&renderer, &target, both.clone());
    close(
        out.pixel(6, 6).unwrap(),
        rgba(255, 255, 255, 255),
        "front face kept",
    );
    close(out.pixel(26, 6).unwrap(), BLACK, "back face culled");

    // Only the second triangle's indices: a material subset.
    both.cull_backfaces = false;
    both.indices = Some(3..6);
    let out = render_one(&renderer, &target, both.clone());
    close(out.pixel(6, 6).unwrap(), BLACK, "first triangle skipped");
    close(
        out.pixel(26, 6).unwrap(),
        rgba(255, 255, 255, 255),
        "second triangle drawn",
    );

    // A range past the end is clipped, not an error.
    both.indices = Some(3..60);
    let out = render_one(&renderer, &target, both);
    close(
        out.pixel(26, 6).unwrap(),
        rgba(255, 255, 255, 255),
        "clipped range",
    );
}

#[test]
fn interleaves_meshes_with_quads_and_depth_tests_among_meshes() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let target = Offscreen::new(gpu, 64, 32);
    let white = renderer.white();
    let far = full_mesh(&renderer, 0.8);
    let (vertices, indices) = rect_mesh(-1.0, 0.0, -1.0, 1.0, 0.2);
    let near_left = renderer.upload_mesh(&vertices, &indices);
    let red = |mesh| MeshDraw {
        material: emissive([1.0, 0.0, 0.0]),
        ..MeshDraw::new(mesh, IDENTITY, IDENTITY)
    };
    let green = |mesh| MeshDraw {
        material: emissive([0.0, 1.0, 0.0]),
        ..MeshDraw::new(mesh, IDENTITY, IDENTITY)
    };

    // Background quad, far red mesh, near green mesh on the left, a
    // yellow quad in the top-right corner over everything.
    let mut list = DrawList::new(64.0, 32.0);
    list.fill(white, Rect::new(0.0, 0.0, 64.0, 32.0), rgba(0, 0, 255, 255));
    list.push_mesh(red(far));
    list.push_mesh(green(near_left));
    list.fill(
        white,
        Rect::new(48.0, 0.0, 16.0, 16.0),
        rgba(255, 255, 0, 255),
    );
    renderer.render(target.target(), BLACK, &list).unwrap();
    let out = target.read(gpu).unwrap();
    close(
        out.pixel(16, 16).unwrap(),
        rgba(0, 255, 0, 255),
        "near mesh wins",
    );
    close(
        out.pixel(40, 24).unwrap(),
        rgba(255, 0, 0, 255),
        "far mesh alone",
    );
    close(
        out.pixel(56, 8).unwrap(),
        rgba(255, 255, 0, 255),
        "later quad over the mesh",
    );

    // The near mesh first: the far one fails the depth test on the left.
    let mut list = DrawList::new(64.0, 32.0);
    list.fill(white, Rect::new(0.0, 0.0, 64.0, 32.0), rgba(0, 0, 255, 255));
    list.push_mesh(green(near_left));
    list.push_mesh(red(far));
    renderer.render(target.target(), BLACK, &list).unwrap();
    let out = target.read(gpu).unwrap();
    close(
        out.pixel(16, 16).unwrap(),
        rgba(0, 255, 0, 255),
        "far mesh rejected",
    );
    close(
        out.pixel(40, 24).unwrap(),
        rgba(255, 0, 0, 255),
        "far mesh right",
    );

    // A quad between the meshes neither reads nor writes depth: it covers
    // the green mesh, and the red mesh drawn after it still fails the
    // depth test where the green one was.
    let mut list = DrawList::new(64.0, 32.0);
    list.push_mesh(green(near_left));
    list.fill(
        white,
        Rect::new(0.0, 0.0, 64.0, 32.0),
        rgba(100, 100, 100, 255),
    );
    list.push_mesh(red(far));
    renderer.render(target.target(), BLACK, &list).unwrap();
    let out = target.read(gpu).unwrap();
    close(
        out.pixel(16, 16).unwrap(),
        rgba(100, 100, 100, 255),
        "quad over the near mesh, far mesh still rejected",
    );
    close(
        out.pixel(40, 24).unwrap(),
        rgba(255, 0, 0, 255),
        "far mesh over the quad",
    );

    // The depth buffer is cleared every frame: the far mesh alone now
    // covers the left too.
    let mut list = DrawList::new(64.0, 32.0);
    list.push_mesh(red(far));
    renderer.render(target.target(), BLACK, &list).unwrap();
    let out = target.read(gpu).unwrap();
    close(
        out.pixel(16, 16).unwrap(),
        rgba(255, 0, 0, 255),
        "depth cleared",
    );

    // Meshes alone, before any quad, and many of them (the uniform buffer
    // grows).
    let mut list = DrawList::new(64.0, 32.0);
    for i in 0..40 {
        list.push_mesh(if i % 2 == 0 { red(far) } else { green(far) });
    }
    renderer.render(target.target(), BLACK, &list).unwrap();
    let out = target.read(gpu).unwrap();
    close(
        out.pixel(40, 24).unwrap(),
        rgba(0, 255, 0, 255),
        "equal depth: the later draw wins (<=)",
    );
}

#[test]
fn updates_mesh_vertices_between_frames() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let target = Offscreen::new(gpu, 32, 32);
    let (left, indices) = rect_mesh(-1.0, 0.0, -1.0, 1.0, 0.5);
    let mesh = renderer.upload_mesh(&left, &indices);
    let mut white = draw(mesh, None);
    white.material = emissive([1.0, 1.0, 1.0]);

    let out = render_one(&renderer, &target, white.clone());
    close(
        out.pixel(8, 16).unwrap(),
        rgba(255, 255, 255, 255),
        "first pose left",
    );
    close(out.pixel(24, 16).unwrap(), BLACK, "first pose right");

    let (right, _) = rect_mesh(0.0, 1.0, -1.0, 1.0, 0.5);
    renderer.update_mesh(mesh, &right).unwrap();
    let out = render_one(&renderer, &target, white.clone());
    close(out.pixel(8, 16).unwrap(), BLACK, "second pose left");
    close(
        out.pixel(24, 16).unwrap(),
        rgba(255, 255, 255, 255),
        "second pose right",
    );

    // More vertices than uploaded grow the buffer; the indices still
    // address the first four.
    let mut more = right.clone();
    more.extend_from_slice(&left);
    renderer.update_mesh(mesh, &more).unwrap();
    let out = render_one(&renderer, &target, white.clone());
    close(
        out.pixel(24, 16).unwrap(),
        rgba(255, 255, 255, 255),
        "grown buffer",
    );

    // A freed mesh is an error to draw or to update.
    renderer.free_mesh(mesh);
    assert!(renderer.update_mesh(mesh, &right).is_err());
    let mut list = DrawList::new(32.0, 32.0);
    list.push_mesh(white);
    assert!(renderer.render(target.target(), BLACK, &list).is_err());
}

#[test]
fn alpha_tests_keyed_texels_and_fogs_by_depth() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let target = Offscreen::new(gpu, 32, 32);
    let texture = renderer.mesh_texture("keyed", &block_image(true));
    let mesh = full_mesh(&renderer, 0.5);
    let mut lit = draw(mesh, Some(&texture));
    lit.ambient = [1.0, 1.0, 1.0];

    // The transparent green block is discarded: the clear colour shows.
    let mut list = DrawList::new(32.0, 32.0);
    list.push_mesh(lit.clone());
    renderer
        .render(target.target(), rgba(10, 20, 30, 255), &list)
        .unwrap();
    let out = target.read(gpu).unwrap();
    close(
        out.pixel(8, 8).unwrap(),
        rgba(255, 0, 0, 255),
        "opaque block",
    );
    close(
        out.pixel(24, 8).unwrap(),
        rgba(10, 20, 30, 255),
        "keyed block discarded",
    );

    // Fog from depth 0.5 to 1.5: the mesh at view depth 1 (clip `w`) is
    // half fogged towards white.
    lit.fog = Some(Fog {
        color: [1.0, 1.0, 1.0],
        start: 0.5,
        end: 1.5,
    });
    let out = render_one(&renderer, &target, lit.clone());
    close(
        out.pixel(8, 8).unwrap(),
        rgba(255, 128, 128, 255),
        "half fogged",
    );
    close(
        out.pixel(8, 24).unwrap(),
        rgba(128, 128, 255, 255),
        "half fogged blue",
    );

    // Beyond the fog end everything is the fog colour, keyed texels still
    // discarded.
    lit.fog = Some(Fog {
        color: [0.0, 1.0, 1.0],
        start: 0.0,
        end: 0.5,
    });
    let out = render_one(&renderer, &target, lit);
    close(out.pixel(8, 8).unwrap(), rgba(0, 255, 255, 255), "full fog");
    close(out.pixel(24, 8).unwrap(), BLACK, "keyed in fog");
}

#[test]
fn meshes_share_the_letterboxed_viewport_and_the_matrices_apply() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let target = Offscreen::new(gpu, 128, 48);
    let mesh = full_mesh(&renderer, 0.5);
    let mut white = draw(mesh, None);
    white.material = emissive([1.0, 1.0, 1.0]);

    // A 4:3 list in a 2:1 target: the mesh fills the centred 64x48 view
    // and the bars stay black.
    let mut list = DrawList::new(640.0, 480.0);
    list.letterbox = true;
    list.push_mesh(white.clone());
    renderer.render(target.target(), BLACK, &list).unwrap();
    let out = target.read(gpu).unwrap();
    close(out.pixel(8, 24).unwrap(), BLACK, "left bar");
    close(out.pixel(64, 24).unwrap(), rgba(255, 255, 255, 255), "view");
    close(out.pixel(120, 24).unwrap(), BLACK, "right bar");

    // A world matrix halving x and a view-projection moving everything
    // right by half the view: the mesh covers the right half of the view.
    let scale_x: Matrix = [
        [0.5, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let shift_x: Matrix = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.5, 0.0, 0.0, 1.0],
    ];
    white.world = scale_x;
    white.view_projection = shift_x;
    let mut list = DrawList::new(640.0, 480.0);
    list.letterbox = true;
    list.push_mesh(white);
    renderer.render(target.target(), BLACK, &list).unwrap();
    let out = target.read(gpu).unwrap();
    close(out.pixel(40, 24).unwrap(), BLACK, "left of the moved mesh");
    close(
        out.pixel(80, 24).unwrap(),
        rgba(255, 255, 255, 255),
        "moved mesh",
    );
    close(out.pixel(120, 24).unwrap(), BLACK, "still the bar");
}
