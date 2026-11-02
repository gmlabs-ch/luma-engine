//! Renders the items of the stencil shadows (volumes, depth-only
//! receivers, the masked fill) and the stencil-once and depth-write
//! options of the mesh draws into an offscreen target, and checks the
//! pixels. Skipped when the machine has no graphics adapter.

use luma_formats::{Image, Rgba};
use luma_render::{
    DepthOnlyDraw, DrawList, Gpu, IDENTITY, MeshDraw, MeshHandle, MeshMaterial, MeshVertex,
    Offscreen, Rect, Renderer, ShadowVolumeDraw, StencilFill,
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

const GREY: Rgba = Rgba {
    r: 200,
    g: 200,
    b: 200,
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

/// A rectangle in clip space (`x0..x1`, `y0..y1`, `y` up) at depth `z`,
/// wound clockwise on screen (a front face) when `front`.
fn rect(x0: f32, x1: f32, y0: f32, y1: f32, z: f32, front: bool) -> (Vec<MeshVertex>, Vec<u32>) {
    let v = |x: f32, y: f32| MeshVertex {
        position: [x, y, z],
        normal: [0.0, 0.0, -1.0],
        uv: [0.0, 0.0],
    };
    let vertices = vec![v(x0, y1), v(x1, y1), v(x1, y0), v(x0, y0)];
    let indices = if front {
        vec![0, 1, 2, 0, 2, 3]
    } else {
        vec![0, 2, 1, 0, 3, 2]
    };
    (vertices, indices)
}

fn upload(renderer: &Renderer, mesh: (Vec<MeshVertex>, Vec<u32>)) -> MeshHandle {
    renderer.upload_mesh(&mesh.0, &mesh.1)
}

/// The triangles of a rectangle as a shadow volume takes them.
fn triangles(mesh: &(Vec<MeshVertex>, Vec<u32>)) -> Vec<[f32; 3]> {
    mesh.1
        .iter()
        .map(|i| mesh.0[*i as usize].position)
        .collect()
}

/// A volume covering the left half of the screen between depths `near`
/// and `far`: a front face at `near` and a back face at `far`, wound
/// outward (clockwise toward the camera), or inward when `inward`, as
/// the original's volumes are.
fn left_half_volume(near: f32, far: f32, inward: bool) -> ShadowVolumeDraw {
    let mut vertices = triangles(&rect(-1.0, 0.0, -1.0, 1.0, near, !inward));
    vertices.extend(triangles(&rect(-1.0, 0.0, -1.0, 1.0, far, inward)));
    ShadowVolumeDraw {
        vertices,
        world: IDENTITY,
        view_projection: IDENTITY,
    }
}

const HALF_BLACK: Rgba = Rgba {
    r: 0,
    g: 0,
    b: 0,
    a: 128,
};

fn solid(color: [f32; 4]) -> MeshMaterial {
    MeshMaterial {
        diffuse: [0.0, 0.0, 0.0, color[3]],
        emissive: [color[0], color[1], color[2]],
    }
}

#[test]
fn a_volume_darkens_the_receiver_inside_it_and_the_fill_clears_the_stencil() {
    let Some(renderer) = renderer() else { return };
    let target = Offscreen::new(renderer.gpu(), 8, 8);
    let white = renderer.white();
    let receiver = upload(&renderer, rect(-1.0, 1.0, -1.0, 1.0, 0.5, true));

    // The receiver's depth is written, the volume's front face (0.2)
    // passes the depth test over the left half and its back face (0.8)
    // fails: the stencil counts one there, zero on the right.
    let mut list = DrawList::new(8.0, 8.0);
    list.fill(white, Rect::new(0.0, 0.0, 8.0, 8.0), GREY);
    list.push_depth_only(DepthOnlyDraw {
        mesh: receiver,
        world: IDENTITY,
        view_projection: IDENTITY,
        cull_backfaces: true,
    });
    list.push_shadow_volume(left_half_volume(0.2, 0.8, false));
    list.push_stencil_fill(StencilFill { color: HALF_BLACK });
    // A second fill right after finds the stencil cleared: no double
    // darkening.
    list.push_stencil_fill(StencilFill { color: HALF_BLACK });
    renderer.render(target.target(), GREY, &list).unwrap();
    let image = target.read(renderer.gpu()).unwrap();
    close(
        image.pixel(1, 4).unwrap(),
        rgba(100, 100, 100, 255),
        "shadowed left",
    );
    close(image.pixel(6, 4).unwrap(), GREY, "lit right");

    // Two volumes, or one wound inward (the far side counts up and fails,
    // the near side counts down and wraps to 255): still one shadow.
    for volumes in [
        vec![
            left_half_volume(0.2, 0.8, false),
            left_half_volume(0.3, 0.9, false),
        ],
        vec![left_half_volume(0.2, 0.8, true)],
    ] {
        let mut list = DrawList::new(8.0, 8.0);
        list.fill(white, Rect::new(0.0, 0.0, 8.0, 8.0), GREY);
        list.push_depth_only(DepthOnlyDraw {
            mesh: receiver,
            world: IDENTITY,
            view_projection: IDENTITY,
            cull_backfaces: true,
        });
        let count = volumes.len();
        for volume in volumes {
            list.push_shadow_volume(volume);
        }
        list.push_stencil_fill(StencilFill { color: HALF_BLACK });
        renderer.render(target.target(), GREY, &list).unwrap();
        let image = target.read(renderer.gpu()).unwrap();
        close(
            image.pixel(1, 4).unwrap(),
            rgba(100, 100, 100, 255),
            &format!("{count} volume(s)"),
        );
        close(image.pixel(6, 4).unwrap(), GREY, "lit right");
    }

    // Without a receiver nothing is behind the volume: both faces pass
    // the depth test, the count returns to zero, nothing darkens.
    let mut list = DrawList::new(8.0, 8.0);
    list.fill(white, Rect::new(0.0, 0.0, 8.0, 8.0), GREY);
    list.push_shadow_volume(left_half_volume(0.2, 0.8, false));
    list.push_stencil_fill(StencilFill { color: HALF_BLACK });
    renderer.render(target.target(), GREY, &list).unwrap();
    let image = target.read(renderer.gpu()).unwrap();
    close(image.pixel(1, 4).unwrap(), GREY, "open volume");
}

#[test]
fn a_depth_only_receiver_hides_the_meshes_behind_it() {
    let Some(renderer) = renderer() else { return };
    let target = Offscreen::new(renderer.gpu(), 8, 8);
    let receiver = upload(&renderer, rect(-1.0, 1.0, -1.0, 1.0, 0.5, true));
    let behind = upload(&renderer, rect(-1.0, 0.0, -1.0, 1.0, 0.7, true));
    let in_front = upload(&renderer, rect(0.0, 1.0, -1.0, 1.0, 0.3, true));
    let mut list = DrawList::new(8.0, 8.0);
    list.push_depth_only(DepthOnlyDraw {
        mesh: receiver,
        world: IDENTITY,
        view_projection: IDENTITY,
        cull_backfaces: true,
    });
    for mesh in [behind, in_front] {
        list.push_mesh(MeshDraw {
            material: solid([1.0, 0.0, 0.0, 1.0]),
            ..MeshDraw::new(mesh, IDENTITY, IDENTITY)
        });
    }
    renderer.render(target.target(), GREY, &list).unwrap();
    let image = target.read(renderer.gpu()).unwrap();
    // The receiver itself leaves no colour.
    close(image.pixel(1, 4).unwrap(), GREY, "behind the receiver");
    close(
        image.pixel(6, 4).unwrap(),
        rgba(255, 0, 0, 255),
        "in front of it",
    );
}

#[test]
fn stencil_once_darkens_overlapping_triangles_evenly_and_depth_writes_can_be_off() {
    let Some(renderer) = renderer() else { return };
    let target = Offscreen::new(renderer.gpu(), 8, 8);
    let quad = upload(&renderer, rect(-1.0, 1.0, -1.0, 1.0, 0.2, true));
    let translucent = |once: Option<u8>| MeshDraw {
        material: solid([0.0, 0.0, 0.0, 0.5]),
        depth_write: false,
        stencil_once: once,
        ..MeshDraw::new(quad, IDENTITY, IDENTITY)
    };

    // Drawn twice with the same reference: darkened once.
    let mut list = DrawList::new(8.0, 8.0);
    list.push_mesh(translucent(Some(1)));
    list.push_mesh(translucent(Some(1)));
    renderer.render(target.target(), GREY, &list).unwrap();
    let image = target.read(renderer.gpu()).unwrap();
    close(image.pixel(4, 4).unwrap(), rgba(100, 100, 100, 255), "once");

    // Another reference darkens again; without the option every draw
    // darkens.
    let mut list = DrawList::new(8.0, 8.0);
    list.push_mesh(translucent(Some(1)));
    list.push_mesh(translucent(Some(2)));
    renderer.render(target.target(), GREY, &list).unwrap();
    let image = target.read(renderer.gpu()).unwrap();
    close(
        image.pixel(4, 4).unwrap(),
        rgba(50, 50, 50, 255),
        "two references",
    );
    let mut list = DrawList::new(8.0, 8.0);
    list.push_mesh(translucent(None));
    list.push_mesh(translucent(None));
    renderer.render(target.target(), GREY, &list).unwrap();
    let image = target.read(renderer.gpu()).unwrap();
    close(
        image.pixel(4, 4).unwrap(),
        rgba(50, 50, 50, 255),
        "no stencil",
    );

    // The translucent quad at 0.2 wrote no depth, so an opaque mesh
    // behind it at 0.4 still passes the depth test.
    let behind = upload(&renderer, rect(-1.0, 1.0, -1.0, 1.0, 0.4, true));
    let mut list = DrawList::new(8.0, 8.0);
    list.push_mesh(translucent(None));
    list.push_mesh(MeshDraw {
        material: solid([0.0, 0.0, 1.0, 1.0]),
        ..MeshDraw::new(behind, IDENTITY, IDENTITY)
    });
    renderer.render(target.target(), GREY, &list).unwrap();
    let image = target.read(renderer.gpu()).unwrap();
    close(
        image.pixel(4, 4).unwrap(),
        rgba(0, 0, 255, 255),
        "no depth write",
    );
    let mut list = DrawList::new(8.0, 8.0);
    list.push_mesh(MeshDraw {
        depth_write: true,
        ..translucent(None)
    });
    list.push_mesh(MeshDraw {
        material: solid([0.0, 0.0, 1.0, 1.0]),
        ..MeshDraw::new(behind, IDENTITY, IDENTITY)
    });
    renderer.render(target.target(), GREY, &list).unwrap();
    let image = target.read(renderer.gpu()).unwrap();
    close(
        image.pixel(4, 4).unwrap(),
        rgba(100, 100, 100, 255),
        "depth write",
    );
}

#[test]
fn the_items_keep_their_place_among_the_quads() {
    let Some(renderer) = renderer() else { return };
    let target = Offscreen::new(renderer.gpu(), 8, 8);
    let white = renderer.white();
    let receiver = upload(&renderer, rect(-1.0, 1.0, -1.0, 1.0, 0.5, true));
    // Background, the shadow, then a foreground quad over the top half
    // drawn after the fill: the shadow shows only in the bottom half.
    let mut list = DrawList::new(8.0, 8.0);
    list.fill(white, Rect::new(0.0, 0.0, 8.0, 8.0), GREY);
    list.push_depth_only(DepthOnlyDraw {
        mesh: receiver,
        world: IDENTITY,
        view_projection: IDENTITY,
        cull_backfaces: true,
    });
    list.push_shadow_volume(left_half_volume(0.2, 0.8, false));
    list.push_stencil_fill(StencilFill { color: HALF_BLACK });
    list.fill(white, Rect::new(0.0, 0.0, 8.0, 4.0), rgba(0, 255, 0, 255));
    renderer.render(target.target(), GREY, &list).unwrap();
    let image: Image = target.read(renderer.gpu()).unwrap();
    close(
        image.pixel(1, 1).unwrap(),
        rgba(0, 255, 0, 255),
        "foreground",
    );
    close(
        image.pixel(1, 6).unwrap(),
        rgba(100, 100, 100, 255),
        "shadow below",
    );
    close(image.pixel(6, 6).unwrap(), GREY, "background");
}
