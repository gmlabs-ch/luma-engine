//! Windows and their controls: menus, captions, dialogs and the frame of
//! the inventory, drawn over the scene and driven by scripts.
//!
//! Written with reference to `UIWindow.cpp`, `UIObject.cpp`,
//! `UIButton.cpp`, `UIText.cpp`, `UIEdit.cpp`, `UITiledImage.cpp` and
//! `BGame.cpp` of WME Lite (MIT License, Copyright (c) 2011 Jan Nedoma);
//! see `NOTICE`. The rules are documented in `docs/formats/window.md`.

use std::collections::HashMap;

use luma_formats::font::TextAlign;
use luma_formats::sprite::Rect as SpriteRect;
use luma_formats::{
    ButtonDef, ControlDef, DEFAULT_COLOR_KEY, EditDef, Entity, Rgba, StaticDef, TiledImage,
    VerticalAlign, WidgetDef, WindowDef,
};
use luma_render::{DrawList, Quad, Rect, Texture};
use luma_script::{Engine, NativeId};

use crate::input::{HitArea, HitBank};
use crate::objects::Kind;
use crate::scene_view::{AnimState, EntityRef, SpriteBank, place};
use crate::world::World;

/// Windows' virtual key codes the controls react to.
const VK_TAB: i32 = 9;
const VK_BACK: i32 = 8;
const VK_RETURN: i32 = 13;
const VK_ESCAPE: i32 = 27;
const VK_END: i32 = 35;
const VK_HOME: i32 = 36;
const VK_LEFT: i32 = 37;
const VK_UP: i32 = 38;
const VK_RIGHT: i32 = 39;
const VK_DOWN: i32 = 40;
const VK_DELETE: i32 = 46;

/// How long a `Press()` asked by a script shows the button pressed.
const ONE_TIME_PRESS_MS: u64 = 100;

/// One of the images a control shows, each a sprite of its own with
/// its own animation (`m_Image`, `m_ImageInactive`, `m_ImageHover`...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ImageSlot {
    /// The background image every control has (`IMAGE`).
    Image,
    /// A window's image while it is not focused (`IMAGE_INACTIVE`).
    Inactive,
    /// A button's image under the mouse (`IMAGE_HOVER`).
    Hover,
    /// A button's image while pressed (`IMAGE_PRESS`).
    Press,
    /// A button's image while disabled (`IMAGE_DISABLE`).
    Disable,
    /// A button's image while focused (`IMAGE_FOCUS`).
    Focus,
}

/// How a window relates to the rest of the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum WindowMode {
    /// The game goes on around the window.
    #[default]
    Normal,
    /// The window takes the mouse; the game runs (`GoExclusive`).
    Exclusive,
    /// The window takes the mouse and the game is frozen
    /// (`GoSystemExclusive`).
    SystemExclusive,
}

/// The state of a window.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WindowState {
    /// The file it was loaded from.
    pub filename: Option<String>,
    /// Its mode.
    pub mode: WindowMode,
    /// Whether the scripts waiting for it may go on (`false` while
    /// exclusive).
    pub ready: bool,
    /// Alignment of the title.
    pub title_align: TextAlign,
    /// Where the title is drawn.
    pub title_rect: Option<SpriteRect>,
    /// The area the window is dragged by.
    pub drag_rect: Option<SpriteRect>,
    /// Whether the mouse is dragging it.
    pub dragging: bool,
    /// Where the mouse was at the last drag step.
    pub drag_from: (i32, i32),
    /// The window itself does not catch the mouse.
    pub transparent: bool,
    /// A click outside closes it.
    pub menu: bool,
    /// Drawn with the scene, under the other windows.
    pub in_game: bool,
    /// The controls are clipped to the window.
    pub clip_contents: bool,
    /// `GoSystemExclusive` pauses the music too.
    pub pause_music: bool,
    /// The screen is tinted with this colour under the window.
    pub fade_color: Option<Rgba>,
    /// A colour multiplied with the window and its controls.
    pub alpha_color: Option<Rgba>,
    /// Background, image and font while not focused.
    pub back_inactive: Option<TiledImage>,
    /// See `back_inactive`.
    pub image_inactive: Option<String>,
    /// See `back_inactive`.
    pub font_inactive: Option<String>,
    /// The animation of the inactive image.
    pub anim_inactive: Option<AnimState>,
    /// The controls, in drawing order.
    pub widgets: Vec<NativeId>,
    /// The control holding the keyboard focus.
    pub focused_widget: Option<NativeId>,
    /// The shield under an exclusive window, or the `close` button under
    /// a menu, created when first needed.
    pub shield: Option<NativeId>,
    /// Whether this control is a shield of another window.
    pub is_shield: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        WindowState {
            filename: None,
            mode: WindowMode::Normal,
            ready: true,
            title_align: TextAlign::Left,
            title_rect: None,
            drag_rect: None,
            dragging: false,
            drag_from: (0, 0),
            transparent: false,
            menu: false,
            in_game: false,
            clip_contents: false,
            pause_music: true,
            fade_color: None,
            alpha_color: None,
            back_inactive: None,
            image_inactive: None,
            font_inactive: None,
            anim_inactive: None,
            widgets: Vec::new(),
            focused_widget: None,
            shield: None,
            is_shield: false,
        }
    }
}

/// Who a button reports to besides its scripts (`SetListener`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ButtonListener {
    /// The shield of a menu: a press closes the parent window.
    CloseParent,
    /// A button of the inventory box: `prev`, `next` and `close`.
    Inventory,
    /// A choice of the response box, by index in the pending list.
    Response(usize),
    /// The response box's `prev` and `next` buttons.
    ResponseBox,
}

/// The state of a button.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct ButtonState {
    /// Tiled backgrounds of the states.
    pub back_hover: Option<TiledImage>,
    /// See `back_hover`.
    pub back_press: Option<TiledImage>,
    /// See `back_hover`.
    pub back_disable: Option<TiledImage>,
    /// See `back_hover`.
    pub back_focus: Option<TiledImage>,
    /// Images of the states.
    pub image_hover: Option<String>,
    /// See `image_hover`.
    pub image_press: Option<String>,
    /// See `image_hover`.
    pub image_disable: Option<String>,
    /// See `image_hover`.
    pub image_focus: Option<String>,
    /// Animations of the state images, each advancing while shown.
    pub anim_hover: Option<AnimState>,
    /// See `anim_hover`.
    pub anim_press: Option<AnimState>,
    /// See `anim_hover`.
    pub anim_disable: Option<AnimState>,
    /// See `anim_hover`.
    pub anim_focus: Option<AnimState>,
    /// Fonts of the states.
    pub font_hover: Option<String>,
    /// See `font_hover`.
    pub font_press: Option<String>,
    /// See `font_hover`.
    pub font_disable: Option<String>,
    /// See `font_hover`.
    pub font_focus: Option<String>,
    /// Alignment of the label.
    pub text_align: TextAlign,
    /// The mouse is over it, this frame.
    pub hover: bool,
    /// The mouse holds it down.
    pub press: bool,
    /// When a script's `Press()` started showing it pressed.
    pub one_time_press: Option<u64>,
    /// Stays drawn pressed (`PRESSED`).
    pub stay_pressed: bool,
    /// The image is centred in the button.
    pub center_image: bool,
    /// Only the opaque pixels of the image catch the mouse.
    pub pixel_perfect: bool,
    /// Who a press is reported to.
    pub listener: Option<ButtonListener>,
}

/// The state of a static text.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct StaticState {
    /// Horizontal alignment.
    pub text_align: TextAlign,
    /// Vertical placement.
    pub vertical_align: VerticalAlign,
}

/// The state of a text editor.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EditState {
    /// The font of the selected part of the text.
    pub font_selected: Option<String>,
    /// Start of the selection, in bytes of the text. A value past the
    /// end of the text stands for a character index past its last
    /// character, as far past it as the value is past the end (see
    /// [`selection_index`]): the original keeps the number a script or
    /// its constructor gave until the editor is next drawn.
    pub sel_start: usize,
    /// End of the selection and position of the caret, as `sel_start`.
    pub sel_end: usize,
    /// First character shown.
    pub scroll_offset: usize,
    /// Margin between the frame and the text.
    pub frame_width: i32,
    /// Longest text accepted; negative for no limit.
    pub max_length: i32,
    /// The caret.
    pub cursor_char: String,
    /// Milliseconds between blinks of the caret.
    pub cursor_blink_rate: i32,
    /// When the caret last blinked, on the wall clock.
    pub last_blink: u64,
    /// Whether the caret is shown.
    pub cursor_visible: bool,
    /// How far from the frame the text starts: the room left after the
    /// text and the caret when right-to-left text sits against the
    /// right edge (`CUIEdit::Display`'s `AlignOffset`), 0 otherwise.
    pub align_offset: i32,
    /// The parts of the text the editor draws this frame, laid out by
    /// the last update; not saved.
    #[serde(skip)]
    pub pieces: Vec<EditPiece>,
    /// The caret this frame, when the editor has the focus; not saved.
    #[serde(skip)]
    pub caret: Option<EditPiece>,
    /// The height the parts are drawn in: the taller letter of the two
    /// fonts; not saved.
    #[serde(skip)]
    pub line_height: i32,
}

/// A part of an editor's text as `CUIEdit::Display` draws it: the text
/// before the selection, the selection, the text after it, the caret.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EditPiece {
    /// The characters drawn.
    pub text: String,
    /// Where the part starts, from the editor's left edge.
    pub x: i32,
    /// The room left up to the right frame, which the part is cut at.
    pub width: i32,
    /// Whether it is drawn with `FONT_SELECTED`.
    pub selected: bool,
}

impl Default for EditState {
    fn default() -> Self {
        EditState {
            font_selected: None,
            // `m_SelStart = m_SelEnd = 10000`: past any text until the
            // editor is drawn.
            sel_start: 10000,
            sel_end: 10000,
            scroll_offset: 0,
            frame_width: 0,
            max_length: -1,
            cursor_char: "|".to_string(),
            // `GetCaretBlinkTime()` on a Windows left at its default.
            cursor_blink_rate: 530,
            last_blink: 0,
            cursor_visible: true,
            align_offset: 0,
            pieces: Vec::new(),
            caret: None,
            line_height: 0,
        }
    }
}

/// What a control is.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ControlKind {
    /// A window.
    Window(Box<WindowState>),
    /// A button.
    Button(Box<ButtonState>),
    /// A static text.
    Static(StaticState),
    /// A text editor.
    Edit(Box<EditState>),
    /// An entity container: the index of its entity in the world's list,
    /// `None` when the entity file could not be loaded.
    EntityContainer(Option<usize>),
}

/// An entity shown by a window's entity container (`CUIEntity`'s
/// `CAdEntity`): it lives outside the scene, at the container's position.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UiEntity {
    /// The entity, as its file defines it and scripts modify it.
    pub def: Entity,
    /// The file it was loaded from.
    pub filename: String,
    /// The animation of its sprite.
    pub anim: Option<AnimState>,
    /// The control showing it.
    pub container: NativeId,
    /// Whether its scripts pause with the game (`Freezable`); off for a
    /// container's entity unless a script turns it on.
    pub freezable: bool,
}

/// A control: a window, a button, a static text or an editor
/// (`CUIObject`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Control {
    /// Name used by scripts.
    pub name: String,
    /// Free caption.
    pub caption: String,
    /// Position, relative to the parent window.
    pub x: i32,
    /// See `x`.
    pub y: i32,
    /// Size.
    pub width: i32,
    /// See `width`.
    pub height: i32,
    /// Drawn and clickable.
    pub visible: bool,
    /// Ignored by the mouse and the focus.
    pub disabled: bool,
    /// A press raises an event named after the control on its parent.
    pub parent_notify: bool,
    /// Can take the keyboard focus.
    pub can_focus: bool,
    /// The label, the text, the title.
    pub text: String,
    /// Whether the control was ever given a text, even an empty one
    /// (`m_Text` is not null): a static sizes itself to an empty text
    /// and leaves its size alone without one.
    pub has_text: bool,
    /// The font of the text.
    pub font: Option<String>,
    /// The image or sprite.
    pub image: Option<String>,
    /// The tiled background.
    pub back: Option<TiledImage>,
    /// The cursor over the control.
    pub cursor: Option<String>,
    /// The parent window.
    pub parent: Option<NativeId>,
    /// What it is.
    pub kind: ControlKind,
    /// The animation of the image shown.
    pub anim: Option<AnimState>,
    /// The height of the text in the control's width, measured this
    /// frame.
    pub text_height: i32,
}

impl Control {
    fn new(kind: ControlKind, parent: Option<NativeId>) -> Control {
        let can_focus = matches!(kind, ControlKind::Window(_) | ControlKind::Edit(_));
        Control {
            name: luma_formats::UNNAMED.to_string(),
            caption: String::new(),
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            visible: true,
            disabled: false,
            parent_notify: false,
            can_focus,
            text: String::new(),
            // An editor is made with an empty text (`SetText("")`).
            has_text: matches!(kind, ControlKind::Edit(_)),
            font: None,
            image: None,
            back: None,
            cursor: None,
            parent,
            kind,
            anim: None,
            text_height: 0,
        }
    }

    fn apply_base(&mut self, def: &ControlDef) {
        self.name = def.name.clone();
        self.caption = def.caption.clone();
        self.x = def.x;
        self.y = def.y;
        self.width = def.width;
        self.height = def.height;
        self.visible = def.visible;
        self.disabled = def.disabled;
        self.parent_notify = def.parent_notify;
        self.font = def.font.clone();
        self.image = def.image.clone();
        self.cursor = def.cursor.clone();
    }

    /// The window state, if the control is a window.
    pub fn window(&self) -> Option<&WindowState> {
        match &self.kind {
            ControlKind::Window(w) => Some(w),
            _ => None,
        }
    }

    /// The window state, mutable.
    pub fn window_mut(&mut self) -> Option<&mut WindowState> {
        match &mut self.kind {
            ControlKind::Window(w) => Some(w),
            _ => None,
        }
    }

    /// The button state, if the control is a button.
    pub fn button(&self) -> Option<&ButtonState> {
        match &self.kind {
            ControlKind::Button(b) => Some(b),
            _ => None,
        }
    }

