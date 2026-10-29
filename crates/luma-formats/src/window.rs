//! Windows and their controls (`.window`), tiled images (`.tiled`), the
//! inventory box (`.def`) and the items (`.items`).
//!
//! The formats are documented in `docs/formats/window.md` and were
//! established from the MIT-licensed WME Lite sources (`UIWindow.cpp`,
//! `UIObject.cpp`, `UIButton.cpp`, `UIText.cpp`, `UIEdit.cpp`,
//! `UITiledImage.cpp`, `AdInventoryBox.cpp`, `AdItem.cpp`, Copyright (c)
//! 2011 Jan Nedoma). See `NOTICE` at the root of the repository.

use crate::def::{Document, Node, Nodes};
use crate::error::{Error, Result};
use crate::font::TextAlign;
use crate::image::{Rgb, Rgba};
use crate::scene::Property;
use crate::sprite::Rect;

/// Vertical placement of a text in its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerticalAlign {
    /// At the top.
    Top,
    /// Centred.
    #[default]
    Center,
    /// At the bottom.
    Bottom,
}

impl VerticalAlign {
    /// From the key's value: `top`, `bottom`, anything else centred.
    pub fn from_name(name: &str) -> VerticalAlign {
        if name.eq_ignore_ascii_case("top") {
            VerticalAlign::Top
        } else if name.eq_ignore_ascii_case("bottom") {
            VerticalAlign::Bottom
        } else {
            VerticalAlign::Center
        }
    }

    /// The engine's code (`VAL_TOP` 0, `VAL_CENTER` 1, `VAL_BOTTOM` 2).
    pub fn code(self) -> i32 {
        match self {
            VerticalAlign::Top => 0,
            VerticalAlign::Center => 1,
            VerticalAlign::Bottom => 2,
        }
    }

    /// From the engine's code; anything else is centred.
    pub fn from_code(code: i32) -> VerticalAlign {
        match code {
            0 => VerticalAlign::Top,
            2 => VerticalAlign::Bottom,
            _ => VerticalAlign::Center,
        }
    }
}

/// Alignment from a key's value: `left`, `right`, anything else centred
/// (buttons) or left (statics and titles choose their own default).
fn text_align(name: &str, other: TextAlign) -> TextAlign {
    if name.eq_ignore_ascii_case("left") {
        TextAlign::Left
    } else if name.eq_ignore_ascii_case("right") {
        TextAlign::Right
    } else {
        other
    }
}

fn rect_of(node: &Node) -> Option<Rect> {
    let v = node.as_ints()?;
    if v.len() < 4 {
        return None;
    }
    Some(Rect {
        left: v[0] as i32,
        top: v[1] as i32,
        right: v[2] as i32,
        bottom: v[3] as i32,
    })
}

fn rgb_of(node: &Node) -> Option<Rgb> {
    let v = node.as_ints()?;
    if v.len() < 3 {
        return None;
    }
    Some(Rgb {
        r: v[0].clamp(0, 255) as u8,
        g: v[1].clamp(0, 255) as u8,
        b: v[2].clamp(0, 255) as u8,
    })
}

fn text(node: &Node) -> String {
    node.as_str().unwrap_or_default().to_string()
}

fn path(node: &Node) -> Option<String> {
    node.as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

/// The engine turns `|` into a line break when a control's text is set
/// (`CUIObject::SetText`).
pub fn control_text(text: &str) -> String {
    text.replace('|', "\n")
}

/// An image cut into nine tiles (`TILED_IMAGE`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TiledImage {
    /// A `.tiled` file loaded first (`TEMPLATE`), to be merged by the
    /// caller.
    pub template: Option<String>,
    /// The image.
    pub image: Option<String>,
    /// The tiles, rows from the top, columns from the left.
    pub tiles: [[Rect; 3]; 3],
}

impl TiledImage {
    /// Parses a `.tiled` file: a `TILED_IMAGE` root block.
    pub fn parse(bytes: &[u8]) -> Result<TiledImage> {
        let doc = Document::parse(bytes)?;
        let root = doc.get("TILED_IMAGE").ok_or_else(|| Error::Invalid {
            reason: "'TILED_IMAGE' keyword expected".into(),
        })?;
        let mut tiled = TiledImage::default();
        tiled.apply(root);
        Ok(tiled)
    }

