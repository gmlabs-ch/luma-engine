//! Mouse and keyboard: the object under the cursor, the cursor shown,
//! the key state scripts read (see `docs/game-loop.md`).
//!
//! Written with reference to `BGame.cpp`, `AdGame.cpp`, `BRenderer.cpp`,
//! `BSubFrame.cpp`, `AdEntity.cpp`, `AdActor.cpp` and
//! `BKeyboardState.cpp` of WME Lite (MIT License, Copyright (c) 2011 Jan
//! Nedoma); see `NOTICE`.

use std::collections::{HashMap, HashSet};

use luma_formats::scene::Point;
use luma_formats::{Rgb, Sprite};
use luma_render::{DrawList, Quad, Rect};
use luma_script::NativeId;

use crate::objects::Kind;
use crate::scene_view::{AnimState, DrawItem, EntityRef, Mask, SpriteBank, Transform, key};
use crate::world::World;

/// The mouse as scripts see it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Mouse {
    /// Position in game pixels (`Game.MouseX`).
    pub x: i32,
    /// See `x`.
    pub y: i32,
    /// Whether the left button is held.
    pub left_down: bool,
    /// The last left click: wall clock and position, for double clicks.
    pub last_left_click: Option<(u64, i32, i32)>,
    /// The last right click, for double clicks.
    pub last_right_click: Option<(u64, i32, i32)>,
}

/// The keyboard as scripts see it (`Game.Keyboard`).
///
/// A key press reaches the engine as two messages, as on the engine's
/// home platform: the key going down, with its virtual key code and not
/// printable, then the character it types, if any, printable, with the
/// character's code. Each overwrites the state the last one left, and a
/// `Keypress` handler runs after both: it sees the character of a key
/// that types one (`KeyCode` 97 and `Key` "a" for the A key), the key
/// code of one that does not (112 for F1).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Keyboard {
    /// The code of the last message (`KeyCode`): the Windows virtual
    /// key code of a key going down, or the code of the character
    /// typed (see [`char_code`]).
    pub key_code: i32,
    /// The character of the last message when it was a character
    /// (`Key`), else empty.
    pub key: String,
    /// Whether the last message was a character (`Printable`).
    pub printable: bool,
    /// Modifier state at the last message.
    pub shift: bool,
    /// See `shift`.
    pub alt: bool,
    /// See `shift`.
    pub control: bool,
    /// The keys and mouse buttons held, by virtual key code (the mouse
    /// buttons are 1, 2 and 4: `VK_LBUTTON`, `VK_RBUTTON`, `VK_MBUTTON`).
    pub down: HashSet<i32>,
}

impl Keyboard {
    /// Whether a key or a mouse button is held (`IsKeyDown`), by code; a
    /// letter's code may be given in either case.
    pub fn is_key_down(&self, code: i32) -> bool {
        let code = match u8::try_from(code) {
            Ok(c) if c.is_ascii_lowercase() => i32::from(c.to_ascii_uppercase()),
            _ => code,
        };
        self.down.contains(&code)
    }
}

/// Where the editors copy to and paste from (Ctrl+C, Ctrl+X, Ctrl+V in
/// `CUIEdit::HandleKeypress`). The engine has no platform code: a
/// front-end hands the system's clipboard to
/// [`World::set_clipboard`]; until then the world keeps the text
/// itself ([`MemoryClipboard`]).
pub trait Clipboard {
    /// The text on the clipboard, `None` when it holds none or cannot
    /// be read.
    fn get_text(&mut self) -> Option<String>;
    /// Puts a text on the clipboard; whether it could.
    fn set_text(&mut self, text: &str) -> bool;
}

/// A clipboard of the game's own: what one editor copies, another
/// pastes, and nothing leaves the game.
#[derive(Debug, Default)]
pub struct MemoryClipboard(Option<String>);

impl Clipboard for MemoryClipboard {
    fn get_text(&mut self) -> Option<String> {
        self.0.clone()
    }

    fn set_text(&mut self, text: &str) -> bool {
        self.0 = Some(text.to_string());
        true
    }
}

/// `VK_LBUTTON`: the left mouse button among the held keys.
pub const VK_LBUTTON: i32 = 1;
/// `VK_RBUTTON`: the right mouse button among the held keys.
pub const VK_RBUTTON: i32 = 2;
/// `VK_MBUTTON`: the middle mouse button among the held keys.
pub const VK_MBUTTON: i32 = 4;

/// The code of a typed character as `Keyboard.KeyCode` gives it: the
/// character's byte in Windows-1252, the code page the original engine
/// receives characters in on a western system (`a` is 97, `é` 233, `€`
/// 128); the Unicode code point of a character the code page lacks.
pub fn char_code(ch: char) -> i32 {
    match crate::text::ansi_byte(ch) {
        Some(byte) => i32::from(byte),
        None => u32::from(ch) as i32,
    }
}

/// The character Windows sends after a key going down when the key
/// types none of its own (what `TranslateMessage` adds on a US layout):
/// Backspace, Tab, Enter, Escape and the space bar give their control
/// character or a space; with Ctrl, Backspace gives 127, Enter a line
/// feed, Tab nothing, and a letter the control character of that
/// letter (Ctrl+C is 3) unless Alt is held too.
pub fn translated_char(code: i32, alt: bool, control: bool) -> Option<char> {
    match code {
        8 if control => Some('\u{7f}'),
        8 => Some('\u{8}'),
        9 if control => None,
        9 => Some('\t'),
        13 if control => Some('\n'),
        13 => Some('\r'),
        27 => Some('\u{1b}'),
        32 => Some(' '),
        65..=90 if control && !alt => char::from_u32((code - 64) as u32),
        _ => None,
    }
}

/// The keys of a US keyboard that type a character besides the letters
/// and the digits: the character alone, the character with Shift, the
/// virtual key code.
const US_KEYS: &[(char, char, i32)] = &[
    (' ', ' ', 32),
    ('0', ')', 48),
    ('1', '!', 49),
    ('2', '@', 50),
    ('3', '#', 51),
    ('4', '$', 52),
    ('5', '%', 53),
    ('6', '^', 54),
    ('7', '&', 55),
    ('8', '*', 56),
    ('9', '(', 57),
    (';', ':', 186),
    ('=', '+', 187),
    (',', '<', 188),
    ('-', '_', 189),
    ('.', '>', 190),
    ('/', '?', 191),
    ('`', '~', 192),
    ('[', '{', 219),
    ('\\', '|', 220),
    (']', '}', 221),
    ('\'', '"', 222),
];

