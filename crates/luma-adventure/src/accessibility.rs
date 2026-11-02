//! Accessibility (`CBAccessMgr`): text-to-speech of captions, speech
//! and typed keys, and keyboard-only play, as the `Acc*` members of
//! `Game` and `AccCaption` of the objects expose them.
//!
//! Written with reference to `BAccessMgr.cpp`, `BGame.cpp`,
//! `BObject.cpp`, `UIObject.cpp`, `UIWindow.cpp`, `AdScene.cpp` and
//! `AdResponseBox.cpp` of Wintermute Engine 1.x (MIT License, Copyright
//! (c) 2003-2013 Jan Nedoma); see `NOTICE`.
//!
//! The original reads seven switches from its settings dialog (the
//! `Accessibility` section of its registry): the text-to-speech master
//! switch and which texts it reads (captions of the object under the
//! mouse, sentences without a speech file, keys typed in an editor),
//! and the keyboard switches (Ctrl+Tab cycles through the clickable
//! objects, the mouse cursor jumps to the object picked, Ctrl+Space
//! pauses the game). Luma reads the same keys from the `Accessibility`
//! section of `settings.ini` in the save directory (see
//! `docs/saving.md`), and speaks through a [`Speech`] backend: the
//! system's `say` command on macOS, nothing elsewhere; tests record.

use std::collections::VecDeque;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use luma_formats::Rgba;
use luma_render::{DrawList, Quad, Rect, Texture};
use luma_script::{Engine, NativeId, Value};

use crate::objects::Kind;
use crate::registry::{Ini, SETTINGS_INI};
use crate::scene_view::EntityRef;
use crate::ui::{ControlKind, StaticState};
use crate::world::World;

/// The section of `settings.ini` holding the switches, with the
/// original's key names (`TTSEnabled`, `TTSCaptions`, `TTSTalk`,
/// `TTSKeypress`, `KeyboardEnabled`, `KeyboardCursorSkip`,
/// `KeyboardPause`).
pub const SETTINGS_SECTION: &str = "Accessibility";

/// The Windows virtual key codes the keyboard switches use.
const VK_TAB: i32 = 9;
const VK_SPACE: i32 = 32;
const VK_CONTROL: i32 = 17;

/// What a spoken text is (`TTTSType`): each kind has its own switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpeechKind {
    /// The caption of the object under the mouse (`TTS_CAPTION`, 0).
    #[default]
    Caption,
    /// A sentence an object says without a speech file (`TTS_TALK`, 1).
    Talk,
    /// A key typed in an editor (`TTS_KEYPRESS`, 2).
    Keypress,
}

impl SpeechKind {
    /// The kind of a `Game.AccOutputText` type argument; an unknown
    /// value is a caption, as the original's enum cast lands there for
    /// its checks.
    pub fn from_code(code: i32) -> SpeechKind {
        match code {
            1 => SpeechKind::Talk,
            2 => SpeechKind::Keypress,
            _ => SpeechKind::Caption,
        }
    }
}

/// A text-to-speech backend (`ISpVoice` in the original).
pub trait Speech: Send {
    /// Speaks a text without blocking; with `purge`, what is being said
    /// and what waits are dropped first (`SPF_PURGEBEFORESPEAK`), else
    /// the text waits its turn.
    fn speak(&mut self, text: &str, purge: bool);

    /// Stops speaking and drops what waits.
    fn stop(&mut self);

    /// Once per frame: a backend speaking through a process starts the
    /// next waiting text when the current one ended.
    fn tick(&mut self) {}
}

/// What a [`RecordingSpeech`] was asked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpeechRecord {
    /// The texts spoken, with whether each dropped what came before.
    pub spoken: Vec<(String, bool)>,
    /// How many times speech was stopped.
    pub stops: usize,
}

/// A backend that records what it is asked, for tests: clones share
/// the record, so a test keeps one and gives the other to the game.
#[derive(Debug, Clone, Default)]
pub struct RecordingSpeech {
    record: Arc<Mutex<SpeechRecord>>,
}

impl RecordingSpeech {
    /// What was asked so far.
    pub fn record(&self) -> SpeechRecord {
        self.record.lock().map(|r| r.clone()).unwrap_or_default()
    }

    /// The texts spoken so far, in order.
    pub fn spoken(&self) -> Vec<String> {
        self.record().spoken.into_iter().map(|(t, _)| t).collect()
    }
}