    /// Reads the keys of a `TILED_IMAGE` block, overriding what is
    /// there. `HORIZONTAL_TILES` with `VERTICAL_TILES` lay out the nine
    /// tiles from three column widths and three row heights.
    pub fn apply(&mut self, node: &Node) {
        let mut horizontal: Option<[i32; 3]> = None;
        let mut vertical: Option<[i32; 3]> = None;
        for child in node.nodes() {
            let key = child.key.to_ascii_uppercase();
            let (row, col) = match key.as_str() {
                "TEMPLATE" => {
                    self.template = path(child);
                    continue;
                }
                "IMAGE" => {
                    self.image = path(child);
                    continue;
                }
                "HORIZONTAL_TILES" | "VERTICAL_TILES" => {
                    let v = child.as_ints().unwrap_or_default();
                    if v.len() >= 3 {
                        let sizes = [v[0] as i32, v[1] as i32, v[2] as i32];
                        if key == "HORIZONTAL_TILES" {
                            horizontal = Some(sizes);
                        } else {
                            vertical = Some(sizes);
                        }
                    }
                    continue;
                }
                "UP_LEFT" => (0, 0),
                "UP_MIDDLE" => (0, 1),
                "UP_RIGHT" => (0, 2),
                "MIDDLE_LEFT" => (1, 0),
                "MIDDLE_MIDDLE" => (1, 1),
                "MIDDLE_RIGHT" => (1, 2),
                "DOWN_LEFT" => (2, 0),
                "DOWN_MIDDLE" => (2, 1),
                "DOWN_RIGHT" => (2, 2),
                _ => continue,
            };
            if let Some(rect) = rect_of(child) {
                self.tiles[row][col] = rect;
            }
        }
        if let (Some(h), Some(v)) = (horizontal, vertical) {
            let xs = [0, h[0], h[0] + h[1], h[0] + h[1] + h[2]];
            let ys = [0, v[0], v[0] + v[1], v[0] + v[1] + v[2]];
            for (row, tiles) in self.tiles.iter_mut().enumerate() {
                for (col, tile) in tiles.iter_mut().enumerate() {
                    *tile = Rect {
                        left: xs[col],
                        top: ys[row],
                        right: xs[col + 1],
                        bottom: ys[row + 1],
                    };
                }
            }
        }
    }

    /// Fills the tiles left empty with thirds of an image of the given
    /// size (the engine's default once the image is loaded).
    pub fn fill_defaults(&mut self, image_width: i32, image_height: i32) {
        let (w, h) = (image_width / 3, image_height / 3);
        for (row, tiles) in self.tiles.iter_mut().enumerate() {
            for (col, tile) in tiles.iter_mut().enumerate() {
                if tile.is_empty() {
                    *tile = Rect {
                        left: col as i32 * w,
                        top: row as i32 * h,
                        right: (col as i32 + 1) * w,
                        bottom: (row as i32 + 1) * h,
                    };
                }
            }
        }
    }

    /// The tile of a row and column (0 to 2 each).
    pub fn tile(&self, row: usize, col: usize) -> Rect {
        self.tiles[row][col]
    }

    /// How many columns and rows of the middle tile a box of `width` by
    /// `height` holds (`CUITiledImage::Display`); zero when the middle
    /// tile is empty.
    pub fn grid(&self, width: i32, height: i32) -> (i32, i32) {
        let middle = self.tiles[1][1];
        let (tw, th) = (middle.width() as i32, middle.height() as i32);
        if tw <= 0 || th <= 0 {
            return (0, 0);
        }
        let left = self.tiles[1][0].width() as i32;
        let right = self.tiles[1][2].width() as i32;
        let top = self.tiles[0][1].height() as i32;
        let bottom = self.tiles[2][1].height() as i32;
        ((width - left - right) / tw, (height - top - bottom) / th)
    }

    /// The size a box of `width` by `height` is reduced to so that whole
    /// tiles fill it (`CUITiledImage::CorrectSize`).
    pub fn corrected_size(&self, width: i32, height: i32) -> (i32, i32) {
        let middle = self.tiles[1][1];
        let (tw, th) = (middle.width() as i32, middle.height() as i32);
        if tw <= 0 || th <= 0 {
            return (width, height);
        }
        let (columns, rows) = self.grid(width, height);
        let left = self.tiles[1][0].width() as i32;
        let right = self.tiles[1][2].width() as i32;
        let top = self.tiles[0][1].height() as i32;
        let bottom = self.tiles[2][1].height() as i32;
        (left + right + columns * tw, top + bottom + rows * th)
    }
}

/// The keys every control has (`UIObject`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlDef {
    /// Name used by scripts.
    pub name: String,
    /// Free caption.
    pub caption: String,
    /// Position, relative to the parent window.
    pub x: i32,
    /// See `x`.
    pub y: i32,
    /// Size; zero until corrected.
    pub width: i32,
    /// See `width`.
    pub height: i32,
    /// A `.tiled` background.
    pub back: Option<String>,
    /// An image or sprite.
    pub image: Option<String>,
    /// The font of the text.
    pub font: Option<String>,
    /// The cursor over the control.
    pub cursor: Option<String>,
    /// Drawn and clickable.
    pub visible: bool,
    /// Ignored by the mouse and the focus.
    pub disabled: bool,
    /// A press raises an event named after the control on its parent.
    pub parent_notify: bool,
    /// Scripts attached to the control.
    pub scripts: Vec<String>,
}

impl Default for ControlDef {
    fn default() -> Self {
        ControlDef {
            name: String::new(),
            caption: String::new(),
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            back: None,
            image: None,
            font: None,
            cursor: None,
            visible: true,
            disabled: false,
            parent_notify: false,
            scripts: Vec::new(),
        }
    }
}

impl ControlDef {
    /// Reads a common key; `false` when the key is not a common one.
    fn apply_key(&mut self, key: &str, child: &Node) -> bool {
        match key {
            "NAME" => self.name = text(child),
            "CAPTION" => self.caption = text(child),
            "X" => self.x = child.as_int().unwrap_or(0) as i32,
            "Y" => self.y = child.as_int().unwrap_or(0) as i32,
            "WIDTH" => self.width = child.as_int().unwrap_or(0) as i32,
            "HEIGHT" => self.height = child.as_int().unwrap_or(0) as i32,
            "BACK" => self.back = path(child),
            "IMAGE" => self.image = path(child),
            "FONT" => self.font = path(child),
            "CURSOR" => self.cursor = path(child),
            "VISIBLE" => self.visible = child.as_bool().unwrap_or(true),
            "DISABLED" => self.disabled = child.as_bool().unwrap_or(false),
            "PARENT_NOTIFY" => self.parent_notify = child.as_bool().unwrap_or(false),
            "SCRIPT" => {
                if let Some(p) = path(child) {
                    self.scripts.push(p);
                }
            }
            _ => return false,
        }
        true
    }
}