/// The key that types a character on a US keyboard: its virtual key
/// code and whether Shift is held; `None` for a character the layout
/// lacks. For tooling that types a text key by key.
pub fn key_for_char(ch: char) -> Option<(i32, bool)> {
    if ch.is_ascii_lowercase() {
        return Some((i32::from(ch.to_ascii_uppercase() as u8), false));
    }
    if ch.is_ascii_uppercase() {
        return Some((i32::from(ch as u8), true));
    }
    US_KEYS.iter().find_map(|&(plain, shifted, code)| {
        if ch == plain {
            Some((code, false))
        } else if ch == shifted {
            Some((code, true))
        } else {
            None
        }
    })
}

/// The character a key types on a US keyboard, with or without Shift;
/// `None` for a key that types none. The reverse of [`key_for_char`].
pub fn char_for_key(code: i32, shift: bool) -> Option<char> {
    if (65..=90).contains(&code) {
        let letter = char::from(code as u8);
        return Some(if shift {
            letter
        } else {
            letter.to_ascii_lowercase()
        });
    }
    US_KEYS
        .iter()
        .find(|k| k.2 == code)
        .map(|&(plain, shifted, _)| if shift { shifted } else { plain })
}

/// Which bank holds the image of a hit area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitBank {
    Scene,
    Actor(NativeId),
    Ui,
    /// The bank of the game's own entities and attachments.
    Game,
}

/// The area of a drawn sub-frame, as the engine registers it
/// (`CBActiveRect` from `CBSubFrame::Draw`) and tests it
/// (`CBRenderer::GetObjectAt`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PixelArea {
    /// Where the sub-frame is drawn before rotation, in layer
    /// coordinates: `(x, y, width, height)`. A zero size means the size
    /// is the image's, scaled, known only once the image is decoded.
    pub dst: (i32, i32, i32, i32),
    /// The part of the image drawn, `None` for the whole image.
    pub src: Option<(i32, i32, i32, i32)>,
    /// Scale in percent on each axis.
    pub zoom_x: f32,
    /// See `zoom_x`.
    pub zoom_y: f32,
    /// Rotation in degrees, and the point the sub-frame turns around (the
    /// hotspot), in layer coordinates.
    pub rotation: f32,
    /// See `rotation`.
    pub pivot: (f32, f32),
    /// Mirrored horizontally.
    pub mirror_x: bool,
    /// Mirrored vertically.
    pub mirror_y: bool,
    /// Only the opaque pixels count (`PRECISE`).
    pub precise: bool,
}

impl PixelArea {
    /// The point of the unrotated sub-frame a point of the layer falls
    /// on: the sub-frame is drawn turned around its pivot, so the point
    /// is turned back. The engine's own test ignores the rotation and
    /// hits the sprite where it would be unrotated; Luma follows the
    /// transform, so a turned sprite is hit where it is seen.
    fn unrotate(&self, x: i32, y: i32) -> (i32, i32) {
        if self.rotation == 0.0 {
            return (x, y);
        }
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let (px, py) = self.pivot;
        // The centre of the pixel, so that the rotation does not shift
        // the sampled texel.
        let (dx, dy) = (x as f32 + 0.5 - px, y as f32 + 0.5 - py);
        (
            (px + dx * cos + dy * sin).floor() as i32,
            (py - dx * sin + dy * cos).floor() as i32,
        )
    }

    /// The rectangle drawn before rotation, in layer coordinates, its
    /// size taken from the image when the sub-frame has no rectangle.
    fn rect(&self, mask: Option<&Mask>) -> (i32, i32, i32, i32) {
        let (dx, dy, mut dw, mut dh) = self.dst;
        if dw == 0 || dh == 0 {
            let (sw, sh) = match (self.src, mask) {
                (Some((_, _, w, h)), _) => (w, h),
                (None, Some(mask)) => (mask.width as i32, mask.height as i32),
                (None, None) => (0, 0),
            };
            dw = (sw as f32 * self.zoom_x / 100.0) as i32;
            dh = (sh as f32 * self.zoom_y / 100.0) as i32;
        }
        (dx, dy, dw, dh)
    }

    /// Whether a point of the layer hits the sub-frame: inside its
    /// rectangle and, when precise, on an opaque pixel of the image,
    /// found as `CBRenderer::GetObjectAt` finds it, with the scale and
    /// the mirroring undone.
    pub fn contains(&self, mask: &Mask, x: i32, y: i32) -> bool {
        let (lx, ly) = self.unrotate(x, y);
        let (sx, sy, sw, sh) = self
            .src
            .unwrap_or((0, 0, mask.width as i32, mask.height as i32));
        let (dx, dy, dw, dh) = self.rect(Some(mask));
        if lx < dx || ly < dy || lx >= dx + dw || ly >= dy + dh {
            return false;
        }
        if !self.precise {
            return true;
        }
        let mut xx = ((sx + lx - dx) as f32 / (self.zoom_x / 100.0)) as i32;
        let mut yy = ((sy + ly - dy) as f32 / (self.zoom_y / 100.0)) as i32;
        if self.mirror_x {
            xx = sw - xx;
        }
        if self.mirror_y {
            yy = sh - yy;
        }
        mask.is_opaque(xx, yy)
    }

    /// The centre of the sub-frame as drawn, in layer coordinates, or
    /// `None` when its size is unknown or empty: the centre of
    /// [`PixelArea::bounds`], which `World::hit_point` takes.
    #[cfg(test)]
    pub fn center(&self, mask: Option<&Mask>) -> Option<(i32, i32)> {
        let (x, y, w, h) = self.bounds(mask)?;
        Some((x + w / 2, y + h / 2))
    }