impl Speech for RecordingSpeech {
    fn speak(&mut self, text: &str, purge: bool) {
        if let Ok(mut record) = self.record.lock() {
            record.spoken.push((text.to_string(), purge));
        }
    }

    fn stop(&mut self) {
        if let Ok(mut record) = self.record.lock() {
            record.stops += 1;
        }
    }
}

/// A backend speaking through a command that reads the text on its
/// standard input and returns when it finished saying it: macOS's `say`.
/// One process at a time; the texts that must not interrupt the current
/// one wait in a queue.
#[derive(Debug)]
pub struct CommandSpeech {
    program: String,
    child: Option<Child>,
    queue: VecDeque<String>,
}

impl CommandSpeech {
    /// A backend running `program`.
    pub fn new(program: &str) -> CommandSpeech {
        CommandSpeech {
            program: program.to_string(),
            child: None,
            queue: VecDeque::new(),
        }
    }

    /// The system's speech command: `say` on macOS, none elsewhere (the
    /// original's SAPI voice fails to initialise the same way when no
    /// voice is installed).
    pub fn system() -> Option<CommandSpeech> {
        if cfg!(target_os = "macos") {
            Some(CommandSpeech::new("say"))
        } else {
            None
        }
    }

    fn start(&mut self, text: &str) {
        let child = Command::new(&self.program)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else {
            return;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
            let _ = stdin.write_all(b"\n");
        }
        self.child = Some(child);
    }
}

impl Speech for CommandSpeech {
    fn speak(&mut self, text: &str, purge: bool) {
        if purge {
            self.stop();
        }
        self.tick();
        if self.child.is_some() {
            self.queue.push_back(text.to_string());
        } else {
            self.start(text);
        }
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.queue.clear();
    }

    fn tick(&mut self) {
        if let Some(child) = &mut self.child {
            if matches!(child.try_wait(), Ok(Some(_)) | Err(_)) {
                self.child = None;
            }
        }
        if self.child.is_none() {
            if let Some(next) = self.queue.pop_front() {
                self.start(&next);
            }
        }
    }
}

impl Drop for CommandSpeech {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The accessibility switches and state of a game.
#[derive(Default)]
pub struct Accessibility {
    /// `AccTTSEnabled`: text-to-speech on; off by default.
    pub tts_enabled: bool,
    /// `AccTTSTalk`: sentences without a speech file are read out.
    pub tts_talk: bool,
    /// `AccTTSCaptions`: the caption of the object under the mouse is
    /// read out.
    pub tts_captions: bool,
    /// `AccTTSKeypress`: keys typed in an editor are read out.
    pub tts_keypress: bool,
    /// `AccKeyboardEnabled`: Ctrl+Tab and Ctrl+Shift+Tab cycle through
    /// the clickable objects, the one picked outlined in red while Ctrl
    /// is held; off by default.
    pub keyboard_enabled: bool,
    /// `AccKeyboardCursorSkip`: the mouse cursor jumps to the object
    /// picked.
    pub keyboard_cursor_skip: bool,
    /// `AccKeyboardPause`: Ctrl+Space pauses the game under a shield
    /// window; off by default.
    pub keyboard_pause: bool,
    /// The speech backend; `None` when text-to-speech is unavailable
    /// (`m_TTSAvailable`).
    speech: Option<Box<dyn Speech>>,
    /// The kind of the last text spoken (`m_PlayingType`): a sentence
    /// is not interrupted by the next one.
    playing: SpeechKind,
    /// The object picked with the keyboard (`m_ActiveObject`).
    pub active: Option<NativeId>,
    /// The object the cursor last jumped to (`m_PrevActiveObject`).
    prev_active: Option<NativeId>,
    /// Where the picked object is on screen this frame (`m_HintRect`):
    /// left, top, right, bottom.
    pub hint: Option<(i32, i32, i32, i32)>,
    /// Whether Ctrl+Space paused the game (`m_AccessGlobalPaused`).
    pub paused: bool,
    /// The shield window of the pause (`m_AccessShieldWin`).
    pub shield: Option<NativeId>,
}

impl Accessibility {
    /// The original's defaults: everything off but the three
    /// text-to-speech kinds and the cursor skip, which apply once their
    /// master switch is on.
    pub fn new() -> Accessibility {
        Accessibility {
            tts_talk: true,
            tts_captions: true,
            tts_keypress: true,
            keyboard_cursor_skip: true,
            ..Accessibility::default()
        }
    }