/// A button (`BUTTON`): images, backgrounds and fonts per state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonDef {
    /// The common keys.
    pub base: ControlDef,
    /// A `.button` file loaded first (`TEMPLATE`).
    pub template: Option<String>,
    /// Tiled backgrounds of the hovered, pressed, disabled and focused
    /// states.
    pub back_hover: Option<String>,
    /// See `back_hover`.
    pub back_press: Option<String>,
    /// See `back_hover`.
    pub back_disable: Option<String>,
    /// See `back_hover`.
    pub back_focus: Option<String>,
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
    /// The label, before string table expansion.
    pub text: String,
    /// Alignment of the label.
    pub text_align: TextAlign,
    /// The image is centred in the button.
    pub center_image: bool,
    /// Stays drawn pressed.
    pub pressed: bool,
    /// Can take the keyboard focus.
    pub focusable: bool,
    /// Only the opaque pixels of the image catch the mouse.
    pub pixel_perfect: bool,
}

impl Default for ButtonDef {
    fn default() -> Self {
        ButtonDef {
            base: ControlDef::default(),
            template: None,
            back_hover: None,
            back_press: None,
            back_disable: None,
            back_focus: None,
            image_hover: None,
            image_press: None,
            image_disable: None,
            image_focus: None,
            font_hover: None,
            font_press: None,
            font_disable: None,
            font_focus: None,
            text: String::new(),
            text_align: TextAlign::Center,
            center_image: false,
            pressed: false,
            focusable: false,
            pixel_perfect: false,
        }
    }
}

impl ButtonDef {
    /// Parses a `.button` file: a `BUTTON` root block.
    pub fn parse(bytes: &[u8]) -> Result<ButtonDef> {
        let doc = Document::parse(bytes)?;
        let root = doc.get("BUTTON").ok_or_else(|| Error::Invalid {
            reason: "'BUTTON' keyword expected".into(),
        })?;
        let mut button = ButtonDef::default();
        button.apply(root);
        Ok(button)
    }

    /// Reads the keys of a `BUTTON` block, overriding what is there.
    pub fn apply(&mut self, node: &Node) {
        for child in node.nodes() {
            let key = child.key.to_ascii_uppercase();
            if self.base.apply_key(&key, child) {
                continue;
            }
            match key.as_str() {
                "TEMPLATE" => self.template = path(child),
                "BACK_HOVER" => self.back_hover = path(child),
                "BACK_PRESS" => self.back_press = path(child),
                "BACK_DISABLE" => self.back_disable = path(child),
                "BACK_FOCUS" => self.back_focus = path(child),
                "IMAGE_HOVER" => self.image_hover = path(child),
                "IMAGE_PRESS" => self.image_press = path(child),
                "IMAGE_DISABLE" => self.image_disable = path(child),
                "IMAGE_FOCUS" => self.image_focus = path(child),
                "FONT_HOVER" => self.font_hover = path(child),
                "FONT_PRESS" => self.font_press = path(child),
                "FONT_DISABLE" => self.font_disable = path(child),
                "FONT_FOCUS" => self.font_focus = path(child),
                "TEXT" => self.text = control_text(&text(child)),
                "TEXT_ALIGN" => self.text_align = text_align(&text(child), TextAlign::Center),
                "CENTER_IMAGE" => self.center_image = child.as_bool().unwrap_or(false),
                "PRESSED" => self.pressed = child.as_bool().unwrap_or(false),
                "FOCUSABLE" => self.focusable = child.as_bool().unwrap_or(false),
                "PIXEL_PERFECT" => self.pixel_perfect = child.as_bool().unwrap_or(false),
                _ => {}
            }
        }
    }

    /// The image the size is corrected from: the normal one, else the
    /// first of the states (`CUIButton::CorrectSize`).
    pub fn size_image(&self) -> Option<&str> {
        self.base
            .image
            .as_deref()
            .or(self.image_disable.as_deref())
            .or(self.image_hover.as_deref())
            .or(self.image_press.as_deref())
            .or(self.image_focus.as_deref())
    }
}

/// A static text (`STATIC`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticDef {
    /// The common keys.
    pub base: ControlDef,
    /// A `.static` file loaded first (`TEMPLATE`).
    pub template: Option<String>,
    /// The text, before string table expansion.
    pub text: String,
    /// Horizontal alignment.
    pub text_align: TextAlign,
    /// Vertical placement.
    pub vertical_align: VerticalAlign,
}

impl Default for StaticDef {
    fn default() -> Self {
        StaticDef {
            base: ControlDef::default(),
            template: None,
            text: String::new(),
            text_align: TextAlign::Left,
            vertical_align: VerticalAlign::Center,
        }
    }
}

impl StaticDef {
    /// Parses a `.static` file: a `STATIC` root block.
    pub fn parse(bytes: &[u8]) -> Result<StaticDef> {
        let doc = Document::parse(bytes)?;
        let root = doc.get("STATIC").ok_or_else(|| Error::Invalid {
            reason: "'STATIC' keyword expected".into(),
        })?;
        let mut def = StaticDef::default();
        def.apply(root);
        Ok(def)
    }