    /// The button state, mutable.
    pub fn button_mut(&mut self) -> Option<&mut ButtonState> {
        match &mut self.kind {
            ControlKind::Button(b) => Some(b),
            _ => None,
        }
    }

    /// The editor state, if the control is an editor.
    pub fn edit(&self) -> Option<&EditState> {
        match &self.kind {
            ControlKind::Edit(e) => Some(e),
            _ => None,
        }
    }

    /// The editor state, mutable.
    pub fn edit_mut(&mut self) -> Option<&mut EditState> {
        match &mut self.kind {
            ControlKind::Edit(e) => Some(e),
            _ => None,
        }
    }

    /// The static state, if the control is a static text.
    pub fn static_text(&self) -> Option<&StaticState> {
        match &self.kind {
            ControlKind::Static(s) => Some(s),
            _ => None,
        }
    }

    /// The static state, mutable.
    pub fn static_mut(&mut self) -> Option<&mut StaticState> {
        match &mut self.kind {
            ControlKind::Static(s) => Some(s),
            _ => None,
        }
    }

    /// The engine's type name (`Type`).
    pub fn type_name(&self) -> &'static str {
        match &self.kind {
            ControlKind::Window(_) => "window",
            ControlKind::Button(_) => "button",
            ControlKind::Static(_) => "static",
            ControlKind::Edit(_) => "editor",
            ControlKind::EntityContainer(_) => "entity container",
        }
    }

    /// The image file in a slot, when the control has that slot and an
    /// image in it.
    pub fn image_in(&self, slot: ImageSlot) -> Option<&str> {
        match slot {
            ImageSlot::Image => self.image.as_deref(),
            ImageSlot::Inactive => self.window()?.image_inactive.as_deref(),
            ImageSlot::Hover => self.button()?.image_hover.as_deref(),
            ImageSlot::Press => self.button()?.image_press.as_deref(),
            ImageSlot::Disable => self.button()?.image_disable.as_deref(),
            ImageSlot::Focus => self.button()?.image_focus.as_deref(),
        }
    }

    /// The animation of the image in a slot, if the control has the
    /// slot and the image has been shown.
    pub fn anim_in(&self, slot: ImageSlot) -> Option<&AnimState> {
        match slot {
            ImageSlot::Image => self.anim.as_ref(),
            ImageSlot::Inactive => self.window()?.anim_inactive.as_ref(),
            ImageSlot::Hover => self.button()?.anim_hover.as_ref(),
            ImageSlot::Press => self.button()?.anim_press.as_ref(),
            ImageSlot::Disable => self.button()?.anim_disable.as_ref(),
            ImageSlot::Focus => self.button()?.anim_focus.as_ref(),
        }
    }

    /// The place of the animation of a slot's image, for creating or
    /// replacing it; `None` when the control has no such slot.
    pub fn anim_slot_mut(&mut self, slot: ImageSlot) -> Option<&mut Option<AnimState>> {
        match slot {
            ImageSlot::Image => Some(&mut self.anim),
            ImageSlot::Inactive => Some(&mut self.window_mut()?.anim_inactive),
            ImageSlot::Hover => Some(&mut self.button_mut()?.anim_hover),
            ImageSlot::Press => Some(&mut self.button_mut()?.anim_press),
            ImageSlot::Disable => Some(&mut self.button_mut()?.anim_disable),
            ImageSlot::Focus => Some(&mut self.button_mut()?.anim_focus),
        }
    }

    /// The frame shown of a slot's image: its animation's, else the
    /// first.
    fn frame_in(&self, slot: ImageSlot) -> usize {
        self.anim_in(slot).map_or(0, |a| a.current_frame)
    }

    /// The image a button shows now: the slot of its state, when that
    /// state has an image, else the plain image (`CUIButton::Display`).
    fn button_image_slot(&self, focused: bool) -> Option<ImageSlot> {
        let b = self.button()?;
        let slot = if self.disabled {
            Some(ImageSlot::Disable)
        } else if self.pressed() {
            Some(ImageSlot::Press)
        } else if b.hover {
            Some(ImageSlot::Hover)
        } else if self.can_focus && focused {
            Some(ImageSlot::Focus)
        } else {
            None
        };
        slot.filter(|s| self.image_in(*s).is_some())
            .or_else(|| self.image.as_ref().map(|_| ImageSlot::Image))
    }

    /// Sets the text; `|` becomes a line break (`SetText`). An editor's
    /// selection keeps its character indexes, as the original keeps
    /// `m_SelStart` and `m_SelEnd` untouched: past the end of a shorter
    /// text until the editor is next drawn.
    pub fn set_text(&mut self, text: &str) {
        let text = luma_formats::window::control_text(text);
        if let ControlKind::Edit(e) = &mut self.kind {
            let start = selection_index(&self.text, e.sel_start);
            let end = selection_index(&self.text, e.sel_end);
            e.sel_start = selection_offset(&text, start);
            e.sel_end = selection_offset(&text, end);
        }
        self.text = text;
        self.has_text = true;
    }

    /// Whether the control is drawn pressed.
    pub fn pressed(&self) -> bool {
        self.button()
            .is_some_and(|b| b.press || b.one_time_press.is_some() || b.stay_pressed)
    }
}

/// The colour a window forces on the texts drawn in it, from the tint
/// its images are drawn with (`m_ForceAlphaColor`, set by `ALPHA_COLOR`
/// and `ALPHA`): none when nothing is tinted.
fn forced_color(tint: Rgba) -> Option<Rgba> {
    (tint != Rgba::OPAQUE_WHITE).then_some(tint)
}

/// The character index a selection end stands for (`m_SelStart`,
/// `m_SelEnd`): the characters before its byte offset, or, past the end
/// of the text, as many more as it is past it.
pub(crate) fn selection_index(text: &str, at: usize) -> usize {
    match at.checked_sub(text.len()) {
        Some(past) => text.chars().count().saturating_add(past),
        None => text[..char_floor(text, at)].chars().count(),
    }
}

/// The selection end of a character index: the byte offset of that
/// character, or, past the last character, as far past the end of the
/// text.
pub(crate) fn selection_offset(text: &str, index: usize) -> usize {
    match text.char_indices().nth(index) {
        Some((at, _)) => at,
        None => text
            .len()
            .saturating_add(index - text.chars().count().min(index)),
    }
}

/// The windows of a game and what freezing them holds.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Ui {
    /// Every control by handle.
    pub controls: HashMap<NativeId, Control>,
    /// The top-level windows, in order; the last is the topmost.
    pub windows: Vec<NativeId>,
    /// The window the keys go to.
    pub focused_window: Option<NativeId>,
    /// The sprites of the controls; loaded again after a saved game,
    /// except those scripts edited.
    pub bank: SpriteBank,
    /// The object the left button went down on (`m_CapturedObject`).
    pub captured: Option<NativeId>,
    /// Whether the game is frozen by a system-exclusive window.
    pub frozen: bool,
    freeze_level: u32,
    orig_interactive: bool,
    /// Mode changes asked by property writes, applied with the engine at
    /// the next window update.
    pending_modes: Vec<(NativeId, PendingMode)>,
    /// `Freezable` writes on entity containers, applied to the entity's
    /// scripts with the engine at the next window update or freeze.
    pub(crate) pending_freezable: Vec<(NativeId, bool)>,
}

/// A window mode change waiting for the script engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum PendingMode {
    /// `SystemExclusive = true`.
    SystemExclusive,
    /// `Exclusive = false` or `SystemExclusive = false` on a
    /// system-exclusive window.
    CloseAndShow,
}