    /// Reads the switches from the `Accessibility` section of a
    /// settings file, keeping the current value of a key that is absent
    /// (`CBRegistry::ReadBool` with the current value as default). When
    /// the game forbids the accessibility tab (`ALLOW_ACCESSIBILITY_TAB`
    /// false), speech, keyboard play and the pause stay off, as the
    /// original forces them.
    pub fn read_settings(&mut self, ini: &Ini, allow_tab: bool) {
        let read = |key: &str, value: &mut bool| {
            if let Some(text) = ini.get(SETTINGS_SECTION, key) {
                *value = parse_bool(text, *value);
            }
        };
        read("TTSEnabled", &mut self.tts_enabled);
        read("TTSCaptions", &mut self.tts_captions);
        read("TTSTalk", &mut self.tts_talk);
        read("TTSKeypress", &mut self.tts_keypress);
        read("KeyboardEnabled", &mut self.keyboard_enabled);
        read("KeyboardCursorSkip", &mut self.keyboard_cursor_skip);
        read("KeyboardPause", &mut self.keyboard_pause);
        if !allow_tab {
            self.tts_enabled = false;
            self.keyboard_enabled = false;
            self.keyboard_pause = false;
        }
    }

    /// Gives the speech backend (`CBAccessMgr::Initialize`); it is used
    /// while `tts_enabled` is on.
    pub fn set_speech(&mut self, speech: Option<Box<dyn Speech>>) {
        self.speech = speech;
    }

    /// Whether a backend is there and text-to-speech is on.
    pub fn tts_available(&self) -> bool {
        self.tts_enabled && self.speech.is_some()
    }

    /// Speaks a text of a kind (`CBAccessMgr::Speak`): nothing without a
    /// backend, when the kind's switch is off or when the text is empty.
    /// A text interrupts what plays unless the last text spoken was a
    /// sentence, which nothing cuts short.
    pub fn speak(&mut self, text: &str, kind: SpeechKind) {
        if !self.tts_enabled {
            return;
        }
        let allowed = match kind {
            SpeechKind::Caption => self.tts_captions,
            SpeechKind::Talk => self.tts_talk,
            SpeechKind::Keypress => self.tts_keypress,
        };
        if !allowed || text.is_empty() {
            return;
        }
        let purge = self.playing != SpeechKind::Talk;
        if let Some(speech) = &mut self.speech {
            speech.speak(text, purge);
            self.playing = kind;
        }
    }

    /// Stops speaking (`CBAccessMgr::Stop`), when the sentences are
    /// skipped.
    pub fn stop(&mut self) {
        if !self.tts_enabled {
            return;
        }
        if let Some(speech) = &mut self.speech {
            speech.stop();
            self.playing = SpeechKind::Caption;
        }
    }

    /// Once per frame: lets the backend go on with what waits.
    pub(crate) fn tick(&mut self) {
        if let Some(speech) = &mut self.speech {
            speech.tick();
        }
    }

    /// Forgets the object picked with the keyboard
    /// (`SetActiveObject(NULL)`), when a window takes the screen.
    pub(crate) fn clear_active(&mut self) {
        self.active = None;
        self.prev_active = None;
        self.hint = None;
    }
}

/// `true`, `yes`, `on` or a non-zero number; else `false`; `default`
/// for anything unreadable.
fn parse_bool(text: &str, default: bool) -> bool {
    let text = text.trim();
    match text.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" => true,
        "false" | "no" | "off" => false,
        other => other.parse::<i64>().map_or(default, |n| n != 0),
    }
}

impl World {
    /// The accessibility switches and state.
    pub fn accessibility(&self) -> &Accessibility {
        &self.access
    }

    /// The accessibility switches and state, mutable: a front-end sets
    /// the switches from its own options and the speech backend
    /// ([`Accessibility::set_speech`]).
    pub fn accessibility_mut(&mut self) -> &mut Accessibility {
        &mut self.access
    }

    /// Reads the switches from `settings.ini` and, when text-to-speech is
    /// on, takes the system's speech command (`CBAccessMgr::Initialize`
    /// after the settings are read). Called when the game starts.
    pub(crate) fn init_accessibility(&mut self) {
        let path = self.save_directory().join(SETTINGS_INI);
        let ini = std::fs::read_to_string(path)
            .map(|text| Ini::parse(&text))
            .unwrap_or_default();
        let allow_tab = self.game_file.allow_accessibility_tab;
        self.access.read_settings(&ini, allow_tab);
        if self.access.tts_enabled && self.access.speech.is_none() {
            self.access
                .set_speech(CommandSpeech::system().map(|s| Box::new(s) as Box<dyn Speech>));
        }
    }