    /// Reads the keys of a `STATIC` block, overriding what is there.
    pub fn apply(&mut self, node: &Node) {
        for child in node.nodes() {
            let key = child.key.to_ascii_uppercase();
            if self.base.apply_key(&key, child) {
                continue;
            }
            match key.as_str() {
                "TEMPLATE" => self.template = path(child),
                "TEXT" => self.text = control_text(&text(child)),
                "TEXT_ALIGN" => self.text_align = text_align(&text(child), TextAlign::Center),
                "VERTICAL_ALIGN" => self.vertical_align = VerticalAlign::from_name(&text(child)),
                _ => {}
            }
        }
    }
}

/// A text editor (`EDIT`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditDef {
    /// The common keys.
    pub base: ControlDef,
    /// An `.edit` file loaded first (`TEMPLATE`).
    pub template: Option<String>,
    /// The text being edited.
    pub text: String,
    /// The font of the selected part of the text.
    pub font_selected: Option<String>,
    /// Longest text accepted; `-1` for no limit.
    pub max_length: i32,
    /// Margin between the frame and the text.
    pub frame_width: i32,
    /// Milliseconds between blinks of the caret.
    pub cursor_blink_rate: i32,
}

impl Default for EditDef {
    fn default() -> Self {
        EditDef {
            base: ControlDef::default(),
            template: None,
            text: String::new(),
            font_selected: None,
            max_length: -1,
            frame_width: 0,
            cursor_blink_rate: 600,
        }
    }
}

impl EditDef {
    /// Parses an `.edit` file: an `EDIT` root block.
    pub fn parse(bytes: &[u8]) -> Result<EditDef> {
        let doc = Document::parse(bytes)?;
        let root = doc.get("EDIT").ok_or_else(|| Error::Invalid {
            reason: "'EDIT' keyword expected".into(),
        })?;
        let mut def = EditDef::default();
        def.apply(root);
        Ok(def)
    }

    /// Reads the keys of an `EDIT` block, overriding what is there.
    pub fn apply(&mut self, node: &Node) {
        for child in node.nodes() {
            let key = child.key.to_ascii_uppercase();
            if self.base.apply_key(&key, child) {
                continue;
            }
            match key.as_str() {
                "TEMPLATE" => self.template = path(child),
                "TEXT" => self.text = control_text(&text(child)),
                "FONT_SELECTED" => self.font_selected = path(child),
                "MAX_LENGTH" => self.max_length = child.as_int().unwrap_or(-1) as i32,
                "FRAME_WIDTH" => self.frame_width = child.as_int().unwrap_or(0) as i32,
                "CURSOR_BLINK_RATE" => {
                    self.cursor_blink_rate = child.as_int().unwrap_or(600) as i32;
                }
                _ => {}
            }
        }
    }
}

/// A control of a window, in drawing order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WidgetDef {
    /// A button.
    Button(Box<ButtonDef>),
    /// A static text.
    Static(StaticDef),
    /// A text editor.
    Edit(EditDef),
    /// A nested window.
    Window(Box<WindowDef>),
}

impl WidgetDef {
    /// The common keys of the control.
    pub fn base(&self) -> &ControlDef {
        match self {
            WidgetDef::Button(b) => &b.base,
            WidgetDef::Static(s) => &s.base,
            WidgetDef::Edit(e) => &e.base,
            WidgetDef::Window(w) => &w.base,
        }
    }
}

/// A window (`WINDOW`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowDef {
    /// The common keys.
    pub base: ControlDef,
    /// A `.window` file loaded first (`TEMPLATE`), to be merged by the
    /// caller.
    pub template: Option<String>,
    /// Background, image and font while the window is not focused.
    pub back_inactive: Option<String>,
    /// See `back_inactive`.
    pub image_inactive: Option<String>,
    /// See `back_inactive`.
    pub font_inactive: Option<String>,
    /// The title, before string table expansion.
    pub title: String,
    /// Alignment of the title.
    pub title_align: TextAlign,
    /// Where the title is drawn; `None` for no title.
    pub title_rect: Option<Rect>,
    /// The area the window is dragged by.
    pub drag_rect: Option<Rect>,
    /// The window itself does not catch the mouse.
    pub transparent: bool,
    /// A click outside closes the window.
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
    /// The controls, in drawing order.
    pub widgets: Vec<WidgetDef>,
}

impl Default for WindowDef {
    fn default() -> Self {
        WindowDef {
            base: ControlDef::default(),
            template: None,
            back_inactive: None,
            image_inactive: None,
            font_inactive: None,
            title: String::new(),
            title_align: TextAlign::Left,
            title_rect: None,
            drag_rect: None,
            transparent: false,
            menu: false,
            in_game: false,
            clip_contents: false,
            pause_music: true,
            fade_color: None,
            alpha_color: None,
            widgets: Vec::new(),
        }
    }
}

impl WindowDef {
    /// Parses a `.window` file: a `WINDOW` root block.
    pub fn parse(bytes: &[u8]) -> Result<WindowDef> {
        let doc = Document::parse(bytes)?;
        let root = doc.get("WINDOW").ok_or_else(|| Error::Invalid {
            reason: "'WINDOW' keyword expected".into(),
        })?;
        let mut window = WindowDef::default();
        window.apply(root);
        Ok(window)
    }

