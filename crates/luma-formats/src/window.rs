//! Windows and their controls (`.window`), tiled images (`.tiled`), the
//! inventory box (`.def`) and the items (`.items`).
//!
//! The formats are documented in `docs/formats/window.md` and were
//! established from the MIT-licensed WME Lite sources (`UIWindow.cpp`,
//! `UIObject.cpp`, `UIButton.cpp`, `UIText.cpp`, `UIEdit.cpp`,
//! `UITiledImage.cpp`, `AdInventoryBox.cpp`, `AdItem.cpp`, Copyright (c)
//! 2011 Jan Nedoma). See `NOTICE` at the root of the repository.

use crate::def::{Document, Node, document_keys, keys};
use crate::error::{Error, Result};
use crate::font::TextAlign;
use crate::image::Rgba;
use crate::scene::Property;
use crate::sprite::Rect;
use crate::template::Files;

/// The root block a file must start with, or the original's
/// `'<KEY>' keyword expected` error.
fn root_of<'a>(doc: &'a Document, key: &str) -> Result<&'a Node> {
    doc.root(key).ok_or_else(|| Error::Invalid {
        reason: format!("'{key}' keyword expected"),
    })
}

/// Vertical placement of a text in its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

/// `ScanStr(params, "%d,%d,%d,%d", &rect.left, ...)`: the numbers the
/// value gives go over the rectangle, the others stay.
fn read_rect(node: &Node, rect: &mut Rect) {
    node.read_ints(&mut [
        &mut rect.left,
        &mut rect.top,
        &mut rect.right,
        &mut rect.bottom,
    ]);
}

fn path(node: &Node) -> Option<String> {
    Some(node.text())
}

/// The engine turns `|` into a line break when a control's text is set
/// (`CUIObject::SetText`).
pub fn control_text(text: &str) -> String {
    text.replace('|', "\n")
}

