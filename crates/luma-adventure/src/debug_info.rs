//! The debug overlay (`CBGame::DisplayDebugInfo`, `CAdGame::
//! DisplayDebugInfo`, `CBRenderD3D::DisplayDebugInfo`): lines of text in
//! the system font, drawn over everything else after the quick
//! messages.
//!
//! Written with reference to `BGame.cpp`, `AdGame.cpp`, `BRenderD3D.cpp`
//! and `ScEngine.cpp` of Wintermute Engine 1.x (MIT License, Copyright
//! (c) 2003-2013 Jan Nedoma); see `NOTICE`.
//!
//! Two switches turn it on, both read from the `Debug` section of
//! `settings.ini` (or of the game's `wme.ini`) when the game starts, as
//! the original reads them from its registry: `ShowFPS` draws the frame
//! rate at the top left, and `DebugMode` draws, right-aligned, the
//! video mode and renderer, the polygon count, the scripts, the mouse
//! position, the scene names, the game timer and the name of the active
//! object. The front-ends can also set them (`luma-player --debug`,
//! `--show-fps`). Debug mode is what `Game.DebugMode` reads.

use luma_formats::font::TextAlign;
use luma_render::DrawList;
use luma_script::{Engine, Natives, State, Value};

use crate::world::World;

/// The name the renderer gives itself on the overlay
/// (`CBRenderer::GetName`; the original's is `Direct3D9`).
pub const RENDERER_NAME: &str = "wgpu";

/// The colour depth the overlay states for a full-screen mode
/// (`CBRenderer::m_BPP`): Luma always renders in 32 bits.
const BITS_PER_PIXEL: i32 = 32;

/// One line of the overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugLine {
    /// The text.
    pub text: String,
    /// The box the text is drawn in (`DrawText(text, x, y, width, align)`).
    pub x: i32,
    /// See `x`.
    pub y: i32,
    /// See `x`.
    pub width: i32,
    /// See `x`.
    pub align: TextAlign,
}

/// What the script engine counts for the overlay
/// (`CScEngine::GetNumScripts`): the scripts not finished, and how many
/// run, wait or stay persistent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScriptCounts {
    /// Scripts not finished.
    pub total: i32,
    /// Running, sleeping or paused.
    pub running: i32,
    /// Waiting for an object.
    pub waiting: i32,
    /// Persistent (their event handlers stay).
    pub persistent: i32,
}

impl ScriptCounts {
    /// Counts the scripts of an engine as `CScEngine::GetNumScripts`
    /// does: a finished script is skipped, running, sleeping and paused
    /// ones run, and the other states (errors, threads waiting for a
    /// thread) are in the total only.
    pub fn of(engine: &Engine) -> ScriptCounts {
        let mut counts = ScriptCounts::default();
        for script in engine.scripts() {
            match script.state() {
                State::Finished => continue,
                State::Running | State::Sleeping | State::Paused => counts.running += 1,
                State::Waiting => counts.waiting += 1,
                State::Persistent => counts.persistent += 1,
                _ => {}
            }
            counts.total += 1;
        }
        counts
    }
}

impl World {
    /// Whether the game runs in debug mode (`m_DEBUG_DebugMode`,
    /// `Game.DebugMode`).
    pub fn debug_mode(&self) -> bool {
        self.debug_mode
    }

    /// Switches debug mode (`CBGame::DEBUG_DebugEnable`) on or off.
    pub fn set_debug_mode(&mut self, on: bool) {
        self.debug_mode = on;
    }

    /// Whether the frame rate is drawn (`m_DEBUG_ShowFPS`).
    pub fn show_fps(&self) -> bool {
        self.show_fps
    }

    /// Shows or hides the frame rate.
    pub fn set_show_fps(&mut self, on: bool) {
        self.show_fps = on;
    }