    /// Reads the keys of a `WINDOW` block, overriding what is there;
    /// the controls are appended.
    pub fn apply(&mut self, node: &Node) {
        let mut fade_rgb: Option<Rgb> = None;
        let mut fade_alpha: i32 = 255;
        let mut alpha_rgb: Option<Rgb> = None;
        let mut alpha: i32 = 255;
        for child in node.nodes() {
            let key = child.key.to_ascii_uppercase();
            if self.base.apply_key(&key, child) {
                continue;
            }
            match key.as_str() {
                "TEMPLATE" => self.template = path(child),
                "BACK_INACTIVE" => self.back_inactive = path(child),
                "IMAGE_INACTIVE" => self.image_inactive = path(child),
                "FONT_INACTIVE" => self.font_inactive = path(child),
                "TITLE" => self.title = control_text(&text(child)),
                "TITLE_ALIGN" => self.title_align = text_align(&text(child), TextAlign::Center),
                "TITLE_RECT" => self.title_rect = rect_of(child).filter(|r| !r.is_empty()),
                "DRAG_RECT" => self.drag_rect = rect_of(child).filter(|r| !r.is_empty()),
                "TRANSPARENT" => self.transparent = child.as_bool().unwrap_or(false),
                "MENU" => self.menu = child.as_bool().unwrap_or(false),
                "IN_GAME" => self.in_game = child.as_bool().unwrap_or(false),
                "CLIP_CONTENTS" => self.clip_contents = child.as_bool().unwrap_or(false),
                "PAUSE_MUSIC" => self.pause_music = child.as_bool().unwrap_or(true),
                "FADE_COLOR" => fade_rgb = rgb_of(child),
                "FADE_ALPHA" => fade_alpha = child.as_int().unwrap_or(255) as i32,
                "ALPHA_COLOR" => alpha_rgb = rgb_of(child),
                "ALPHA" => alpha = child.as_int().unwrap_or(255) as i32,
                "BUTTON" => {
                    let mut def = ButtonDef::default();
                    def.apply(child);
                    self.widgets.push(WidgetDef::Button(Box::new(def)));
                }
                "STATIC" => {
                    let mut def = StaticDef::default();
                    def.apply(child);
                    self.widgets.push(WidgetDef::Static(def));
                }
                "EDIT" => {
                    let mut def = EditDef::default();
                    def.apply(child);
                    self.widgets.push(WidgetDef::Edit(def));
                }
                "WINDOW" => {
                    let mut def = WindowDef::default();
                    def.apply(child);
                    self.widgets.push(WidgetDef::Window(Box::new(def)));
                }
                _ => {}
            }
        }
        if let Some(rgb) = fade_rgb {
            self.fade_color = Some(Rgba {
                r: rgb.r,
                g: rgb.g,
                b: rgb.b,
                a: fade_alpha.clamp(0, 255) as u8,
            });
        }
        if let Some(rgb) = alpha_rgb {
            self.alpha_color = Some(Rgba {
                r: rgb.r,
                g: rgb.g,
                b: rgb.b,
                a: alpha.clamp(0, 255) as u8,
            });
        }
    }

    /// The control named `name`, ignoring case, among the direct
    /// controls.
    pub fn widget(&self, name: &str) -> Option<&WidgetDef> {
        self.widgets
            .iter()
            .find(|w| w.base().name.eq_ignore_ascii_case(name))
    }
}

/// The inventory box (`INVENTORY_BOX`, a `.def` file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryBoxDef {
    /// A `.def` file loaded first (`TEMPLATE`).
    pub template: Option<String>,
    /// Name used by scripts.
    pub name: String,
    /// Free caption.
    pub caption: String,
    /// The box's window.
    pub window: Option<WindowDef>,
    /// Where the items are laid out, relative to the window.
    pub area: Rect,
    /// The cell of one item.
    pub item_width: i32,
    /// See `item_width`.
    pub item_height: i32,
    /// Pixels between cells.
    pub spacing: i32,
    /// Items scrolled by `prev` and `next`.
    pub scroll_by: i32,
    /// The selected item is not drawn in the box.
    pub hide_selected: bool,
    /// A click outside the box closes it.
    pub exclusive: bool,
    /// The box starts visible.
    pub always_visible: bool,
}

impl Default for InventoryBoxDef {
    fn default() -> Self {
        InventoryBoxDef {
            template: None,
            name: String::new(),
            caption: String::new(),
            window: None,
            area: Rect::default(),
            item_width: 50,
            item_height: 50,
            spacing: 0,
            scroll_by: 1,
            hide_selected: false,
            exclusive: false,
            always_visible: false,
        }
    }
}

impl InventoryBoxDef {
    /// Parses a `.def` file: an `INVENTORY_BOX` root block.
    pub fn parse(bytes: &[u8]) -> Result<InventoryBoxDef> {
        let doc = Document::parse(bytes)?;
        let root = doc.get("INVENTORY_BOX").ok_or_else(|| Error::Invalid {
            reason: "'INVENTORY_BOX' keyword expected".into(),
        })?;
        let mut def = InventoryBoxDef::default();
        def.apply(root);
        Ok(def)
    }

