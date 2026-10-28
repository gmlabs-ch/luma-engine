//! Desktop player for games running on Luma Engine.
//!
//! Milestone 2: shows one scene of a game, without scripts.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use luma_adventure::{Camera, GameSettings, SceneView};
use luma_archive::Vfs;
use luma_formats::Rgba;
use luma_render::winit::event::{ElementState, KeyEvent, WindowEvent};
use luma_render::winit::keyboard::{Key, NamedKey};
use luma_render::{App, DrawList, Flow, Frame, Gpu, Offscreen, Renderer, WindowConfig};

const USAGE: &str = "\
luma-player - desktop player for Luma Engine

USAGE:
    luma-player <game> --scene <file> [OPTIONS]

OPTIONS:
    --scene <file>        Scene to show (a path inside the game, e.g. scenes/room/room.scene)
    --scroll <x>,<y>      Initial scroll offset in scene pixels (default 0,0)
    --screenshot <png>    Render one frame to a PNG file and exit, without a window
    --animate             Play sprite animations (the default shows every first frame)
    --help                Show this message

<game> is a game directory (its *.dcp packages and loose files are read
the way the engine does).

In the window: arrow keys scroll the scene, Escape quits.
";

const BLACK: Rgba = Rgba {
    r: 0,
    g: 0,
    b: 0,
    a: 255,
};