    /// The bounding box of the sub-frame as drawn, in layer coordinates
    /// (`x`, `y`, `width`, `height`): its rectangle, or the box around
    /// the four corners once turned, whose centre is the turned centre;
    /// `None` when its size is unknown or empty.
    pub fn bounds(&self, mask: Option<&Mask>) -> Option<(i32, i32, i32, i32)> {
        let (dx, dy, dw, dh) = self.rect(mask);
        if dw <= 0 || dh <= 0 {
            return None;
        }
        if self.rotation == 0.0 {
            return Some((dx, dy, dw, dh));
        }
        let (l, t, r, b) = (dx as f32, dy as f32, (dx + dw) as f32, (dy + dh) as f32);
        let corners = [(l, t), (r, t), (l, b), (r, b)].map(|(x, y)| self.rotate(x, y));
        let min_x = corners.iter().map(|c| c.0).fold(f32::INFINITY, f32::min);
        let max_x = corners
            .iter()
            .map(|c| c.0)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = corners.iter().map(|c| c.1).fold(f32::INFINITY, f32::min);
        let max_y = corners
            .iter()
            .map(|c| c.1)
            .fold(f32::NEG_INFINITY, f32::max);
        Some((
            min_x.floor() as i32,
            min_y.floor() as i32,
            (max_x - min_x).ceil() as i32,
            (max_y - min_y).ceil() as i32,
        ))
    }

    /// A point of the layer turned by the rotation around the pivot.
    fn rotate(&self, x: f32, y: f32) -> (f32, f32) {
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let (px, py) = self.pivot;
        let (ox, oy) = (x - px, y - py);
        (px + ox * cos - oy * sin, py + ox * sin + oy * cos)
    }
}

/// Where an object can be hit, as the engine registers it while drawing
/// (`CBActiveRect`).
#[derive(Debug, Clone, PartialEq)]
enum HitShape {
    /// A sub-frame of an image, possibly scaled, turned or mirrored;
    /// when precise, only its opaque pixels count.
    Pixels {
        bank: HitBank,
        image: String,
        color_key: Rgb,
        area: PixelArea,
    },
    /// A polygon in scene coordinates.
    Polygon(Vec<Point>),
    /// A rectangle on screen (a window, a button).
    Rect(i32, i32, i32, i32),
    /// The projected bounding rectangle of a 3D actor's model, in layer
    /// coordinates (left, top, right, bottom); a point inside is
    /// confirmed by a pick into the model (`CXModel::IsTransparentAt`).
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    Model3D(i32, i32, i32, i32),
}

/// An object's hit area.
#[derive(Debug, Clone, PartialEq)]
pub struct HitArea {
    owner: NativeId,
    /// The scroll offset of the layer the object is drawn in.
    layer_offset: (i32, i32),
    shape: HitShape,
    /// The viewport in force when the area was registered, on screen
    /// (left, top, right, bottom): the area takes the mouse only inside
    /// it (`CBActiveRect::ClipRect`). `None` outside any viewport.
    clip: Option<(i32, i32, i32, i32)>,
}

/// Limits the hit areas registered from `start` on to a viewport, those
/// an inner viewport already limited excepted (a viewport replaces the
/// one under it while it is pushed).
pub(crate) fn clip_hits(hits: &mut [HitArea], start: usize, clip: (i32, i32, i32, i32)) {
    for hit in hits.iter_mut().skip(start) {
        if hit.clip.is_none() {
            hit.clip = Some(clip);
        }
    }
}

impl HitArea {
    /// A rectangle on screen.
    pub(crate) fn rect(owner: NativeId, x: i32, y: i32, w: i32, h: i32) -> HitArea {
        HitArea {
            owner,
            layer_offset: (0, 0),
            shape: HitShape::Rect(x, y, w, h),
            clip: None,
        }
    }

    /// The projected bounding rectangle of a 3D actor, in the
    /// coordinates of the layer it is drawn in.
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    pub(crate) fn model3d(
        owner: NativeId,
        layer_offset: (i32, i32),
        (left, top, right, bottom): (i32, i32, i32, i32),
    ) -> HitArea {
        HitArea {
            owner,
            layer_offset,
            shape: HitShape::Model3D(left, top, right, bottom),
            clip: None,
        }
    }
}

/// The cursors a game shows.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Cursors {
    /// The cursor sprites; loaded again after a saved game.
    #[serde(skip)]
    pub bank: SpriteBank,
    /// The default cursor (`CURSOR`).
    pub normal: Option<String>,
    /// The cursor over an interactive object (`ACTIVE_CURSOR`).
    pub active: Option<String>,
    /// The cursor while the game is not interactive
    /// (`NONINTERACTIVE_CURSOR`).
    pub noninteractive: Option<String>,
    /// Whether the cursor is hidden.
    pub hidden: bool,
    /// The cursor of the selected inventory item, drawn with (or
    /// instead of) the cursor.
    pub item_cursor: Option<String>,
    anims: HashMap<String, AnimState>,
    last: Option<String>,
    pub(crate) shown: Option<String>,
}

impl Cursors {
    /// The cursor shown this frame.
    pub fn shown(&self) -> Option<&str> {
        self.shown.as_deref()
    }

    /// Advances the animation of a cursor sprite to `timer`.
    pub(crate) fn step_anim(&mut self, path: &str, timer: u64) {
        let k = key(path);
        if let Some(loaded) = self.bank.sprite(path) {
            let state = self.anims.entry(k).or_insert_with(|| AnimState::new(path));
            state.step(&loaded.sprite, timer);
        }
    }

    /// The frame of a cursor sprite's animation.
    pub fn frame_of(&self, path: &str) -> usize {
        self.anims.get(&key(path)).map_or(0, |a| a.current_frame)
    }

    /// The animation state of a cursor sprite, once it was shown or
    /// restarted.
    pub fn anim(&self, path: &str) -> Option<&AnimState> {
        self.anims.get(&key(path))
    }

    /// The animation state of a cursor sprite, mutable, created at its
    /// first frame.
    pub(crate) fn anim_mut(&mut self, path: &str) -> &mut AnimState {
        self.anims
            .entry(key(path))
            .or_insert_with(|| AnimState::new(path))
    }

    /// The frame the shown cursor is at.
    pub fn frame(&self) -> usize {
        self.shown
            .as_ref()
            .and_then(|s| self.anims.get(&key(s)))
            .map_or(0, |a| a.current_frame)
    }
}

impl World {
    /// The mouse.
    pub fn mouse(&self) -> &Mouse {
        &self.mouse
    }

    /// The pointer is at a point in game pixels, as the front-end
    /// reports it (`CBGame::GetMousePos`). The game sees it kept inside
    /// the lock rectangle when one applies; a pointer outside is put
    /// back on the rectangle's edge, which the front-end learns from
    /// [`World::take_cursor_warp`].
    pub fn set_mouse(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
        self.mouse.x = x;
        self.mouse.y = y;
        self.apply_mouse_lock();
    }