    /// Reads the keys of an `INVENTORY_BOX` block, overriding what is
    /// there.
    pub fn apply(&mut self, node: &Node) {
        for child in node.nodes() {
            match child.key.to_ascii_uppercase().as_str() {
                "TEMPLATE" => self.template = path(child),
                "NAME" => self.name = text(child),
                "CAPTION" => self.caption = text(child),
                "WINDOW" => {
                    let mut window = WindowDef::default();
                    window.apply(child);
                    self.window = Some(window);
                }
                "AREA" => {
                    if let Some(rect) = rect_of(child) {
                        self.area = rect;
                    }
                }
                "ITEM_WIDTH" => self.item_width = child.as_int().unwrap_or(50) as i32,
                "ITEM_HEIGHT" => self.item_height = child.as_int().unwrap_or(50) as i32,
                "SPACING" => self.spacing = child.as_int().unwrap_or(0) as i32,
                "SCROLL_BY" => self.scroll_by = child.as_int().unwrap_or(1) as i32,
                "HIDE_SELECTED" => self.hide_selected = child.as_bool().unwrap_or(false),
                "EXCLUSIVE" => self.exclusive = child.as_bool().unwrap_or(false),
                "ALWAYS_VISIBLE" => self.always_visible = child.as_bool().unwrap_or(false),
                _ => {}
            }
        }
    }

    /// Columns and rows of items the area holds
    /// (`CAdInventoryBox::Display`).
    pub fn grid(&self) -> (i32, i32) {
        let cell_w = self.item_width + self.spacing;
        let cell_h = self.item_height + self.spacing;
        let columns = if cell_w > 0 {
            (self.area.width() as i32 + self.spacing) / cell_w
        } else {
            0
        };
        let rows = if cell_h > 0 {
            (self.area.height() as i32 + self.spacing) / cell_h
        } else {
            0
        };
        (columns, rows)
    }
}

/// An inventory item (`ITEM`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemDef {
    /// An item file loaded first (`TEMPLATE`).
    pub template: Option<String>,
    /// Name used by scripts and as the event raised when the item is
    /// used on an object.
    pub name: String,
    /// Caption shown by the game's scripts, before string table
    /// expansion.
    pub caption: String,
    /// The sprite in the box.
    pub sprite: Option<String>,
    /// Shown while the mouse is over the item.
    pub sprite_hover: Option<String>,
    /// The cursor while the item is selected.
    pub cursor: Option<String>,
    /// The cursor while the selected item is over a usable object.
    pub cursor_hover: Option<String>,
    /// The item's cursor is drawn over the normal cursor.
    pub cursor_combined: bool,
    /// Font of the item's speech and amount.
    pub font: Option<String>,
    /// A count on the item.
    pub amount: i32,
    /// Whether the count is drawn.
    pub display_amount: bool,
    /// Offset of the count.
    pub amount_offset_x: i32,
    /// See `amount_offset_x`.
    pub amount_offset_y: i32,
    /// Alignment of the count.
    pub amount_align: TextAlign,
    /// Text drawn instead of the count.
    pub amount_string: Option<String>,
    /// Talk sprites.
    pub talk: Vec<String>,
    /// Talk sprites only used by name.
    pub talk_special: Vec<String>,
    /// Scripts attached to the item.
    pub scripts: Vec<String>,
    /// Script properties.
    pub properties: Vec<Property>,
    /// Tint and opacity.
    pub alpha_color: Option<Rgba>,
}

impl Default for ItemDef {
    fn default() -> Self {
        ItemDef {
            template: None,
            name: String::new(),
            caption: String::new(),
            sprite: None,
            sprite_hover: None,
            cursor: None,
            cursor_hover: None,
            cursor_combined: true,
            font: None,
            amount: 0,
            display_amount: false,
            amount_offset_x: 0,
            amount_offset_y: 0,
            amount_align: TextAlign::Right,
            amount_string: None,
            talk: Vec::new(),
            talk_special: Vec::new(),
            scripts: Vec::new(),
            properties: Vec::new(),
            alpha_color: None,
        }
    }
}

impl ItemDef {
    /// Parses an `.items` file: every `ITEM` root block.
    pub fn parse_all(bytes: &[u8]) -> Result<Vec<ItemDef>> {
        let doc = Document::parse(bytes)?;
        Ok(doc
            .get_all("ITEM")
            .map(|node| {
                let mut item = ItemDef::default();
                item.apply(node);
                item
            })
            .collect())
    }

