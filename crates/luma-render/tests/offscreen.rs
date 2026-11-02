//! Renders into an offscreen target and checks the pixels. Skipped when the
//! machine has no graphics adapter (some CI runners).

use luma_formats::{Image, Rgba};
use luma_render::{
    Attributes, BlendMode, DrawList, Gpu, Offscreen, Quad, Rect, Renderer, rotated_corners,
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

#[test]
fn rotates_a_quad_around_its_pivot() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let target = Offscreen::new(gpu, 64, 64);
    let image = renderer.texture("quad", &quad_image());

    // A 32x16 quad at (32, 32), turned a quarter clockwise around its
    // top-left corner: it lands over x 16..32, y 32..64, its top edge
    // (red, green) now running down its right side.
    let mut list = DrawList::new(64.0, 64.0);
    list.push_with(
        Quad {
            texture: &image,
            dst: Rect::new(32.0, 32.0, 32.0, 16.0),
            src: None,
            tint: Rgba::OPAQUE_WHITE,
            flip_x: false,
            flip_y: false,
        },
        Attributes {
            rotation: 90.0,
            pivot: (32.0, 32.0),
            blend: BlendMode::Normal,
        },
    );
    renderer.render(target.target(), BLACK, &list).unwrap();
    let out = target.read(gpu).unwrap();

    close(
        out.pixel(28, 36).unwrap(),
        rgba(255, 0, 0, 255),
        "red corner",
    );
    close(
        out.pixel(28, 60).unwrap(),
        rgba(0, 255, 0, 255),
        "green below",
    );
    close(
        out.pixel(18, 36).unwrap(),
        rgba(0, 0, 255, 255),
        "blue left",
    );
    close(
        out.pixel(18, 60).unwrap(),
        rgba(255, 255, 255, 255),
        "white",
    );
    close(out.pixel(40, 40).unwrap(), BLACK, "nothing where it was");
    close(out.pixel(24, 28).unwrap(), BLACK, "nothing above the pivot");

    // The corners the renderer computes are the ones drawn.
    let corners = rotated_corners(
        Rect::new(32.0, 32.0, 32.0, 16.0),
        &Attributes {
            rotation: 90.0,
            pivot: (32.0, 32.0),
            blend: BlendMode::Normal,
        },
    );
    let round = |(x, y): (f32, f32)| (x.round() as i32, y.round() as i32);
    assert_eq!(corners.map(round), [(32, 32), (32, 64), (16, 64), (16, 32)]);
    // The offset moves the pivot with the quad.
    let mut list = DrawList::new(64.0, 64.0);
    list.set_offset(-32.0, 0.0);
    list.push_with(
        Quad {
            texture: &image,
            dst: Rect::new(32.0, 32.0, 32.0, 16.0),
            src: None,
            tint: Rgba::OPAQUE_WHITE,
            flip_x: false,
            flip_y: false,
        },
        Attributes {
            rotation: 90.0,
            pivot: (32.0, 32.0),
            blend: BlendMode::Normal,
        },
    );
    assert_eq!(list.attributes()[0].pivot, (0.0, 32.0));
    assert_eq!(list.quads()[0].dst.x, 0.0);
}

#[test]
fn blends_additively_and_subtractively() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let target = Offscreen::new(gpu, 32, 32);
    let white = renderer.white();
    let grey = rgba(100, 100, 100, 255);

    let mut list = DrawList::new(32.0, 32.0);
    let stripe = |x: f32, color: Rgba, blend: BlendMode| {
        (
            Quad {
                texture: white,
                dst: Rect::new(x, 0.0, 8.0, 32.0),
                src: None,
                tint: color,
                flip_x: false,
                flip_y: false,
            },
            Attributes {
                rotation: 0.0,
                pivot: (0.0, 0.0),
                blend,
            },
        )
    };
    let mut fill = |x: f32, color: Rgba, blend: BlendMode| {
        let (quad, attributes) = stripe(x, color, blend);
        list.push_with(quad, attributes);
    };
    // Additive: the source, weighted by its alpha, adds to the grey.
    fill(0.0, rgba(60, 0, 0, 255), BlendMode::Additive);
    fill(8.0, rgba(60, 0, 0, 128), BlendMode::Additive);
    // Subtractive: the grey keeps `1 - src` of each channel.
    fill(16.0, rgba(255, 0, 0, 255), BlendMode::Subtractive);
    fill(24.0, rgba(128, 128, 128, 255), BlendMode::Subtractive);
    // A normal quad after them still blends normally.
    fill(0.0, rgba(0, 0, 200, 128), BlendMode::Normal);
    renderer.render(target.target(), grey, &list).unwrap();
    let out = target.read(gpu).unwrap();

    close(
        out.pixel(4, 4).unwrap(),
        rgba(80, 50, 150, 255),
        "normal over the additive result",
    );
    close(
        out.pixel(12, 4).unwrap(),
        rgba(130, 100, 100, 255),
        "half additive",
    );
    close(
        out.pixel(20, 4).unwrap(),
        rgba(0, 100, 100, 255),
        "red subtracted",
    );
    close(
        out.pixel(28, 4).unwrap(),
        rgba(50, 50, 50, 255),
        "half subtracted",
    );
}