/// An image cut into nine tiles (`TILED_IMAGE`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// Parses a `.tiled` file: a `TILED_IMAGE` root block. No other file
    /// is read.
    pub fn parse(bytes: &[u8]) -> Result<TiledImage> {
        TiledImage::parse_with(bytes, &mut Files::none())
    }

    /// Parses a `.tiled` file, reading the `TEMPLATE` files it names
    /// through `files`.
    pub fn parse_with(bytes: &[u8], files: &mut Files<'_>) -> Result<TiledImage> {
        let doc = Document::parse(bytes)?;
        let mut tiled = TiledImage::default();
        tiled.apply_with(root_of(&doc, "TILED_IMAGE")?, files)?;
        Ok(tiled)
    }

    /// Reads the keys of a `TILED_IMAGE` block, overriding what is
    /// there. No `TEMPLATE` file is read.
    pub fn apply(&mut self, node: &Node) -> Result<()> {
        self.apply_with(node, &mut Files::none())
    }

    /// `CUITiledImage::LoadBuffer`: reads the keys of a `TILED_IMAGE`
    /// block in order, overriding what is there. A `TEMPLATE` loads the
    /// named `.tiled` file where the key stands (one that cannot be
    /// loaded is skipped). `HORIZONTAL_TILES` with `VERTICAL_TILES` lay
    /// out the nine tiles from three column widths and three row heights
    /// (a number that is missing is 0). A key that is not in the table
    /// is a syntax error.
    pub fn apply_with(&mut self, node: &Node, files: &mut Files<'_>) -> Result<()> {
        let mut horizontal: Option<[i32; 3]> = None;
        let mut vertical: Option<[i32; 3]> = None;
        for item in keys(node, TILED_IMAGE_KEYS, "TILED_IMAGE") {
            let (key, child) = item?;
            let (row, col) = match key {
                "TEMPLATE" => {
                    let name = child.text();
                    let _ = files.template(&name, |doc, files| {
                        self.apply_with(root_of(doc, "TILED_IMAGE")?, files)
                    });
                    self.template = Some(name);
                    continue;
                }
                "IMAGE" => {
                    self.image = path(child);
                    continue;
                }
                "HORIZONTAL_TILES" | "VERTICAL_TILES" => {
                    let mut sizes = [0, 0, 0];
                    let [a, b, c] = &mut sizes;
                    child.read_ints(&mut [a, b, c]);
                    if key == "HORIZONTAL_TILES" {
                        horizontal = Some(sizes);
                    } else {
                        vertical = Some(sizes);
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
            read_rect(child, &mut self.tiles[row][col]);
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
        Ok(())
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
            "NAME" => self.name = child.text(),
            "CAPTION" => self.caption = child.text(),
            "X" => child.read_int(&mut self.x),
            "Y" => child.read_int(&mut self.y),
            "WIDTH" => child.read_int(&mut self.width),
            "HEIGHT" => child.read_int(&mut self.height),
            "BACK" => self.back = path(child),
            "IMAGE" => self.image = path(child),
            "FONT" => self.font = path(child),
            "CURSOR" => self.cursor = path(child),
            "VISIBLE" => child.read_bool(&mut self.visible),
            "DISABLED" => child.read_bool(&mut self.disabled),
            "PARENT_NOTIFY" => child.read_bool(&mut self.parent_notify),
            "SCRIPT" => self.scripts.push(child.text()),
            _ => return false,
        }
        true
    }
}

/// Loads a control's `TEMPLATE` file where the key stands (`LoadFile`
/// on the control itself). A template that cannot be loaded fails the
/// control: these loaders stop at the first error.
fn control_template(
    files: &mut Files<'_>,
    name: &str,
    root: &str,
    apply: impl FnOnce(&Node, &mut Files<'_>) -> Result<()>,
) -> Result<()> {
    files.template(name, |doc, files| apply(root_of(doc, root)?, files))
}

/// The keys of `CUIButton::LoadBuffer`, in the order of its table.
const BUTTON_KEYS: &[&str] = &[
    "BUTTON",
    "TEMPLATE",
    "DISABLED",
    "VISIBLE",
    "FOCUSABLE",
    "BACK_HOVER",
    "BACK_PRESS",
    "BACK_DISABLE",
    "BACK_FOCUS",
    "BACK",
    "CENTER_IMAGE",
    "IMAGE_HOVER",
    "IMAGE_PRESS",
    "IMAGE_DISABLE",
    "IMAGE_FOCUS",
    "IMAGE",
    "FONT_HOVER",
    "FONT_PRESS",
    "FONT_DISABLE",
    "FONT_FOCUS",
    "FONT",
    "TEXT_ALIGN",
    "TEXT",
    "X",
    "Y",
    "WIDTH",
    "HEIGHT",
    "CURSOR",
    "NAME",
    "EVENTS",
    "SCRIPT",
    "CAPTION",
    "PARENT_NOTIFY",
    "PRESSED",
    "PIXEL_PERFECT",
    "EDITOR_PROPERTY",
];

/// The keys of `CUIText::LoadBuffer`, in the order of its table.
const STATIC_KEYS: &[&str] = &[
    "STATIC",
    "TEMPLATE",
    "DISABLED",
    "VISIBLE",
    "BACK",
    "IMAGE",
    "FONT",
    "TEXT_ALIGN",
    "VERTICAL_ALIGN",
    "TEXT",
    "X",
    "Y",
    "WIDTH",
    "HEIGHT",
    "CURSOR",
    "NAME",
    "SCRIPT",
    "CAPTION",
    "PARENT_NOTIFY",
    "EDITOR_PROPERTY",
];

/// The keys of `CUIEdit::LoadBuffer`, in the order of its table.
const EDIT_KEYS: &[&str] = &[
    "TEMPLATE",
    "DISABLED",
    "VISIBLE",
    "BACK",
    "IMAGE",
    "FONT_SELECTED",
    "FONT",
    "TEXT",
    "X",
    "Y",
    "WIDTH",
    "HEIGHT",
    "CURSOR_BLINK_RATE",
    "CURSOR",
    "FRAME_WIDTH",
    "NAME",
    "SCRIPT",
    "PARENT_NOTIFY",
    "MAX_LENGTH",
    "EDITOR_PROPERTY",
    "EDIT",
    "CAPTION",
];

/// The keys of `CUIEntity::LoadBuffer`, in the order of its table.
const ENTITY_CONTAINER_KEYS: &[&str] = &[
    "ENTITY_CONTAINER",
    "TEMPLATE",
    "DISABLED",
    "VISIBLE",
    "X",
    "Y",
    "NAME",
    "ENTITY",
    "SCRIPT",
    "EDITOR_PROPERTY",
];

/// The keys of `CUIWindow::LoadBuffer`, in the order of its table, then
/// the one `CAdGame::WindowLoadHook` adds.
const WINDOW_KEYS: &[&str] = &[
    "WINDOW",
    "ALPHA_COLOR",
    "ALPHA",
    "TEMPLATE",
    "DISABLED",
    "VISIBLE",
    "BACK_INACTIVE",
    "BACK",
    "IMAGE_INACTIVE",
    "IMAGE",
    "FONT_INACTIVE",
    "FONT",
    "TITLE_ALIGN",
    "TITLE_RECT",
    "TITLE",
    "DRAG_RECT",
    "X",
    "Y",
    "WIDTH",
    "HEIGHT",
    "FADE_ALPHA",
    "FADE_COLOR",
    "CURSOR",
    "NAME",
    "BUTTON",
    "STATIC",
    "TRANSPARENT",
    "SCRIPT",
    "CAPTION",
    "PARENT_NOTIFY",
    "MENU",
    "IN_GAME",
    "CLIP_CONTENTS",
    "PAUSE_MUSIC",
    "EDITOR_PROPERTY",
    "EDIT",
    "ENTITY_CONTAINER",
];

/// The keys of `CUITiledImage::LoadBuffer`, in the order of its table.
const TILED_IMAGE_KEYS: &[&str] = &[
    "TILED_IMAGE",
    "TEMPLATE",
    "IMAGE",
    "UP_LEFT",
    "UP_RIGHT",
    "UP_MIDDLE",
    "DOWN_LEFT",
    "DOWN_RIGHT",
    "DOWN_MIDDLE",
    "MIDDLE_LEFT",
    "MIDDLE_RIGHT",
    "MIDDLE_MIDDLE",
    "VERTICAL_TILES",
    "HORIZONTAL_TILES",
    "EDITOR_PROPERTY",
];

/// The keys of `CAdResponseBox::LoadBuffer`, in the order of its table.
const RESPONSE_BOX_KEYS: &[&str] = &[
    "RESPONSE_BOX",
    "TEMPLATE",
    "FONT_HOVER",
    "FONT",
    "AREA",
    "HORIZONTAL",
    "SPACING",
    "WINDOW",
    "CURSOR",
    "TEXT_ALIGN",
    "VERTICAL_ALIGN",
    "EDITOR_PROPERTY",
];

/// The keys of `CAdInventoryBox::LoadBuffer`, in the order of its table.
const INVENTORY_BOX_KEYS: &[&str] = &[
    "INVENTORY_BOX",
    "TEMPLATE",
    "WINDOW",
    "EXCLUSIVE",
    "ALWAYS_VISIBLE",
    "AREA",
    "SPACING",
    "ITEM_WIDTH",
    "ITEM_HEIGHT",
    "SCROLL_BY",
    "NAME",
    "CAPTION",
    "HIDE_SELECTED",
    "EDITOR_PROPERTY",
];

/// The keys of `CAdItem::LoadBuffer`, in the order of its table.
const ITEM_KEYS: &[&str] = &[
    "ITEM",
    "TEMPLATE",
    "CURSOR_HOVER",
    "CURSOR_COMBINED",
    "CURSOR",
    "NAME",
    "IMAGE_HOVER",
    "IMAGE",
    "EVENTS",
    "SCRIPT",
    "CAPTION",
    "PROPERTY",
    "EDITOR_PROPERTY",
    "FONT",
    "ALPHA_COLOR",
    "ALPHA",
    "TALK_SPECIAL",
    "TALK",
    "SPRITE_HOVER",
    "SPRITE",
    "DISPLAY_AMOUNT",
    "AMOUNT_OFFSET_X",
    "AMOUNT_OFFSET_Y",
    "AMOUNT_ALIGN",
    "AMOUNT_STRING",
    "AMOUNT",
];

/// A button (`BUTTON`): images, backgrounds and fonts per state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// Parses a `.button` file: a `BUTTON` root block. No other file is
    /// read: a `TEMPLATE` key fails the button.
    pub fn parse(bytes: &[u8]) -> Result<ButtonDef> {
        let doc = Document::parse(bytes)?;
        let mut button = ButtonDef::default();
        button.apply(root_of(&doc, "BUTTON")?)?;
        Ok(button)
    }

    /// Reads the keys of a `BUTTON` block, overriding what is there. No
    /// `TEMPLATE` file is read; see [`ButtonDef::apply_with`].
    pub fn apply(&mut self, node: &Node) -> Result<()> {
        self.apply_with(node, &mut Files::none())
    }

    /// `CUIButton::LoadBuffer`: reads the keys of a `BUTTON` block in
    /// order, overriding what is there. A `TEMPLATE` names a file of the
    /// same kind read through `files`: its keys are applied where the
    /// `TEMPLATE` key stands, as the original engine loads the file in
    /// place, so the keys after it override the template and the keys
    /// before it are overridden. A key that is not in the table, or a
    /// template that cannot be loaded, fails the button.
    pub fn apply_with(&mut self, node: &Node, files: &mut Files<'_>) -> Result<()> {
        for item in keys(node, BUTTON_KEYS, "BUTTON") {
            let (key, child) = item?;
            if self.base.apply_key(key, child) {
                continue;
            }
            match key {
                "TEMPLATE" => {
                    let file = child.text();
                    control_template(files, &file, "BUTTON", |root, files| {
                        self.apply_with(root, files)
                    })?;
                    self.template = Some(file);
                }
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
                "TEXT" => self.text = control_text(&child.text()),
                "TEXT_ALIGN" => self.text_align = text_align(&child.text(), TextAlign::Center),
                "CENTER_IMAGE" => child.read_bool(&mut self.center_image),
                "PRESSED" => child.read_bool(&mut self.pressed),
                "FOCUSABLE" => child.read_bool(&mut self.focusable),
                "PIXEL_PERFECT" => child.read_bool(&mut self.pixel_perfect),
                _ => {}
            }
        }
        Ok(())
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// Parses a `.static` file: a `STATIC` root block. No other file is
    /// read: a `TEMPLATE` key fails the control.
    pub fn parse(bytes: &[u8]) -> Result<StaticDef> {
        let doc = Document::parse(bytes)?;
        let mut def = StaticDef::default();
        def.apply(root_of(&doc, "STATIC")?)?;
        Ok(def)
    }

    /// Reads the keys of a `STATIC` block, overriding what is there. No
    /// `TEMPLATE` file is read; see [`StaticDef::apply_with`].
    pub fn apply(&mut self, node: &Node) -> Result<()> {
        self.apply_with(node, &mut Files::none())
    }

    /// `CUIText::LoadBuffer`: reads the keys of a `STATIC` block in
    /// order, overriding what is there; a `TEMPLATE` is loaded in place
    /// (see [`ButtonDef::apply_with`]). A key that is not in the table,
    /// or a template that cannot be loaded, fails the control.
    pub fn apply_with(&mut self, node: &Node, files: &mut Files<'_>) -> Result<()> {
        for item in keys(node, STATIC_KEYS, "STATIC") {
            let (key, child) = item?;
            if self.base.apply_key(key, child) {
                continue;
            }
            match key {
                "TEMPLATE" => {
                    let file = child.text();
                    control_template(files, &file, "STATIC", |root, files| {
                        self.apply_with(root, files)
                    })?;
                    self.template = Some(file);
                }
                "TEXT" => self.text = control_text(&child.text()),
                "TEXT_ALIGN" => self.text_align = text_align(&child.text(), TextAlign::Center),
                "VERTICAL_ALIGN" => {
                    self.vertical_align = VerticalAlign::from_name(&child.text());
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// A text editor (`EDIT`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// Parses an `.edit` file: an `EDIT` root block. No other file is
    /// read: a `TEMPLATE` key fails the control.
    pub fn parse(bytes: &[u8]) -> Result<EditDef> {
        let doc = Document::parse(bytes)?;
        let mut def = EditDef::default();
        def.apply(root_of(&doc, "EDIT")?)?;
        Ok(def)
    }

    /// Reads the keys of an `EDIT` block, overriding what is there. No
    /// `TEMPLATE` file is read; see [`EditDef::apply_with`].
    pub fn apply(&mut self, node: &Node) -> Result<()> {
        self.apply_with(node, &mut Files::none())
    }

    /// `CUIEdit::LoadBuffer`: reads the keys of an `EDIT` block in order,
    /// overriding what is there; a `TEMPLATE` is loaded in place (see
    /// [`ButtonDef::apply_with`]). A key that is not in the table, or a
    /// template that cannot be loaded, fails the control.
    pub fn apply_with(&mut self, node: &Node, files: &mut Files<'_>) -> Result<()> {
        for item in keys(node, EDIT_KEYS, "EDIT") {
            let (key, child) = item?;
            if self.base.apply_key(key, child) {
                continue;
            }
            match key {
                "TEMPLATE" => {
                    let file = child.text();
                    control_template(files, &file, "EDIT", |root, files| {
                        self.apply_with(root, files)
                    })?;
                    self.template = Some(file);
                }
                "TEXT" => self.text = control_text(&child.text()),
                "FONT_SELECTED" => self.font_selected = path(child),
                "MAX_LENGTH" => child.read_int(&mut self.max_length),
                "FRAME_WIDTH" => child.read_int(&mut self.frame_width),
                "CURSOR_BLINK_RATE" => child.read_int(&mut self.cursor_blink_rate),
                _ => {}
            }
        }
        Ok(())
    }
}

/// A control of a window, in drawing order.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WidgetDef {
    /// A button.
    Button(Box<ButtonDef>),
    /// A static text.
    Static(StaticDef),
    /// A text editor.
    Edit(EditDef),
    /// A nested window.
    Window(Box<WindowDef>),
    /// An entity container.
    EntityContainer(EntityContainerDef),
}

impl WidgetDef {
    /// The common keys of the control.
    pub fn base(&self) -> &ControlDef {
        match self {
            WidgetDef::Button(b) => &b.base,
            WidgetDef::Static(s) => &s.base,
            WidgetDef::Edit(e) => &e.base,
            WidgetDef::Window(w) => &w.base,
            WidgetDef::EntityContainer(c) => &c.base,
        }
    }
}

/// The dialogue response box (`RESPONSE_BOX`, `CAdResponseBox`): where
/// the choices of a dialogue are listed for the player.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ResponseBoxDef {
    /// A `.def` file loaded first (`TEMPLATE`).
    pub template: Option<String>,
    /// The font of the choices; the system font without one.
    pub font: Option<String>,
    /// The font of the choice under the mouse.
    pub font_hover: Option<String>,
    /// Where the choices are laid out, relative to the window.
    pub area: Rect,
    /// Choices side by side instead of one under the other.
    pub horizontal: bool,
    /// Pixels between choices.
    pub spacing: i32,
    /// The box's window, with its `prev` and `next` buttons.
    pub window: Option<WindowDef>,
    /// The cursor over the choices.
    pub cursor: Option<String>,
    /// Alignment of the text of a choice.
    pub text_align: TextAlign,
    /// Where the choices sit in the area when they do not fill it.
    pub vertical_align: VerticalAlign,
}

impl Default for ResponseBoxDef {
    fn default() -> Self {
        ResponseBoxDef {
            template: None,
            font: None,
            font_hover: None,
            area: Rect::default(),
            horizontal: false,
            spacing: 0,
            window: None,
            cursor: None,
            text_align: TextAlign::Left,
            vertical_align: VerticalAlign::Bottom,
        }
    }
}

impl ResponseBoxDef {
    /// Parses a `.def` file: a `RESPONSE_BOX` root block. No other file
    /// is read.
    pub fn parse(bytes: &[u8]) -> Result<ResponseBoxDef> {
        ResponseBoxDef::parse_with(bytes, &mut Files::none())
    }

    /// Parses a `.def` file, reading the `TEMPLATE` files it names (its
    /// own and those of its window and controls) through `files`.
    pub fn parse_with(bytes: &[u8], files: &mut Files<'_>) -> Result<ResponseBoxDef> {
        let doc = Document::parse(bytes)?;
        let mut def = ResponseBoxDef::default();
        def.apply_with(root_of(&doc, "RESPONSE_BOX")?, files)?;
        Ok(def)
    }

    /// Reads the keys of a `RESPONSE_BOX` block, overriding what is
    /// there. No `TEMPLATE` file is read.
    pub fn apply(&mut self, node: &Node) -> Result<()> {
        self.apply_with(node, &mut Files::none())
    }

    /// `CAdResponseBox::LoadBuffer`: reads the keys of the block in
    /// order, overriding what is there. A `TEMPLATE` loads the named file
    /// where the key stands. A window that cannot be read leaves the box
    /// without one, and a template that cannot be loaded is skipped (the
    /// original's error code is overwritten by the next statement); a
    /// key that is not in the table is a syntax error.
    pub fn apply_with(&mut self, node: &Node, files: &mut Files<'_>) -> Result<()> {
        for item in keys(node, RESPONSE_BOX_KEYS, "RESPONSE_BOX") {
            let (key, child) = item?;
            match key {
                "TEMPLATE" => {
                    let name = child.text();
                    let _ = files.template(&name, |doc, files| {
                        self.apply_with(root_of(doc, "RESPONSE_BOX")?, files)
                    });
                    self.template = Some(name);
                }
                "FONT" => self.font = path(child),
                "FONT_HOVER" => self.font_hover = path(child),
                "AREA" => read_rect(child, &mut self.area),
                "HORIZONTAL" => child.read_bool(&mut self.horizontal),
                "SPACING" => child.read_int(&mut self.spacing),
                "WINDOW" => {
                    let mut window = WindowDef::default();
                    self.window = window.apply_with(child, files).is_ok().then_some(window);
                }
                "CURSOR" => self.cursor = path(child),
                "TEXT_ALIGN" => {
                    let name = child.text();
                    self.text_align = if name.eq_ignore_ascii_case("center") {
                        TextAlign::Center
                    } else if name.eq_ignore_ascii_case("right") {
                        TextAlign::Right
                    } else {
                        TextAlign::Left
                    };
                }
                "VERTICAL_ALIGN" => {
                    let name = child.text();
                    self.vertical_align = if name.eq_ignore_ascii_case("top") {
                        VerticalAlign::Top
                    } else if name.eq_ignore_ascii_case("center") {
                        VerticalAlign::Center
                    } else {
                        VerticalAlign::Bottom
                    };
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// An entity container (`ENTITY_CONTAINER`): a control that shows an
/// entity loaded from an `.entity` file at its position, with the
/// entity's own sprite, scripts and mouse events (`CUIEntity`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EntityContainerDef {
    /// The common keys (`NAME`, `X`, `Y`, `DISABLED`, `VISIBLE`, `SCRIPT`).
    pub base: ControlDef,
    /// `ENTITY`: the entity file shown.
    pub entity: Option<String>,
    /// The last `TEMPLATE` file named; it was applied where the key
    /// stands.
    pub template: Option<String>,
}

impl EntityContainerDef {
    /// Parses a container file: an `ENTITY_CONTAINER` root block.
    pub fn parse_with(bytes: &[u8], files: &mut Files<'_>) -> Result<EntityContainerDef> {
        let doc = Document::parse(bytes)?;
        let mut def = EntityContainerDef::default();
        def.apply_with(root_of(&doc, "ENTITY_CONTAINER")?, files)?;
        Ok(def)
    }

    /// Applies the keys of an `ENTITY_CONTAINER` block. No `TEMPLATE`
    /// file is read.
    pub fn apply(&mut self, node: &Node) -> Result<()> {
        self.apply_with(node, &mut Files::none())
    }

    /// `CUIEntity::LoadBuffer`: reads the keys of the block in order. A
    /// `TEMPLATE` loads the named container file where the key stands.
    /// A key that is not in the table (the size, image and caption keys
    /// of the other controls are not), or a template that cannot be
    /// loaded, fails the container.
    pub fn apply_with(&mut self, node: &Node, files: &mut Files<'_>) -> Result<()> {
        for item in keys(node, ENTITY_CONTAINER_KEYS, "ENTITY_CONTAINER") {
            let (key, child) = item?;
            if self.base.apply_key(key, child) {
                continue;
            }
            match key {
                "TEMPLATE" => {
                    let file = child.text();
                    control_template(files, &file, "ENTITY_CONTAINER", |root, files| {
                        self.apply_with(root, files)
                    })?;
                    self.template = Some(file);
                }
                "ENTITY" => self.entity = path(child),
                _ => {}
            }
        }
        Ok(())
    }
}

/// A window (`WINDOW`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WindowDef {
    /// The common keys.
    pub base: ControlDef,
    /// The last `TEMPLATE` file named; it was applied where the key
    /// stands.
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
    /// A `FADE_COLOR` or `FADE_ALPHA` key was read, in this block or in
    /// a template: the window fades the screen under it.
    pub fade_background: bool,
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
            fade_background: false,
            fade_color: None,
            alpha_color: None,
            widgets: Vec::new(),
        }
    }
}

impl WindowDef {
    /// Parses a `.window` file: a `WINDOW` root block. No other file is
    /// read: a `TEMPLATE` key fails the window.
    pub fn parse(bytes: &[u8]) -> Result<WindowDef> {
        WindowDef::parse_with(bytes, &mut Files::none())
    }

    /// Parses a `.window` file, reading the `TEMPLATE` files it names
    /// (its own, and those of its controls and nested windows) through
    /// `files`.
    pub fn parse_with(bytes: &[u8], files: &mut Files<'_>) -> Result<WindowDef> {
        let doc = Document::parse(bytes)?;
        let mut window = WindowDef::default();
        window.apply_with(root_of(&doc, "WINDOW")?, files)?;
        Ok(window)
    }

    /// Reads the keys of a `WINDOW` block, overriding what is there;
    /// the controls are appended. No `TEMPLATE` file is read; see
    /// [`WindowDef::apply_with`].
    pub fn apply(&mut self, node: &Node) -> Result<()> {
        self.apply_with(node, &mut Files::none())
    }

    /// `CUIWindow::LoadBuffer`: reads the keys of a `WINDOW` block in
    /// order, overriding what is there; the controls are appended. A
    /// `TEMPLATE` (of the window, of a nested window, of a control) is
    /// read through `files` and applied where the key stands, so the
    /// template's controls come before those that follow the key.
    ///
    /// A key that is not in the table, a control that cannot be read or
    /// a template that cannot be loaded fails the window. The tint and
    /// the fade are put together at the end from this block's own
    /// `ALPHA_COLOR`, `ALPHA`, `FADE_COLOR` and `FADE_ALPHA`, all 0 by
    /// default: those of a template are replaced.
    pub fn apply_with(&mut self, node: &Node, files: &mut Files<'_>) -> Result<()> {
        let (mut fade_r, mut fade_g, mut fade_b, mut fade_a) = (0, 0, 0, 0);
        // Whether this block names a fade of its own.
        let mut faded = false;
        let (mut r, mut g, mut b, mut alpha) = (0, 0, 0, 0);
        // Whether this block names a tint of its own.
        let mut tinted = false;
        for item in keys(node, WINDOW_KEYS, "WINDOW") {
            let (key, child) = item?;
            if self.base.apply_key(key, child) {
                continue;
            }
            match key {
                "TEMPLATE" => {
                    let file = child.text();
                    control_template(files, &file, "WINDOW", |root, files| {
                        self.apply_with(root, files)
                    })?;
                    self.template = Some(file);
                }
                "BACK_INACTIVE" => self.back_inactive = path(child),
                "IMAGE_INACTIVE" => self.image_inactive = path(child),
                "FONT_INACTIVE" => self.font_inactive = path(child),
                "TITLE" => self.title = control_text(&child.text()),
                "TITLE_ALIGN" => self.title_align = text_align(&child.text(), TextAlign::Center),
                "TITLE_RECT" => {
                    let mut rect = self.title_rect.unwrap_or_default();
                    read_rect(child, &mut rect);
                    self.title_rect = (!rect.is_empty()).then_some(rect);
                }
                "DRAG_RECT" => {
                    let mut rect = self.drag_rect.unwrap_or_default();
                    read_rect(child, &mut rect);
                    self.drag_rect = (!rect.is_empty()).then_some(rect);
                }
                "TRANSPARENT" => child.read_bool(&mut self.transparent),
                "MENU" => child.read_bool(&mut self.menu),
                "IN_GAME" => child.read_bool(&mut self.in_game),
                "CLIP_CONTENTS" => child.read_bool(&mut self.clip_contents),
                "PAUSE_MUSIC" => child.read_bool(&mut self.pause_music),
                "FADE_COLOR" => {
                    child.read_ints(&mut [&mut fade_r, &mut fade_g, &mut fade_b]);
                    self.fade_background = true;
                    faded = true;
                }
                "FADE_ALPHA" => {
                    child.read_int(&mut fade_a);
                    self.fade_background = true;
                    faded = true;
                }
                "ALPHA_COLOR" => {
                    child.read_ints(&mut [&mut r, &mut g, &mut b]);
                    tinted = true;
                }
                "ALPHA" => {
                    child.read_int(&mut alpha);
                    tinted = true;
                }
                "BUTTON" => {
                    let mut def = ButtonDef::default();
                    def.apply_with(child, files)?;
                    self.widgets.push(WidgetDef::Button(Box::new(def)));
                }
                "STATIC" => {
                    let mut def = StaticDef::default();
                    def.apply_with(child, files)?;
                    self.widgets.push(WidgetDef::Static(def));
                }
                "EDIT" => {
                    let mut def = EditDef::default();
                    def.apply_with(child, files)?;
                    self.widgets.push(WidgetDef::Edit(def));
                }
                "WINDOW" => {
                    let mut def = WindowDef::default();
                    def.apply_with(child, files)?;
                    self.widgets.push(WidgetDef::Window(Box::new(def)));
                }
                "ENTITY_CONTAINER" => {
                    let mut def = EntityContainerDef::default();
                    def.apply_with(child, files)?;
                    self.widgets.push(WidgetDef::EntityContainer(def));
                }
                _ => {}
            }
        }
        // An alpha without a colour means white at that alpha; nothing at
        // all means no tint. A block that names neither keeps the tint
        // it has, a template's, and the same holds for the fade. (The
        // original computes both again from this block alone and loses
        // the template's.)
        if tinted {
            if alpha != 0 && r == 0 && g == 0 && b == 0 {
                (r, g, b) = (255, 255, 255);
            }
            let tint = Rgba::from_ints(r, g, b, alpha);
            self.alpha_color = (tint != Rgba::from_ints(0, 0, 0, 0)).then_some(tint);
        }
        if faded {
            self.fade_color = Some(Rgba::from_ints(fade_r, fade_g, fade_b, fade_a));
        }
        Ok(())
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// Parses a `.def` file: an `INVENTORY_BOX` root block. No other file
    /// is read: a `TEMPLATE` key fails the box.
    pub fn parse(bytes: &[u8]) -> Result<InventoryBoxDef> {
        InventoryBoxDef::parse_with(bytes, &mut Files::none())
    }

    /// Parses a `.def` file, reading the `TEMPLATE` files it names (its
    /// own and those of its window and controls) through `files`.
    pub fn parse_with(bytes: &[u8], files: &mut Files<'_>) -> Result<InventoryBoxDef> {
        let doc = Document::parse(bytes)?;
        let mut def = InventoryBoxDef::default();
        def.apply_with(root_of(&doc, "INVENTORY_BOX")?, files)?;
        Ok(def)
    }

    /// Reads the keys of an `INVENTORY_BOX` block, overriding what is
    /// there. No `TEMPLATE` file is read.
    pub fn apply(&mut self, node: &Node) -> Result<()> {
        self.apply_with(node, &mut Files::none())
    }

    /// `CAdInventoryBox::LoadBuffer`: reads the keys of the block in
    /// order, overriding what is there. A `TEMPLATE` loads the named file
    /// where the key stands. A key that is not in the table, a window
    /// that cannot be read or a template that cannot be loaded fails the
    /// box. `EXCLUSIVE` starts false in every file read, and
    /// `ALWAYS_VISIBLE` is that of this block alone: a template's is
    /// replaced.
    pub fn apply_with(&mut self, node: &Node, files: &mut Files<'_>) -> Result<()> {
        let mut always_visible = false;
        self.exclusive = false;
        for item in keys(node, INVENTORY_BOX_KEYS, "INVENTORY_BOX") {
            let (key, child) = item?;
            match key {
                "TEMPLATE" => {
                    let name = child.text();
                    files.template(&name, |doc, files| {
                        self.apply_with(root_of(doc, "INVENTORY_BOX")?, files)
                    })?;
                    self.template = Some(name);
                }
                "NAME" => self.name = child.text(),
                "CAPTION" => self.caption = child.text(),
                "WINDOW" => {
                    self.window = None;
                    let mut window = WindowDef::default();
                    window.apply_with(child, files)?;
                    self.window = Some(window);
                }
                "AREA" => read_rect(child, &mut self.area),
                "ITEM_WIDTH" => child.read_int(&mut self.item_width),
                "ITEM_HEIGHT" => child.read_int(&mut self.item_height),
                "SPACING" => child.read_int(&mut self.spacing),
                "SCROLL_BY" => child.read_int(&mut self.scroll_by),
                "HIDE_SELECTED" => child.read_bool(&mut self.hide_selected),
                "EXCLUSIVE" => child.read_bool(&mut self.exclusive),
                "ALWAYS_VISIBLE" => child.read_bool(&mut always_visible),
                _ => {}
            }
        }
        self.always_visible = always_visible;
        Ok(())
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// Tint and opacity (`m_AlphaColor`); opaque white leaves the item
    /// as it is.
    pub alpha_color: Rgba,
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
            alpha_color: Rgba::OPAQUE_WHITE,
        }
    }
}

impl ItemDef {
    /// Parses an `.items` file: every `ITEM` root block. An error is
    /// returned when the file has one; see [`ItemDef::parse_all_with`]
    /// for the items read all the same.
    pub fn parse_all(bytes: &[u8]) -> Result<Vec<ItemDef>> {
        let (items, error) = ItemDef::parse_all_with(bytes, &mut Files::none());
        match error {
            Some(err) => Err(err),
            None => Ok(items),
        }
    }

    /// `CAdGame::LoadItemsBuffer`: the `ITEM` blocks of an `.items` file,
    /// in order, reading their `TEMPLATE` files through `files`. An item
    /// that cannot be read is left out and the others are kept; the
    /// error returned with them is the syntax error that ended the file
    /// (a statement that is not an `ITEM`), which fails the load after
    /// the items before it were added.
    pub fn parse_all_with(bytes: &[u8], files: &mut Files<'_>) -> (Vec<ItemDef>, Option<Error>) {
        let doc = match Document::parse(bytes) {
            Ok(doc) => doc,
            Err(err) => return (Vec::new(), Some(err.into())),
        };
        let mut items = Vec::new();
        for entry in document_keys(&doc, &["ITEM"], "ITEMS") {
            match entry {
                Ok((_, node)) => {
                    let mut item = ItemDef::default();
                    if item.apply_with(node, files).is_ok() {
                        items.push(item);
                    }
                }
                Err(err) => return (items, Some(err.into())),
            }
        }
        (items, None)
    }

    /// Reads the keys of an `ITEM` block, overriding what is there. No
    /// `TEMPLATE` file is read.
    pub fn apply(&mut self, node: &Node) -> Result<()> {
        self.apply_with(node, &mut Files::none())
    }

    /// `CAdItem::LoadBuffer`: reads the keys of an `ITEM` block in order,
    /// overriding what is there. A `TEMPLATE` loads the named item file
    /// where the key stands. A key that is not in the table or a template
    /// that cannot be loaded fails the item. The tint is put together at
    /// the end from this block's `ALPHA_COLOR` (0,0,0 by default) and
    /// `ALPHA` (255 by default): a black colour with an alpha means
    /// white, so `ALPHA` alone makes the item translucent.
    pub fn apply_with(&mut self, node: &Node, files: &mut Files<'_>) -> Result<()> {
        let (mut r, mut g, mut b, mut alpha) = (0, 0, 0, 255);
        // Whether this block names a tint of its own.
        let mut tinted = false;
        for item in keys(node, ITEM_KEYS, "ITEM") {
            let (key, child) = item?;
            match key {
                "TEMPLATE" => {
                    let name = child.text();
                    files.template(&name, |doc, files| {
                        self.apply_with(root_of(doc, "ITEM")?, files)
                    })?;
                    self.template = Some(name);
                }
                "NAME" => self.name = child.text(),
                "CAPTION" => self.caption = child.text(),
                "IMAGE" | "SPRITE" => self.sprite = path(child),
                "IMAGE_HOVER" | "SPRITE_HOVER" => self.sprite_hover = path(child),
                "CURSOR" => self.cursor = path(child),
                "CURSOR_HOVER" => self.cursor_hover = path(child),
                "CURSOR_COMBINED" => child.read_bool(&mut self.cursor_combined),
                "FONT" => self.font = path(child),
                "AMOUNT" => child.read_int(&mut self.amount),
                "DISPLAY_AMOUNT" => child.read_bool(&mut self.display_amount),
                "AMOUNT_OFFSET_X" => child.read_int(&mut self.amount_offset_x),
                "AMOUNT_OFFSET_Y" => child.read_int(&mut self.amount_offset_y),
                "AMOUNT_ALIGN" => {
                    self.amount_align = text_align(&child.text(), TextAlign::Center);
                }
                "AMOUNT_STRING" => self.amount_string = Some(child.text()),
                "TALK" => self.talk.push(child.text()),
                "TALK_SPECIAL" => self.talk_special.push(child.text()),
                "SCRIPT" => self.scripts.push(child.text()),
                "PROPERTY" => self.properties.extend(Property::from_node(child)),
                "ALPHA_COLOR" => {
                    child.read_ints(&mut [&mut r, &mut g, &mut b]);
                    tinted = true;
                }
                "ALPHA" => {
                    child.read_int(&mut alpha);
                    tinted = true;
                }
                _ => {}
            }
        }
        // An item that names no tint keeps the one it has: a template's.
        if tinted {
            if alpha != 0 && r == 0 && g == 0 && b == 0 {
                (r, g, b) = (255, 255, 255);
            }
            self.alpha_color = Rgba::from_ints(r, g, b, alpha);
        }
        Ok(())
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

    /// A loader serving `files` by name.
    fn files<'a>(files: &'a [(&'a str, &'a str)]) -> impl FnMut(&str) -> Option<Vec<u8>> + 'a {
        move |name| {
            files
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, body)| body.as_bytes().to_vec())
        }
    }

    fn try_window(src: &str, mut load: impl FnMut(&str) -> Option<Vec<u8>>) -> Result<WindowDef> {
        WindowDef::parse_with(src.as_bytes(), &mut Files::new(&mut load))
    }

    fn window_with(src: &str, load: impl FnMut(&str) -> Option<Vec<u8>>) -> WindowDef {
        try_window(src, load).unwrap_or_else(|e| panic!("{e}\n{src}"))
    }

    fn first_button(window: &WindowDef) -> &ButtonDef {
        match &window.widgets[0] {
            WidgetDef::Button(b) => b,
            other => panic!("button expected, got {other:?}"),
        }
    }

    const TPL_BUTTON: &str =
        r#"BUTTON { BACK="b.image" FONT="f.font" WIDTH=30 HEIGHT=30 NAME="tpl" }"#;

    #[test]
    fn a_button_template_is_loaded_in_place() {
        let window = window_with(
            r#"WINDOW { BUTTON { TEMPLATE="t.button" NAME="yes" TEXT="Yes" WIDTH=70 } }"#,
            files(&[("t.button", TPL_BUTTON)]),
        );
        let b = first_button(&window);
        assert_eq!(b.base.back.as_deref(), Some("b.image"));
        assert_eq!(b.base.font.as_deref(), Some("f.font"));
        assert_eq!(b.base.height, 30);
        assert_eq!(b.base.name, "yes");
        assert_eq!(b.text, "Yes");
        assert_eq!(b.base.width, 70);
        assert_eq!(b.template.as_deref(), Some("t.button"));
    }

    #[test]
    fn a_key_before_the_template_is_overridden_by_it() {
        let window = window_with(
            r#"WINDOW { BUTTON { NAME="mine" WIDTH=70 HEIGHT=5 TEMPLATE="t.button" } }"#,
            files(&[("t.button", TPL_BUTTON)]),
        );
        let b = first_button(&window);
        assert_eq!(b.base.name, "tpl");
        assert_eq!(b.base.width, 30);
        assert_eq!(b.base.height, 30);
    }

    #[test]
    fn templates_chain() {
        let window = window_with(
            r#"WINDOW { BUTTON { TEMPLATE="a.button" } }"#,
            files(&[
                ("a.button", r#"BUTTON { TEMPLATE="b.button" HEIGHT=12 }"#),
                (
                    "b.button",
                    r#"BUTTON { FONT="deep.font" HEIGHT=99 WIDTH=8 }"#,
                ),
            ]),
        );
        let b = first_button(&window);
        assert_eq!(b.base.font.as_deref(), Some("deep.font"));
        assert_eq!(b.base.width, 8);
        assert_eq!(b.base.height, 12);
    }

    /// The original recurses until the stack is gone; here the chain is
    /// cut and the control fails, as any template that does not load.
    #[test]
    fn a_template_naming_itself_stops_at_the_depth_bound() {
        let mut loads = 0;
        let src = r#"BUTTON { TEMPLATE="self.button" FONT="f.font" }"#;
        let result = try_window(r#"WINDOW { BUTTON { TEMPLATE="self.button" } }"#, |name| {
            loads += 1;
            (name == "self.button").then(|| src.as_bytes().to_vec())
        });
        assert!(result.is_err());
        assert_eq!(loads, crate::template::TEMPLATE_DEPTH as usize);
    }

    /// `CUIButton::LoadBuffer` stops at the first error: a template that
    /// is missing, is of another kind or has a syntax error fails the
    /// button, and the button fails its window.
    #[test]
    fn a_template_that_does_not_load_fails_the_window() {
        let missing = try_window(
            r#"WINDOW { BUTTON { TEMPLATE="gone.button" NAME="yes" WIDTH=70 } }"#,
            |_| None,
        );
        assert!(missing.is_err());
        let wrong_kind = try_window(
            r#"WINDOW { BUTTON { TEMPLATE="t.static" NAME="yes" } }"#,
            files(&[("t.static", r#"STATIC { FONT="f.font" WIDTH=30 }"#)]),
        );
        assert!(wrong_kind.is_err());
        let bad = try_window(
            r#"WINDOW { BUTTON { TEMPLATE="t.button" NAME="yes" } }"#,
            files(&[("t.button", "BUTTON { BOGUS=1 }")]),
        );
        assert!(bad.is_err());
        // Without a reader every template is missing.
        assert!(
            WindowDef::parse(br#"WINDOW { BUTTON { TEMPLATE="t.button" NAME="yes" } }"#).is_err()
        );
    }

    #[test]
    fn static_and_edit_templates_are_loaded_too() {
        let window = window_with(
            r#"WINDOW {
  STATIC { TEMPLATE="t.static" TEXT="own" }
  EDIT { TEMPLATE="t.edit" NAME="e" }
}"#,
            files(&[
                (
                    "t.static",
                    r#"STATIC { FONT="s.font" HEIGHT=11 TEXT="tpl" }"#,
                ),
                (
                    "t.edit",
                    r#"EDIT { FONT="e.font" HEIGHT=22 NAME="tpl" MAX_LENGTH=9 }"#,
                ),
            ]),
        );
        let WidgetDef::Static(s) = &window.widgets[0] else {
            panic!("static expected");
        };
        assert_eq!(s.base.font.as_deref(), Some("s.font"));
        assert_eq!(s.base.height, 11);
        assert_eq!(s.text, "own");
        assert_eq!(s.template.as_deref(), Some("t.static"));
        let WidgetDef::Edit(e) = &window.widgets[1] else {
            panic!("edit expected");
        };
        assert_eq!(e.base.font.as_deref(), Some("e.font"));
        assert_eq!(e.base.height, 22);
        assert_eq!(e.max_length, 9);
        assert_eq!(e.base.name, "e");
        assert_eq!(e.template.as_deref(), Some("t.edit"));
    }

    #[test]
    fn a_button_in_a_nested_window_gets_its_template() {
        let window = window_with(
            r#"WINDOW { WINDOW { NAME="inner" BUTTON { TEMPLATE="t.button" NAME="n" } } }"#,
            files(&[("t.button", TPL_BUTTON)]),
        );
        let WidgetDef::Window(inner) = &window.widgets[0] else {
            panic!("window expected");
        };
        let b = first_button(inner);
        assert_eq!(b.base.back.as_deref(), Some("b.image"));
        assert_eq!(b.base.name, "n");
    }

    /// `CUIWindow::LoadBuffer TEMPLATE`: the template file is loaded into
    /// the window where the key stands, in a root window and in a nested
    /// one: keys before it are overridden, keys after it override, and
    /// its controls come before those that follow.
    #[test]
    fn a_window_template_is_loaded_in_place() {
        let template = r#"WINDOW { NAME="tpl" X=1 Y=2 MENU=TRUE BUTTON { NAME="from_tpl" } }"#;
        let window = window_with(
            r#"WINDOW {
  X=50 WIDTH=9
  BUTTON { NAME="before" }
  TEMPLATE="t.window"
  Y=60
  BUTTON { NAME="after" }
  WINDOW { NAME="mine" TEMPLATE="t.window" TRANSPARENT=TRUE }
}"#,
            files(&[("t.window", template)]),
        );
        assert_eq!(window.base.name, "tpl");
        assert_eq!((window.base.x, window.base.y), (1, 60));
        assert_eq!(window.base.width, 9);
        assert!(window.menu);
        let names: Vec<&str> = window
            .widgets
            .iter()
            .map(|w| w.base().name.as_str())
            .collect();
        assert_eq!(names, ["before", "from_tpl", "after", "tpl"]);
        let WidgetDef::Window(inner) = &window.widgets[3] else {
            panic!("window expected");
        };
        assert!(inner.menu && inner.transparent);
        assert_eq!(inner.widgets.len(), 1);
    }

    /// `CUIWindow::LoadBuffer`: the tint and the fade are computed at the
    /// end of a block from its own keys, which all default to 0.
    #[test]
    fn window_tint_and_fade_follow_the_original() {
        let get = |src: &str| WindowDef::parse(src.as_bytes()).unwrap();
        // Nothing: no tint, no fade.
        let w = get("WINDOW { }");
        assert_eq!((w.alpha_color, w.fade_color), (None, None));
        // `ALPHA` alone: white at that alpha.
        let w = get("WINDOW { ALPHA=128 }");
        assert_eq!(w.alpha_color, Some(Rgba::from_ints(255, 255, 255, 128)));
        // `ALPHA_COLOR` alone: alpha 0.
        let w = get("WINDOW { ALPHA_COLOR { 10,20,30 } }");
        assert_eq!(w.alpha_color, Some(Rgba::from_ints(10, 20, 30, 0)));
        // Black with an alpha becomes white.
        let w = get("WINDOW { ALPHA_COLOR { 0,0,0 } ALPHA=200 }");
        assert_eq!(w.alpha_color, Some(Rgba::from_ints(255, 255, 255, 200)));
        // `FADE_ALPHA` alone turns the fade on, black at that alpha.
        let w = get("WINDOW { FADE_ALPHA=100 }");
        assert_eq!(w.fade_color, Some(Rgba::from_ints(0, 0, 0, 100)));
        // `FADE_COLOR` alone: alpha 0.
        let w = get("WINDOW { FADE_COLOR { 1,2,3 } }");
        assert_eq!(w.fade_color, Some(Rgba::from_ints(1, 2, 3, 0)));

        // A template's tint and fade stay when the outer block names
        // none (the original loses them), and are replaced when it does.
        let w = window_with(
            r#"WINDOW { TEMPLATE="t.window" }"#,
            files(&[(
                "t.window",
                "WINDOW { ALPHA=128 FADE_COLOR { 9,9,9 } FADE_ALPHA=77 }",
            )]),
        );
        assert_eq!(w.alpha_color, Some(Rgba::from_ints(255, 255, 255, 128)));
        assert_eq!(w.fade_color, Some(Rgba::from_ints(9, 9, 9, 77)));
        let w = window_with(
            r#"WINDOW { TEMPLATE="t.window" ALPHA=10 FADE_ALPHA=20 }"#,
            files(&[(
                "t.window",
                "WINDOW { ALPHA=128 FADE_COLOR { 9,9,9 } FADE_ALPHA=77 }",
            )]),
        );
        assert_eq!(w.alpha_color, Some(Rgba::from_ints(255, 255, 255, 10)));
        assert_eq!(w.fade_color, Some(Rgba::from_ints(0, 0, 0, 20)));
    }

    /// A key outside the loader's table is a syntax error; a control
    /// that fails makes its window fail.
    #[test]
    fn an_unknown_key_fails_the_control_and_its_window() {
        assert!(WindowDef::parse(b"WINDOW { BOGUS=1 }").is_err());
        assert!(WindowDef::parse(b"WINDOW { BUTTON { BOGUS=1 } }").is_err());
        // `FOCUSABLE` is a button key, `VERTICAL_ALIGN` a static key.
        assert!(WindowDef::parse(b"WINDOW { STATIC { FOCUSABLE=TRUE } }").is_err());
        assert!(WindowDef::parse(b"WINDOW { BUTTON { VERTICAL_ALIGN=top } }").is_err());
        assert!(WindowDef::parse(b"WINDOW { EDIT { TEXT_ALIGN=left } }").is_err());
        // A container has no size, image or caption keys.
        assert!(
            WindowDef::parse(b"WINDOW { ENTITY_CONTAINER { NAME=\"c\" X=1 ENTITY=\"e.entity\" } }")
                .is_ok()
        );
        assert!(WindowDef::parse(b"WINDOW { ENTITY_CONTAINER { WIDTH=10 } }").is_err());
        // The editor's keys are accepted.
        assert!(WindowDef::parse(b"WINDOW { EDITOR_PROPERTY { NAME=\"a\" VALUE=\"b\" } }").is_ok());
    }

    /// `CUIEntity::LoadBuffer TEMPLATE`.
    #[test]
    fn an_entity_container_template_is_loaded_in_place() {
        let window = window_with(
            r#"WINDOW { ENTITY_CONTAINER { X=5 TEMPLATE="c.def" NAME="mine" } }"#,
            files(&[(
                "c.def",
                r#"ENTITY_CONTAINER { NAME="tpl" X=1 Y=2 ENTITY="e.entity" }"#,
            )]),
        );
        let WidgetDef::EntityContainer(c) = &window.widgets[0] else {
            panic!("container expected");
        };
        assert_eq!(c.base.name, "mine");
        assert_eq!((c.base.x, c.base.y), (1, 2));
        assert_eq!(c.entity.as_deref(), Some("e.entity"));
    }

    /// `CUITiledImage::LoadBuffer`: the template is applied where the key
    /// stands, a template that does not load is skipped, a short tile
    /// list reads zeros and an unknown key is a syntax error.
    #[test]
    fn tiled_image_templates_and_tile_lists() {
        let mut load = files(&[(
            "base.tiled",
            r#"TILED_IMAGE { IMAGE="base.png" UP_LEFT { 0,0,5,5 } }"#,
        )]);
        let tiled = TiledImage::parse_with(
            br#"TILED_IMAGE { IMAGE="mine.png" UP_LEFT { 1,1,2,2 } TEMPLATE="base.tiled" UP_RIGHT { 7,0,9,5 } }"#,
            &mut Files::new(&mut load),
        )
        .unwrap();
        assert_eq!(tiled.image.as_deref(), Some("base.png"));
        assert_eq!(
            tiled.tile(0, 0).right,
            5,
            "the template overrides what was before"
        );
        assert_eq!(tiled.tile(0, 2).left, 7);

        let tiled =
            TiledImage::parse(br#"TILED_IMAGE { TEMPLATE="gone.tiled" IMAGE="a.png" }"#).unwrap();
        assert_eq!(tiled.image.as_deref(), Some("a.png"));

        let tiled =
            TiledImage::parse(b"TILED_IMAGE { HORIZONTAL_TILES { 4 } VERTICAL_TILES { 3,3,3 } }")
                .unwrap();
        assert_eq!(tiled.tile(0, 0).right, 4);
        assert_eq!(tiled.tile(0, 1).width(), 0);
        assert_eq!(tiled.tile(2, 0).bottom, 9);

        assert!(TiledImage::parse(b"TILED_IMAGE { BOGUS=1 }").is_err());
    }

    /// `CAdResponseBox::LoadBuffer`: alignment values, the template in
    /// place, and what a failure does (nothing but drop the piece).
    #[test]
    fn response_box_follows_the_original() {
        let def = ResponseBoxDef::parse(b"RESPONSE_BOX { }").unwrap();
        assert_eq!(def.text_align, TextAlign::Left);
        assert_eq!(def.vertical_align, VerticalAlign::Bottom);
        assert!(def.window.is_none());

        let def = ResponseBoxDef::parse(
            b"RESPONSE_BOX { TEXT_ALIGN=\"center\" VERTICAL_ALIGN=\"center\" }",
        )
        .unwrap();
        assert_eq!(def.text_align, TextAlign::Center);
        assert_eq!(def.vertical_align, VerticalAlign::Center);
        let def = ResponseBoxDef::parse(b"RESPONSE_BOX { TEXT_ALIGN=\"x\" VERTICAL_ALIGN=\"x\" }")
            .unwrap();
        assert_eq!(def.text_align, TextAlign::Left);
        assert_eq!(
            def.vertical_align,
            VerticalAlign::Bottom,
            "anything else is bottom"
        );
        let def = ResponseBoxDef::parse(b"RESPONSE_BOX { VERTICAL_ALIGN=\"top\" }").unwrap();
        assert_eq!(def.vertical_align, VerticalAlign::Top);

        // A window that cannot be read is dropped, the box goes on.
        let def = ResponseBoxDef::parse(b"RESPONSE_BOX { WINDOW { BOGUS=1 } SPACING=4 }").unwrap();
        assert!(def.window.is_none());
        assert_eq!(def.spacing, 4);
        assert!(ResponseBoxDef::parse(b"RESPONSE_BOX { BOGUS=1 }").is_err());

        let mut load = files(&[(
            "base.def",
            r#"RESPONSE_BOX { FONT="base.font" SPACING=9 HORIZONTAL=TRUE }"#,
        )]);
        let def = ResponseBoxDef::parse_with(
            br#"RESPONSE_BOX { FONT="mine.font" TEMPLATE="base.def" SPACING=2 }"#,
            &mut Files::new(&mut load),
        )
        .unwrap();
        assert_eq!(def.font.as_deref(), Some("base.font"));
        assert_eq!(def.spacing, 2);
        assert!(def.horizontal);
        // A template that cannot be loaded is skipped.
        let def =
            ResponseBoxDef::parse(br#"RESPONSE_BOX { TEMPLATE="gone.def" SPACING=2 }"#).unwrap();
        assert_eq!(def.spacing, 2);
    }

    /// `CAdInventoryBox::LoadBuffer`: the template in place, the flags
    /// that are reset by every file, and the failures that fail the box.
    #[test]
    fn inventory_box_follows_the_original() {
        let base = r#"INVENTORY_BOX { NAME="base" SPACING=9 EXCLUSIVE=TRUE ALWAYS_VISIBLE=TRUE WINDOW { NAME="w" } }"#;
        let served = [("base.def", base)];
        let mut load = files(&served);
        let def = InventoryBoxDef::parse_with(
            br#"INVENTORY_BOX { SPACING=1 ITEM_WIDTH=20 TEMPLATE="base.def" NAME="mine" }"#,
            &mut Files::new(&mut load),
        )
        .unwrap();
        assert_eq!(def.name, "mine");
        assert_eq!(def.spacing, 9);
        assert_eq!(def.item_width, 20);
        assert!(def.exclusive, "the template's flag stays");
        assert!(!def.always_visible, "the outer file's own value counts");
        assert_eq!(def.window.as_ref().map(|w| w.base.name.as_str()), Some("w"));

        assert!(InventoryBoxDef::parse(br#"INVENTORY_BOX { TEMPLATE="gone.def" }"#).is_err());
        assert!(InventoryBoxDef::parse(b"INVENTORY_BOX { WINDOW { BOGUS=1 } }").is_err());
        assert!(InventoryBoxDef::parse(b"INVENTORY_BOX { BOGUS=1 }").is_err());
    }

    /// `CAdItem::LoadBuffer`, `CAdGame::LoadItemsBuffer`.
    #[test]
    fn items_follow_the_original() {
        let one = |src: &str| {
            let mut items = ItemDef::parse_all(src.as_bytes()).unwrap();
            assert_eq!(items.len(), 1);
            items.remove(0)
        };
        // No tint key: opaque white.
        assert_eq!(one("ITEM { }").alpha_color, Rgba::OPAQUE_WHITE);
        // `ALPHA` alone: translucent white.
        assert_eq!(
            one("ITEM { ALPHA=100 }").alpha_color,
            Rgba::from_ints(255, 255, 255, 100)
        );
        // Black with an alpha is white: no tint.
        assert_eq!(
            one("ITEM { ALPHA_COLOR { 0,0,0 } }").alpha_color,
            Rgba::OPAQUE_WHITE
        );
        assert_eq!(
            one("ITEM { ALPHA_COLOR { 9,8,7 } ALPHA=0 }").alpha_color,
            Rgba::from_ints(9, 8, 7, 0)
        );

        // A template is an item file, loaded where the key stands.
        let mut load = files(&[(
            "base.item",
            r#"ITEM { NAME="base" CAPTION="Base" SPRITE="base.sprite" ALPHA=50 TALK="t.sprite" }"#,
        )]);
        let (items, error) = ItemDef::parse_all_with(
            br#"ITEM { CAPTION="Mine" TEMPLATE="base.item" NAME="mine" }"#,
            &mut Files::new(&mut load),
        );
        assert!(error.is_none());
        let item = &items[0];
        assert_eq!(item.name, "mine");
        assert_eq!(item.caption, "Base");
        assert_eq!(item.sprite.as_deref(), Some("base.sprite"));
        assert_eq!(item.talk, ["t.sprite"]);
        assert_eq!(item.alpha_color, Rgba::from_ints(255, 255, 255, 50));

        // An item that fails is left out; a statement that is not an
        // `ITEM` ends the file with an error, after the items before it.
        let (items, error) = ItemDef::parse_all_with(
            br#"ITEM { NAME="a" } ITEM { NAME="b" BOGUS=1 } ITEM { TEMPLATE="gone" NAME="c" } ITEM { NAME="d" } OTHER { } ITEM { NAME="e" }"#,
            &mut Files::none(),
        );
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["a", "d"]);
        assert!(error.is_some());
    }
}