    /// Records how many polygons the last frame drew, for the next
    /// frame's `PolyCount` line (`CBRenderD3D::m_NumPolygons`, which the
    /// original counts while the frame draws and shows after it).
    pub fn note_polygons(&self, polygons: u32) {
        self.polygons.set(polygons);
    }

    /// The lines of the overlay as the last frame left them, in the
    /// order the original draws them.
    pub fn debug_lines(&self) -> &[DebugLine] {
        &self.debug_lines
    }

    /// Lays out the overlay for the frame (`DisplayDebugInfo`'s text) and
    /// has the system font prepare it. Nothing is drawn unless
    /// [`World::show_fps`] or [`World::debug_mode`] is on.
    pub(crate) fn update_debug_info(&mut self, scripts: ScriptCounts) {
        self.debug_lines.clear();
        if !self.show_fps && !self.debug_mode {
            return;
        }
        let width = self.settings.width as i32;
        let mut lines = Vec::new();
        let mut line = |text: String, y: i32, width: i32, align: TextAlign| {
            lines.push(DebugLine {
                text,
                x: 0,
                y,
                width,
                align,
            });
        };
        if self.debug_mode {
            // `CAdGame::DisplayDebugInfo`, then `CBGame`'s.
            let (mx, my) = (self.mouse.x, self.mouse.y);
            let (viewport_x, viewport_y, _, _) = self.viewport();
            let (offset_x, offset_y) = self.offset();
            let scene = self
                .scene
                .as_ref()
                .map(|v| v.scene.name.as_str())
                .filter(|n| !n.is_empty())
                .unwrap_or("???");
            let previous = Some(self.prev_scene_name.as_str())
                .filter(|n| !n.is_empty())
                .unwrap_or("???");
            line(
                format!(
                    "Mouse: {mx}, {my} (scene: {}, {})",
                    mx + offset_x - viewport_x,
                    my + offset_y - viewport_y
                ),
                90,
                width,
                TextAlign::Right,
            );
            line(
                format!("Scene: {scene} (prev: {previous})"),
                110,
                width,
                TextAlign::Right,
            );
        }
        if self.show_fps {
            line(format!("FPS: {}", self.fps), 0, 100, TextAlign::Left);
        }
        if self.debug_mode {
            let mode = if self.windowed {
                format!("Mode: {width}x{} windowed", self.settings.height)
            } else {
                format!("Mode: {width}x{}x{BITS_PER_PIXEL}", self.settings.height)
            };
            line(
                format!("{mode} ({RENDERER_NAME})"),
                0,
                width,
                TextAlign::Right,
            );
            line(
                format!("PolyCount: {}", self.polygons.get()),
                20,
                width,
                TextAlign::Right,
            );
            line(
                format!(
                    "Running scripts: {} (r:{} w:{} p:{})",
                    scripts.total, scripts.running, scripts.waiting, scripts.persistent
                ),
                70,
                width,
                TextAlign::Right,
            );
            line(
                format!("Timer: {}", self.timer),
                130,
                width,
                TextAlign::Right,
            );
            if let Some(id) = self.active_object() {
                let name = match self.get_property(id, "Name") {
                    Some(Value::String(name)) => name,
                    _ => String::new(),
                };
                line(name, 150, width, TextAlign::Right);
            }
        }
        if let Some(font) = self
            .system_font
            .clone()
            .and_then(|p| self.fonts.get_mut(&p))
        {
            for l in &lines {
                font.prepare(&l.text, l.width, l.align, -1);
            }
        }
        self.debug_lines = lines;
    }

    /// Draws the overlay in the system font, over everything.
    pub(crate) fn draw_debug_info<'a>(&'a self, list: &mut DrawList<'a>) {
        let Some(font) = self.system_font.as_ref().and_then(|p| self.fonts.get(p)) else {
            return;
        };
        for l in &self.debug_lines {
            font.draw(list, &l.text, l.x, l.y, l.width, l.align, -1);
        }
    }
}