    /// `AccCaption` of an object: the text the speech reads for it in
    /// place of its caption; `None` when unset.
    pub fn access_caption(&self, id: NativeId) -> Option<&str> {
        self.object_state(id)?.access_caption.as_deref()
    }

    /// Sets or clears `AccCaption` of an object.
    pub fn set_access_caption(&mut self, id: NativeId, caption: Option<String>) {
        if let Some(state) = self.object_state_mut(id) {
            state.access_caption = caption;
        }
    }

    /// Whether an object has an `AccCaption` (`CBObject`: the game,
    /// entities, actors, items, windows and controls, `new Object`s).
    pub(crate) fn has_access_caption(&self, id: NativeId) -> bool {
        matches!(
            self.native(id).map(|n| &n.kind),
            Some(
                Kind::Game
                    | Kind::Entity(_)
                    | Kind::Actor
                    | Kind::Actor3D
                    | Kind::Item
                    | Kind::Control
                    | Kind::Object
            )
        )
    }

    /// The `AccCaption` property as scripts read it.
    pub(crate) fn access_caption_value(&self, id: NativeId) -> Value {
        self.access_caption(id).map_or(Value::Null, Value::from)
    }

    /// What the speech reads for an object (`GetAccessCaption`): its
    /// `AccCaption`, else a control's text, else its caption.
    pub fn spoken_caption(&self, id: NativeId) -> String {
        if let Some(caption) = self.access_caption(id) {
            return caption.to_string();
        }
        let caption = match self.native(id).map(|n| &n.kind) {
            Some(Kind::Game) => Some(self.caption.clone()),
            Some(Kind::Entity(r)) => self.entity_def(*r).map(|e| e.caption.clone()),
            Some(Kind::Actor) => self.actor(id).map(|a| a.def.caption.clone()),
            #[cfg(feature = "3d")]
            Some(Kind::Actor3D) => self.actor3d(id).map(|a| a.caption.clone()),
            Some(Kind::Item) => self.item(id).map(|i| i.def.caption.clone()),
            Some(Kind::Control) => self.control(id).map(|c| {
                if c.text.is_empty() {
                    c.caption.clone()
                } else {
                    c.text.clone()
                }
            }),
            _ => None,
        }
        .unwrap_or_default();
        self.expand(&caption)
    }

    /// Reads an object's caption out (`SetActiveObject`), when the mouse
    /// comes over it.
    pub(crate) fn speak_caption_of(&mut self, id: NativeId) {
        if !self.access.tts_available() {
            return;
        }
        let caption = self.spoken_caption(id);
        self.access.speak(&caption, SpeechKind::Caption);
    }

    /// Speaks a text (`Game.AccOutputText`, a sentence without a speech
    /// file, a key typed).
    pub fn access_speak(&mut self, text: &str, kind: SpeechKind) {
        self.access.speak(text, kind);
    }

    // ----------------------------------------------------------------
    // Keyboard play
    // ----------------------------------------------------------------

    /// A key going down, before anything else sees it
    /// (`CBGame::HandleAccessKey`): Ctrl+Tab picks the next clickable
    /// object, Ctrl+Shift+Tab the previous one, Ctrl+Space pauses and
    /// resumes the game. Returns whether the key was taken.
    pub(crate) fn access_key(&mut self, engine: &mut Engine, code: i32) -> bool {
        let control = self.keyboard.control || self.keyboard.is_key_down(VK_CONTROL);
        if self.access.keyboard_enabled && code == VK_TAB && control {
            let forward = !self.keyboard.shift;
            self.access.active = self.next_access_object(self.access.active, forward);
            return true;
        }
        if self.access.keyboard_pause && code == VK_SPACE && control {
            if self.access.paused {
                self.access_unpause(engine);
            } else {
                self.access_pause(engine);
            }
            return true;
        }
        false
    }