    /// Reads the keys of an `ITEM` block, overriding what is there.
    pub fn apply(&mut self, node: &Node) {
        let mut alpha_rgb: Option<Rgb> = None;
        let mut alpha: i32 = 255;
        for child in node.nodes() {
            match child.key.to_ascii_uppercase().as_str() {
                "TEMPLATE" => self.template = path(child),
                "NAME" => self.name = text(child),
                "CAPTION" => self.caption = text(child),
                "IMAGE" | "SPRITE" => self.sprite = path(child),
                "IMAGE_HOVER" | "SPRITE_HOVER" => self.sprite_hover = path(child),
                "CURSOR" => self.cursor = path(child),
                "CURSOR_HOVER" => self.cursor_hover = path(child),
                "CURSOR_COMBINED" => self.cursor_combined = child.as_bool().unwrap_or(true),
                "FONT" => self.font = path(child),
                "AMOUNT" => self.amount = child.as_int().unwrap_or(0) as i32,
                "DISPLAY_AMOUNT" => self.display_amount = child.as_bool().unwrap_or(false),
                "AMOUNT_OFFSET_X" => self.amount_offset_x = child.as_int().unwrap_or(0) as i32,
                "AMOUNT_OFFSET_Y" => self.amount_offset_y = child.as_int().unwrap_or(0) as i32,
                "AMOUNT_ALIGN" => self.amount_align = text_align(&text(child), TextAlign::Center),
                "AMOUNT_STRING" => self.amount_string = Some(text(child)),
                "TALK" => self.talk.extend(path(child)),
                "TALK_SPECIAL" => self.talk_special.extend(path(child)),
                "SCRIPT" => self.scripts.extend(path(child)),
                "PROPERTY" => {
                    let name = child.get_str("NAME").unwrap_or_default().to_string();
                    let value = child.get_str("VALUE").unwrap_or_default().to_string();
                    if !name.is_empty() {
                        self.properties.push(Property { name, value });
                    }
                }
                "ALPHA_COLOR" => alpha_rgb = rgb_of(child),
                "ALPHA" => alpha = child.as_int().unwrap_or(255) as i32,
                _ => {}
            }
        }
        if let Some(rgb) = alpha_rgb {
            self.alpha_color = Some(Rgba {
                r: rgb.r,
                g: rgb.g,
                b: rgb.b,
                a: alpha.clamp(0, 255) as u8,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_window_with_its_controls() {
        let window = WindowDef::parse(
            br#"WINDOW {
  X = 200  Y = 200  WIDTH = 520  HEIGHT = 450
  NAME = "NeoMainMenu"
  TITLE = "/INTERF0004/Main Menu"
  TITLE_RECT {60,35,260,70}
  TITLE_ALIGN = left
  DRAG_RECT {58,30,420,70}
  IMAGE = "interface\system\artwork\guibackground.png"
  FONT = "primaryttf.font"
  SCRIPT = "interface\system\NeoMainMenu.script"
  FADE_COLOR = 10,20,30
  FADE_ALPHA = 128
  BUTTON {
    NAME = "close"
    TEXT = "/INTERF0005/Resume|Game"
    TEXT_ALIGN = "center"
    FONT = "secondaryttf.font"
    FONT_HOVER = "secondaryinvertttf.font"
    X = 115  Y = 130  WIDTH = 300
    PARENT_NOTIFY = TRUE
  }
  BUTTON { NAME = "cross" IMAGE = "cross.png" IMAGE_HOVER = "glow.png" X = 420 Y = 30 PIXEL_PERFECT = TRUE }
  STATIC { NAME = "scanlines" X = 0 Y = 0 IMAGE = "scan.png" TEXT_ALIGN = "right" VERTICAL_ALIGN = "bottom" }
  EDIT { NAME = "name" TEXT = "player" MAX_LENGTH = 20 FRAME_WIDTH = 2 }
  WINDOW { NAME = "inner" TRANSPARENT = TRUE MENU = TRUE IN_GAME = TRUE PAUSE_MUSIC = FALSE }
}"#,
        )
        .unwrap();
        assert_eq!(window.base.name, "NeoMainMenu");
        assert_eq!((window.base.x, window.base.y), (200, 200));
        assert_eq!((window.base.width, window.base.height), (520, 450));
        assert_eq!(window.title, "/INTERF0004/Main Menu");
        assert_eq!(window.title_align, TextAlign::Left);
        assert_eq!(
            window.title_rect,
            Some(Rect {
                left: 60,
                top: 35,
                right: 260,
                bottom: 70
            })
        );
        assert!(window.drag_rect.is_some());
        assert_eq!(
            window.base.image.as_deref(),
            Some("interface\\system\\artwork\\guibackground.png")
        );
        assert_eq!(
            window.base.scripts,
            ["interface\\system\\NeoMainMenu.script"]
        );
        assert_eq!(
            window.fade_color,
            Some(Rgba {
                r: 10,
                g: 20,
                b: 30,
                a: 128
            })
        );
        assert!(window.pause_music);
        assert_eq!(window.widgets.len(), 5);
        let WidgetDef::Button(close) = &window.widgets[0] else {
            panic!("button expected");
        };
        assert_eq!(close.base.name, "close");
        assert_eq!(close.text, "/INTERF0005/Resume\nGame");
        assert_eq!(close.text_align, TextAlign::Center);
        assert_eq!(close.font_hover.as_deref(), Some("secondaryinvertttf.font"));
        assert!(close.base.parent_notify);
        assert_eq!(close.base.width, 300);
        let WidgetDef::Button(cross) = window.widget("CROSS").unwrap() else {
            panic!("button expected");
        };
        assert!(cross.pixel_perfect);
        assert_eq!(cross.size_image(), Some("cross.png"));
        let WidgetDef::Static(scan) = &window.widgets[2] else {
            panic!("static expected");
        };
        assert_eq!(scan.text_align, TextAlign::Right);
        assert_eq!(scan.vertical_align, VerticalAlign::Bottom);
        let WidgetDef::Edit(edit) = &window.widgets[3] else {
            panic!("edit expected");
        };
        assert_eq!(
            (edit.text.as_str(), edit.max_length, edit.frame_width),
            ("player", 20, 2)
        );
        let WidgetDef::Window(inner) = &window.widgets[4] else {
            panic!("window expected");
        };
        assert!(inner.transparent && inner.menu && inner.in_game && !inner.pause_music);
        assert_eq!(window.widget("nothing"), None);
    }

    #[test]
    fn a_button_without_text_defaults() {
        let button = ButtonDef::parse(b"BUTTON { NAME = \"b\" IMAGE_HOVER = \"h.png\" }").unwrap();
        assert_eq!(button.text_align, TextAlign::Center);
        assert!(!button.pixel_perfect && !button.focusable && !button.pressed);
        assert_eq!(button.size_image(), Some("h.png"));
        assert!(WindowDef::parse(b"BUTTON { }").is_err());
    }

    #[test]
    fn tiled_images_cut_from_tile_sizes_or_thirds() {
        let mut tiled = TiledImage::parse(
            b"TILED_IMAGE { IMAGE = \"frame.png\" HORIZONTAL_TILES { 4, 8, 4 } VERTICAL_TILES { 2, 6, 2 } }",
        )
        .unwrap();
        assert_eq!(tiled.image.as_deref(), Some("frame.png"));
        assert_eq!(
            tiled.tile(0, 0),
            Rect {
                left: 0,
                top: 0,
                right: 4,
                bottom: 2
            }
        );
        assert_eq!(
            tiled.tile(2, 2),
            Rect {
                left: 12,
                top: 8,
                right: 16,
                bottom: 10
            }
        );
        assert_eq!(tiled.grid(100, 50), ((100 - 8) / 8, (50 - 4) / 6));
        assert_eq!(tiled.corrected_size(100, 50), (8 + 11 * 8, 4 + 7 * 6));
        // Explicit tiles win; the rest comes from thirds of the image.
        tiled =
            TiledImage::parse(b"TILED_IMAGE { IMAGE = \"f.png\" MIDDLE_MIDDLE { 1, 1, 5, 5 } }")
                .unwrap();
        tiled.fill_defaults(30, 60);
        assert_eq!(
            tiled.tile(1, 1),
            Rect {
                left: 1,
                top: 1,
                right: 5,
                bottom: 5
            }
        );
        assert_eq!(
            tiled.tile(2, 1),
            Rect {
                left: 10,
                top: 40,
                right: 20,
                bottom: 60
            }
        );
        let empty = TiledImage::default();
        assert_eq!(empty.grid(100, 100), (0, 0));
        assert_eq!(empty.corrected_size(100, 100), (100, 100));
    }

    #[test]
    fn parses_the_inventory_box_and_the_items() {
        let inv = InventoryBoxDef::parse(
            br#"INVENTORY_BOX {
  ITEM_WIDTH = 100  ITEM_HEIGHT = 100  SPACING = 0  SCROLL_BY = 1
  HIDE_SELECTED = TRUE
  AREA { 12, 0, 1012, 100 }
  EXCLUSIVE = FALSE
  WINDOW {
    X = 0 Y = 0 WIDTH = 1024 HEIGHT = 100 IMAGE = "interface\Inventory.png"
    BUTTON { NAME = "prev" X = 0 Y = 0 WIDTH = 12 HEIGHT = 100 PARENT_NOTIFY = TRUE }
    BUTTON { NAME = "next" X = 1012 Y = 0 WIDTH = 12 HEIGHT = 100 PARENT_NOTIFY = TRUE }
  }
}"#,
        )
        .unwrap();
        assert_eq!(
            (inv.item_width, inv.item_height, inv.spacing),
            (100, 100, 0)
        );
        assert!(inv.hide_selected && !inv.exclusive && !inv.always_visible);
        assert_eq!(inv.grid(), (10, 1));
        let window = inv.window.as_ref().unwrap();
        assert_eq!(window.base.width, 1024);
        assert_eq!(window.widgets.len(), 2);

        let items = ItemDef::parse_all(
            br#"ITEM {
   CURSOR_COMBINED = TRUE
   CAPTION = "/ITEMIS0001/Axe"
   NAME = "axe"
   IMAGE = "items\ItemAxe.png"
   CURSOR = "items\ItemAxe.png"
   CURSOR_HOVER = "items\ItemAxeHOVER.png"
   SPRITE_HOVER = "items\ItemAxeHOVER.png"
   SCRIPT = "items\ItemAxe.script"
   PROPERTY { NAME = "weight" VALUE = "3" }
}
ITEM { NAME = "key" SPRITE = "items\key.sprite" CURSOR_COMBINED = FALSE AMOUNT = 2 DISPLAY_AMOUNT = TRUE AMOUNT_ALIGN = "left" }"#,
        )
        .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "axe");
        assert_eq!(items[0].caption, "/ITEMIS0001/Axe");
        assert_eq!(items[0].sprite.as_deref(), Some("items\\ItemAxe.png"));
        assert_eq!(
            items[0].sprite_hover.as_deref(),
            Some("items\\ItemAxeHOVER.png")
        );
        assert_eq!(
            items[0].cursor_hover.as_deref(),
            Some("items\\ItemAxeHOVER.png")
        );
        assert!(items[0].cursor_combined);
        assert_eq!(items[0].scripts, ["items\\ItemAxe.script"]);
        assert_eq!(items[0].properties[0].name, "weight");
        assert!(!items[1].cursor_combined);
        assert_eq!((items[1].amount, items[1].display_amount), (2, true));
        assert_eq!(items[1].amount_align, TextAlign::Left);
        assert!(items[1].sprite_hover.is_none());
    }

    #[test]
    fn vertical_alignment_codes() {
        assert_eq!(VerticalAlign::from_name("TOP"), VerticalAlign::Top);
        assert_eq!(VerticalAlign::from_name("x"), VerticalAlign::Center);
        assert_eq!(VerticalAlign::from_code(2), VerticalAlign::Bottom);
        assert_eq!(VerticalAlign::Bottom.code(), 2);
        assert_eq!(control_text("a|b"), "a\nb");
    }
}