    /// Keeps the mouse inside the lock rectangle (`CBGame::
    /// GetMousePos`, which every frame reads the pointer, clamps it and
    /// moves the system cursor back when it was outside).
    pub(crate) fn apply_mouse_lock(&mut self) {
        let Some((left, top, right, bottom)) = self.mouse_lock() else {
            return;
        };
        let (x, y) = (
            self.mouse.x.clamp(left, right),
            self.mouse.y.clamp(top, bottom),
        );
        if (x, y) != (self.mouse.x, self.mouse.y) {
            self.warp_mouse(x, y);
        }
    }

    /// The game moves the mouse (`CBPlatform::SetCursorPos`, from
    /// `Game.SetMousePos`, `Game.MouseX` and `MouseY`, the mouse lock
    /// and the accessibility cursor skip): the position the game sees
    /// changes at once, and the front-end is asked to move the system
    /// cursor there ([`World::take_cursor_warp`]), without which the
    /// next mouse movement would undo the change.
    pub fn warp_mouse(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
        self.mouse.x = x;
        self.mouse.y = y;
        self.cursor_warp = Some((x, y));
        self.apply_mouse_lock();
    }

    /// Where the game last moved the mouse, in game pixels, when it did
    /// since the last call: the front-end moves the system cursor
    /// there.
    pub fn take_cursor_warp(&mut self) -> Option<(i32, i32)> {
        self.cursor_warp.take()
    }

    /// The keyboard state.
    pub fn keyboard(&self) -> &Keyboard {
        &self.keyboard
    }

    /// Records a keyboard message (`CBKeyboardState::ReadKey`): a key
    /// going down (`printable` false, `code` its virtual key code) or a
    /// character typed (`printable` true, `code` from [`char_code`],
    /// `text` the character), with the modifiers held.
    pub fn read_key(
        &mut self,
        printable: bool,
        code: i32,
        text: Option<char>,
        shift: bool,
        alt: bool,
        control: bool,
    ) {
        self.keyboard.key_code = code;
        self.keyboard.printable = printable;
        self.keyboard.key = text
            .filter(|_| printable)
            .map(String::from)
            .unwrap_or_default();
        self.keyboard.shift = shift;
        self.keyboard.alt = alt;
        self.keyboard.control = control;
    }

    /// Gives the editors a clipboard, usually the system's.
    pub fn set_clipboard(&mut self, clipboard: Box<dyn Clipboard>) {
        self.clipboard = clipboard;
    }

    /// Forgets the last clicks, so that the next press is a click and
    /// not the second press of a double click, however soon it comes.
    /// For tooling whose steps are separate clicks.
    pub fn forget_clicks(&mut self) {
        self.mouse.last_left_click = None;
        self.mouse.last_right_click = None;
    }

    /// Notes a key or a mouse button as held, by virtual key code, for
    /// `Keyboard.IsKeyDown`; nothing else happens.
    pub fn key_held(&mut self, code: i32) {
        self.keyboard.down.insert(code);
    }

    /// Records a key going up.
    pub fn key_up(&mut self, code: i32) {
        self.keyboard.down.remove(&code);
    }

    /// The object under the mouse (`Game.ActiveObject`).
    pub fn active_object(&self) -> Option<NativeId> {
        self.active_object.filter(|id| self.objects.contains(*id))
    }

    /// The cursors.
    pub fn cursors(&self) -> &Cursors {
        &self.cursors
    }

    /// Loads the game's cursors from `default.game`.
    pub(crate) fn load_cursors(&mut self) {
        let (normal, active, non) = (
            self.game_file().cursor.clone(),
            self.game_file().active_cursor.clone(),
            self.game_file().noninteractive_cursor.clone(),
        );
        self.cursors.normal = normal.filter(|p| self.load_cursor(p));
        self.cursors.active = active.filter(|p| self.load_cursor(p));
        self.cursors.noninteractive = non.filter(|p| self.load_cursor(p));
    }

    /// Loads a cursor sprite; whether it loaded.
    pub fn load_cursor(&mut self, path: &str) -> bool {
        let ok = self.cursors.bank.add_sprite(&self.vfs, path);
        if !ok {
            if let Some((file, reason)) = self.cursors.bank.missing().last() {
                let message = format!("cannot load cursor '{file}': {reason}");
                self.log(message);
            }
        }
        ok
    }

    /// Sets the cursor over interactive objects (`SetActiveCursor`).
    pub fn set_active_cursor(&mut self, path: Option<&str>) -> bool {
        match path {
            Some(path) if self.load_cursor(path) => {
                self.cursors.active = Some(path.to_string());
                true
            }
            Some(_) => false,
            None => {
                self.cursors.active = None;
                true
            }
        }
    }

    /// Sets the cursor shown while the game is not interactive
    /// (`SetWaitCursor`).
    pub fn set_wait_cursor(&mut self, path: Option<&str>) -> bool {
        match path {
            Some(path) if self.load_cursor(path) => {
                self.cursors.noninteractive = Some(path.to_string());
                true
            }
            Some(_) => false,
            None => {
                self.cursors.noninteractive = None;
                true
            }
        }
    }

    /// The cursor to show (`CBGame::ShowCursor`): the non-interactive one
    /// while the game is not interactive, else the active object's own,
    /// else the active cursor over an object, else the default.
    fn cursor_to_show(&mut self) -> Option<String> {
        if self.cursors.hidden {
            return None;
        }
        if !self.interactive {
            return self.cursors.noninteractive.clone();
        }
        if let Some(active) = self.active_object() {
            if let Some(own) = self.object_cursor(active) {
                if self.load_cursor(&own) {
                    return Some(own);
                }
            }
            // Only over what can be used (`GetExtendedFlag("usable")`:
            // the scene's objects and the items), not over a window or
            // a button.
            let usable = matches!(
                self.objects.get(active).map(|n| &n.kind),
                Some(Kind::Actor | Kind::Actor3D | Kind::Entity(_) | Kind::Item)
            );
            if usable && self.cursors.active.is_some() {
                return self.cursors.active.clone();
            }
        }
        self.cursors.normal.clone()
    }