struct Options {
    game: PathBuf,
    scene: String,
    scroll: (f32, f32),
    screenshot: Option<PathBuf>,
    animate: bool,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut game = None;
    let mut scene = None;
    let mut scroll = (0.0, 0.0);
    let mut screenshot = None;
    let mut animate = false;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let value = |i: &mut usize| -> Result<&String, String> {
            *i += 1;
            args.get(*i).ok_or_else(|| format!("{arg} needs a value"))
        };
        match arg.as_str() {
            "--scene" => scene = Some(value(&mut i)?.clone()),
            "--scroll" => {
                let v = value(&mut i)?;
                let (x, y) = v
                    .split_once(',')
                    .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
                    .ok_or_else(|| format!("--scroll expects <x>,<y>, got `{v}`"))?;
                scroll = (x, y);
            }
            "--screenshot" => screenshot = Some(PathBuf::from(value(&mut i)?)),
            "--animate" => animate = true,
            other if other.starts_with('-') => return Err(format!("unknown option `{other}`")),
            _ if game.is_none() => game = Some(PathBuf::from(arg)),
            other => return Err(format!("unexpected argument `{other}`")),
        }
        i += 1;
    }
    Ok(Options {
        game: game.ok_or("missing <game>")?,
        scene: scene.ok_or("missing --scene <file>")?,
        scroll,
        screenshot,
        animate,
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let options = match parse_args(&args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("luma-player: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("luma-player: {message}");
            ExitCode::FAILURE
        }
    }
}

fn open(game: &Path) -> Result<(Vfs, GameSettings), String> {
    let vfs = Vfs::open_game(game).map_err(|e| format!("{}: {e}", game.display()))?;
    if vfs.packages().is_empty() && !vfs.exists(luma_adventure::SETTINGS_FILE) {
        return Err(format!("{}: not a game directory", game.display()));
    }
    let settings = GameSettings::load(&vfs).map_err(|e| e.to_string())?;
    Ok((vfs, settings))
}

fn load(vfs: &Vfs, renderer: &Renderer, scene: &str) -> Result<SceneView, String> {
    let view = SceneView::load(vfs, renderer, scene).map_err(|e| e.to_string())?;
    eprintln!(
        "scene {:?}: {}x{}, {} layer(s), {} entities, {} texture(s)",
        view.scene.name,
        view.scene.width(),
        view.scene.height(),
        view.scene.layers.len(),
        view.scene.entities().count(),
        view.texture_count()
    );
    for (path, reason) in &view.missing {
        eprintln!("  missing: {path}: {reason}");
    }
    Ok(view)
}

fn run(options: Options) -> Result<(), String> {
    let (vfs, settings) = open(&options.game)?;
    let mut camera = Camera::new(settings.width as f32, settings.height as f32);
    camera.offset_x = options.scroll.0;
    camera.offset_y = options.scroll.1;

    if let Some(output) = &options.screenshot {
        let gpu = Gpu::headless().map_err(|e| e.to_string())?;
        eprintln!("adapter: {}", gpu.describe());
        let renderer = Renderer::new(gpu);
        let view = load(&vfs, &renderer, &options.scene)?;
        camera.clamp_to(&view.scene);
        let target = Offscreen::new(renderer.gpu(), settings.width, settings.height);
        let mut list = DrawList::new(camera.width, camera.height);
        view.draw(&mut list, &camera, 0);
        renderer
            .render(target.target(), BLACK, &list)
            .map_err(|e| e.to_string())?;
        let image = target.read(renderer.gpu()).map_err(|e| e.to_string())?;
        let png = image.to_png().map_err(|e| e.to_string())?;
        std::fs::write(output, png).map_err(|e| format!("{}: {e}", output.display()))?;
        eprintln!(
            "wrote {} ({}x{})",
            output.display(),
            image.width,
            image.height
        );
        return Ok(());
    }

    // Undocumented, for smoke tests: exit after that many frames.
    let frame_limit = std::env::var("LUMA_PLAYER_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok());
    let player = Player {
        frame_limit,
        vfs,
        scene: options.scene.clone(),
        animate: options.animate,
        camera,
        view: None,
        started: Instant::now(),
        keys: Keys::default(),
    };
    luma_render::window::run(
        WindowConfig {
            title: format!("Luma Engine - {}", options.scene),
            width: settings.width,
            height: settings.height,
            ..WindowConfig::default()
        },
        player,
    )
    .map_err(|e| e.to_string())
}

#[derive(Default)]
struct Keys {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
}

struct Player {
    frame_limit: Option<u64>,
    vfs: Vfs,
    scene: String,
    animate: bool,
    camera: Camera,
    view: Option<SceneView>,
    started: Instant,
    keys: Keys,
}

impl App for Player {
    fn init(&mut self, renderer: &Renderer) -> luma_render::Result<()> {
        eprintln!("adapter: {}", renderer.gpu().describe());
        let view = load(&self.vfs, renderer, &self.scene)
            .map_err(|reason| luma_render::Error::Render { reason })?;
        self.camera.clamp_to(&view.scene);
        self.view = Some(view);
        self.started = Instant::now();
        Ok(())
    }

    fn event(&mut self, event: &WindowEvent) -> Flow {
        if let WindowEvent::KeyboardInput {
            event: KeyEvent {
                logical_key, state, ..
            },
            ..
        } = event
        {
            let pressed = *state == ElementState::Pressed;
            match logical_key {
                Key::Named(NamedKey::Escape) => return Flow::Exit,
                Key::Named(NamedKey::ArrowLeft) => self.keys.left = pressed,
                Key::Named(NamedKey::ArrowRight) => self.keys.right = pressed,
                Key::Named(NamedKey::ArrowUp) => self.keys.up = pressed,
                Key::Named(NamedKey::ArrowDown) => self.keys.down = pressed,
                _ => {}
            }
        }
        Flow::Continue
    }

    fn draw<'a>(&'a mut self, frame: &Frame<'a>, list: &mut DrawList<'a>) -> Flow {
        let Some(view) = &self.view else {
            return Flow::Exit;
        };
        let step = 400.0 * frame.dt.as_secs_f32();
        if self.keys.left {
            self.camera.offset_x -= step;
        }
        if self.keys.right {
            self.camera.offset_x += step;
        }
        if self.keys.up {
            self.camera.offset_y -= step;
        }
        if self.keys.down {
            self.camera.offset_y += step;
        }
        self.camera.clamp_to(&view.scene);
        list.width = self.camera.width;
        list.height = self.camera.height;
        list.letterbox = true;
        let elapsed = if self.animate {
            self.started.elapsed().as_millis() as u64
        } else {
            0
        };
        view.draw(list, &self.camera, elapsed);
        match self.frame_limit {
            Some(limit) if frame.frame_index + 1 >= limit => Flow::Exit,
            _ => Flow::Continue,
        }
    }
}