    /// The objects the keyboard cycles through, in order
    /// (`GetNextAccessObject`): the buttons and editors of the focused
    /// window (into its child windows, the disabled ones skipped); else
    /// the response buttons when the dialogue waits for a pick; else the
    /// scene's active, interactive entities in layer order (a close-up
    /// layer hides what is below it) and the active, interactive actors.
    pub fn access_objects(&self) -> Vec<NativeId> {
        if let Some(window) = self.focused_window() {
            let controls = self.window_access_objects(window);
            if !controls.is_empty() {
                return controls;
            }
        }
        if self.responses_visible() {
            let mut objects = self.responses.buttons.clone();
            if let Some(window) = self.responses.window {
                objects.extend(self.window_access_objects(window));
            }
            if !objects.is_empty() {
                return objects;
            }
        }
        let mut objects = Vec::new();
        if let Some(view) = &self.scene {
            for (layer_index, layer) in view.scene.layers.iter().enumerate() {
                if layer.close_up {
                    objects.clear();
                }
                for (index, node) in layer.nodes.iter().enumerate() {
                    let luma_formats::scene::SceneNode::Entity(entity) = node else {
                        continue;
                    };
                    if !entity.active || !entity.interactive {
                        continue;
                    }
                    let r = EntityRef::Node {
                        layer: layer_index,
                        index,
                    };
                    if let Some(id) = self.node_id(r) {
                        objects.push(id);
                    }
                }
            }
            for (id, actor) in &self.actors {
                if actor.def.active && actor.def.interactive && !objects.contains(id) {
                    objects.push(*id);
                }
            }
        }
        objects
    }

    /// The buttons and editors under a window, depth first
    /// (`CUIWindow::GetWindowObjects` with `InteractiveOnly`).
    fn window_access_objects(&self, window: NativeId) -> Vec<NativeId> {
        let mut out = Vec::new();
        for widget in self.widgets_of(window) {
            let Some(control) = self.control(widget) else {
                continue;
            };
            if control.disabled {
                continue;
            }
            match control.kind {
                ControlKind::Window(_) => out.extend(self.window_access_objects(widget)),
                ControlKind::Button(_) | ControlKind::Edit(_) => out.push(widget),
                _ => {}
            }
        }
        out
    }

    /// The object after (or before) `current` in the cycle; the first
    /// (or last) when `current` is not in it or ends it. `None` when
    /// there is nothing to pick.
    fn next_access_object(&self, current: Option<NativeId>, forward: bool) -> Option<NativeId> {
        let objects = self.access_objects();
        let position = current.and_then(|c| objects.iter().position(|o| *o == c));
        let pick = match (position, forward) {
            (Some(i), true) if i + 1 < objects.len() => i + 1,
            (Some(i), false) if i > 0 => i - 1,
            (_, true) => 0,
            (_, false) => objects.len().saturating_sub(1),
        };
        objects.get(pick).copied()
    }

    /// Once per frame, after the hit areas are known
    /// (`CBAccessMgr::DisplayInternal`): notes where the picked object is
    /// and, with the cursor skip, moves the mouse to its centre once per
    /// pick. The picked object is forgotten when it goes.
    pub(crate) fn update_accessibility(&mut self) {
        self.access.tick();
        if !self.access.keyboard_enabled {
            return;
        }
        let Some(active) = self.access.active else {
            self.access.hint = None;
            return;
        };
        if !self.objects.contains(active) {
            self.access.clear_active();
            return;
        }
        let rect = self.hit_rect(active).or_else(|| {
            let control = self.control(active)?;
            let (x, y) = self.control_screen_position(active);
            Some((x, y, control.width, control.height))
        });
        let Some((x, y, w, h)) = rect else {
            self.access.hint = None;
            return;
        };
        self.access.hint = Some((x, y, x + w, y + h));
        if self.access.keyboard_cursor_skip && self.access.prev_active != Some(active) {
            self.access.prev_active = Some(active);
            let (sw, sh) = (self.settings.width as i32, self.settings.height as i32);
            let cx = (x + w / 2).clamp(0, sw - 1);
            let cy = (y + h / 2).clamp(0, sh - 1);
            self.set_mouse(cx, cy);
        }
    }

