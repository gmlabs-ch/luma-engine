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

/// A 4x2 logical list of eight distinct opaque colours, letterboxed.
fn palette_list(renderer: &Renderer) -> (DrawList<'_>, Vec<Rgba>) {
    let colors: Vec<Rgba> = (0..8u8)
        .map(|i| rgba(30 * i + 10, 255 - 30 * i, i * 31 + 7, 255))
        .collect();
    let mut list = DrawList::new(4.0, 2.0);
    list.letterbox = true;
    for (i, color) in colors.iter().enumerate() {
        let (x, y) = ((i % 4) as f32, (i / 4) as f32);
        list.fill(renderer.white(), Rect::new(x, y, 1.0, 1.0), *color);
    }
    (list, colors)
}

#[test]
fn letterboxed_whole_multiples_scale_without_blending() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let (list, colors) = palette_list(&renderer);
    let clear = rgba(9, 9, 9, 255);

    // 4x: every output pixel is exactly its source pixel, also next to the
    // boundary between two colours.
    let target = Offscreen::new(gpu, 16, 8);
    renderer.render(target.target(), clear, &list).unwrap();
    let out = target.read(gpu).unwrap();
    for y in 0..8u32 {
        for x in 0..16u32 {
            let expected = colors[((y / 4) * 4 + x / 4) as usize];
            assert_eq!(out.pixel(x, y).unwrap(), expected, "pixel {x},{y}");
        }
    }

    // In a 16x16 target the picture sits in rows 4..12 between two bars.
    let target = Offscreen::new(gpu, 16, 16);
    renderer.render(target.target(), clear, &list).unwrap();
    let out = target.read(gpu).unwrap();
    for y in 0..16u32 {
        for x in 0..16u32 {
            let expected = if (4..12).contains(&y) {
                colors[(((y - 4) / 4) * 4 + x / 4) as usize]
            } else {
                clear
            };
            assert_eq!(out.pixel(x, y).unwrap(), expected, "pixel {x},{y}");
        }
    }
}

#[test]
fn letterboxed_fractional_scale_keeps_the_pixel_centres() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let (list, colors) = palette_list(&renderer);
    // 4x2 into 10x5: a scale of 2.5, source pixel i covers 2.5i..2.5(i+1).
    let target = Offscreen::new(gpu, 10, 5);
    renderer
        .render(target.target(), rgba(9, 9, 9, 255), &list)
        .unwrap();
    let out = target.read(gpu).unwrap();
    // Output pixels whose centres lie well inside a source pixel.
    for (x, sx) in [(1u32, 0usize), (3, 1), (6, 2), (8, 3)] {
        for (y, sy) in [(1u32, 0usize), (3, 1)] {
            assert_eq!(
                out.pixel(x, y).unwrap(),
                colors[sy * 4 + sx],
                "pixel {x},{y}"
            );
        }
    }
}

#[test]
fn letterboxed_scaled_half_transparent_texture_survives_a_target_change() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let image = Image {
        width: 2,
        height: 2,
        pixels: vec![
            201, 3, 77, 128, 5, 250, 91, 128, 13, 99, 240, 128, 255, 1, 1, 128,
        ],
        has_alpha_channel: true,
    };
    let texture = renderer.texture("half", &image);
    let mut list = DrawList::new(8.0, 8.0);
    list.letterbox = true;
    list.sprite(
        &texture,
        Rect::new(1.0, 1.0, 6.0, 6.0),
        None,
        Rgba::OPAQUE_WHITE,
    );
    // Two different target sizes in a row: the internal texture is made
    // again for the second.
    for (w, h) in [(24, 16), (40, 40)] {
        let target = Offscreen::new(gpu, w, h);
        renderer.render(target.target(), BLACK, &list).unwrap();
        let out = target.read(gpu).unwrap();
        assert_eq!((out.width, out.height), (w, h));
        // The corner of the picture is outside the sprite: still black.
        let left = (w - w.min(h)) / 2;
        close(out.pixel(left, 0).unwrap(), BLACK, "outside the sprite");
        let centre = out.pixel(w / 2, h / 2).unwrap();
        assert!(
            centre.r > 0 || centre.g > 0 || centre.b > 0,
            "the sprite is drawn: {centre:?}"
        );
    }
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