    /// Picks the cursor for this frame and advances its animation; a
    /// cursor that was not shown last frame restarts.
    pub(crate) fn update_cursor(&mut self) {
        let shown = self.cursor_to_show();
        if let Some(path) = &shown {
            let k = key(path);
            if self.cursors.last.as_deref() != Some(k.as_str()) {
                self.cursors.anims.insert(k.clone(), AnimState::new(path));
                self.cursors.last = Some(k.clone());
            }
            let timer = self.timer();
            if let Some(loaded) = self.cursors.bank.sprite(path) {
                let state = self
                    .cursors
                    .anims
                    .entry(k)
                    .or_insert_with(|| AnimState::new(path));
                state.step(&loaded.sprite, timer);
            }
        }
        self.cursors.shown = shown;
    }

    /// Draws the cursor at the mouse, and the selected item's over it.
    pub fn draw_cursor<'a>(&'a self, list: &mut DrawList<'a>) {
        if let Some(path) = self.cursors.shown.as_deref() {
            self.draw_cursor_sprite(list, path);
        }
        if let Some(path) = self.cursors.item_cursor.as_deref() {
            self.draw_cursor_sprite(list, path);
        }
    }

    fn draw_cursor_sprite<'a>(&'a self, list: &mut DrawList<'a>, path: &str) {
        let Some(loaded) = self.cursors.bank.sprite(path) else {
            return;
        };
        let frame = self
            .cursors
            .frame_of(path)
            .min(loaded.sprite.frames.len().saturating_sub(1));
        let Some(frame) = loaded.sprite.frames.get(frame) else {
            return;
        };
        for sub in crate::scene_view::drawn_subframes(frame) {
            let Some(image) = &sub.image else { continue };
            let Some(texture) = self.cursors.bank.texture(image, sub.color_key) else {
                continue;
            };
            list.push(crate::scene_view::place(
                sub,
                texture,
                self.mouse.x,
                self.mouse.y,
                1.0,
                luma_formats::Rgba::OPAQUE_WHITE,
            ));
        }
    }

    /// Gathers the hit areas of this frame in drawing order
    /// (`CBRenderer::m_RectList`): the entities and actors that react to
    /// the mouse, by their region or by their sprite's pixels.
    pub(crate) fn collect_hits(&mut self) {
        self.hits.clear();
        let mut hits = Vec::new();
        self.scene_hits(&mut hits);
        // The scene's objects are registered inside its viewport.
        if let Some(clip) = self.scene_clip() {
            clip_hits(&mut hits, 0, clip);
        }
        self.ui_hits_in_game(&mut hits);
        self.inventory_hits(&mut hits);
        // In drawing order: the response box is under the normal windows.
        self.responses_hits(&mut hits);
        self.ui_hits_normal(&mut hits);
        self.hits = hits;
    }

    /// The hit areas of the scene's objects, in drawing order.
    fn scene_hits(&self, hits: &mut Vec<HitArea>) {
        let Some(view) = &self.scene else {
            return;
        };
        let camera = self.camera();
        let free_objects = self.free_objects();
        let positions: Vec<crate::scene_view::FreeObject> =
            free_objects.iter().map(|(_, o)| *o).collect();
        for item in view.draw_order(&positions, |i| self.free_entity_stick(i)) {
            let layer = &view.scene.layers[item.layer()];
            let (ox, oy) = camera.layer_offset(&view.scene, layer);
            let layer_offset = (ox as i32, oy as i32);
            match item {
                DrawItem::Node { layer, index } => {
                    let r = EntityRef::Node { layer, index };
                    if let Some(id) = self.node_id(r) {
                        self.entity_hits(id, r, layer_offset, hits);
                    }
                }
                DrawItem::Free { index, .. } => {
                    let r = EntityRef::Free(index);
                    if let Some(id) = self.node_id(r) {
                        self.entity_hits(id, r, layer_offset, hits);
                    }
                }
                DrawItem::Other { index, .. } => {
                    let Some((id, _)) = free_objects.get(index) else {
                        continue;
                    };
                    match self.objects.get(*id).map(|n| n.kind.clone()) {
                        Some(Kind::Actor) => self.actor_hits(*id, layer_offset, hits),
                        Some(Kind::Actor3D) => self.actor3d_hits(*id, layer_offset, hits),
                        Some(Kind::Entity(r)) => self.entity_hits(*id, r, layer_offset, hits),
                        _ => {}
                    }
                }
            }
        }
    }

    /// The hit areas of an object's attachments, registered as the
    /// object itself (`m_RegisterAlias`), around its position with its
    /// scale and rotation.
    fn attachment_hits(
        &self,
        owner: NativeId,
        x: i32,
        y: i32,
        transform: Transform,
        layer_offset: (i32, i32),
        hits: &mut Vec<HitArea>,
    ) {
        let Some(state) = self.object_state(owner) else {
            return;
        };
        for attachment in &state.attachments {
            let Some(Kind::Entity(r)) = self.objects.get(attachment.id).map(|n| &n.kind) else {
                continue;
            };
            let view = &self.game_view;
            let Some(entity) = view.entity(*r).filter(|e| e.active) else {
                continue;
            };
            let Some(loaded) = view.shown_sprite(*r).and_then(|p| view.sprite(&p)) else {
                continue;
            };
            World::sprite_hits_with(
                owner,
                HitBank::Game,
                &loaded.sprite,
                view.frame_of(*r),
                (
                    x + (entity.x as f32 * transform.scale_x / 100.0) as i32,
                    y + (entity.y as f32 * transform.scale_y / 100.0) as i32,
                ),
                transform,
                layer_offset,
                hits,
            );
        }
    }

    /// Registers the hit areas of a sprite's frame drawn at `(x, y)` at a
    /// uniform `scale` in percent, unrotated; see
    /// [`World::sprite_hits_with`].
    #[allow(
        clippy::too_many_arguments,
        reason = "the registration of a sprite's sub-frames takes what the engine's does"
    )]
    pub(crate) fn sprite_hits(
        owner: NativeId,
        bank: HitBank,
        sprite: &Sprite,
        frame: usize,
        x: i32,
        y: i32,
        scale: f32,
        layer_offset: (i32, i32),
        hits: &mut Vec<HitArea>,
    ) {
        World::sprite_hits_with(
            owner,
            bank,
            sprite,
            frame,
            (x, y),
            Transform::uniform(scale),
            layer_offset,
            hits,
        );
    }

    /// Registers the hit areas of a sprite's frame drawn with its hotspot
    /// at `position` and a transform (`CBSubFrame::Draw` registering a
    /// `CBActiveRect`): every drawn sub-frame that is not a decoration,
    /// over the rectangle it is drawn in before rotation, with the scale
    /// per axis, the rotation around the hotspot and the mirroring for
    /// the pixel test.
    #[allow(
        clippy::too_many_arguments,
        reason = "the registration of a sprite's sub-frames takes what the engine's does"
    )]
    pub(crate) fn sprite_hits_with(
        owner: NativeId,
        bank: HitBank,
        sprite: &Sprite,
        frame: usize,
        position: (i32, i32),
        transform: Transform,
        layer_offset: (i32, i32),
        hits: &mut Vec<HitArea>,
    ) {
        let Some(frame) = sprite
            .frames
            .get(frame.min(sprite.frames.len().saturating_sub(1)))
        else {
            return;
        };
        let (x, y) = position;
        for sub in crate::scene_view::drawn_subframes(frame) {
            let Some(image) = &sub.image else { continue };
            if sub.decoration {
                continue;
            }
            hits.push(HitArea {
                owner,
                layer_offset,
                shape: HitShape::Pixels {
                    bank,
                    image: image.clone(),
                    color_key: sub.color_key,
                    area: pixel_area(sub, x, y, transform, sprite.precise),
                },
                clip: None,
            });
        }
    }

    fn entity_hits(
        &self,
        id: NativeId,
        r: EntityRef,
        layer_offset: (i32, i32),
        hits: &mut Vec<HitArea>,
    ) {
        // A game-level entity lives in the game's view and is scaled by
        // the scene it is shown in.
        let (view, bank) = match r {
            EntityRef::Game(_) => (&self.game_view, HitBank::Game),
            _ => match &self.scene {
                Some(view) => (view, HitBank::Scene),
                None => return,
            },
        };
        let levels = self.scene.as_ref().map_or(&view.scene, |v| &v.scene);
        let Some(entity) = view.entity(r) else { return };
        if !entity.active || !entity.interactive {
            return;
        }
        if entity.ignore_items && self.selected_item.is_some() {
            return;
        }
        let transform = Transform::of_entity(levels, entity);
        self.attachment_hits(id, entity.x, entity.y, transform, layer_offset, hits);
        // The region's points are scene coordinates, registered with the
        // offset of the layer the entity is drawn in (`CAdEntity::Display`
        // hands `Game->m_OffsetX/Y` to the active rectangle), so a scene
        // smaller than its viewport, whose scroll offset is negative, has
        // its hotspots where its layers are drawn.
        if let Some(region) = &entity.region {
            hits.push(HitArea {
                owner: id,
                layer_offset,
                shape: HitShape::Polygon(region.points.clone()),
                clip: None,
            });
        }
        let Some(path) = view.shown_sprite(r) else {
            return;
        };
        let Some(loaded) = view.sprite(&path) else {
            return;
        };
        World::sprite_hits_with(
            id,
            bank,
            &loaded.sprite,
            view.frame_of(r),
            (entity.x, entity.y),
            transform,
            layer_offset,
            hits,
        );
    }

    fn actor_hits(&self, id: NativeId, layer_offset: (i32, i32), hits: &mut Vec<HitArea>) {
        let Some(actor) = self.actor(id) else { return };
        let Some(view) = &self.scene else { return };
        if !actor.def.interactive || (actor.def.ignore_items && self.selected_item.is_some()) {
            return;
        }
        self.attachment_hits(
            id,
            actor.x,
            actor.y,
            actor.transform(&view.scene),
            layer_offset,
            hits,
        );
        let Some(path) = actor.current_sprite() else {
            return;
        };
        let Some(loaded) = actor.bank.sprite(path) else {
            return;
        };
        World::sprite_hits_with(
            id,
            HitBank::Actor(id),
            &loaded.sprite,
            actor.frame_of(path),
            (actor.x, actor.y),
            actor.transform(&view.scene),
            layer_offset,
            hits,
        );
    }

    /// A point of the screen inside an object's hit area: the centre of the
    /// last area drawn for it (its sprite's frame, its region's bounding
    /// box or its rectangle), or `None` when the object is not on screen.
    /// For tooling that clicks an object where it is at the moment, such
    /// as a wandering character; the centre of a sprite may fall on a
    /// transparent pixel, which nothing here checks.
    pub fn hit_point(&mut self, id: NativeId) -> Option<(i32, i32)> {
        let (x, y, w, h) = self.hit_rect(id)?;
        Some((x + w / 2, y + h / 2))
    }

    /// The rectangle on screen of the last hit area drawn for an object
    /// (`x`, `y`, `width`, `height`): the bounding box of its region, its
    /// sprite's frame or its own rectangle; `None` when the object is
    /// not on screen.
    pub fn hit_rect(&mut self, id: NativeId) -> Option<(i32, i32, i32, i32)> {
        let hits = std::mem::take(&mut self.hits);
        let mut found = None;
        for hit in hits.iter().rev() {
            if hit.owner != id {
                continue;
            }
            let (x, y, w, h) = match &hit.shape {
                HitShape::Polygon(points) => {
                    let Some(first) = points.first() else {
                        continue;
                    };
                    let (mut l, mut t, mut r, mut b) = (first.x, first.y, first.x, first.y);
                    for p in points {
                        l = l.min(p.x);
                        t = t.min(p.y);
                        r = r.max(p.x);
                        b = b.max(p.y);
                    }
                    (l, t, r - l, b - t)
                }
                HitShape::Rect(x, y, w, h) => (*x, *y, *w, *h),
                HitShape::Model3D(l, t, r, b) => (*l, *t, r - l, b - t),
                HitShape::Pixels {
                    bank,
                    image,
                    color_key,
                    area,
                } => {
                    // The size may come from the image, as in `object_at`;
                    // a turned sprite gives the box around it.
                    let mask = self.hit_mask(*bank, image, *color_key);
                    let Some(bounds) = area.bounds(mask.as_ref()) else {
                        continue;
                    };
                    bounds
                }
            };
            if w <= 0 || h <= 0 {
                continue;
            }
            let (mut x, mut y, mut w, mut h) =
                (x - hit.layer_offset.0, y - hit.layer_offset.1, w, h);
            // Only the part inside the viewport it was registered in.
            if let Some((cl, ct, cr, cb)) = hit.clip {
                let (l, t) = (x.max(cl), y.max(ct));
                let (r, b) = ((x + w).min(cr), (y + h).min(cb));
                if r <= l || b <= t {
                    continue;
                }
                (x, y, w, h) = (l, t, r - l, b - t);
            }
            found = Some((x, y, w, h));
            break;
        }
        self.hits = hits;
        found
    }

    /// The opaque pixels of a hit area's image, decoded on first use.
    fn hit_mask(&mut self, bank: HitBank, image: &str, color_key: Rgb) -> Option<Mask> {
        match bank {
            HitBank::Scene => self
                .scene
                .as_mut()
                .and_then(|v| v.bank.mask(&self.vfs, image, color_key).cloned()),
            HitBank::Actor(actor) => self
                .actor_and_vfs(actor)
                .and_then(|(vfs, a)| a.bank.mask(vfs, image, color_key).cloned()),
            HitBank::Ui => self.ui.bank.mask(&self.vfs, image, color_key).cloned(),
            HitBank::Game => self
                .game_view
                .bank
                .mask(&self.vfs, image, color_key)
                .cloned(),
        }
    }

    /// The object at a point of the screen (`CBRenderer::GetObjectAt`):
    /// the last hit area drawn that contains it, by its polygon or by an
    /// opaque pixel of its sprite.
    pub fn object_at(&mut self, x: i32, y: i32) -> Option<NativeId> {
        let hits = std::mem::take(&mut self.hits);
        let mut found = None;
        let screen = (
            0,
            0,
            self.settings.width as i32,
            self.settings.height as i32,
        );
        for hit in hits.iter().rev() {
            // `CBActiveRect::ClipRect`: the rectangle was cut to the
            // viewport in force (the screen without one), so a point
            // outside it misses.
            let (cl, ct, cr, cb) = hit.clip.unwrap_or(screen);
            if x < cl || y < ct || x >= cr || y >= cb {
                continue;
            }
            let (lx, ly) = (x + hit.layer_offset.0, y + hit.layer_offset.1);
            let inside = match &hit.shape {
                HitShape::Polygon(points) => crate::path::polygon_contains(points, lx, ly),
                HitShape::Rect(rx, ry, rw, rh) => {
                    lx >= *rx && ly >= *ry && lx < rx + rw && ly < ry + rh
                }
                HitShape::Model3D(l, t, r, b) => {
                    lx >= *l
                        && ly >= *t
                        && lx <= *r
                        && ly <= *b
                        && self.actor3d_hit_at(hit.owner, lx, ly)
                }
                HitShape::Pixels {
                    bank,
                    image,
                    color_key,
                    area,
                } => {
                    let Some(mask) = self.hit_mask(*bank, image, *color_key) else {
                        continue;
                    };
                    area.contains(&mask, lx, ly)
                }
            };
            if inside {
                found = Some(hit.owner);
                break;
            }
        }
        self.hits = hits;
        found
    }

    /// Sets the object under the mouse (`CBGame::SetActiveObject`): none
    /// while the game is not interactive; `MouseLeave` and `MouseEntry`
    /// are queued when it changes.
    pub(crate) fn update_active_object(&mut self) {
        let (x, y) = (self.mouse.x, self.mouse.y);
        let mut object = self.object_at(x, y);
        // Not active while the game is not interactive, except the
        // objects that take mouse events then (`m_NonIntMouseEvents`):
        // the windows and their controls, and the containers' entities.
        if !self.interactive && !object.is_some_and(|o| self.non_int_mouse_events(o)) {
            object = None;
        }
        let current = self.active_object();
        if object == current {
            self.active_object = object;
            return;
        }
        if let Some(previous) = current {
            self.queue_event(previous, "MouseLeave".to_string());
        }
        self.active_object = object;
        if let Some(entered) = object {
            self.speak_caption_of(entered);
            self.queue_event(entered, "MouseEntry".to_string());
        }
    }

    /// Draws a full-screen colour (the fader).
    pub fn draw_fade<'a>(&self, list: &mut DrawList<'a>, white: &'a luma_render::Texture) {
        if let Some(color) = self.fader.color() {
            let (w, h) = (self.settings.width as f32, self.settings.height as f32);
            list.push(Quad {
                texture: white,
                dst: Rect::new(0.0, 0.0, w, h),
                src: None,
                tint: color,
                flip_x: false,
                flip_y: false,
            });
        }
    }

    /// Draws the scene fader's colour over the scene viewport
    /// (`CBFader::Display` through `FadeToColor`, which fills the
    /// current viewport).
    pub fn draw_scene_fade<'a>(&self, list: &mut DrawList<'a>, white: &'a luma_render::Texture) {
        if let Some(color) = self.scene_fader.color() {
            let (x, y, w, h) = self.viewport();
            list.push(Quad {
                texture: white,
                dst: Rect::new(x as f32, y as f32, w as f32, h as f32),
                src: None,
                tint: color,
                flip_x: false,
                flip_y: false,
            });
        }
    }
}