    /// Draws the red frame around the picked object while Ctrl is held
    /// (`DrawRect(..., 0xFFFF0000, 4)`).
    pub(crate) fn draw_access_hint<'a>(&self, list: &mut DrawList<'a>, white: &'a Texture) {
        if !self.access.keyboard_enabled || !self.keyboard.is_key_down(VK_CONTROL) {
            return;
        }
        let Some((left, top, right, bottom)) = self.access.hint else {
            return;
        };
        let (l, t, r, b) = (left as f32, top as f32, right as f32, bottom as f32);
        let width = 4.0;
        let red = Rgba {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        };
        for dst in [
            Rect::new(l, t, r - l, width),
            Rect::new(l, b - width, r - l, width),
            Rect::new(l, t, width, b - t),
            Rect::new(r - width, t, width, b - t),
        ] {
            list.push(Quad {
                texture: white,
                dst,
                src: None,
                tint: red,
                flip_x: false,
                flip_y: false,
            });
        }
    }

    /// Pauses the game under a shield window (`CBGame::AccessPause`): a
    /// system-exclusive window covering the screen, with "Game paused.
    /// Press Ctrl+Space to resume." in the system font at the bottom.
    pub(crate) fn access_pause(&mut self, engine: &mut Engine) {
        if let Some(old) = self.access.shield.take() {
            self.unload_control(engine, old);
        }
        self.access.paused = true;
        let (sw, sh) = (self.settings.width as i32, self.settings.height as i32);
        let window = self.create_window(None);
        if let Some(control) = self.control_mut(window) {
            control.width = sw;
            control.height = sh;
        }
        let text = self.expand("/SYSENG0040/Game paused. Press Ctrl+Space to resume.");
        let label = self.create_control(window, ControlKind::Static(StaticState::default()), None);
        let font = self.system_font.clone();
        if let Some(control) = self.control_mut(label) {
            control.text = text;
            control.font = font;
        }
        self.size_static_to_fit(label, true);
        if let Some(control) = self.control_mut(label) {
            control.y = sh - control.height;
            control.x = (sw - control.width) / 2;
        }
        self.go_system_exclusive(engine, window);
        self.access.shield = Some(window);
    }

    /// Resumes the game (`CBGame::AccessUnpause`).
    pub(crate) fn access_unpause(&mut self, engine: &mut Engine) {
        self.access.paused = false;
        if let Some(window) = self.access.shield.take() {
            self.close_window(engine, window);
            self.unload_control(engine, window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_and_speech_switches() {
        let mut access = Accessibility::new();
        assert!(!access.tts_enabled && access.tts_talk && access.tts_captions);
        assert!(!access.keyboard_enabled && access.keyboard_cursor_skip && !access.keyboard_pause);
        let ini = Ini::parse(
            "[Accessibility]\nTTSEnabled=1\nTTSCaptions=false\nKeyboardEnabled=yes\nKeyboardPause=1\n",
        );
        access.read_settings(&ini, true);
        assert!(access.tts_enabled && !access.tts_captions && access.tts_talk);
        assert!(access.keyboard_enabled && access.keyboard_pause);
        access.read_settings(&ini, false);
        assert!(!access.tts_enabled && !access.keyboard_enabled && !access.keyboard_pause);

        let mut access = Accessibility::new();
        let recording = RecordingSpeech::default();
        access.set_speech(Some(Box::new(recording.clone())));
        access.speak("off", SpeechKind::Caption);
        access.tts_enabled = true;
        access.speak("", SpeechKind::Caption);
        access.speak("door", SpeechKind::Caption);
        access.speak("Hello there.", SpeechKind::Talk);
        access.speak("And more.", SpeechKind::Talk);
        access.speak("k", SpeechKind::Keypress);
        access.tts_captions = false;
        access.speak("window", SpeechKind::Caption);
        access.stop();
        access.speak("x", SpeechKind::Talk);
        access.tts_enabled = false;
        access.speak("silent", SpeechKind::Talk);
        let record = recording.record();
        assert_eq!(
            record.spoken,
            [
                ("door".to_string(), true),
                ("Hello there.".to_string(), true),
                ("And more.".to_string(), false),
                ("k".to_string(), false),
                ("x".to_string(), true),
            ]
        );
        assert_eq!(record.stops, 1);
    }

    #[test]
    fn speech_kinds_and_booleans() {
        assert_eq!(SpeechKind::from_code(0), SpeechKind::Caption);
        assert_eq!(SpeechKind::from_code(1), SpeechKind::Talk);
        assert_eq!(SpeechKind::from_code(2), SpeechKind::Keypress);
        assert_eq!(SpeechKind::from_code(9), SpeechKind::Caption);
        assert!(parse_bool("1", false) && parse_bool("True", false) && parse_bool(" on ", false));
        assert!(!parse_bool("0", true) && !parse_bool("no", true));
        assert!(parse_bool("maybe", true) && !parse_bool("maybe", false));
    }
}