/// A rectangle contains a point.
fn rect_contains(rect: SpriteRect, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

impl World {
    /// The windows and their controls.
    pub fn ui(&self) -> &Ui {
        &self.ui
    }

    /// A control by handle.
    pub fn control(&self, id: NativeId) -> Option<&Control> {
        self.ui.controls.get(&id)
    }

    /// A control by handle, mutable.
    pub fn control_mut(&mut self, id: NativeId) -> Option<&mut Control> {
        self.ui.controls.get_mut(&id)
    }

    /// The top-level windows, in order.
    pub fn window_ids(&self) -> &[NativeId] {
        &self.ui.windows
    }

    /// The window the keys go to.
    pub fn focused_window(&self) -> Option<NativeId> {
        self.ui.focused_window
    }

    /// Whether a system-exclusive window froze the game.
    pub fn is_frozen(&self) -> bool {
        self.ui.frozen
    }

    // ----------------------------------------------------------------
    // Loading
    // ----------------------------------------------------------------

    /// Reads a `.window` file (`CUIWindow::LoadFile`), with the `TEMPLATE`
    /// files it names, of the window and of its controls, loaded where
    /// the keys stand. A file that cannot be read, a key the original
    /// does not know or a template that cannot be loaded gives no
    /// window.
    fn read_window_def(&mut self, path: &str) -> Option<WindowDef> {
        let bytes = match self.vfs.read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.log(format!(
                    "CUIWindow::LoadFile failed for file '{path}': {err}"
                ));
                return None;
            }
        };
        let vfs = &self.vfs;
        let mut read = |file: &str| vfs.read(file).ok();
        match WindowDef::parse_with(&bytes, &mut luma_formats::Files::new(&mut read)) {
            Ok(def) => Some(def),
            Err(err) => {
                self.log(format!("Error parsing WINDOW file '{path}': {err}"));
                None
            }
        }
    }

    /// Loads a window file and adds the window to the game
    /// (`Game.LoadWindow`); `None` when the file cannot be read.
    pub fn load_window(&mut self, engine: &mut Engine, path: &str) -> Option<NativeId> {
        let def = self.read_window_def(path)?;
        let id = self.new_control(ControlKind::Window(Box::default()), None);
        self.ui.windows.push(id);
        if !self.apply_window_def(engine, id, def, Some(path)) {
            // A background, an image, a font, a cursor or a control that
            // does not load fails the window (`CUIWindow::LoadBuffer`).
            self.log(format!("Error loading WINDOW definition '{path}'"));
            self.unload_object(engine, id);
            return None;
        }
        Some(id)
    }

    /// Adds an empty window to the game (`Game.CreateWindow`).
    pub fn create_window(&mut self, name: Option<&str>) -> NativeId {
        let id = self.new_control(ControlKind::Window(Box::default()), None);
        self.ui.windows.push(id);
        if let (Some(name), Some(control)) = (name, self.control_mut(id)) {
            control.name = name.to_string();
        }
        id
    }

    /// Reads a window file into an existing window (`LoadFromFile`).
    pub fn load_window_into(&mut self, engine: &mut Engine, id: NativeId, path: &str) -> bool {
        let Some(def) = self.read_window_def(path) else {
            return false;
        };
        if self.control(id).and_then(Control::window).is_none() {
            return false;
        }
        self.apply_window_def(engine, id, def, Some(path))
    }

    fn new_control(&mut self, kind: ControlKind, parent: Option<NativeId>) -> NativeId {
        let id = self.objects.insert(Kind::Control);
        self.ui.controls.insert(id, Control::new(kind, parent));
        id
    }

    /// A new empty window control, outside the game's window list (the
    /// inventory box's).
    pub(crate) fn new_window_control(&mut self, parent: Option<NativeId>) -> NativeId {
        self.new_control(ControlKind::Window(Box::default()), parent)
    }

    /// A new named button reporting to a listener, outside its parent's
    /// control list (a shield).
    pub(crate) fn new_button_control(
        &mut self,
        parent: Option<NativeId>,
        name: &str,
        listener: ButtonListener,
    ) -> NativeId {
        let state = ButtonState {
            listener: Some(listener),
            ..ButtonState::default()
        };
        let id = self.new_control(ControlKind::Button(Box::new(state)), parent);
        if let Some(c) = self.control_mut(id) {
            c.name = name.to_string();
        }
        id
    }

    /// Loads a sprite or image for the controls.
    pub fn load_ui_image(&mut self, path: &str) -> bool {
        let ok = self.ui.bank.add_sprite(&self.vfs, path);
        if !ok {
            if let Some((file, reason)) = self.ui.bank.missing().last() {
                let (file, reason) = (file.clone(), reason.clone());
                self.log(format!("Error loading image '{file}': {reason}"));
            }
        }
        ok
    }

    /// Loads a `.tiled` file and its image; the tiles left empty are
    /// thirds of the image.
    fn load_tiled(&mut self, path: &str) -> Option<TiledImage> {
        let bytes = match self.vfs.read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.log(format!("Error opening tiled image '{path}': {err}"));
                return None;
            }
        };
        // A `TEMPLATE` is another `.tiled` file, loaded where the key
        // stands (`CUITiledImage::LoadBuffer`).
        let parsed = {
            let vfs = &self.vfs;
            let mut read = |file: &str| vfs.read(file).ok();
            TiledImage::parse_with(&bytes, &mut luma_formats::Files::new(&mut read))
        };
        let mut tiled = match parsed {
            Ok(tiled) => tiled,
            Err(err) => {
                self.log(format!("Error parsing TILED_IMAGE file '{path}': {err}"));
                return None;
            }
        };
        if let Some(image) = tiled.image.clone() {
            if self.load_ui_image(&image) {
                if let Some(mask) = self.ui.bank.mask(&self.vfs, &image, DEFAULT_COLOR_KEY) {
                    let (w, h) = (mask.width as i32, mask.height as i32);
                    tiled.fill_defaults(w, h);
                }
            }
        }
        Some(tiled)
    }

    /// Loads the font a control names; `ok` turns false when it names one
    /// that does not load, which fails the control in the original.
    fn load_control_font(&mut self, path: &Option<String>, ok: &mut bool) -> Option<String> {
        let path = path.as_deref()?;
        if self.load_font(path) {
            Some(path.to_string())
        } else {
            *ok = false;
            None
        }
    }

    /// Loads the image a control names; see [`World::load_control_font`].
    /// An image that does not exist is the placeholder and loads.
    fn load_control_image(&mut self, path: &Option<String>, ok: &mut bool) -> Option<String> {
        let path = path.as_deref()?;
        if self.load_ui_image(path) {
            Some(path.to_string())
        } else {
            *ok = false;
            None
        }
    }

    /// Loads the tiled image a control names; see
    /// [`World::load_control_font`].
    fn load_control_back(&mut self, path: &Option<String>, ok: &mut bool) -> Option<TiledImage> {
        let path = path.as_deref()?;
        let tiled = self.load_tiled(path);
        if tiled.is_none() {
            *ok = false;
        }
        tiled
    }

    /// Fills a control's common fields from a definition, loading what
    /// it names. `false` when something it names does not load: the
    /// original's loaders of windows and controls stop there and the
    /// control fails.
    fn apply_control_def(&mut self, engine: &mut Engine, id: NativeId, def: &ControlDef) -> bool {
        let mut ok = true;
        let font = self.load_control_font(&def.font, &mut ok);
        let image = self.load_control_image(&def.image, &mut ok);
        let back = self.load_control_back(&def.back, &mut ok);
        if let Some(cursor) = &def.cursor {
            ok &= self.load_cursor(cursor);
        }
        if let Some(control) = self.control_mut(id) {
            control.apply_base(def);
            control.font = font;
            control.image = image;
            control.back = back;
            control.anim = control.image.as_deref().map(AnimState::new);
        }
        for script in &def.scripts {
            self.attach(engine, id, script);
        }
        ok
    }

    /// Fills a window from its definition and creates its controls
    /// (`CUIWindow::LoadBuffer`). `false` when the window fails: a
    /// background, an image, a font, a cursor or a control of it does
    /// not load. The caller then drops the window.
    pub(crate) fn apply_window_def(
        &mut self,
        engine: &mut Engine,
        id: NativeId,
        def: WindowDef,
        filename: Option<&str>,
    ) -> bool {
        let mut ok = self.apply_control_def(engine, id, &def.base);
        let title = self.expand(&def.title);
        let back_inactive = self.load_control_back(&def.back_inactive, &mut ok);
        let image_inactive = self.load_control_image(&def.image_inactive, &mut ok);
        let font_inactive = self.load_control_font(&def.font_inactive, &mut ok);
        if let Some(control) = self.control_mut(id) {
            control.text = title;
            control.can_focus = true;
            if let Some(w) = control.window_mut() {
                w.filename = filename.map(str::to_string);
                w.title_align = def.title_align;
                w.title_rect = def.title_rect;
                w.drag_rect = def.drag_rect;
                w.transparent = def.transparent;
                w.menu = def.menu;
                w.in_game = def.in_game;
                w.clip_contents = def.clip_contents;
                w.pause_music = def.pause_music;
                w.fade_color = def.fade_color;
                w.alpha_color = def.alpha_color;
                w.back_inactive = back_inactive;
                w.image_inactive = image_inactive;
                w.font_inactive = font_inactive;
            }
        }
        for widget in def.widgets {
            ok &= self.create_widget_checked(engine, id, widget).1;
        }
        self.correct_size(id);
        ok
    }

    /// Adds a control from its definition to a window.
    pub fn create_widget(
        &mut self,
        engine: &mut Engine,
        parent: NativeId,
        def: WidgetDef,
    ) -> NativeId {
        self.create_widget_checked(engine, parent, def).0
    }

    /// Adds a control from its definition to a window; the flag is false
    /// when the control fails to load (something it names does not load,
    /// or its entity for a container), which fails its window.
    fn create_widget_checked(
        &mut self,
        engine: &mut Engine,
        parent: NativeId,
        def: WidgetDef,
    ) -> (NativeId, bool) {
        let (id, ok) = match def {
            WidgetDef::Button(def) => {
                let id = self.new_control(ControlKind::Button(Box::default()), Some(parent));
                (id, self.apply_button_def(engine, id, &def))
            }
            WidgetDef::Static(def) => {
                let id =
                    self.new_control(ControlKind::Static(StaticState::default()), Some(parent));
                (id, self.apply_static_def(engine, id, &def))
            }
            WidgetDef::Edit(def) => {
                let id = self.new_control(ControlKind::Edit(Box::default()), Some(parent));
                (id, self.apply_edit_def(engine, id, &def))
            }
            WidgetDef::Window(def) => {
                let id = self.new_control(ControlKind::Window(Box::default()), Some(parent));
                (id, self.apply_window_def(engine, id, *def, None))
            }
            WidgetDef::EntityContainer(def) => {
                let id = self.new_control(ControlKind::EntityContainer(None), Some(parent));
                let mut ok = self.apply_control_def(engine, id, &def.base);
                if let Some(control) = self.control_mut(id) {
                    control.can_focus = false;
                }
                if let Some(path) = &def.entity {
                    ok &= self.set_container_entity(engine, id, path);
                }
                // `CorrectSize`: a container has no image, so it is 100
                // wide and keeps its height.
                self.correct_size(id);
                (id, ok)
            }
        };
        if let Some(window) = self.control_mut(parent).and_then(Control::window_mut) {
            window.widgets.push(id);
        }
        (id, ok)
    }

    /// Adds an empty control to a window (`CreateButton`, `CreateStatic`,
    /// `CreateEditor`, `CreateWindow` on a window).
    pub fn create_control(
        &mut self,
        parent: NativeId,
        kind: ControlKind,
        name: Option<&str>,
    ) -> NativeId {
        let id = self.new_control(kind, Some(parent));
        if let (Some(name), Some(control)) = (name, self.control_mut(id)) {
            control.name = name.to_string();
        }
        if let Some(window) = self.control_mut(parent).and_then(Control::window_mut) {
            window.widgets.push(id);
        }
        id
    }

    /// `CUIButton::LoadBuffer`, on a parsed definition; `false` when the
    /// button fails (something it names does not load).
    fn apply_button_def(&mut self, engine: &mut Engine, id: NativeId, def: &ButtonDef) -> bool {
        let mut ok = self.apply_control_def(engine, id, &def.base);
        let text = self.expand(&def.text);
        let state = ButtonState {
            back_hover: self.load_control_back(&def.back_hover, &mut ok),
            back_press: self.load_control_back(&def.back_press, &mut ok),
            back_disable: self.load_control_back(&def.back_disable, &mut ok),
            back_focus: self.load_control_back(&def.back_focus, &mut ok),
            image_hover: self.load_control_image(&def.image_hover, &mut ok),
            image_press: self.load_control_image(&def.image_press, &mut ok),
            image_disable: self.load_control_image(&def.image_disable, &mut ok),
            image_focus: self.load_control_image(&def.image_focus, &mut ok),
            anim_hover: None,
            anim_press: None,
            anim_disable: None,
            anim_focus: None,
            font_hover: self.load_control_font(&def.font_hover, &mut ok),
            font_press: self.load_control_font(&def.font_press, &mut ok),
            font_disable: self.load_control_font(&def.font_disable, &mut ok),
            font_focus: self.load_control_font(&def.font_focus, &mut ok),
            text_align: def.text_align,
            hover: false,
            press: false,
            one_time_press: None,
            stay_pressed: def.pressed,
            center_image: def.center_image,
            pixel_perfect: def.pixel_perfect,
            listener: None,
        };
        if let Some(control) = self.control_mut(id) {
            control.text = text;
            control.can_focus = def.focusable;
            control.kind = ControlKind::Button(Box::new(state));
        }
        self.correct_size(id);
        ok
    }

    /// `CUIText::LoadBuffer`, on a parsed definition; `false` when the
    /// control fails.
    fn apply_static_def(&mut self, engine: &mut Engine, id: NativeId, def: &StaticDef) -> bool {
        let ok = self.apply_control_def(engine, id, &def.base);
        let text = self.expand(&def.text);
        if let Some(control) = self.control_mut(id) {
            // A file's empty `TEXT` is not told from none.
            control.has_text |= !text.is_empty();
            control.text = text;
            control.can_focus = false;
            control.kind = ControlKind::Static(StaticState {
                text_align: def.text_align,
                vertical_align: def.vertical_align,
            });
        }
        self.correct_size(id);
        ok
    }

    /// `CUIEdit::LoadBuffer`, on a parsed definition; `false` when the
    /// control fails. The text goes through the string table.
    fn apply_edit_def(&mut self, engine: &mut Engine, id: NativeId, def: &EditDef) -> bool {
        let mut ok = self.apply_control_def(engine, id, &def.base);
        let font_selected = self.load_control_font(&def.font_selected, &mut ok);
        // `Game->m_StringTable->Expand(&m_Text)`; an editor holds
        // characters.
        let text = self.display_text(&self.expand(&def.text));
        if let Some(control) = self.control_mut(id) {
            control.set_text(&text);
            control.can_focus = true;
            control.kind = ControlKind::Edit(Box::new(EditState {
                font_selected,
                frame_width: def.frame_width,
                max_length: def.max_length,
                cursor_blink_rate: def.cursor_blink_rate,
                ..EditState::default()
            }));
        }
        self.correct_size(id);
        ok
    }

    /// The bounding size of a sprite placed at the origin
    /// (`CBSprite::GetBoundingRect`): the rectangle around the
    /// sub-frames of every frame, reading the images for the sub-frames
    /// without a rectangle.
    fn sprite_size(&mut self, path: &str) -> Option<(i32, i32)> {
        let subframes: Vec<luma_formats::SubFrame> = self
            .ui
            .bank
            .sprite(path)?
            .sprite
            .frames
            .iter()
            .flat_map(|f| f.subframes.iter().cloned())
            .collect();
        let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for sub in &subframes {
            let (w, h) = match sub.rect.filter(|r| !r.is_empty()) {
                Some(r) => (r.width() as i32, r.height() as i32),
                None => {
                    let Some(image) = &sub.image else { continue };
                    let Some(mask) = self.ui.bank.mask(&self.vfs, image, sub.color_key) else {
                        continue;
                    };
                    (mask.width as i32, mask.height as i32)
                }
            };
            // `UnionRect` leaves an empty rectangle out.
            if w <= 0 || h <= 0 {
                continue;
            }
            left = left.min(-sub.hotspot_x);
            top = top.min(-sub.hotspot_y);
            right = right.max(-sub.hotspot_x + w);
            bottom = bottom.max(-sub.hotspot_y + h);
        }
        if left > right {
            return None;
        }
        Some((right - left, bottom - top))
    }

    /// Gives a control without a size the size of its image, else 100
    /// wide; a button grows to its text; a tiled background rounds the
    /// size to whole tiles (`CorrectSize`).
    pub(crate) fn correct_size(&mut self, id: NativeId) {
        let Some(control) = self.control(id) else {
            return;
        };
        let image = match &control.kind {
            ControlKind::Button(b) => control
                .image
                .clone()
                .or_else(|| b.image_disable.clone())
                .or_else(|| b.image_hover.clone())
                .or_else(|| b.image_press.clone())
                .or_else(|| b.image_focus.clone()),
            _ => control.image.clone(),
        };
        let (mut width, mut height) = (control.width, control.height);
        let is_button = control.button().is_some();
        let text = control.text.clone();
        let font = control.font.clone();
        let back = control.back.clone();
        let size = image.as_deref().and_then(|p| self.sprite_size(p));
        if width <= 0 {
            width = size.map_or(100, |(w, _)| w);
        }
        if height <= 0 {
            height = size.map_or(0, |(_, h)| h);
        }
        if is_button {
            if !text.is_empty() {
                let text_height = self.measure_text_height(font.as_deref(), &text, width);
                height = height.max(text_height);
            }
            if height <= 0 {
                height = 100;
            }
        }
        if let Some(back) = &back {
            let (w, h) = back.corrected_size(width, height);
            width = w;
            height = h;
        }
        if let Some(control) = self.control_mut(id) {
            control.width = width;
            control.height = height;
        }
    }

    /// The height of a text in a font (the system font without one).
    fn measure_text_height(&mut self, font: Option<&str>, text: &str, width: i32) -> i32 {
        let path = font
            .map(str::to_string)
            .or_else(|| self.system_font.clone());
        let Some(path) = path else { return 0 };
        self.fonts
            .get_mut(&path)
            .map_or(0, |f| f.text_height(text, width))
    }

    /// The width of a text in a font.
    fn measure_text_width(&mut self, font: Option<&str>, text: &str) -> i32 {
        let path = font
            .map(str::to_string)
            .or_else(|| self.system_font.clone());
        let Some(path) = path else { return 0 };
        self.fonts.get_mut(&path).map_or(0, |f| f.text_width(text))
    }

    /// Sets a static's size to its text (`CUIText::SizeToFit`), or only
    /// its height (`HeightToFit`): the width of the text on one line,
    /// then its height in that width. Nothing without a font of its own
    /// or without a text; an empty text is a text, 0 wide and as high as
    /// its font makes an empty text (a line of a bitmap font, 0 for a
    /// TrueType font).
    pub fn size_static_to_fit(&mut self, id: NativeId, width_too: bool) {
        let Some(control) = self.control(id) else {
            return;
        };
        if control.font.is_none() || !control.has_text {
            return;
        }
        let (font, text, width) = (control.font.clone(), control.text.clone(), control.width);
        let width = if width_too {
            self.measure_text_width(font.as_deref(), &text)
        } else {
            width
        };
        let height = self.measure_text_height(font.as_deref(), &text, width);
        if let Some(control) = self.control_mut(id) {
            control.width = width;
            control.height = height;
        }
    }

    // ----------------------------------------------------------------
    // The tree
    // ----------------------------------------------------------------

    /// The direct controls of a window.
    pub fn widgets_of(&self, id: NativeId) -> Vec<NativeId> {
        self.control(id)
            .and_then(Control::window)
            .map(|w| w.widgets.clone())
            .unwrap_or_default()
    }

    /// A direct control of a window by name (ignoring case) or by index
    /// (`GetWidget`, `GetControl`).
    pub fn widget(&self, window: NativeId, name: &str) -> Option<NativeId> {
        self.widgets_of(window).into_iter().find(|w| {
            self.control(*w)
                .is_some_and(|c| c.name.eq_ignore_ascii_case(name))
        })
    }

    /// A direct control of a window by index.
    pub fn widget_at(&self, window: NativeId, index: i32) -> Option<NativeId> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.widgets_of(window).get(i).copied())
    }

    /// Every control under a window, depth first, the window excluded.
    fn descendants(&self, id: NativeId) -> Vec<NativeId> {
        let mut out = Vec::new();
        let mut stack = self.widgets_of(id);
        if let Some(shield) = self
            .control(id)
            .and_then(Control::window)
            .and_then(|w| w.shield)
        {
            stack.push(shield);
        }
        while let Some(next) = stack.pop() {
            out.push(next);
            stack.extend(self.widgets_of(next));
            if let Some(shield) = self
                .control(next)
                .and_then(Control::window)
                .and_then(|w| w.shield)
            {
                stack.push(shield);
            }
        }
        out
    }

    /// The top-level window a control belongs to.
    fn top_window(&self, id: NativeId) -> NativeId {
        let mut current = id;
        while let Some(parent) = self.control(current).and_then(|c| c.parent) {
            current = parent;
        }
        current
    }

    /// The screen position of a control: its position plus its parents'.
    pub fn control_screen_position(&self, id: NativeId) -> (i32, i32) {
        let (mut x, mut y) = (0, 0);
        let mut current = Some(id);
        while let Some(c) = current.and_then(|i| self.control(i)) {
            x += c.x;
            y += c.y;
            current = c.parent;
        }
        (x, y)
    }

    /// Removes a window or a control with everything under it, ending
    /// their scripts; a system-exclusive window unfreezes the game.
    pub(crate) fn unload_control(&mut self, engine: &mut Engine, id: NativeId) -> bool {
        if !self.ui.controls.contains_key(&id) {
            return false;
        }
        if self
            .control(id)
            .and_then(Control::window)
            .is_some_and(|w| w.mode == WindowMode::SystemExclusive)
        {
            self.unfreeze(engine);
        }
        let mut all = self.descendants(id);
        all.push(id);
        let sprites: Vec<NativeId> = self
            .objects
            .iter()
            .filter(|(_, n)| {
                matches!(n.kind, Kind::ControlImage { control, .. } if all.contains(&control))
            })
            .map(|(id, _)| id)
            .collect();
        for sprite in sprites {
            self.drop_sprite_object(sprite);
        }
        for control in &all {
            // A container takes its entity with it (`~CUIEntity`).
            self.drop_container_entity(engine, *control);
            for script in engine.scripts_of(*control) {
                engine.finish(script, true);
            }
            self.ui.controls.remove(control);
            self.objects.remove(*control);
            if self.active_object == Some(*control) {
                self.active_object = None;
            }
            if self.ui.captured == Some(*control) {
                self.ui.captured = None;
            }
        }
        self.ui.windows.retain(|w| *w != id);
        if let Some(parent) = self.control(id).and_then(|c| c.parent) {
            if let Some(window) = self.control_mut(parent).and_then(Control::window_mut) {
                window.widgets.retain(|w| *w != id);
                if window.focused_widget == Some(id) {
                    window.focused_widget = None;
                }
                if window.shield == Some(id) {
                    window.shield = None;
                }
            }
        }
        for control in self.ui.controls.values_mut() {
            if let Some(window) = control.window_mut() {
                window.widgets.retain(|w| !all.contains(w));
                if window.focused_widget.is_some_and(|f| all.contains(&f)) {
                    window.focused_widget = None;
                }
            }
        }
        if self.ui.focused_window.is_some_and(|f| all.contains(&f)) {
            self.ui.focused_window = None;
        }
        true
    }

    // ----------------------------------------------------------------
    // Focus and modes
    // ----------------------------------------------------------------

    /// Brings a top-level window to the top and gives it the keys
    /// (`CBGame::FocusWindow`); focusing a normal window while an
    /// exclusive one was focused keeps the exclusive one on top. A
    /// window that is already the last of the list is left alone, as in
    /// the original: it gets the keys from the next frame, when the
    /// topmost visible window is focused again
    /// ([`World::reset_focused_window`]).
    pub fn focus_window(&mut self, id: NativeId) -> bool {
        let previous = self.ui.focused_window;
        let Some(index) = self.ui.windows.iter().position(|w| *w == id) else {
            return false;
        };
        if index + 1 < self.ui.windows.len() {
            self.ui.windows.remove(index);
            self.ui.windows.push(id);
            self.ui.focused_window = Some(id);
        }
        let mode = self.control(id).and_then(Control::window).map(|w| w.mode);
        if mode == Some(WindowMode::Normal) {
            if let Some(prev) = previous.filter(|p| *p != id) {
                let prev_mode = self.control(prev).and_then(Control::window).map(|w| w.mode);
                if matches!(
                    prev_mode,
                    Some(WindowMode::Exclusive | WindowMode::SystemExclusive)
                ) {
                    return self.focus_window(prev);
                }
            }
        }
        true
    }

    /// The start of a frame (`CBGame::InitLoop`): the focused window is
    /// the topmost visible one, the last of the list, whatever was
    /// focused before. So a window loaded or shown above the others
    /// takes the keys, and what a click or `Focus` changes for good is
    /// the order of the windows. A disabled window found here gives way
    /// to the topmost enabled one when the windows update
    /// ([`World::update_ui`], `CBGame::DisplayWindows`).
    pub(crate) fn reset_focused_window(&mut self) {
        self.ui.focused_window = self
            .ui
            .windows
            .iter()
            .rev()
            .copied()
            .find(|w| self.control(*w).is_some_and(|c| c.visible));
    }

    /// Gives a control the focus: its window chain gets focused and it
    /// becomes the focused control of its parent, unless a window above
    /// it is disabled (`CUIObject::Focus`).
    pub fn focus_control(&mut self, id: NativeId) {
        let mut current = Some(id);
        while let Some(c) = current.and_then(|i| self.control(i)) {
            if c.disabled && c.window().is_some() {
                return;
            }
            current = c.parent;
        }
        current = Some(id);
        while let Some(i) = current {
            let Some(c) = self.control(i) else { break };
            let (parent, disabled, can_focus, is_window) =
                (c.parent, c.disabled, c.can_focus, c.window().is_some());
            match parent {
                Some(parent) => {
                    if !disabled && can_focus {
                        if let Some(w) = self.control_mut(parent).and_then(Control::window_mut) {
                            w.focused_widget = Some(i);
                        }
                    }
                }
                None => {
                    if is_window {
                        self.focus_window(i);
                    }
                }
            }
            current = parent;
        }
    }

    /// Whether a control is on the focus chain of the focused window
    /// (`CUIObject::IsFocused`).
    pub fn is_focused(&self, id: NativeId) -> bool {
        let mut current = self.ui.focused_window;
        while let Some(c) = current {
            if c == id {
                return true;
            }
            current = self
                .control(c)
                .and_then(Control::window)
                .and_then(|w| w.focused_widget);
        }
        false
    }

    /// Moves a window's focus to the next (or previous) focusable
    /// control (`MoveFocus`); `false` when there is none.
    pub fn move_focus(&mut self, window: NativeId, forward: bool) -> bool {
        let widgets = self.widgets_of(window);
        if widgets.is_empty() {
            return false;
        }
        let focused = self
            .control(window)
            .and_then(Control::window)
            .and_then(|w| w.focused_widget);
        let start = focused.and_then(|f| widgets.iter().position(|w| *w == f));
        let n = widgets.len();
        let mut i = start.unwrap_or(0);
        for _ in 0..=n {
            let candidate = widgets[i];
            let ok = Some(candidate) != focused
                && self
                    .control(candidate)
                    .is_some_and(|c| c.can_focus && c.visible && !c.disabled);
            if ok {
                if let Some(w) = self.control_mut(window).and_then(Control::window_mut) {
                    w.focused_widget = Some(candidate);
                }
                return true;
            }
            i = if forward {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            };
        }
        false
    }

    /// Makes a window exclusive: visible, enabled, focused, with the
    /// mouse to itself (`GoExclusive`). Fails on a system-exclusive one.
    pub fn go_exclusive(&mut self, id: NativeId) -> bool {
        let Some(control) = self.control_mut(id) else {
            return false;
        };
        let Some(window) = control.window_mut() else {
            return false;
        };
        match window.mode {
            WindowMode::Exclusive => return true,
            WindowMode::SystemExclusive => return false,
            WindowMode::Normal => {}
        }
        window.mode = WindowMode::Exclusive;
        window.ready = false;
        control.visible = true;
        control.disabled = false;
        // The keyboard's pick is dropped (`m_AccessMgr->SetActiveObject()`),
        // and the window comes to the front when it is a top-level one
        // (`Game->FocusWindow` only knows those).
        self.access.clear_active();
        self.focus_window(id);
        true
    }

    /// Makes a window system-exclusive: as exclusive, and the game
    /// freezes but the window's scripts (`GoSystemExclusive`).
    pub fn go_system_exclusive(&mut self, engine: &mut Engine, id: NativeId) -> bool {
        let Some(control) = self.control(id) else {
            return false;
        };
        let Some(window) = control.window() else {
            return false;
        };
        if window.mode == WindowMode::SystemExclusive {
            return true;
        }
        let pause_music = window.pause_music;
        self.access.clear_active();
        self.make_freezable(engine, id, false);
        if let Some(control) = self.control_mut(id) {
            if let Some(window) = control.window_mut() {
                window.mode = WindowMode::SystemExclusive;
                window.ready = false;
            }
            control.visible = true;
            control.disabled = false;
        }
        self.focus_window(self.top_window(id));
        self.freeze(engine, pause_music);
        true
    }

    /// Closes a window: hidden, normal again, the scripts waiting for it
    /// go on, and the game unfreezes when it was system-exclusive
    /// (`Close`).
    pub fn close_window(&mut self, engine: &mut Engine, id: NativeId) -> bool {
        let Some(mode) = self.control(id).and_then(Control::window).map(|w| w.mode) else {
            return false;
        };
        if mode == WindowMode::SystemExclusive {
            self.unfreeze(engine);
        }
        self.close_window_state(id)
    }

    /// The part of [`World::close_window`] that needs no engine: the
    /// window is hidden, normal again and ready.
    pub(crate) fn close_window_state(&mut self, id: NativeId) -> bool {
        if self.control(id).and_then(Control::window).is_none() {
            return false;
        }
        if let Some(control) = self.control_mut(id) {
            if let Some(window) = control.window_mut() {
                window.mode = WindowMode::Normal;
                window.ready = true;
                window.dragging = false;
            }
            control.visible = false;
        }
        true
    }

    /// Centres a window on the screen (`Center`).
    pub fn center_window(&mut self, id: NativeId) {
        let (w, h) = (self.settings.width as i32, self.settings.height as i32);
        if let Some(control) = self.control_mut(id) {
            control.x = (w - control.width) / 2;
            control.y = (h - control.height) / 2;
        }
    }

    /// Marks a window and its controls, and their scripts, as freezable
    /// or not (`CUIWindow::MakeFreezable`, `CBScriptHolder::MakeFreezable`):
    /// the flag stays on the objects, for the scripts attached later.
    fn make_freezable(&mut self, engine: &mut Engine, id: NativeId, freezable: bool) {
        let mut all = self.descendants(id);
        all.push(id);
        for control in all {
            if let Some(native) = self.objects.get_mut(control) {
                native.state.freezable = freezable;
            }
            for script in engine.scripts_of(control) {
                if let Some(script) = engine.script_mut(script) {
                    script.set_freezable(freezable);
                }
            }
        }
    }

    /// Freezes the game (`CBGame::Freeze`): the freezable scripts pause,
    /// the sounds too (the music when asked), the timer stops and the
    /// game is interactive. Nested freezes count.
    pub fn freeze(&mut self, engine: &mut Engine, including_music: bool) {
        self.apply_pending_freezable(engine);
        if self.ui.freeze_level == 0 {
            engine.pause_all(self);
            self.sounds.audio.pause_all(including_music);
            self.ui.orig_interactive = self.interactive;
            self.interactive = true;
        }
        self.ui.frozen = true;
        self.ui.freeze_level += 1;
    }

    /// Undoes one freeze; the last one resumes the scripts and the
    /// sounds (`CBGame::Unfreeze`).
    pub fn unfreeze(&mut self, engine: &mut Engine) {
        if self.ui.freeze_level == 0 {
            return;
        }
        self.ui.freeze_level -= 1;
        if self.ui.freeze_level == 0 {
            self.ui.frozen = false;
            self.interactive = self.ui.orig_interactive;
            engine.resume_all();
            self.sounds.audio.resume_all();
        }
    }

    /// Switches a window's mode from a property write (`Exclusive`,
    /// `SystemExclusive` set): `true` opens it modally, `false` closes it
    /// and leaves it visible (`CUIWindow::ScSetProperty`). The parts that
    /// freeze or unfreeze the game need the script engine, which a
    /// property write does not have, so they run at the start of the
    /// frame's window update; the rest happens at once.
    pub(crate) fn set_window_mode(&mut self, id: NativeId, system: bool, on: bool) {
        let Some(mode) = self.control(id).and_then(Control::window).map(|w| w.mode) else {
            return;
        };
        match (on, system) {
            (true, false) => {
                self.go_exclusive(id);
            }
            (true, true) => self
                .ui
                .pending_modes
                .push((id, PendingMode::SystemExclusive)),
            (false, _) if mode == WindowMode::SystemExclusive => {
                self.ui.pending_modes.push((id, PendingMode::CloseAndShow));
            }
            (false, _) => {
                self.close_window_state(id);
                if let Some(control) = self.control_mut(id) {
                    control.visible = true;
                }
            }
        }
    }

    /// Applies the `Freezable` writes waiting for the engine.
    fn apply_pending_freezable(&mut self, engine: &mut Engine) {
        for (container, freezable) in std::mem::take(&mut self.ui.pending_freezable) {
            if let Some(entity) = self.container_entity(container) {
                self.make_entity_freezable(engine, entity, freezable);
            }
        }
    }

    fn apply_pending_window_modes(&mut self, engine: &mut Engine) {
        self.apply_pending_freezable(engine);
        for (id, pending) in std::mem::take(&mut self.ui.pending_modes) {
            match pending {
                PendingMode::SystemExclusive => {
                    self.go_system_exclusive(engine, id);
                }
                PendingMode::CloseAndShow => {
                    self.close_window(engine, id);
                    if let Some(control) = self.control_mut(id) {
                        control.visible = true;
                    }
                }
            }
        }
    }

    // ----------------------------------------------------------------
    // Each frame
    // ----------------------------------------------------------------

    /// The shield under an exclusive window or a menu, created when
    /// first needed (`CUIWindow::Display`).
    fn shield_of(&mut self, id: NativeId) -> Option<NativeId> {
        let window = self.control(id).and_then(Control::window)?;
        let kind = match (window.mode, window.menu) {
            (WindowMode::Exclusive | WindowMode::SystemExclusive, _) => "window",
            (WindowMode::Normal, true) => "button",
            (WindowMode::Normal, false) => return None,
        };
        let existing = window.shield;
        let (w, h) = (self.settings.width as i32, self.settings.height as i32);
        let matches = existing
            .and_then(|s| self.control(s))
            .is_some_and(|c| c.type_name() == kind);
        let shield = if matches {
            existing.unwrap_or(id)
        } else {
            if let Some(old) = existing {
                self.ui.controls.remove(&old);
                self.objects.remove(old);
            }
            let shield = if kind == "window" {
                let state = WindowState {
                    is_shield: true,
                    ..WindowState::default()
                };
                self.new_control(ControlKind::Window(Box::new(state)), Some(id))
            } else {
                self.new_button_control(Some(id), "close", ButtonListener::CloseParent)
            };
            if let Some(window) = self.control_mut(id).and_then(Control::window_mut) {
                window.shield = Some(shield);
            }
            shield
        };
        if let Some(c) = self.control_mut(shield) {
            c.x = 0;
            c.y = 0;
            c.width = w;
            c.height = h;
        }
        Some(shield)
    }

    /// The top-level windows displayed in a pass, in order: the visible
    /// ones with `IN_GAME` on or off (`DisplayWindows`).
    fn windows_in_pass(&self, in_game: bool) -> Vec<NativeId> {
        self.ui
            .windows
            .iter()
            .copied()
            .filter(|w| {
                self.control(*w)
                    .is_some_and(|c| c.visible && c.window().is_some_and(|s| s.in_game == in_game))
            })
            .collect()
    }

    /// One frame of the windows (`DisplayWindows` and the state kept by
    /// `Display`): the focused window is checked, dragged windows move,
    /// buttons note the mouse and fire their presses, the texts are
    /// measured and prepared for drawing.
    pub(crate) fn update_ui(&mut self, engine: &mut Engine) {
        self.apply_pending_window_modes(engine);
        self.layout_responses();
        // Did we lose the focused window? Focus the topmost one.
        let focused_ok = self
            .ui
            .focused_window
            .and_then(|f| self.control(f))
            .is_some_and(|c| c.visible && !c.disabled);
        if !focused_ok {
            self.ui.focused_window = self
                .ui
                .windows
                .iter()
                .rev()
                .copied()
                .find(|w| self.control(*w).is_some_and(|c| c.visible && !c.disabled));
        }
        let mut order = self.windows_in_pass(true);
        order.extend(self.windows_in_pass(false));
        for window in order {
            self.update_window(engine, window);
        }
        // The response box's window is not one of the game's windows;
        // its buttons are driven here while it waits for a pick.
        if let (true, Some(window)) = (self.responses_visible(), self.responses.window) {
            self.update_window(engine, window);
        }
    }

    pub(crate) fn update_window(&mut self, engine: &mut Engine, id: NativeId) {
        let Some(control) = self.control(id) else {
            return;
        };
        let Some(window) = control.window() else {
            return;
        };
        let visible = control.visible;
        let (dragging, drag_from) = (window.dragging, window.drag_from);
        let focused_ok = window
            .focused_widget
            .and_then(|f| self.control(f))
            .is_some_and(|c| c.can_focus && !c.disabled && c.visible);
        self.shield_of(id);
        if let Some(shield) = self
            .control(id)
            .and_then(Control::window)
            .and_then(|w| w.shield)
        {
            if self.control(shield).is_some_and(|c| c.button().is_some()) {
                self.update_button(engine, shield);
            }
        }
        if !visible {
            return;
        }
        if dragging {
            let (mx, my) = (self.mouse.x, self.mouse.y);
            if let Some(control) = self.control_mut(id) {
                control.x += mx - drag_from.0;
                control.y += my - drag_from.1;
                if let Some(w) = control.window_mut() {
                    w.drag_from = (mx, my);
                }
            }
        }
        if !focused_ok {
            self.move_focus(id, true);
        }
        let inactive_shown = !self.is_focused(id)
            && self
                .control(id)
                .is_some_and(|c| c.image_in(ImageSlot::Inactive).is_some());
        self.step_control_image(
            id,
            if inactive_shown {
                ImageSlot::Inactive
            } else {
                ImageSlot::Image
            },
        );
        self.prepare_title(id);
        for widget in self.widgets_of(id) {
            let Some(c) = self.control(widget) else {
                continue;
            };
            match &c.kind {
                ControlKind::Window(_) => self.update_window(engine, widget),
                ControlKind::Button(_) => self.update_button(engine, widget),
                ControlKind::Static(_) => self.update_static(widget),
                ControlKind::Edit(_) => self.update_edit(widget),
                ControlKind::EntityContainer(entity) => self.step_ui_entity_anim(*entity),
            }
        }
    }

    /// Gives an entity container the entity of a file (`ENTITY`,
    /// `SetEntity`): the entity's sprite is loaded, its scripts attached
    /// to its own handle, and a previous entity is dropped. The entity
    /// cannot be frozen and gets mouse events while the game is not
    /// interactive, as the original engine sets it up.
    pub fn set_container_entity(&mut self, engine: &mut Engine, id: NativeId, path: &str) -> bool {
        let Some(ControlKind::EntityContainer(_)) = self.control(id).map(|c| c.kind.clone()) else {
            return false;
        };
        // The previous entity is unregistered first: a file that does
        // not load leaves the container without an entity.
        self.drop_container_entity(engine, id);
        let Some(def) = self.read_entity_def(path) else {
            return false;
        };
        let index = self.ui_entities.len();
        let caption = self.expand(&def.caption);
        let scripts = def.scripts.clone();
        let sprites: Vec<String> = def
            .sprite
            .iter()
            .chain(def.talk_sprites.iter())
            .chain(def.talk_special_sprites.iter())
            .cloned()
            .collect();
        for sprite in &sprites {
            if !self.ui.bank.add_sprite(&self.vfs, sprite) {
                self.log(format!("  missing: {sprite}"));
            }
        }
        let mut def = def;
        def.caption = caption;
        self.ui_entities.push(UiEntity {
            def,
            filename: path.to_string(),
            anim: None,
            container: id,
            freezable: false,
        });
        let entity = self.objects.insert(Kind::Entity(EntityRef::Ui(index)));
        // `m_SceneIndependent = true`.
        if let Some(state) = self.object_state_mut(entity) {
            state.scene_independent = true;
        }
        self.set_holder_filename(entity, path);
        self.register_entity_region(entity);
        self.update_entity_position(entity);
        for script in scripts {
            self.attach(engine, entity, &script);
        }
        self.make_entity_freezable(engine, entity, false);
        if let Some(control) = self.control_mut(id) {
            control.kind = ControlKind::EntityContainer(Some(index));
        }
        true
    }

    /// The entity of a container, if it has one.
    pub fn container_entity(&self, id: NativeId) -> Option<NativeId> {
        match self.control(id).map(|c| &c.kind) {
            Some(ControlKind::EntityContainer(Some(index))) => self.node_id(EntityRef::Ui(*index)),
            _ => None,
        }
    }

    /// Advances the animation of a container's entity sprite; a frame
    /// that becomes current plays its sound and raises its events on the
    /// entity.
    fn step_ui_entity_anim(&mut self, index: Option<usize>) {
        let (timer, clock) = (self.timer(), self.clock);
        let Some(index) = index else { return };
        // A hidden container neither places nor updates its entity
        // (`CUIEntity::Display` returns at once).
        let Some(container) = self.ui_entities.get(index).map(|e| e.container) else {
            return;
        };
        if !self.control(container).is_some_and(|c| c.visible) {
            return;
        }
        let (cx, cy) = self.control_screen_position(container);
        let Some(entity) = self.ui_entities.get_mut(index) else {
            return;
        };
        // The entity stands at the container's position, whatever the X
        // and Y of its file (`m_Entity->m_PosX = OffsetX + m_PosX`).
        entity.def.x = cx;
        entity.def.y = cy;
        // The sprite of an owner that is not freezable runs on the live
        // timer (`CBSprite::GetCurrentFrame`): a container's entity is
        // not, unless a script made it so.
        let timer = if entity.freezable { timer } else { clock };
        let Some(path) = entity.def.sprite.clone() else {
            entity.anim = None;
            return;
        };
        let Some(loaded) = self.ui.bank.sprite(&path) else {
            return;
        };
        let anim = entity.anim.get_or_insert_with(|| AnimState::new(&path));
        if anim.sprite != crate::scene_view::key(&path) {
            *anim = AnimState::new(&path);
        }
        if anim.step(&loaded.sprite, timer) {
            let frame = loaded.sprite.frames.get(anim.current_frame).cloned();
            if let (Some(frame), Some(owner)) = (frame, self.node_id(EntityRef::Ui(index))) {
                self.frame_shown(owner, &frame);
            }
        }
    }

    /// Advances the animation of the image in a slot of a control, as
    /// the engine does for the sprite it draws (`CBSprite::Display`): a
    /// frame that becomes current plays its sound, raises `FrameChanged`
    /// on the slot's sprite object and the frame's events on the control.
    fn step_control_image(&mut self, id: NativeId, slot: ImageSlot) {
        // A control's images have no owner: they run on the live timer
        // (`CBSprite::GetCurrentFrame`) and go on while the game is
        // frozen.
        let timer = self.clock;
        let Some(control) = self.ui.controls.get_mut(&id) else {
            return;
        };
        let Some(path) = control.image_in(slot).map(str::to_string) else {
            if let Some(anim) = control.anim_slot_mut(slot) {
                *anim = None;
            }
            return;
        };
        let Some(loaded) = self.ui.bank.sprite(&path) else {
            return;
        };
        let Some(anim) = control.anim_slot_mut(slot) else {
            return;
        };
        let anim = anim.get_or_insert_with(|| AnimState::new(&path));
        if anim.sprite != crate::scene_view::key(&path) {
            *anim = AnimState::new(&path);
        }
        if anim.step(&loaded.sprite, timer) {
            if let Some(frame) = loaded.sprite.frames.get(anim.current_frame).cloned() {
                self.frame_shown_in(id, slot, &frame);
            }
        }
    }

    /// Marks the scripts of a container's entity as freezable or not
    /// (`CAdEntity::MakeFreezable`): the engine turns it off when the
    /// container loads its entity, a script may turn it back on through
    /// `Freezable`.
    pub(crate) fn make_entity_freezable(
        &mut self,
        engine: &mut Engine,
        entity: NativeId,
        freezable: bool,
    ) {
        if let Some(Kind::Entity(EntityRef::Ui(index))) = self.objects.get(entity).map(|n| &n.kind)
        {
            if let Some(e) = self.ui_entities.get_mut(*index) {
                e.freezable = freezable;
            }
        }
        if let Some(native) = self.objects.get_mut(entity) {
            native.state.freezable = freezable;
        }
        for script in engine.scripts_of(entity) {
            if let Some(script) = engine.script_mut(script) {
                script.set_freezable(freezable);
            }
        }
    }

    /// The sprite object of the image in a slot of a control
    /// (`GetImageObject`, `GetInactiveImageObject`, `GetHoverImageObject`
    /// and the others), created on first request; `None` when the slot
    /// has no image, as the engine answers `null` then.
    pub fn control_image_object(&mut self, control: NativeId, slot: ImageSlot) -> Option<NativeId> {
        self.control(control)?.image_in(slot)?;
        let kind = Kind::ControlImage { control, slot };
        if let Some(id) = self.objects.find(|k| *k == kind) {
            return Some(id);
        }
        Some(self.objects.insert(kind))
    }

    /// The font a control draws its text with: its own, else the system
    /// font.
    fn control_font(&self, font: Option<&str>) -> Option<String> {
        font.map(str::to_string)
            .or_else(|| self.system_font.clone())
    }

    /// The font a window draws its title with this frame.
    fn window_font(&self, id: NativeId) -> Option<String> {
        let control = self.control(id)?;
        let window = control.window()?;
        if !self.is_focused(id) {
            if let Some(font) = &window.font_inactive {
                return Some(font.clone());
            }
        }
        // A window without a `FONT` draws no title: there is no fall
        // back to the system font (`CUIWindow::Display`).
        control.font.clone()
    }

    fn prepare_title(&mut self, id: NativeId) {
        let Some(control) = self.control(id) else {
            return;
        };
        let Some(window) = control.window() else {
            return;
        };
        let (Some(rect), false) = (window.title_rect, control.text.is_empty()) else {
            return;
        };
        let align = window.title_align;
        let text = control.text.clone();
        let Some(font) = self.window_font(id) else {
            return;
        };
        if let Some(f) = self.fonts.get_mut(&font) {
            f.prepare(&text, rect.width() as i32, align, rect.height() as i32);
        }
    }

    /// The font a button draws its label with this frame.
    fn button_font(&self, id: NativeId) -> Option<String> {
        let control = self.control(id)?;
        let b = control.button()?;
        if control.text.is_empty() {
            return None;
        }
        let state_font = if control.disabled {
            b.font_disable.as_ref()
        } else if control.pressed() {
            b.font_press.as_ref()
        } else if b.hover {
            b.font_hover.as_ref()
        } else if control.can_focus && self.is_focused(id) {
            b.font_focus.as_ref()
        } else {
            None
        };
        state_font
            .cloned()
            .or_else(|| self.control_font(control.font.as_deref()))
    }

    /// A button's frame (`CUIButton::Display`): the hover and press
    /// states, and the press fired on release.
    pub(crate) fn update_button(&mut self, engine: &mut Engine, id: NativeId) {
        let clock = self.clock;
        // `m_Interactive || m_State == GAME_SEMI_FROZEN`: while the
        // response box waits for a pick the buttons stay live even if
        // the game is not interactive. (A system-exclusive window makes
        // the game interactive when it freezes it.)
        let interactive = self.interactive || self.responses_visible();
        let active = self.active_object();
        let left_down = self.mouse.left_down;
        let captured = self.ui.captured;
        let Some(control) = self.control_mut(id) else {
            return;
        };
        if !control.visible {
            return;
        }
        let disabled = control.disabled;
        let Some(b) = control.button_mut() else {
            return;
        };
        b.hover = !disabled && active == Some(id) && interactive;
        let fire = (b.press && b.hover && !left_down)
            || b.one_time_press
                .is_some_and(|t| clock.saturating_sub(t) >= ONE_TIME_PRESS_MS);
        if fire {
            self.press_button(engine, id);
        }
        let focused = self.is_focused(id);
        let shown = self.control(id).and_then(|c| c.button_image_slot(focused));
        if let Some(slot) = shown {
            self.step_control_image(id, slot);
        }
        // The images not shown start again from their first frame when
        // they next show (`CBSprite::Reset` on the unused sprites).
        if let Some(control) = self.control_mut(id) {
            for slot in [
                ImageSlot::Image,
                ImageSlot::Disable,
                ImageSlot::Focus,
                ImageSlot::Press,
                ImageSlot::Hover,
            ] {
                if Some(slot) == shown {
                    continue;
                }
                if let Some(Some(anim)) = control.anim_slot_mut(slot) {
                    anim.reset();
                }
            }
        }
        let (text, font, width, align) = {
            let Some(control) = self.control(id) else {
                return;
            };
            let Some(b) = control.button() else { return };
            (
                control.text.clone(),
                self.button_font(id),
                control.width,
                b.text_align,
            )
        };
        if !text.is_empty() {
            if let Some(font) = font {
                let height = self.fonts.get_mut(&font).map_or(0, |f| {
                    f.prepare(&text, width, align, -1);
                    f.text_height(&text, width)
                });
                if let Some(control) = self.control_mut(id) {
                    control.text_height = height;
                }
            }
        }
        if let Some(b) = self.control_mut(id).and_then(Control::button_mut) {
            b.press = b.hover && left_down && captured == Some(id);
        }
    }

    /// Fires a button (`CUIButton::Press`): `Press` on the button, the
    /// menu it shields closes, and its parent gets an event named after
    /// it when `PARENT_NOTIFY` is on.
    pub fn press_button(&mut self, engine: &mut Engine, id: NativeId) {
        engine.apply_event(self, id, "Press", false);
        let Some(control) = self.control(id) else {
            return;
        };
        let (parent, parent_notify, name) =
            (control.parent, control.parent_notify, control.name.clone());
        match control.button().and_then(|b| b.listener) {
            Some(ButtonListener::CloseParent) => {
                if let Some(parent) = parent {
                    self.close_window(engine, parent);
                }
            }
            Some(ButtonListener::Inventory) => self.inventory_listen(&name),
            Some(ButtonListener::Response(index)) => self.select_response(engine, index),
            Some(ButtonListener::ResponseBox) => self.responses_listen(&name),
            None => {}
        }
        if parent_notify {
            if let Some(parent) = parent {
                engine.apply_event(self, parent, &name, false);
            }
        }
        if let Some(b) = self.control_mut(id).and_then(Control::button_mut) {
            b.one_time_press = None;
        }
    }

    /// Asks a button to show itself pressed for a moment, then fire
    /// (`Button.Press()`).
    pub fn press_button_later(&mut self, id: NativeId) {
        let clock = self.clock;
        // Nothing on a hidden or disabled button (`m_Visible &&
        // !m_Disable`).
        if !self.control(id).is_some_and(|c| c.visible && !c.disabled) {
            return;
        }
        if let Some(b) = self.control_mut(id).and_then(Control::button_mut) {
            b.one_time_press = Some(clock);
        }
    }

    fn update_static(&mut self, id: NativeId) {
        self.step_control_image(id, ImageSlot::Image);
        let Some(control) = self.control(id) else {
            return;
        };
        let Some(s) = control.static_text() else {
            return;
        };
        if !control.visible || control.text.is_empty() {
            return;
        }
        let (text, width, height, align) = (
            control.text.clone(),
            control.width,
            control.height,
            s.text_align,
        );
        let Some(font) = self.control_font(control.font.as_deref()) else {
            return;
        };
        let text_height = self.fonts.get_mut(&font).map_or(0, |f| {
            f.prepare(&text, width, align, height);
            f.text_height(&text, width)
        });
        if let Some(control) = self.control_mut(id) {
            control.text_height = text_height;
        }
    }

    /// The width of a part of an editor's text in a font. The part is
    /// characters; the fonts take text as the game holds it.
    fn edit_width(&mut self, font: &str, part: &str) -> i32 {
        if part.is_empty() {
            return 0;
        }
        let text = self.game_text(part);
        self.measure_text_width(Some(font), &text)
    }

    /// The half of `CUIEdit::Display` that changes the editor: the
    /// selection is brought inside the text, the first character shown
    /// moves on until the text up to the caret fits, the parts to draw
    /// are laid out, and the caret blinks.
    ///
    /// The parts are the text before the selection, the selection (in
    /// `FONT_SELECTED`) and the text after it, with the caret where the
    /// selection ends. Each is drawn in the room left up to the right
    /// frame and one letter high, so the line is cut there. The caret
    /// keeps its room while the editor has the focus, shown or not.
    ///
    /// Deliberate: the original measures the parts with `GetTextWidth`
    /// and a length, which its TrueType font ignores: with such a font
    /// every part is as wide as all the text that follows, and a caret
    /// inside the text stands far from its place. The parts are
    /// measured for what they are here, as with a bitmap font.
    fn update_edit(&mut self, id: NativeId) {
        self.step_control_image(id, ImageSlot::Image);
        let clock = self.clock;
        let focused = self.is_focused(id);
        let Some(control) = self.control(id) else {
            return;
        };
        if !control.visible {
            return;
        }
        let (text, width) = (control.text.clone(), control.width);
        let font = self.control_font(control.font.as_deref());
        let rtl = self.text_rtl;
        let Some(e) = control.edit() else { return };
        let selected_font = e.font_selected.clone().or_else(|| font.clone());
        let (frame, cursor_char) = (e.frame_width, e.cursor_char.clone());
        let len = text.len();
        let sel_start = char_floor(&text, e.sel_start);
        let sel_end = char_floor(&text, e.sel_end);
        let mut scroll = char_floor(&text, e.scroll_offset);
        let part = |from: usize, to: usize| text.get(from..to.max(from)).unwrap_or("");
        let mut pieces = Vec::new();
        let mut caret = None;
        let (mut align_offset, mut line_height) = (0, 0);
        if let (Some(font), Some(selected_font)) = (&font, &selected_font) {
            let cursor_width = self.edit_width(font, &cursor_char);
            let room = width - cursor_width - 2 * frame;
            // The selection runs from `first` to `last`; the caret is at
            // the end the user moves, before it or after it.
            let caret_first = sel_start >= sel_end;
            let (first, last) = (sel_start.min(sel_end), sel_start.max(sel_end));
            loop {
                let used = if caret_first {
                    self.edit_width(font, part(scroll, sel_end))
                } else {
                    let from = scroll.max(sel_start);
                    self.edit_width(font, part(scroll, sel_start))
                        + self.edit_width(selected_font, part(from, sel_end))
                };
                if used <= room {
                    break;
                }
                scroll += 1;
                while scroll < len && !text.is_char_boundary(scroll) {
                    scroll += 1;
                }
                if scroll >= len {
                    break;
                }
            }
            scroll = scroll.min(sel_end);
            let shown_first = first.max(scroll);
            let before = self.edit_width(font, part(scroll, first));
            let selection = self.edit_width(selected_font, part(shown_first, last));
            let after = self.edit_width(font, part(last, len));
            let caret_room = if focused { cursor_width } else { 0 };
            // Right-to-left text sits against the right edge: it starts
            // after the room the text and the caret leave.
            if rtl {
                align_offset = (width - 2 * frame - before - caret_room - selection - after).max(0);
            }
            line_height = [font, selected_font]
                .iter()
                .filter_map(|f| self.fonts.get(f))
                .map(crate::text::Font::line_height)
                .max()
                .unwrap_or(0);
            let right = width - frame;
            let mut x = frame + align_offset;
            let piece = |x: i32, text: &str, selected: bool| EditPiece {
                text: text.to_string(),
                x,
                width: right - x,
                selected,
            };
            let caret_piece = |x: i32| {
                // The caret is measured whole and drawn as its first
                // character.
                let first: String = cursor_char.chars().take(1).collect();
                EditPiece {
                    text: first,
                    x,
                    width: right - x,
                    selected: false,
                }
            };
            if first > scroll {
                pieces.push(piece(x, part(scroll, first), false));
                x += before;
            }
            if focused && caret_first {
                caret = Some(caret_piece(x));
                x += cursor_width;
            }
            if last > shown_first {
                pieces.push(piece(x, part(shown_first, last), true));
                x += selection;
            }
            if focused && !caret_first {
                caret = Some(caret_piece(x));
                x += cursor_width;
            }
            pieces.push(piece(x, part(last, len), false));
            // Nothing is drawn of an empty part, nor in no room at all.
            pieces.retain(|p| !p.text.is_empty() && p.width > 0);
            caret = caret.filter(|p| !p.text.is_empty() && p.width > 0);
            // A TrueType font draws a text it was given before.
            for p in pieces.iter().chain(&caret) {
                let name = if p.selected { selected_font } else { font };
                let shown = self.game_text(&p.text);
                if let Some(f) = self.fonts.get_mut(name) {
                    f.prepare(&shown, p.width, TextAlign::Left, line_height);
                }
            }
        }
        if let Some(e) = self.control_mut(id).and_then(Control::edit_mut) {
            e.sel_start = sel_start;
            e.sel_end = sel_end;
            e.scroll_offset = scroll;
            e.align_offset = align_offset;
            e.pieces = pieces;
            e.caret = caret;
            e.line_height = line_height;
            // The caret changes each time the blink rate has passed
            // since it last did. The original compares unsigned numbers,
            // against a clock that has run for long: a caret that never
            // blinked is due, and a negative rate never is.
            if focused && e.cursor_blink_rate >= 0 {
                let due = e.last_blink == 0
                    || clock.saturating_sub(e.last_blink) >= e.cursor_blink_rate as u64;
                if due {
                    e.last_blink = clock;
                    e.cursor_visible = !e.cursor_visible;
                }
            }
        }
    }

    // ----------------------------------------------------------------
    // The mouse
    // ----------------------------------------------------------------

    /// Registers where the `IN_GAME` windows and their controls catch
    /// the mouse, in drawing order (the areas registered by `Display`).
    pub(crate) fn ui_hits_in_game(&mut self, hits: &mut Vec<HitArea>) {
        for window in self.windows_in_pass(true) {
            self.window_hits(window, 0, 0, hits);
        }
    }

    /// Registers where the other windows and their controls catch the
    /// mouse.
    pub(crate) fn ui_hits_normal(&mut self, hits: &mut Vec<HitArea>) {
        for window in self.windows_in_pass(false) {
            self.window_hits(window, 0, 0, hits);
        }
    }

    /// The viewport of a window that clips its contents
    /// (`CLIP_CONTENTS`, `CUIWindow::Display`): its rectangle on screen,
    /// kept inside the screen (`CBViewport::SetRect`), as `(left, top,
    /// right, bottom)`; `None` for a window that does not clip.
    pub(crate) fn window_clip(
        &self,
        id: NativeId,
        ox: i32,
        oy: i32,
    ) -> Option<(i32, i32, i32, i32)> {
        let control = self.control(id)?;
        if !control.window()?.clip_contents {
            return None;
        }
        let (x, y) = (control.x.saturating_add(ox), control.y.saturating_add(oy));
        let (sw, sh) = (self.settings.width as i32, self.settings.height as i32);
        Some((
            x.max(0),
            y.max(0),
            x.saturating_add(control.width).min(sw),
            y.saturating_add(control.height).min(sh),
        ))
    }

    pub(crate) fn window_hits(&mut self, id: NativeId, ox: i32, oy: i32, hits: &mut Vec<HitArea>) {
        let Some(control) = self.control(id) else {
            return;
        };
        let Some(window) = control.window() else {
            return;
        };
        let (visible, transparent) = (control.visible, window.transparent);
        let (x, y, w, h) = (
            control.x + ox,
            control.y + oy,
            control.width,
            control.height,
        );
        // The image shown is the one registered (`IMAGE_INACTIVE` while
        // the window is not focused and has one).
        let slot = if !self.is_focused(id) && window.image_inactive.is_some() {
            ImageSlot::Inactive
        } else {
            ImageSlot::Image
        };
        let image = control.image_in(slot).map(str::to_string);
        let frame = control.frame_in(slot);
        if let Some(shield) = self.shield_of(id) {
            let (sw, sh) = (self.settings.width as i32, self.settings.height as i32);
            hits.push(HitArea::rect(shield, 0, 0, sw, sh));
        }
        if !visible {
            return;
        }
        // What follows is registered inside the window's viewport when
        // it clips its contents.
        let clip_start = hits.len();
        let clip = self.window_clip(id, ox, oy);
        if !transparent {
            match image.as_deref().and_then(|p| self.ui.bank.sprite(p)) {
                Some(loaded) => {
                    World::sprite_hits(
                        id,
                        HitBank::Ui,
                        &loaded.sprite,
                        frame,
                        x,
                        y,
                        100.0,
                        (0, 0),
                        hits,
                    );
                }
                None => hits.push(HitArea::rect(id, x, y, w, h)),
            }
        }
        for widget in self.widgets_of(id) {
            let Some(c) = self.control(widget) else {
                continue;
            };
            if !c.visible {
                continue;
            }
            match &c.kind {
                ControlKind::Window(_) => self.window_hits(widget, x, y, hits),
                ControlKind::Button(b) => {
                    let (bx, by, bw, bh) = (c.x + x, c.y + y, c.width, c.height);
                    let has_image = c.image.is_some();
                    // `PIXEL_PERFECT`: the image drawn (the state's, else
                    // the plain one) is registered where it is drawn,
                    // centred and shifted like it.
                    if b.pixel_perfect {
                        let focused = self.is_focused(widget);
                        let slot = c.button_image_slot(focused);
                        let drawn = slot
                            .and_then(|s| c.image_in(s).map(|p| (p.to_string(), c.frame_in(s))));
                        let pushed = b.press || b.one_time_press.is_some();
                        let state_back = if c.disabled {
                            b.back_disable.is_some()
                        } else if c.pressed() {
                            b.back_press.is_some()
                        } else if b.hover {
                            b.back_hover.is_some()
                        } else if c.can_focus && focused {
                            b.back_focus.is_some()
                        } else {
                            false
                        };
                        let has_back = state_back || c.back.is_some();
                        let centre = b.center_image;
                        if let Some((path, frame)) = drawn {
                            let (mut ix, mut iy) = (bx, by);
                            if centre {
                                if let Some((w, h)) = self.sprite_size_loaded(&path) {
                                    ix = ix.saturating_add(bw.saturating_sub(w) / 2);
                                    iy += (bh - h) / 2;
                                }
                            }
                            let shift = i32::from(pushed && has_back);
                            if let Some(loaded) = self.ui.bank.sprite(&path) {
                                World::sprite_hits(
                                    widget,
                                    HitBank::Ui,
                                    &loaded.sprite,
                                    frame,
                                    ix + shift,
                                    iy + shift,
                                    100.0,
                                    (0, 0),
                                    hits,
                                );
                            }
                        }
                        // The rectangle too when the button has no plain
                        // image (`!m_PixelPerfect || !m_Image`).
                        if has_image {
                            continue;
                        }
                    }
                    hits.push(HitArea::rect(widget, bx, by, bw, bh));
                }
                ControlKind::Static(_) => {}
                ControlKind::Edit(_) => {
                    hits.push(HitArea::rect(widget, c.x + x, c.y + y, c.width, c.height));
                }
                ControlKind::EntityContainer(index) => {
                    // The entity is the object under the mouse, through its
                    // sprite, when it is active and registrable and the
                    // container is enabled (`CUIEntity::Display`).
                    let (cx, cy, disabled) = (c.x + x, c.y + y, c.disabled);
                    let Some(entity) = index.and_then(|i| self.ui_entities.get(i)) else {
                        continue;
                    };
                    if disabled || !entity.def.active || !entity.def.interactive {
                        continue;
                    }
                    let Some(owner) = self.node_id(EntityRef::Ui(index.unwrap_or(0))) else {
                        continue;
                    };
                    let frame = entity.anim.as_ref().map_or(0, |a| a.current_frame);
                    // The entity's region first, in screen coordinates
                    // (`CAdEntity::Display`), then its sprite.
                    if let Some(region) = &entity.def.region {
                        hits.push(HitArea::polygon(owner, region.points.clone()));
                    }
                    if let Some(loaded) = entity
                        .def
                        .sprite
                        .as_deref()
                        .and_then(|p| self.ui.bank.sprite(p))
                    {
                        World::sprite_hits(
                            owner,
                            HitBank::Ui,
                            &loaded.sprite,
                            frame,
                            cx,
                            cy,
                            100.0,
                            (0, 0),
                            hits,
                        );
                    }
                }
            }
        }
        if let Some(clip) = clip {
            crate::input::clip_hits(hits, clip_start, clip);
        }
    }

    /// Unregisters a container's entity (`Game->UnregisterObject` in
    /// `CUIEntity::SetEntity` and `~CUIEntity`): its scripts end, its
    /// handle goes, and the container has no entity.
    pub(crate) fn drop_container_entity(&mut self, engine: &mut Engine, id: NativeId) {
        let Some(ControlKind::EntityContainer(Some(index))) =
            self.control(id).map(|c| c.kind.clone())
        else {
            return;
        };
        if let Some(old) = self.node_id(EntityRef::Ui(index)) {
            for script in engine.scripts_of(old) {
                engine.finish(script, true);
            }
            self.forget_owned(old);
            self.objects.remove(old);
            if self.active_object == Some(old) {
                self.active_object = None;
            }
        }
        if let Some(control) = self.control_mut(id) {
            control.kind = ControlKind::EntityContainer(None);
        }
    }

    /// The left button went down on the active object, which is then
    /// captured. A control takes the focus (`CUIObject::HandleMouse`):
    /// it becomes the focused control of its window and its top window
    /// comes to the front, so clicking an editor lets the player type
    /// in it and clicking a window raises it. A window also starts
    /// dragging when the point is in its drag rectangle
    /// (`CUIWindow::HandleMouse`).
    pub(crate) fn ui_mouse_down(&mut self) {
        let active = self.active_object();
        self.ui.captured = active;
        let Some(id) = active else { return };
        if self.control(id).is_some() {
            self.focus_control(id);
        }
        let (sx, sy) = self.control_screen_position(id);
        let (mx, my) = (self.mouse.x, self.mouse.y);
        let Some(window) = self.control_mut(id).and_then(Control::window_mut) else {
            return;
        };
        if window
            .drag_rect
            .is_some_and(|r| rect_contains(r, mx - sx, my - sy))
        {
            window.dragging = true;
            window.drag_from = (mx, my);
        }
    }

    /// The left button went up: nothing is captured or dragged any more.
    pub(crate) fn ui_mouse_up(&mut self) {
        self.ui.captured = None;
        for control in self.ui.controls.values_mut() {
            if let Some(window) = control.window_mut() {
                window.dragging = false;
            }
        }
    }

    // ----------------------------------------------------------------
    // The keyboard
    // ----------------------------------------------------------------

    /// A keyboard message reaches the focused window
    /// (`CUIWindow::HandleKeypress`): the `Tab` key moves the focus,
    /// anything else goes to the focused control. `printable` tells a
    /// character typed (`code` its code, `text` the character) from a
    /// key going down (`code` its virtual key code). `true` when
    /// handled.
    pub(crate) fn window_key(
        &mut self,
        window: NativeId,
        printable: bool,
        code: i32,
        text: Option<char>,
    ) -> bool {
        if !printable && code == VK_TAB {
            let forward = !self.keyboard.shift;
            return self.move_focus(window, forward);
        }
        let Some(focused) = self
            .control(window)
            .and_then(Control::window)
            .and_then(|w| w.focused_widget)
        else {
            return false;
        };
        match self.control(focused).map(|c| &c.kind) {
            Some(ControlKind::Window(_)) => self.window_key(focused, printable, code, text),
            Some(ControlKind::Edit(_)) => self.edit_message(focused, printable, code, text),
            _ => false,
        }
    }

    /// A key press in a text editor, as the two messages Windows sends
    /// for it: the key going down with its virtual key code, then the
    /// character it types (`text`, or the one Backspace, the space bar
    /// and Ctrl with a letter always send). `true` when either was
    /// handled. See [`World::edit_message`].
    pub fn edit_key(&mut self, id: NativeId, code: i32, text: Option<char>) -> bool {
        let (alt, control) = (self.keyboard.alt, self.keyboard.control);
        let down = self.edit_message(id, false, code, None);
        let translated = crate::input::translated_char(code, alt, control);
        let ch = if control && !alt {
            translated
        } else {
            translated.or(text.filter(|c| !c.is_control()))
        };
        let typed = ch.is_some_and(|ch| {
            let code = crate::input::char_code_in(ch, self.ansi_code_page());
            self.edit_message(id, true, code, Some(ch))
        });
        down || typed
    }

    /// A keyboard message in a text editor (`CUIEdit::HandleKeypress`).
    ///
    /// A key going down (`printable` false) moves the caret (the arrows,
    /// `Home`, `End`, with Shift to extend the selection) or deletes
    /// (`Delete`); any other key is left to the caller. A character
    /// (`printable` true) is typed over the selection, `Backspace`
    /// (character 8) deletes, and with Ctrl held and Alt up the control
    /// characters of C, X, V and A copy, cut, paste and select all.
    /// Escape, Tab, Enter and a line feed are never taken.
    ///
    /// Every deletion and every insertion raises an event named after
    /// the editor on its parent when `PARENT_NOTIFY` is on, as
    /// `DeleteChars` and `InsertChars` do: typing over a selection
    /// raises it twice. With right-to-left text (`Game.TextRTL`)
    /// `Backspace` takes the character after the caret and `Delete` the
    /// one before, `Home` and `End` swap, and a typed character leaves
    /// the caret before it.
    pub fn edit_message(
        &mut self,
        id: NativeId,
        printable: bool,
        code: i32,
        text: Option<char>,
    ) -> bool {
        if matches!(code, 0x0A | VK_ESCAPE | VK_TAB | VK_RETURN) {
            return false;
        }
        let (shift, control_down, alt, rtl) = (
            self.keyboard.shift,
            self.keyboard.control,
            self.keyboard.alt,
            self.text_rtl,
        );
        let Some(control) = self.control(id) else {
            return false;
        };
        let (parent, parent_notify, name) =
            (control.parent, control.parent_notify, control.name.clone());
        let mut new_text = control.text.clone();
        let Some(e) = control.edit() else {
            return false;
        };
        let max_length = e.max_length;
        // A script may have put the caret anywhere (`SelStart`,
        // `SelEnd`): keep it inside the text and between two characters.
        let len = new_text.len();
        let mut start = char_floor(&new_text, e.sel_start.min(len));
        let mut end = char_floor(&new_text, e.sel_end.min(len));
        let prev_boundary = |s: &str, i: usize| {
            let mut i = i.saturating_sub(1);
            while i > 0 && !s.is_char_boundary(i) {
                i -= 1;
            }
            i
        };
        let next_boundary = |s: &str, i: usize| {
            let mut i = (i + 1).min(s.len());
            while i < s.len() && !s.is_char_boundary(i) {
                i += 1;
            }
            i
        };
        // How many characters still fit (`MAX_LENGTH`).
        let room = |s: &str| {
            if max_length < 0 {
                usize::MAX
            } else {
                (max_length as usize).saturating_sub(s.chars().count())
            }
        };
        // Calls of `DeleteChars` and `InsertChars`: each notifies the
        // parent.
        let mut edits = 0;
        let mut typed: Option<char> = None;
        if printable {
            let mut handled = false;
            if control_down && !alt {
                match code {
                    // Ctrl+C, Ctrl+X
                    3 | 24 => {
                        let (a, b) = (start.min(end), start.max(end));
                        handled = self.clipboard.set_text(&new_text[a..b]);
                        if code == 24 {
                            new_text.replace_range(a..b, "");
                            edits += 1;
                            start = a;
                            end = a;
                            handled = true;
                        }
                    }
                    // Ctrl+V
                    22 => {
                        if let Some(pasted) = self.clipboard.get_text() {
                            if start != end {
                                let (a, b) = (start.min(end), start.max(end));
                                new_text.replace_range(a..b, "");
                                edits += 1;
                                end = a;
                            }
                            let fits = room(&new_text);
                            let pasted: String = pasted
                                .chars()
                                .filter(|c| !c.is_control())
                                .take(fits)
                                .collect();
                            new_text.insert_str(end, &pasted);
                            edits += 1;
                            if !rtl {
                                end += pasted.len();
                            }
                            start = end;
                            handled = true;
                        }
                    }
                    // Ctrl+A
                    1 => {
                        start = 0;
                        end = new_text.len();
                        handled = true;
                    }
                    _ => {}
                }
            }
            if !handled && code == VK_BACK {
                if start == end && rtl {
                    // The character after the caret goes, and the caret
                    // steps back.
                    let to = next_boundary(&new_text, start);
                    new_text.replace_range(start..to, "");
                    start = prev_boundary(&new_text, start);
                } else if start == end {
                    let from = prev_boundary(&new_text, start);
                    new_text.replace_range(from..start, "");
                    start = from;
                } else {
                    let (a, b) = (start.min(end), start.max(end));
                    new_text.replace_range(a..b, "");
                    start = a;
                }
                end = start;
                edits += 1;
            } else if !handled {
                // A control character that is no command is swallowed.
                if let Some(ch) = text.filter(|c| !c.is_control()) {
                    if start != end {
                        let (a, b) = (start.min(end), start.max(end));
                        new_text.replace_range(a..b, "");
                        edits += 1;
                        end = a;
                    }
                    if room(&new_text) > 0 {
                        new_text.insert(end, ch);
                        // Right-to-left keeps the caret before the new
                        // character: the text is typed backwards in
                        // memory, forwards on screen.
                        if !rtl {
                            end += ch.len_utf8();
                        }
                    }
                    edits += 1;
                    start = end;
                    typed = Some(ch);
                }
            }
        } else {
            match code {
                VK_LEFT | VK_UP => {
                    end = prev_boundary(&new_text, end);
                    if !shift {
                        start = end;
                    }
                }
                VK_RIGHT | VK_DOWN => {
                    end = next_boundary(&new_text, end);
                    if !shift {
                        start = end;
                    }
                }
                VK_HOME => {
                    end = if rtl { new_text.len() } else { 0 };
                    if !shift {
                        start = end;
                    }
                }
                VK_END => {
                    end = if rtl { 0 } else { new_text.len() };
                    if !shift {
                        start = end;
                    }
                }
                VK_DELETE => {
                    if start == end && rtl {
                        // The character before the caret goes.
                        let from = prev_boundary(&new_text, start);
                        new_text.replace_range(from..start, "");
                        start = from;
                    } else if start == end {
                        let to = next_boundary(&new_text, start);
                        new_text.replace_range(start..to, "");
                    } else {
                        let (a, b) = (start.min(end), start.max(end));
                        new_text.replace_range(a..b, "");
                        start = a;
                    }
                    end = start;
                    edits += 1;
                }
                // Keys the editor has no use for are not eaten.
                _ => return false,
            }
        }
        if let Some(control) = self.control_mut(id) {
            control.text = new_text;
            if let Some(e) = control.edit_mut() {
                e.sel_start = start;
                e.sel_end = end;
            }
        }
        if parent_notify {
            if let Some(parent) = parent {
                for _ in 0..edits {
                    self.queue_event(parent, name.clone());
                }
            }
        }
        // The key typed is read out (`CUIEdit::HandleKeypress`).
        if let Some(ch) = typed {
            let spoken = self.game_text(&ch.to_string());
            self.access_speak(&spoken, crate::accessibility::SpeechKind::Keypress);
        }
        true
    }

    // ----------------------------------------------------------------
    // Drawing
    // ----------------------------------------------------------------

    /// Draws the windows of a pass (`DisplayWindows`): those with
    /// `IN_GAME`, or the others.
    pub(crate) fn draw_windows<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        in_game: bool,
        white: Option<&'a Texture>,
    ) {
        for window in self.windows_in_pass(in_game) {
            self.draw_window(list, window, 0, 0, white, Rgba::OPAQUE_WHITE);
        }
    }

    pub(crate) fn draw_window<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        id: NativeId,
        ox: i32,
        oy: i32,
        white: Option<&'a Texture>,
        tint: Rgba,
    ) {
        let Some(control) = self.control(id) else {
            return;
        };
        let Some(window) = control.window() else {
            return;
        };
        if !control.visible {
            return;
        }
        if let (Some(color), Some(white)) = (window.fade_color, white) {
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
        // `CLIP_CONTENTS`: the window and its controls are drawn inside
        // its rectangle (`PushViewport`); the viewport in force before
        // comes back when the window is done (`PopViewport`).
        let outer_clip = list.clip();
        if let Some((l, t, r, b)) = self.window_clip(id, ox, oy) {
            list.set_clip(Some(Rect::new(
                l as f32,
                t as f32,
                (r - l).max(0) as f32,
                (b - t).max(0) as f32,
            )));
        }
        let tint = window.alpha_color.unwrap_or(tint);
        let focused = self.is_focused(id);
        let (x, y) = (control.x + ox, control.y + oy);
        let back = if !focused && window.back_inactive.is_some() {
            window.back_inactive.as_ref()
        } else {
            control.back.as_ref()
        };
        let slot = if !focused && window.image_inactive.is_some() {
            ImageSlot::Inactive
        } else {
            ImageSlot::Image
        };
        if let Some(back) = back {
            self.draw_tiled(list, back, x, y, control.width, control.height, tint);
        }
        if let Some(image) = control.image_in(slot) {
            self.draw_ui_image(list, image, control.frame_in(slot), x, y, tint);
        }
        if let (Some(rect), false) = (window.title_rect, control.text.is_empty()) {
            if let Some(font) = self.window_font(id).and_then(|f| self.fonts.get(&f)) {
                font.draw_forced(
                    list,
                    &control.text,
                    x + rect.left,
                    y + rect.top,
                    rect.width() as i32,
                    window.title_align,
                    rect.height() as i32,
                    forced_color(tint),
                );
            }
        }
        for widget in &window.widgets {
            let Some(c) = self.control(*widget) else {
                continue;
            };
            match &c.kind {
                ControlKind::Window(_) => self.draw_window(list, *widget, x, y, white, tint),
                ControlKind::Button(_) => self.draw_button(list, *widget, x, y, tint),
                ControlKind::Static(_) => self.draw_static(list, *widget, x, y, tint),
                ControlKind::Edit(_) => self.draw_edit(list, *widget, x, y, tint),
                ControlKind::EntityContainer(index) => {
                    self.draw_ui_entity(list, *widget, *index, x, y, tint);
                }
            }
        }
        list.set_clip(outer_clip);
    }

    /// Draws a container's entity at the container's position inside its
    /// window, with the sprite's hotspot, as the scene draws an entity.
    fn draw_ui_entity<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        id: NativeId,
        index: Option<usize>,
        ox: i32,
        oy: i32,
        tint: Rgba,
    ) {
        let Some(control) = self.control(id) else {
            return;
        };
        let Some(entity) = index.and_then(|i| self.ui_entities.get(i)) else {
            return;
        };
        if !control.visible || !entity.def.active {
            return;
        }
        // A video the entity plays takes its sprite's place.
        if let Some(owner) = index.and_then(|i| self.node_id(EntityRef::Ui(i))) {
            if self.entity_videos.contains_key(&owner) {
                let (x, y) = (ox + control.x, oy + control.y);
                if let Some(quad) = self.ui_video_quad(owner, x, y, tint) {
                    list.push(quad);
                }
                return;
            }
        }
        let Some(loaded) = entity
            .def
            .sprite
            .as_deref()
            .and_then(|p| self.ui.bank.sprite(p))
        else {
            return;
        };
        let frame = entity.anim.as_ref().map_or(0, |a| a.current_frame);
        let Some(frame) = loaded
            .sprite
            .frames
            .get(frame.min(loaded.sprite.frames.len().saturating_sub(1)))
        else {
            return;
        };
        let (x, y) = (ox + control.x, oy + control.y);
        let tint = entity.def.tint.unwrap_or(tint);
        for sub in crate::scene_view::drawn_subframes(frame) {
            let Some(image) = &sub.image else { continue };
            let Some(texture) = self.ui.bank.texture(image, sub.color_key) else {
                continue;
            };
            list.push(place(
                sub,
                texture,
                x - sub.hotspot_x,
                y - sub.hotspot_y,
                1.0,
                tint,
            ));
        }
    }

    fn draw_button<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        id: NativeId,
        ox: i32,
        oy: i32,
        tint: Rgba,
    ) {
        let Some(control) = self.control(id) else {
            return;
        };
        let Some(b) = control.button() else { return };
        if !control.visible {
            return;
        }
        let pressed = control.pressed();
        // The one-pixel shift is for a press of the mouse or of
        // `Press()`, not for a button that stays pressed (`PRESSED`).
        let pushed = b.press || b.one_time_press.is_some();
        let focused = self.is_focused(id);
        let state_back = if control.disabled {
            b.back_disable.as_ref()
        } else if pressed {
            b.back_press.as_ref()
        } else if b.hover {
            b.back_hover.as_ref()
        } else if control.can_focus && focused {
            b.back_focus.as_ref()
        } else {
            None
        };
        let back = state_back.or(control.back.as_ref());
        let slot = control.button_image_slot(focused);
        let image = slot.and_then(|s| control.image_in(s));
        let (x, y) = (control.x + ox, control.y + oy);
        let (mut ix, mut iy) = (x, y);
        if let (Some(path), true) = (image, b.center_image) {
            if let Some((w, h)) = self.sprite_size_loaded(path) {
                ix += (control.width - w) / 2;
                iy += (control.height - h) / 2;
            }
        }
        if let Some(back) = back {
            self.draw_tiled(list, back, x, y, control.width, control.height, tint);
        }
        let shift = i32::from(pushed && back.is_some());
        if let (Some(path), Some(slot)) = (image, slot) {
            let frame = control.frame_in(slot);
            self.draw_ui_image(list, path, frame, ix + shift, iy + shift, tint);
        }
        if !control.text.is_empty() {
            if let Some(font) = self.button_font(id).and_then(|f| self.fonts.get(&f)) {
                let offset = (control.height - control.text_height) / 2;
                let shift = i32::from(pushed);
                font.draw_forced(
                    list,
                    &control.text,
                    x + shift,
                    y + offset + shift,
                    control.width,
                    b.text_align,
                    -1,
                    forced_color(tint),
                );
            }
        }
    }

    fn draw_static<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        id: NativeId,
        ox: i32,
        oy: i32,
        tint: Rgba,
    ) {
        let Some(control) = self.control(id) else {
            return;
        };
        let Some(s) = control.static_text() else {
            return;
        };
        if !control.visible {
            return;
        }
        let (x, y) = (control.x + ox, control.y + oy);
        if let Some(back) = &control.back {
            self.draw_tiled(list, back, x, y, control.width, control.height, tint);
        }
        if let Some(path) = &control.image {
            let frame = control.anim.as_ref().map_or(0, |a| a.current_frame);
            self.draw_ui_image(list, path, frame, x, y, tint);
        }
        if !control.text.is_empty() {
            let font = self
                .control_font(control.font.as_deref())
                .and_then(|f| self.fonts.get(&f));
            if let Some(font) = font {
                let offset = match s.vertical_align {
                    VerticalAlign::Top => 0,
                    VerticalAlign::Bottom => control.height - control.text_height,
                    VerticalAlign::Center => (control.height - control.text_height) / 2,
                };
                font.draw_forced(
                    list,
                    &control.text,
                    x,
                    y + offset,
                    control.width,
                    s.text_align,
                    control.height,
                    forced_color(tint),
                );
            }
        }
    }

    fn draw_edit<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        id: NativeId,
        ox: i32,
        oy: i32,
        tint: Rgba,
    ) {
        let Some(control) = self.control(id) else {
            return;
        };
        let Some(e) = control.edit() else { return };
        if !control.visible {
            return;
        }
        let (x, y) = (control.x + ox, control.y + oy);
        if let Some(back) = &control.back {
            self.draw_tiled(list, back, x, y, control.width, control.height, tint);
        }
        if let Some(path) = &control.image {
            let frame = control.anim.as_ref().map_or(0, |a| a.current_frame);
            self.draw_ui_image(list, path, frame, x, y, tint);
        }
        let font = self
            .control_font(control.font.as_deref())
            .and_then(|f| self.fonts.get(&f));
        let Some(font) = font else { return };
        let selected_font = e
            .font_selected
            .as_deref()
            .and_then(|f| self.fonts.get(f))
            .unwrap_or(font);
        // The parts `update_edit` laid out: the text around the
        // selection in the editor's font, the selection in
        // `FONT_SELECTED`, the caret while it shows.
        let caret = e.caret.as_ref().filter(|_| e.cursor_visible);
        for piece in e.pieces.iter().chain(caret) {
            let font = if piece.selected { selected_font } else { font };
            font.draw_forced(
                list,
                &self.game_text(&piece.text),
                x + piece.x,
                y + e.frame_width,
                piece.width,
                TextAlign::Left,
                e.line_height,
                forced_color(tint),
            );
        }
    }

    /// The bounding size of a loaded sprite, every frame counted
    /// (`CBSprite::GetBoundingRect`), from what is already decoded (no
    /// reading at draw time).
    pub(crate) fn sprite_size_loaded(&self, path: &str) -> Option<(i32, i32)> {
        let sprite = &self.ui.bank.sprite(path)?.sprite;
        let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for sub in sprite.frames.iter().flat_map(|f| f.subframes.iter()) {
            let (w, h) = match sub.rect.filter(|r| !r.is_empty()) {
                Some(r) => (r.width() as i32, r.height() as i32),
                None => {
                    let Some(image) = &sub.image else { continue };
                    match self
                        .ui
                        .bank
                        .texture(image, sub.color_key)
                        .map(|t| (t.width as i32, t.height as i32))
                        .or_else(|| {
                            self.ui
                                .bank
                                .mask_if_loaded(image, sub.color_key)
                                .map(|m| (m.width as i32, m.height as i32))
                        }) {
                        Some(size) => size,
                        None => continue,
                    }
                }
            };
            if w <= 0 || h <= 0 {
                continue;
            }
            left = left.min(-sub.hotspot_x);
            top = top.min(-sub.hotspot_y);
            right = right.max(-sub.hotspot_x + w);
            bottom = bottom.max(-sub.hotspot_y + h);
        }
        // Lazily: with no subframe measured the bounds are still the
        // sentinels and their difference overflows.
        (left <= right).then(|| (right - left, bottom - top))
    }

    /// Draws a frame of a sprite of the controls at a point.
    pub(crate) fn draw_ui_image<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        path: &str,
        frame: usize,
        x: i32,
        y: i32,
        tint: Rgba,
    ) {
        let Some(loaded) = self.ui.bank.sprite(path) else {
            return;
        };
        let Some(frame) = loaded
            .sprite
            .frames
            .get(frame.min(loaded.sprite.frames.len().saturating_sub(1)))
        else {
            return;
        };
        for sub in crate::scene_view::drawn_subframes(frame) {
            let Some(image) = &sub.image else { continue };
            let Some(texture) = self.ui.bank.texture(image, sub.color_key) else {
                continue;
            };
            list.push(place(sub, texture, x, y, 1.0, tint));
        }
    }

    /// Draws a tiled image stretched over a box (`CUITiledImage::Display`):
    /// the corners once, the edges along their sides, the middle over the
    /// rest, in whole tiles.
    #[allow(clippy::too_many_arguments, reason = "the engine's own signature")]
    fn draw_tiled<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        tiled: &TiledImage,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        tint: Rgba,
    ) {
        let Some(image) = &tiled.image else { return };
        let Some(texture) = self.ui.bank.texture(image, DEFAULT_COLOR_KEY) else {
            return;
        };
        let (columns, rows) = tiled.grid(width, height);
        let tile = |r: usize, c: usize| tiled.tile(r, c);
        let (tw, th) = (tile(1, 1).width() as i32, tile(1, 1).height() as i32);
        let left_w = tile(0, 0).width() as i32;
        let top_h = tile(0, 1).height() as i32;
        let right_x = x + left_w + columns * tw;
        // The right edge of the middle rows comes after `MIDDLE_LEFT`'s
        // width, the corners' after `UP_LEFT`'s.
        let middle_right_x = x + tile(1, 0).width() as i32 + columns * tw;
        let bottom_y = y + top_h + rows * th;
        let mut push = |rect: SpriteRect, dx: i32, dy: i32| {
            if rect.is_empty() {
                return;
            }
            list.push(Quad {
                texture,
                dst: Rect::new(
                    dx as f32,
                    dy as f32,
                    rect.width() as f32,
                    rect.height() as f32,
                ),
                src: Some(Rect::new(
                    rect.left as f32,
                    rect.top as f32,
                    rect.width() as f32,
                    rect.height() as f32,
                )),
                tint,
                flip_x: false,
                flip_y: false,
            });
        };
        push(tile(0, 0), x, y);
        push(tile(0, 2), right_x, y);
        push(tile(2, 0), x, bottom_y);
        push(tile(2, 2), right_x, bottom_y);
        for row in 0..rows {
            let yy = y + top_h + row * th;
            push(tile(1, 0), x, yy);
            push(tile(1, 2), middle_right_x, yy);
        }
        for col in 0..columns {
            let xx = x + left_w + col * tw;
            push(tile(0, 1), xx, y);
            push(tile(2, 1), xx, bottom_y);
        }
        for row in 0..rows {
            for col in 0..columns {
                push(tile(1, 1), x + left_w + col * tw, y + top_h + row * th);
            }
        }
    }
}

/// The largest offset not above `at` that falls between two characters
/// of `text` (and inside it): where a caret may stand.
fn char_floor(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}