/// The area a sub-frame drawn with its hotspot at `(x, y)` is registered
/// over (`CBSubFrame::Draw`): unscaled, the sub-frame's rectangle moved
/// by the hotspot (the engine adds the source rectangle's corner to the
/// position, a quirk kept here); scaled, the same with each term scaled
/// and truncated. The pivot of the rotation is the hotspot, from the
/// truncated top-left corner the sub-frame is drawn at, as in
/// `scene_view::place_transformed`.
pub(crate) fn pixel_area(
    sub: &luma_formats::SubFrame,
    x: i32,
    y: i32,
    transform: Transform,
    precise: bool,
) -> PixelArea {
    let src = sub
        .rect
        .filter(|r| !r.is_empty())
        .map(|r| (r.left, r.top, r.width() as i32, r.height() as i32));
    // The size is known from the sub-frame's rectangle; without one,
    // from the image, read at hit time.
    let (w, h) = src.map_or((0, 0), |(_, _, w, h)| (w, h));
    let (sx, sy) = src.map_or((0, 0), |s| (s.0, s.1));
    let (zx, zy) = (transform.scale_x / 100.0, transform.scale_y / 100.0);
    let dst = if transform.is_unscaled() {
        (x - sub.hotspot_x + sx, y - sub.hotspot_y + sy, w, h)
    } else {
        (
            (x as f32 - (sub.hotspot_x + sx) as f32 * zx) as i32,
            (y as f32 - (sub.hotspot_y + sy) as f32 * zy) as i32,
            (w as f32 * zx) as i32,
            (h as f32 * zy) as i32,
        )
    };
    let drawn_x = if transform.is_unscaled() {
        (x - sub.hotspot_x) as f32
    } else {
        (x as f32 - sub.hotspot_x as f32 * zx).trunc()
    };
    let drawn_y = if transform.is_unscaled() {
        (y - sub.hotspot_y) as f32
    } else {
        (y as f32 - sub.hotspot_y as f32 * zy).trunc()
    };
    PixelArea {
        dst,
        src,
        zoom_x: transform.scale_x,
        zoom_y: transform.scale_y,
        rotation: transform.rotation,
        pivot: (
            drawn_x + sub.hotspot_x as f32 * zx,
            drawn_y + sub.hotspot_y as f32 * zy,
        ),
        mirror_x: sub.mirror_x,
        mirror_y: sub.mirror_y,
        precise,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use luma_formats::{Image, SubFrame};

    #[test]
    fn keys_and_characters_of_a_us_keyboard() {
        assert_eq!(key_for_char('a'), Some((65, false)));
        assert_eq!(key_for_char('A'), Some((65, true)));
        assert_eq!(key_for_char('7'), Some((55, false)));
        assert_eq!(key_for_char('&'), Some((55, true)));
        assert_eq!(key_for_char('.'), Some((190, false)));
        assert_eq!(key_for_char('"'), Some((222, true)));
        assert_eq!(key_for_char('é'), None);
        for ch in (' '..='~').filter(|c| !c.is_control()) {
            let (code, shift) = key_for_char(ch).unwrap();
            assert_eq!(char_for_key(code, shift), Some(ch), "{ch}");
        }
        assert_eq!(char_for_key(46, false), None, "Delete types nothing");
        assert_eq!(char_for_key(112, false), None);
        // What Windows adds for the keys that type no character of
        // their own.
        assert_eq!(translated_char(8, false, false), Some('\u{8}'));
        assert_eq!(translated_char(8, false, true), Some('\u{7f}'));
        assert_eq!(translated_char(9, false, false), Some('\t'));
        assert_eq!(translated_char(9, false, true), None);
        assert_eq!(translated_char(13, false, false), Some('\r'));
        assert_eq!(translated_char(13, false, true), Some('\n'));
        assert_eq!(translated_char(27, false, false), Some('\u{1b}'));
        assert_eq!(translated_char(67, false, true), Some('\u{3}'));
        assert_eq!(translated_char(67, true, true), None, "AltGr");
        assert_eq!(translated_char(67, false, false), None);
        assert_eq!(char_code('a'), 97);
        assert_eq!(char_code('é'), 233);
        assert_eq!(char_code('€'), 128);
        assert_eq!(char_code('ж'), 0x436);
    }

    /// A 4x2 image whose left half is opaque and right half clear.
    fn half_mask() -> Mask {
        let mut pixels = Vec::new();
        for _ in 0..2 {
            pixels.extend_from_slice(&[255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0]);
        }
        Mask::of(&Image {
            width: 4,
            height: 2,
            pixels,
            has_alpha_channel: true,
        })
    }

    #[test]
    fn scaled_areas_follow_each_axis_and_the_mirroring() {
        let mask = half_mask();
        let sub = SubFrame {
            image: Some("i".into()),
            hotspot_x: 2,
            hotspot_y: 2,
            ..SubFrame::default()
        };
        // Unscaled at (10, 10): the image covers (8..12, 8..10); the
        // opaque half is x 8..10.
        let area = pixel_area(&sub, 10, 10, Transform::IDENTITY, true);
        assert_eq!(area.rect(Some(&mask)), (8, 8, 4, 2));
        assert!(area.contains(&mask, 9, 9));
        assert!(!area.contains(&mask, 10, 9), "the clear half");
        assert!(!area.contains(&mask, 7, 9) && !area.contains(&mask, 9, 10));
        let coarse = PixelArea {
            precise: false,
            ..area
        };
        assert!(coarse.contains(&mask, 10, 9), "imprecise: the rectangle");
        assert_eq!(area.center(Some(&mask)), Some((10, 9)));

        // Twice as wide, half as tall: (6..14, 9..10); the opaque half is
        // x 6..10, and the pixel test maps back through each scale.
        let wide = pixel_area(
            &sub,
            10,
            10,
            Transform {
                scale_x: 200.0,
                scale_y: 50.0,
                rotation: 0.0,
            },
            true,
        );
        assert_eq!(wide.rect(Some(&mask)), (6, 9, 8, 1));
        assert!(wide.contains(&mask, 9, 9));
        assert!(!wide.contains(&mask, 10, 9));
        assert!(!wide.contains(&mask, 9, 10));

        // Mirrored: the opaque half is on the right.
        let mirrored = pixel_area(
            &SubFrame {
                mirror_x: true,
                ..sub.clone()
            },
            10,
            10,
            Transform::IDENTITY,
            true,
        );
        assert!(mirrored.contains(&mask, 11, 9));
        assert!(!mirrored.contains(&mask, 8, 9));
    }

    #[test]
    fn rotated_areas_are_hit_where_they_are_drawn() {
        let mask = half_mask();
        let sub = SubFrame {
            image: Some("i".into()),
            hotspot_x: 0,
            hotspot_y: 0,
            ..SubFrame::default()
        };
        // Drawn at (10, 10) and turned a quarter clockwise around the
        // hotspot: the image (4x2) now covers x 8..10, y 10..14, its
        // opaque left half on top (y 10..12).
        let area = pixel_area(
            &sub,
            10,
            10,
            Transform {
                scale_x: 100.0,
                scale_y: 100.0,
                rotation: 90.0,
            },
            true,
        );
        assert_eq!(area.pivot, (10.0, 10.0));
        assert!(area.contains(&mask, 9, 10));
        assert!(area.contains(&mask, 8, 11));
        assert!(!area.contains(&mask, 9, 12), "the clear half, turned");
        assert!(!area.contains(&mask, 11, 10), "where it would be unrotated");
        assert!(!area.contains(&mask, 9, 9));
        assert_eq!(area.center(Some(&mask)), Some((9, 12)));
    }
}
