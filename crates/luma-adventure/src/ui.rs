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
    /// Start of the selection, in bytes.
    pub sel_start: usize,
    /// End of the selection and position of the caret, in bytes.
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
}

impl Default for EditState {
    fn default() -> Self {
        EditState {
            font_selected: None,
            sel_start: usize::MAX,
            sel_end: usize::MAX,
            scroll_offset: 0,
            frame_width: 0,
            max_length: -1,
            cursor_char: "|".to_string(),
            cursor_blink_rate: 600,
            last_blink: 0,
            cursor_visible: true,
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
            name: String::new(),
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

    /// Sets the text; `|` becomes a line break (`SetText`).
    pub fn set_text(&mut self, text: &str) {
        self.text = luma_formats::window::control_text(text);
        if let ControlKind::Edit(e) = &mut self.kind {
            e.sel_start = e.sel_start.min(self.text.len());
            e.sel_end = e.sel_end.min(self.text.len());
        }
    }

    /// Whether the control is drawn pressed.
    pub fn pressed(&self) -> bool {
        self.button()
            .is_some_and(|b| b.press || b.one_time_press.is_some() || b.stay_pressed)
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
    /// The sprites of the controls; loaded again after a saved game.
    #[serde(skip)]
    pub bank: SpriteBank,
    /// The object the left button went down on (`m_CapturedObject`).
    pub captured: Option<NativeId>,
    /// Whether the game is frozen by a system-exclusive window.
    pub frozen: bool,
    freeze_level: u32,
    orig_interactive: bool,
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

    /// Reads a `.window` file with its `TEMPLATE` chain.
    fn read_window_def(&mut self, path: &str, depth: u32) -> Option<WindowDef> {
        if depth > 8 {
            self.log(format!("window template chain too deep at '{path}'"));
            return None;
        }
        let bytes = match self.vfs.read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.log(format!("Error opening window file '{path}': {err}"));
                return None;
            }
        };
        let doc = match luma_formats::Document::parse(&bytes) {
            Ok(doc) => doc,
            Err(err) => {
                self.log(format!("Error parsing window file '{path}': {err}"));
                return None;
            }
        };
        use luma_formats::Nodes;
        let Some(root) = doc.get("WINDOW") else {
            self.log(format!("'WINDOW' keyword expected in '{path}'"));
            return None;
        };
        let mut def = WindowDef::default();
        def.apply(root);
        if let Some(template) = def.template.clone() {
            let mut base = self.read_window_def(&template, depth + 1)?;
            base.apply(root);
            def = base;
        }
        Some(def)
    }

    /// Loads a window file and adds the window to the game
    /// (`Game.LoadWindow`); `None` when the file cannot be read.
    pub fn load_window(&mut self, engine: &mut Engine, path: &str) -> Option<NativeId> {
        let def = self.read_window_def(path, 0)?;
        let id = self.new_control(ControlKind::Window(Box::default()), None);
        self.ui.windows.push(id);
        self.apply_window_def(engine, id, def, Some(path));
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
        let Some(def) = self.read_window_def(path, 0) else {
            return false;
        };
        if self.control(id).and_then(Control::window).is_none() {
            return false;
        }
        self.apply_window_def(engine, id, def, Some(path));
        true
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
        let mut tiled = match TiledImage::parse(&bytes) {
            Ok(tiled) => tiled,
            Err(err) => {
                self.log(format!("Error parsing tiled image '{path}': {err}"));
                return None;
            }
        };
        if let Some(template) = tiled.template.clone() {
            if let Some(mut base) = self.load_tiled(&template) {
                base.image = tiled.image.clone().or(base.image);
                for (row, tiles) in tiled.tiles.iter().enumerate() {
                    for (col, tile) in tiles.iter().enumerate() {
                        if !tile.is_empty() {
                            base.tiles[row][col] = *tile;
                        }
                    }
                }
                tiled = base;
            }
        }
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

    fn load_control_font(&mut self, path: &Option<String>) -> Option<String> {
        let path = path.as_deref()?;
        if self.load_font(path) {
            Some(path.to_string())
        } else {
            None
        }
    }

    fn load_control_image(&mut self, path: &Option<String>) -> Option<String> {
        let path = path.as_deref()?;
        if self.load_ui_image(path) {
            Some(path.to_string())
        } else {
            None
        }
    }

    fn load_control_back(&mut self, path: &Option<String>) -> Option<TiledImage> {
        let path = path.as_deref()?;
        self.load_tiled(path)
    }

    /// Fills a control's common fields from a definition, loading what
    /// it names.
    fn apply_control_def(&mut self, engine: &mut Engine, id: NativeId, def: &ControlDef) {
        let font = self.load_control_font(&def.font);
        let image = self.load_control_image(&def.image);
        let back = self.load_control_back(&def.back);
        if let Some(cursor) = &def.cursor {
            self.load_cursor(cursor);
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
    }

    pub(crate) fn apply_window_def(
        &mut self,
        engine: &mut Engine,
        id: NativeId,
        def: WindowDef,
        filename: Option<&str>,
    ) {
        self.apply_control_def(engine, id, &def.base);
        let title = self.expand(&def.title);
        let back_inactive = self.load_control_back(&def.back_inactive);
        let image_inactive = self.load_control_image(&def.image_inactive);
        let font_inactive = self.load_control_font(&def.font_inactive);
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
            self.create_widget(engine, id, widget);
        }
        self.correct_size(id);
    }

    /// Adds a control from its definition to a window.
    pub fn create_widget(
        &mut self,
        engine: &mut Engine,
        parent: NativeId,
        def: WidgetDef,
    ) -> NativeId {
        let id = match def {
            WidgetDef::Button(def) => {
                let id = self.new_control(ControlKind::Button(Box::default()), Some(parent));
                self.apply_button_def(engine, id, &def);
                id
            }
            WidgetDef::Static(def) => {
                let id =
                    self.new_control(ControlKind::Static(StaticState::default()), Some(parent));
                self.apply_static_def(engine, id, &def);
                id
            }
            WidgetDef::Edit(def) => {
                let id = self.new_control(ControlKind::Edit(Box::default()), Some(parent));
                self.apply_edit_def(engine, id, &def);
                id
            }
            WidgetDef::Window(def) => {
                let id = self.new_control(ControlKind::Window(Box::default()), Some(parent));
                self.apply_window_def(engine, id, *def, None);
                id
            }
            WidgetDef::EntityContainer(def) => {
                let id = self.new_control(ControlKind::EntityContainer(None), Some(parent));
                self.apply_control_def(engine, id, &def.base);
                if let Some(control) = self.control_mut(id) {
                    control.can_focus = false;
                }
                if let Some(path) = &def.entity {
                    self.set_container_entity(engine, id, path);
                }
                id
            }
        };
        if let Some(window) = self.control_mut(parent).and_then(Control::window_mut) {
            window.widgets.push(id);
        }
        id
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

    fn apply_button_def(&mut self, engine: &mut Engine, id: NativeId, def: &ButtonDef) {
        self.apply_control_def(engine, id, &def.base);
        let text = self.expand(&def.text);
        let state = ButtonState {
            back_hover: self.load_control_back(&def.back_hover),
            back_press: self.load_control_back(&def.back_press),
            back_disable: self.load_control_back(&def.back_disable),
            back_focus: self.load_control_back(&def.back_focus),
            image_hover: self.load_control_image(&def.image_hover),
            image_press: self.load_control_image(&def.image_press),
            image_disable: self.load_control_image(&def.image_disable),
            image_focus: self.load_control_image(&def.image_focus),
            font_hover: self.load_control_font(&def.font_hover),
            font_press: self.load_control_font(&def.font_press),
            font_disable: self.load_control_font(&def.font_disable),
            font_focus: self.load_control_font(&def.font_focus),
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
    }

    fn apply_static_def(&mut self, engine: &mut Engine, id: NativeId, def: &StaticDef) {
        self.apply_control_def(engine, id, &def.base);
        let text = self.expand(&def.text);
        if let Some(control) = self.control_mut(id) {
            control.text = text;
            control.can_focus = false;
            control.kind = ControlKind::Static(StaticState {
                text_align: def.text_align,
                vertical_align: def.vertical_align,
            });
        }
        self.correct_size(id);
    }

    fn apply_edit_def(&mut self, engine: &mut Engine, id: NativeId, def: &EditDef) {
        self.apply_control_def(engine, id, &def.base);
        let font_selected = self.load_control_font(&def.font_selected);
        if let Some(control) = self.control_mut(id) {
            control.set_text(&def.text);
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
    }

    /// The bounding size of a sprite's first frame placed at the origin
    /// (`CBSprite::GetBoundingRect`), reading the images for the
    /// sub-frames without a rectangle.
    fn sprite_size(&mut self, path: &str) -> Option<(i32, i32)> {
        let frame = self.ui.bank.sprite(path)?.sprite.frames.first()?.clone();
        let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for sub in &frame.subframes {
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
    fn correct_size(&mut self, id: NativeId) {
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

    /// Sets a static's size to its text (`SizeToFit`), or only its height
    /// (`HeightToFit`).
    pub fn size_static_to_fit(&mut self, id: NativeId, width_too: bool) {
        let Some(control) = self.control(id) else {
            return;
        };
        if control.font.is_none() || control.text.is_empty() {
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
        for control in &all {
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
    /// exclusive one was focused keeps the exclusive one on top.
    pub fn focus_window(&mut self, id: NativeId) -> bool {
        let previous = self.ui.focused_window;
        let Some(index) = self.ui.windows.iter().position(|w| *w == id) else {
            return false;
        };
        if index + 1 < self.ui.windows.len() {
            self.ui.windows.remove(index);
            self.ui.windows.push(id);
        }
        self.ui.focused_window = Some(id);
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
        self.focus_window(self.top_window(id));
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

    /// Marks the scripts of a window and of its controls as freezable or
    /// not (`MakeFreezable`).
    fn make_freezable(&mut self, engine: &mut Engine, id: NativeId, freezable: bool) {
        let mut all = self.descendants(id);
        all.push(id);
        for control in all {
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
        self.step_control_anim(id);
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
        let def = match self
            .vfs
            .read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| Entity::parse(&bytes).map_err(|e| e.to_string()))
        {
            Ok(def) => def,
            Err(reason) => {
                self.log(format!("Error parsing ENTITY file '{path}': {reason}"));
                return false;
            }
        };
        let Some(ControlKind::EntityContainer(previous)) = self.control(id).map(|c| c.kind.clone())
        else {
            return false;
        };
        if let Some(previous) = previous {
            if let Some(old) = self.node_id(EntityRef::Ui(previous)) {
                for script in engine.scripts_of(old) {
                    engine.finish(script, true);
                }
                self.objects.remove(old);
            }
        }
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
        });
        let entity = self.objects.insert(Kind::Entity(EntityRef::Ui(index)));
        for script in scripts {
            self.attach(engine, entity, &script);
        }
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

    /// Advances the animation of a container's entity sprite.
    fn step_ui_entity_anim(&mut self, index: Option<usize>) {
        let timer = self.timer();
        let Some(entity) = index.and_then(|i| self.ui_entities.get_mut(i)) else {
            return;
        };
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
        anim.step(&loaded.sprite, timer);
    }

    /// Advances the animation of a control's image.
    fn step_control_anim(&mut self, id: NativeId) {
        let timer = self.timer();
        let Some(control) = self.ui.controls.get_mut(&id) else {
            return;
        };
        let Some(path) = control.image.clone() else {
            control.anim = None;
            return;
        };
        let Some(loaded) = self.ui.bank.sprite(&path) else {
            return;
        };
        let anim = control.anim.get_or_insert_with(|| AnimState::new(&path));
        if anim.sprite != crate::scene_view::key(&path) {
            *anim = AnimState::new(&path);
        }
        anim.step(&loaded.sprite, timer);
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
        self.control_font(control.font.as_deref())
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
        let interactive = self.interactive;
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
        self.step_control_anim(id);
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
        if let Some(b) = self.control_mut(id).and_then(Control::button_mut) {
            b.one_time_press = Some(clock);
        }
    }

    fn update_static(&mut self, id: NativeId) {
        self.step_control_anim(id);
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

    fn update_edit(&mut self, id: NativeId) {
        self.step_control_anim(id);
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
        let Some(e) = control.edit() else { return };
        let (frame, sel_end, mut scroll, blink_rate, last_blink, cursor_char) = (
            e.frame_width,
            e.sel_end.min(text.len()),
            e.scroll_offset.min(text.len()),
            e.cursor_blink_rate,
            e.last_blink,
            e.cursor_char.clone(),
        );
        // Keep the caret in view.
        if let Some(font) = &font {
            let cursor_width = self.measure_text_width(Some(font), &cursor_char);
            loop {
                let shown = text.get(scroll..sel_end).unwrap_or("");
                let w = self.measure_text_width(Some(font), shown);
                if w <= width - cursor_width - 2 * frame || scroll >= text.len() {
                    break;
                }
                scroll += 1;
                while scroll < text.len() && !text.is_char_boundary(scroll) {
                    scroll += 1;
                }
            }
            scroll = scroll.min(sel_end);
            let shown = text.get(scroll..).unwrap_or("").to_string();
            let with_caret = format!("{shown}{cursor_char}");
            if let Some(f) = self.fonts.get_mut(font) {
                f.prepare(&shown, -1, TextAlign::Left, -1);
                f.prepare(&with_caret, -1, TextAlign::Left, -1);
                f.prepare(&cursor_char, -1, TextAlign::Left, -1);
            }
        }
        let blink = blink_rate > 0 && clock.saturating_sub(last_blink) >= blink_rate as u64;
        if let Some(e) = self.control_mut(id).and_then(Control::edit_mut) {
            e.scroll_offset = scroll;
            if !focused {
                e.cursor_visible = false;
            } else if blink || last_blink == 0 {
                e.cursor_visible = last_blink == 0 || !e.cursor_visible;
                e.last_blink = clock;
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
        let image = control.image.clone();
        let frame = control.anim.as_ref().map_or(0, |a| a.current_frame);
        if let Some(shield) = self.shield_of(id) {
            let (sw, sh) = (self.settings.width as i32, self.settings.height as i32);
            hits.push(HitArea::rect(shield, 0, 0, sw, sh));
        }
        if !visible {
            return;
        }
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
                    let image = c.image.clone();
                    let frame = c.anim.as_ref().map_or(0, |a| a.current_frame);
                    if b.pixel_perfect {
                        if let Some(loaded) = image.as_deref().and_then(|p| self.ui.bank.sprite(p))
                        {
                            World::sprite_hits(
                                widget,
                                HitBank::Ui,
                                &loaded.sprite,
                                frame,
                                bx,
                                by,
                                100.0,
                                (0, 0),
                                hits,
                            );
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
                            cx + entity.def.x,
                            cy + entity.def.y,
                            100.0,
                            (0, 0),
                            hits,
                        );
                    }
                }
            }
        }
    }

    /// The left button went down on the active object: a window starts
    /// dragging when the point is in its drag rectangle
    /// (`CUIWindow::HandleMouse`).
    pub(crate) fn ui_mouse_down(&mut self) {
        let active = self.active_object();
        self.ui.captured = active;
        let Some(id) = active else { return };
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

    /// A key reaches the focused window (`CUIWindow::HandleKeypress`):
    /// `Tab` moves the focus, else the focused control takes it. `true`
    /// when handled.
    pub(crate) fn window_key(&mut self, window: NativeId, code: i32, text: Option<char>) -> bool {
        if code == VK_TAB {
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
            Some(ControlKind::Window(_)) => self.window_key(focused, code, text),
            Some(ControlKind::Edit(_)) => self.edit_key(focused, code, text),
            _ => false,
        }
    }

    /// A key in a text editor (`CUIEdit::HandleKeypress`); an edit of
    /// the text raises an event named after the editor on its parent
    /// when `PARENT_NOTIFY` is on, as `InsertChars` and `DeleteChars`
    /// do.
    pub fn edit_key(&mut self, id: NativeId, code: i32, text: Option<char>) -> bool {
        let (shift, control_down) = (self.keyboard.shift, self.keyboard.control);
        let Some(control) = self.control_mut(id) else {
            return false;
        };
        let (parent, parent_notify, name) =
            (control.parent, control.parent_notify, control.name.clone());
        let len = control.text.len();
        let Some(e) = control.edit_mut() else {
            return false;
        };
        e.sel_start = e.sel_start.min(len);
        e.sel_end = e.sel_end.min(len);
        let (mut start, mut end) = (e.sel_start, e.sel_end);
        let max_length = e.max_length;
        let mut handled = true;
        let mut edited = false;
        let mut new_text = control.text.clone();
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
        match code {
            VK_ESCAPE | VK_TAB | VK_RETURN => return false,
            VK_BACK => {
                edited = true;
                if start == end {
                    let from = prev_boundary(&new_text, start);
                    new_text.replace_range(from..start, "");
                    start = from;
                } else {
                    let (a, b) = (start.min(end), start.max(end));
                    new_text.replace_range(a..b, "");
                    start = a;
                }
                end = start;
            }
            VK_DELETE => {
                edited = true;
                if start == end {
                    let to = next_boundary(&new_text, start);
                    new_text.replace_range(start..to, "");
                } else {
                    let (a, b) = (start.min(end), start.max(end));
                    new_text.replace_range(a..b, "");
                    start = a;
                }
                end = start;
            }
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
                end = 0;
                if !shift {
                    start = end;
                }
            }
            VK_END => {
                end = new_text.len();
                if !shift {
                    start = end;
                }
            }
            _ if control_down && code == i32::from(b'A') => {
                start = 0;
                end = new_text.len();
            }
            _ => match text.filter(|c| !c.is_control()) {
                Some(ch) => {
                    edited = true;
                    if start != end {
                        let (a, b) = (start.min(end), start.max(end));
                        new_text.replace_range(a..b, "");
                        end = a;
                    }
                    let fits = max_length < 0 || new_text.chars().count() < max_length as usize;
                    if fits {
                        new_text.insert(end, ch);
                        end += ch.len_utf8();
                    }
                    start = end;
                }
                None => handled = false,
            },
        }
        control.text = new_text;
        if let Some(e) = control.edit_mut() {
            e.sel_start = start;
            e.sel_end = end;
        }
        if edited && parent_notify {
            if let Some(parent) = parent {
                self.queue_event(parent, name);
            }
        }
        handled
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
        let tint = window.alpha_color.unwrap_or(tint);
        let focused = self.is_focused(id);
        let (x, y) = (control.x + ox, control.y + oy);
        let back = if !focused && window.back_inactive.is_some() {
            window.back_inactive.as_ref()
        } else {
            control.back.as_ref()
        };
        let image = if !focused && window.image_inactive.is_some() {
            window.image_inactive.as_deref()
        } else {
            control.image.as_deref()
        };
        if let Some(back) = back {
            self.draw_tiled(list, back, x, y, control.width, control.height, tint);
        }
        if let Some(image) = image {
            let frame = control.anim.as_ref().map_or(0, |a| a.current_frame);
            self.draw_ui_image(list, image, frame, x, y, tint);
        }
        if let (Some(rect), false) = (window.title_rect, control.text.is_empty()) {
            if let Some(font) = self.window_font(id).and_then(|f| self.fonts.get(&f)) {
                font.draw(
                    list,
                    &control.text,
                    x + rect.left,
                    y + rect.top,
                    rect.width() as i32,
                    window.title_align,
                    rect.height() as i32,
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
        let (x, y) = (ox + control.x + entity.def.x, oy + control.y + entity.def.y);
        let tint = entity.def.tint.unwrap_or(tint);
        for sub in &frame.subframes {
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
        let (state_back, state_image) = if control.disabled {
            (b.back_disable.as_ref(), b.image_disable.as_deref())
        } else if pressed {
            (b.back_press.as_ref(), b.image_press.as_deref())
        } else if b.hover {
            (b.back_hover.as_ref(), b.image_hover.as_deref())
        } else if control.can_focus && self.is_focused(id) {
            (b.back_focus.as_ref(), b.image_focus.as_deref())
        } else {
            (None, None)
        };
        let back = state_back.or(control.back.as_ref());
        let image = state_image.or(control.image.as_deref());
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
        let shift = i32::from(pressed && back.is_some());
        if let Some(path) = image {
            let frame = if Some(path) == control.image.as_deref() {
                control.anim.as_ref().map_or(0, |a| a.current_frame)
            } else {
                0
            };
            self.draw_ui_image(list, path, frame, ix + shift, iy + shift, tint);
        }
        if !control.text.is_empty() {
            if let Some(font) = self.button_font(id).and_then(|f| self.fonts.get(&f)) {
                let offset = (control.height - control.text_height) / 2;
                let shift = i32::from(pressed);
                font.draw(
                    list,
                    &control.text,
                    x + shift,
                    y + offset + shift,
                    control.width,
                    b.text_align,
                    -1,
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
                font.draw(
                    list,
                    &control.text,
                    x,
                    y + offset,
                    control.width,
                    s.text_align,
                    control.height,
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
        let shown = control
            .text
            .get(e.scroll_offset.min(control.text.len())..)
            .unwrap_or("");
        let frame = e.frame_width;
        let caret_at = e.sel_end.min(control.text.len());
        if e.cursor_visible && self.is_focused(id) {
            let before = control
                .text
                .get(e.scroll_offset.min(caret_at)..caret_at)
                .unwrap_or("");
            let after = control.text.get(caret_at..).unwrap_or("");
            let with_caret = format!("{before}{}{after}", e.cursor_char);
            font.draw(
                list,
                &with_caret,
                x + frame,
                y + frame,
                -1,
                TextAlign::Left,
                -1,
            );
        } else {
            font.draw(list, shown, x + frame, y + frame, -1, TextAlign::Left, -1);
        }
    }

    /// The bounding size of a loaded sprite's first frame, from what is
    /// already decoded (no reading at draw time).
    pub(crate) fn sprite_size_loaded(&self, path: &str) -> Option<(i32, i32)> {
        let frame = self.ui.bank.sprite(path)?.sprite.frames.first()?;
        let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for sub in &frame.subframes {
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
            left = left.min(-sub.hotspot_x);
            top = top.min(-sub.hotspot_y);
            right = right.max(-sub.hotspot_x + w);
            bottom = bottom.max(-sub.hotspot_y + h);
        }
        (left <= right).then_some((right - left, bottom - top))
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
        for sub in &frame.subframes {
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
            push(tile(1, 2), right_x, yy);
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
