//! Opens a window and draws a few moving quads: a smoke test of the
//! window, the swap chain and the batching.
//!
//! Set `LUMA_RENDER_FRAMES=120` to exit after that many frames.

use luma_formats::{Image, Rgba};
use luma_render::{App, DrawList, Flow, Frame, Rect, Renderer, Texture, WindowConfig};

struct Demo {
    frames: Option<u64>,
    checker: Option<Texture>,
    time: f32,
}

fn checker(size: u32) -> Image {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let on = ((x / 8) + (y / 8)) % 2 == 0;
            pixels.extend_from_slice(if on {
                &[240, 200, 60, 255]
            } else {
                &[40, 60, 120, 255]
            });
        }
    }
    Image {
        width: size,
        height: size,
        pixels,
        has_alpha_channel: true,
    }
}

impl App for Demo {
    fn init(&mut self, renderer: &Renderer) -> luma_render::Result<()> {
        eprintln!("adapter: {}", renderer.gpu().describe());
        self.checker = Some(renderer.texture("checker", &checker(64)));
        Ok(())
    }

    fn draw<'a>(&'a mut self, frame: &Frame<'a>, list: &mut DrawList<'a>) -> Flow {
        self.time += frame.dt.as_secs_f32();
        let checker = self.checker.as_ref().expect("init ran");
        let white = frame.renderer.white();
        list.letterbox = true;
        list.width = 640.0;
        list.height = 480.0;
        list.fill(
            white,
            Rect::new(0.0, 0.0, 640.0, 480.0),
            Rgba {
                r: 30,
                g: 30,
                b: 40,
                a: 255,
            },
        );
        // A scrolling layer of checkers.
        let scroll = (self.time * 40.0) % 64.0;
        list.set_offset(-scroll, 0.0);
        for i in 0..12 {
            list.image(checker, i as f32 * 64.0, 100.0);
        }
        list.set_offset(0.0, 0.0);
        // A moving, tinted, flipped quad over it.
        let x = 320.0 + (self.time * 1.5).sin() * 200.0;
        list.push(luma_render::Quad {
            texture: checker,
            dst: Rect::new(x - 64.0, 200.0, 128.0, 128.0),
            src: Some(Rect::new(0.0, 0.0, 32.0, 32.0)),
            tint: Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 160,
            },
            flip_x: true,
            flip_y: false,
        });
        // A translucent red bar.
        list.fill(
            white,
            Rect::new(0.0, 400.0, 640.0, 40.0),
            Rgba {
                r: 255,
                g: 0,
                b: 0,
                a: 128,
            },
        );
        match self.frames {
            Some(n) if frame.frame_index + 1 >= n => Flow::Exit,
            _ => Flow::Continue,
        }
    }
}

fn main() {
    let frames = std::env::var("LUMA_RENDER_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok());
    let demo = Demo {
        frames,
        checker: None,
        time: 0.0,
    };
    if let Err(err) = luma_render::window::run(
        WindowConfig {
            title: "luma-render: quads".into(),
            width: 640,
            height: 480,
            ..WindowConfig::default()
        },
        demo,
    ) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
