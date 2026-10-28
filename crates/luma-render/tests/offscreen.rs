//! Renders into an offscreen target and checks the pixels. Skipped when the
//! machine has no graphics adapter (some CI runners).

use luma_formats::{Image, Rgba};
use luma_render::{DrawList, Gpu, Offscreen, Quad, Rect, Renderer};

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

/// A 2x2 image: red, green / blue, white.
fn quad_image() -> Image {
    Image {
        width: 2,
        height: 2,
        pixels: vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ],
        has_alpha_channel: true,
    }
}

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

#[test]
fn draws_scaled_textures_in_order_with_blending() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let target = Offscreen::new(gpu, 64, 32);
    let image = renderer.texture("quad", &quad_image());
    let white = renderer.white();

    let mut list = DrawList::new(64.0, 32.0);
    // The 2x2 image stretched to 32x32 on the left: four 16x16 blocks.
    list.sprite(
        &image,
        Rect::new(0.0, 0.0, 32.0, 32.0),
        None,
        Rgba::OPAQUE_WHITE,
    );
    // Half-transparent red over the right half.
    list.fill(
        white,
        Rect::new(32.0, 0.0, 32.0, 32.0),
        rgba(255, 0, 0, 128),
    );
    // An opaque grey square drawn last wins over the red.
    list.fill(
        white,
        Rect::new(48.0, 16.0, 16.0, 16.0),
        rgba(100, 100, 100, 255),
    );
    renderer.render(target.target(), BLACK, &list).unwrap();
    let out = target.read(gpu).unwrap();
    assert_eq!((out.width, out.height), (64, 32));

    close(
        out.pixel(4, 4).unwrap(),
        rgba(255, 0, 0, 255),
        "top-left texel",
    );
    close(
        out.pixel(28, 4).unwrap(),
        rgba(0, 255, 0, 255),
        "top-right texel",
    );
    close(
        out.pixel(4, 28).unwrap(),
        rgba(0, 0, 255, 255),
        "bottom-left texel",
    );
    close(
        out.pixel(28, 28).unwrap(),
        rgba(255, 255, 255, 255),
        "bottom-right texel",
    );
    close(
        out.pixel(40, 8).unwrap(),
        rgba(128, 0, 0, 255),
        "red at 50% over black",
    );
    close(
        out.pixel(56, 24).unwrap(),
        rgba(100, 100, 100, 255),
        "later quad on top",
    );
}

#[test]
fn applies_source_rect_tint_flip_and_offset() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let target = Offscreen::new(gpu, 32, 32);
    let image = renderer.texture("quad", &quad_image());

    let mut list = DrawList::new(32.0, 32.0);
    // Only the top row (red, green), flipped horizontally, tinted to half
    // brightness, moved right by the layer offset.
    list.set_offset(16.0, 0.0);
    list.push(Quad {
        texture: &image,
        dst: Rect::new(0.0, 0.0, 16.0, 16.0),
        src: Some(Rect::new(0.0, 0.0, 2.0, 1.0)),
        tint: rgba(128, 128, 128, 255),
        flip_x: true,
        flip_y: false,
    });
    renderer.render(target.target(), BLACK, &list).unwrap();
    let out = target.read(gpu).unwrap();

    close(out.pixel(2, 2).unwrap(), BLACK, "left of the offset quad");
    close(
        out.pixel(18, 2).unwrap(),
        rgba(0, 128, 0, 255),
        "flipped: green first",
    );
    close(
        out.pixel(30, 2).unwrap(),
        rgba(128, 0, 0, 255),
        "flipped: red second",
    );
    close(
        out.pixel(18, 4).unwrap(),
        rgba(0, 128, 0, 255),
        "source rect: top row only",
    );
    // Bilinear filtering blends the rows towards the edge of a partial
    // source rectangle, as the engine does when stretching.
    assert!(out.pixel(18, 15).unwrap().r > 40, "blend at the edge");
    close(out.pixel(18, 20).unwrap(), BLACK, "below the quad");
}

#[test]
fn letterboxes_a_logical_size_into_the_target() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    // A 4:3 logical size in a 2:1 target: the view is 64x48 centred, with
    // 32-pixel bars on both sides.
    let target = Offscreen::new(gpu, 128, 48);
    let white = renderer.white();
    let mut list = DrawList::new(640.0, 480.0);
    list.letterbox = true;
    list.fill(
        white,
        Rect::new(0.0, 0.0, 640.0, 480.0),
        rgba(0, 0, 255, 255),
    );
    list.fill(
        white,
        Rect::new(0.0, 0.0, 320.0, 240.0),
        rgba(255, 255, 0, 255),
    );
    renderer.render(target.target(), BLACK, &list).unwrap();
    let out = target.read(gpu).unwrap();

    close(out.pixel(8, 24).unwrap(), BLACK, "left bar");
    close(out.pixel(120, 24).unwrap(), BLACK, "right bar");
    close(
        out.pixel(40, 8).unwrap(),
        rgba(255, 255, 0, 255),
        "top-left quarter",
    );
    close(
        out.pixel(88, 40).unwrap(),
        rgba(0, 0, 255, 255),
        "bottom-right quarter",
    );

    // Without letterboxing the logical size is stretched over the target.
    list.letterbox = false;
    renderer.render(target.target(), BLACK, &list).unwrap();
    let out = target.read(gpu).unwrap();
    close(
        out.pixel(8, 8).unwrap(),
        rgba(255, 255, 0, 255),
        "stretched top-left",
    );
    close(
        out.pixel(120, 40).unwrap(),
        rgba(0, 0, 255, 255),
        "stretched bottom-right",
    );
}

#[test]
fn handles_many_quads_and_empty_lists() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let target = Offscreen::new(gpu, 16, 16);
    let white = renderer.white();
    let image = renderer.texture("quad", &quad_image());

    let empty = DrawList::new(16.0, 16.0);
    renderer
        .render(target.target(), rgba(10, 20, 30, 255), &empty)
        .unwrap();
    close(
        target.read(gpu).unwrap().pixel(8, 8).unwrap(),
        rgba(10, 20, 30, 255),
        "clear colour",
    );

    // More quads than the initial buffers hold, alternating textures so
    // that batching has to split them.
    let mut list = DrawList::new(16.0, 16.0);
    for i in 0..2001 {
        let texture = if i % 2 == 0 { white } else { &image };
        list.sprite(
            texture,
            Rect::new(0.0, 0.0, 16.0, 16.0),
            None,
            rgba(0, 255, 0, 255),
        );
    }
    renderer.render(target.target(), BLACK, &list).unwrap();
    close(
        target.read(gpu).unwrap().pixel(8, 8).unwrap(),
        rgba(0, 255, 0, 255),
        "last quad: white tinted green",
    );
}