#[test]
fn a_clip_rectangle_cuts_the_quads_pushed_under_it() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let target = Offscreen::new(gpu, 32, 32);
    let white = renderer.white();
    let red = rgba(255, 0, 0, 255);
    let green = rgba(0, 255, 0, 255);

    let mut list = DrawList::new(32.0, 32.0);
    // Clipped to the 8x8 square at (4, 4): a fill of the whole target
    // shows there only.
    list.set_clip(Some(Rect::new(4.0, 4.0, 8.0, 8.0)));
    list.fill(white, Rect::new(0.0, 0.0, 32.0, 32.0), red);
    // The clip replaced, not intersected: this one is outside the first.
    list.set_clip(Some(Rect::new(20.0, 20.0, 4.0, 4.0)));
    list.fill(white, Rect::new(0.0, 0.0, 32.0, 32.0), green);
    // Wholly outside the target: nothing, and no error.
    list.set_clip(Some(Rect::new(40.0, 0.0, 8.0, 8.0)));
    list.fill(white, Rect::new(0.0, 0.0, 32.0, 32.0), green);
    // Back to the whole screen.
    list.set_clip(None);
    list.fill(white, Rect::new(30.0, 0.0, 2.0, 2.0), green);
    renderer.render(target.target(), BLACK, &list).unwrap();
    let out = target.read(gpu).unwrap();

    close(out.pixel(4, 4).unwrap(), red, "inside the first clip");
    close(
        out.pixel(11, 11).unwrap(),
        red,
        "last pixel of the first clip",
    );
    close(out.pixel(3, 4).unwrap(), BLACK, "left of the first clip");
    close(out.pixel(12, 11).unwrap(), BLACK, "right of the first clip");
    close(out.pixel(21, 21).unwrap(), green, "inside the second clip");
    close(
        out.pixel(24, 21).unwrap(),
        BLACK,
        "right of the second clip",
    );
    close(out.pixel(31, 1).unwrap(), green, "unclipped again");
    close(out.pixel(16, 16).unwrap(), BLACK, "between the clips");
}

#[test]
fn lines_cover_the_pixels_of_a_line_list_without_the_last_one() {
    let Some(renderer) = renderer() else { return };
    let gpu = renderer.gpu();
    let target = Offscreen::new(gpu, 16, 16);
    let white = renderer.white();
    let red = rgba(255, 0, 0, 255);

    let mut list = DrawList::new(16.0, 16.0);
    list.line(white, 2, 1, 6, 1, red);
    list.line(white, 6, 3, 2, 3, red);
    list.line(white, 1, 5, 1, 9, red);
    list.line(white, 8, 8, 12, 12, red);
    list.line(white, 5, 5, 5, 5, red);
    renderer.render(target.target(), BLACK, &list).unwrap();
    let out = target.read(gpu).unwrap();

    for x in 2..6 {
        close(out.pixel(x, 1).unwrap(), red, "horizontal");
    }
    close(out.pixel(6, 1).unwrap(), BLACK, "the end is left out");
    close(out.pixel(1, 1).unwrap(), BLACK, "before the start");
    // Drawn backwards: the end (2) is left out, the start (6) is drawn.
    close(out.pixel(6, 3).unwrap(), red, "backwards start");
    close(out.pixel(3, 3).unwrap(), red, "backwards");
    close(out.pixel(2, 3).unwrap(), BLACK, "backwards end");
    for y in 5..9 {
        close(out.pixel(1, y).unwrap(), red, "vertical");
    }
    close(out.pixel(1, 9).unwrap(), BLACK, "vertical end");
    for i in 8..12 {
        close(out.pixel(i, i).unwrap(), red, "diagonal");
    }
    close(out.pixel(12, 12).unwrap(), BLACK, "diagonal end");
    close(out.pixel(9, 8).unwrap(), BLACK, "beside the diagonal");
    close(out.pixel(5, 5).unwrap(), BLACK, "an empty line");

    // The outline of a rectangle covers both ends on each axis.
    let mut list = DrawList::new(16.0, 16.0);
    list.rect_outline(white, 2, 2, 9, 7, red, 2);
    renderer.render(target.target(), BLACK, &list).unwrap();
    let out = target.read(gpu).unwrap();
    for (x, y) in [
        (2, 2),
        (9, 2),
        (2, 7),
        (9, 7),
        (3, 3),
        (8, 6),
        (5, 3),
        (8, 4),
    ] {
        close(out.pixel(x, y).unwrap(), red, "frame");
    }
    for (x, y) in [(1, 2), (10, 7), (2, 8), (4, 4), (7, 5)] {
        close(
            out.pixel(x, y).unwrap(),
            BLACK,
            "outside or inside the frame",
        );
    }
}
