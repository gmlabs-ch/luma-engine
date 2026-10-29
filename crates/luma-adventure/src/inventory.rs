//! The inventory: the game's items, those taken, and the box that shows
//! them with its window.
//!
//! Written with reference to `AdItem.cpp`, `AdInventory.cpp`,
//! `AdInventoryBox.cpp` and `AdGame.cpp` of WME Lite (MIT License,
//! Copyright (c) 2011 Jan Nedoma); see `NOTICE`. The formats are
//! documented in `docs/formats/window.md`.

use luma_formats::{InventoryBoxDef, ItemDef, Rgba};
use luma_render::{DrawList, Texture};
use luma_script::{Engine, NativeId, Value};

use crate::input::HitArea;
use crate::objects::Kind;
use crate::scene_view::AnimState;
use crate::ui::{ButtonListener, Control};
use crate::world::World;

/// An item of the game (`CAdItem`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Item {
    /// The definition, which scripts modify.
    pub def: ItemDef,
    /// The sprite shown this frame: the hover one under the mouse.
    pub current_sprite: Option<String>,
    /// The animation of the sprite shown.
    pub anim: Option<AnimState>,
    /// Where the item was last drawn (its sprite's hotspot).
    pub x: i32,
    /// See `x`.
    pub y: i32,
}

/// The items and the box (`CAdInventory`, `CAdInventoryBox`).
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Inventory {
    /// Every item of the game, by handle.
    pub items: Vec<(NativeId, Item)>,
    /// The items taken, in order.
    pub taken: Vec<NativeId>,
    /// The box's definition, when the game has one.
    pub def: Option<InventoryBoxDef>,
    /// The box's window.
    pub window: Option<NativeId>,
    /// The button under the whole screen that closes an `EXCLUSIVE` box.
    pub close_button: Option<NativeId>,
    /// Whether the box is shown (`Game.InventoryVisible`).
    pub visible: bool,
    /// The first item shown.
    pub scroll_offset: i32,
    /// Show the hover cursor only over objects that handle the item
    /// (`Game.SmartItemCursor`).
    pub smart_item_cursor: bool,
}

impl World {
    /// The inventory.
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    /// An item by handle.
    pub fn item(&self, id: NativeId) -> Option<&Item> {
        self.inventory
            .items
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, item)| item)
    }

    /// An item by handle, mutable.
    pub fn item_mut(&mut self, id: NativeId) -> Option<&mut Item> {
        self.inventory
            .items
            .iter_mut()
            .find(|(i, _)| *i == id)
            .map(|(_, item)| item)
    }

    /// Every item, in file order.
    pub fn item_ids(&self) -> Vec<NativeId> {
        self.inventory.items.iter().map(|(id, _)| *id).collect()
    }

    /// An item by name, ignoring case (`GetItemByName`).
    pub fn item_by_name(&self, name: &str) -> Option<NativeId> {
        self.inventory
            .items
            .iter()
            .find(|(_, item)| item.def.name.eq_ignore_ascii_case(name))
            .map(|(id, _)| *id)
    }

    /// The items taken, in order.
    pub fn taken_items(&self) -> &[NativeId] {
        &self.inventory.taken
    }

    /// The item in use (`Game.SelectedItem`).
    pub fn selected_item(&self) -> Option<NativeId> {
        self.selected_item.filter(|id| self.item(*id).is_some())
    }

    /// Loads the game's items file (`ITEMS` of `default.game`) and
    /// attaches the items' scripts.
    pub(crate) fn load_items(&mut self, engine: &mut Engine) {
        let Some(path) = self.game_file().items.clone() else {
            return;
        };
        let bytes = match self.vfs.read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.log(format!(
                    "CAdGame::LoadItemsFile failed for file '{path}': {err}"
                ));
                return;
            }
        };
        let defs = match ItemDef::parse_all(&bytes) {
            Ok(defs) => defs,
            Err(err) => {
                self.log(format!("Error parsing ITEMS file '{path}': {err}"));
                return;
            }
        };
        for def in defs {
            self.add_item(engine, def);
        }
    }

    /// Adds an item from its definition, loading its sprites and
    /// cursors.
    pub fn add_item(&mut self, engine: &mut Engine, mut def: ItemDef) -> NativeId {
        def.caption = self.expand(&def.caption);
        for sprite in [&def.sprite, &def.sprite_hover].into_iter().flatten() {
            self.load_ui_image(sprite);
        }
        for cursor in [&def.cursor, &def.cursor_hover].into_iter().flatten() {
            self.load_cursor(cursor);
        }
        if let Some(font) = &def.font {
            self.load_font(font);
        }
        let id = self.objects.insert(Kind::Item);
        if let Some(native) = self.objects.get_mut(id) {
            for property in &def.properties {
                native
                    .props
                    .insert(property.name.clone(), Value::from(property.value.clone()));
            }
        }
        let scripts = def.scripts.clone();
        self.inventory.items.push((
            id,
            Item {
                def,
                current_sprite: None,
                anim: None,
                x: 0,
                y: 0,
            },
        ));
        for script in scripts {
            self.attach(engine, id, &script);
        }
        id
    }

    /// Loads the inventory box (`INVENTORY_BOX` of `default.game`): its
    /// window, whose buttons the box listens to.
    pub(crate) fn load_inventory_box(&mut self, engine: &mut Engine) {
        let Some(path) = self.game_file().inventory_box.clone() else {
            return;
        };
        let bytes = match self.vfs.read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.log(format!("Error opening inventory box '{path}': {err}"));
                return;
            }
        };
        let mut def = match InventoryBoxDef::parse(&bytes) {
            Ok(def) => def,
            Err(err) => {
                self.log(format!("Error parsing inventory box '{path}': {err}"));
                return;
            }
        };
        if let Some(window_def) = def.window.take() {
            let window = self.new_window_control(None);
            self.apply_window_def(engine, window, window_def, None);
            for widget in self.widgets_of(window) {
                if let Some(b) = self.control_mut(widget).and_then(Control::button_mut) {
                    b.listener = Some(ButtonListener::Inventory);
                }
            }
            self.inventory.window = Some(window);
            if def.exclusive {
                let close =
                    self.new_button_control(Some(window), "close", ButtonListener::Inventory);
                self.inventory.close_button = Some(close);
            }
        }
        self.inventory.visible = def.always_visible;
        self.inventory.def = Some(def);
    }

    /// Puts an item in the inventory (`TakeItem`): again if it was
    /// there, after the named item or at the end. `false` when there is
    /// no such item.
    pub fn take_item(&mut self, name: &str, after: Option<&str>) -> bool {
        let Some(id) = self.item_by_name(name) else {
            return false;
        };
        let after_id = after.and_then(|a| self.item_by_name(a));
        self.inventory.taken.retain(|t| *t != id);
        let index = after_id
            .and_then(|a| self.inventory.taken.iter().position(|t| *t == a))
            .map(|i| i + 1);
        match index {
            Some(i) => self.inventory.taken.insert(i, id),
            None => self.inventory.taken.push(id),
        }
        self.handle_item_associations(name, false);
        true
    }

    /// Takes an item out of the inventory (`DropItem`), deselecting it.
    /// `false` when it was not there.
    pub fn drop_item(&mut self, name: &str) -> bool {
        let Some(id) = self.item_by_name(name) else {
            return false;
        };
        let Some(index) = self.inventory.taken.iter().position(|t| *t == id) else {
            return false;
        };
        self.inventory.taken.remove(index);
        if self.selected_item == Some(id) {
            self.selected_item = None;
        }
        self.handle_item_associations(name, true);
        true
    }

    /// Whether an item is in the inventory (`HasItem`, `IsItemTaken`).
    pub fn has_item(&self, name: &str) -> bool {
        self.item_by_name(name)
            .is_some_and(|id| self.inventory.taken.contains(&id))
    }

    /// Sets the item in use from a script value (`Game.SelectedItem`):
    /// `null`, an item, or an item's name.
    pub fn select_item(&mut self, value: &Value) {
        self.selected_item = match value {
            Value::Null => None,
            Value::Native(id) => Some(*id).filter(|id| self.item(*id).is_some()),
            v => {
                let name = v.get_string(self);
                self.item_by_name(&name)
            }
        };
    }

    /// Shows or hides the scene's entities tied to an item (`ITEM`):
    /// hidden while the item is taken (`HandleItemAssociations`).
    fn handle_item_associations(&mut self, name: &str, show: bool) {
        let Some(view) = self.scene.as_mut() else {
            return;
        };
        for r in view.entity_refs() {
            if let Some(e) = view.entity_mut(r) {
                if e.item
                    .as_deref()
                    .is_some_and(|i| i.eq_ignore_ascii_case(name))
                {
                    e.active = show;
                }
            }
        }
    }

    /// Hides the entities of a freshly loaded scene whose item is taken
    /// (`CAdEntity::LoadBuffer`).
    pub(crate) fn hide_taken_item_entities(&mut self) {
        let taken: Vec<String> = self
            .inventory
            .taken
            .iter()
            .filter_map(|id| self.item(*id).map(|i| i.def.name.clone()))
            .collect();
        for name in taken {
            self.handle_item_associations(&name, false);
        }
    }

    /// The items shown in the box with the screen position of their
    /// cells (`CAdInventoryBox::Display`).
    fn item_cells(&self) -> Vec<(NativeId, i32, i32)> {
        let Some(def) = &self.inventory.def else {
            return Vec::new();
        };
        let (columns, rows) = def.grid();
        let (wx, wy) = self
            .inventory
            .window
            .and_then(|w| self.control(w))
            .map_or((0, 0), |c| (c.x, c.y));
        let (left, top) = (def.area.left + wx, def.area.top + wy);
        let selected = self.selected_item();
        let mut cells = Vec::new();
        for row in 0..rows {
            for col in 0..columns {
                let index = self.inventory.scroll_offset + row * columns + col;
                let Some(id) = usize::try_from(index)
                    .ok()
                    .and_then(|i| self.inventory.taken.get(i).copied())
                else {
                    continue;
                };
                if Some(id) == selected && def.hide_selected {
                    continue;
                }
                cells.push((
                    id,
                    left + col * (def.item_width + def.spacing),
                    top + row * (def.item_height + def.spacing),
                ));
            }
        }
        cells
    }

    /// Whether the box is shown this frame.
    fn box_shown(&self) -> bool {
        self.inventory.def.is_some() && self.inventory.visible
    }

    /// One frame of the box (`CAdInventoryBox::Display`): the scroll
    /// buttons follow the pages, the window and the items under the
    /// mouse update.
    pub(crate) fn update_inventory(&mut self, engine: &mut Engine) {
        if !self.box_shown() {
            return;
        }
        let Some(def) = self.inventory.def.clone() else {
            return;
        };
        let (columns, rows) = def.grid();
        let taken = self.inventory.taken.len() as i32;
        let offset = self.inventory.scroll_offset;
        if let Some(window) = self.inventory.window {
            if let Some(prev) = self.widget(window, "prev") {
                if let Some(c) = self.control_mut(prev) {
                    c.disabled = offset <= 0;
                }
            }
            if let Some(next) = self.widget(window, "next") {
                if let Some(c) = self.control_mut(next) {
                    c.disabled = offset + columns * rows >= taken;
                }
            }
        }
        if let Some(close) = self.inventory.close_button {
            let (w, h) = (self.settings.width as i32, self.settings.height as i32);
            if let Some(c) = self.control_mut(close) {
                c.x = 0;
                c.y = 0;
                c.width = w;
                c.height = h;
            }
            self.update_button(engine, close);
        }
        if let Some(window) = self.inventory.window {
            self.update_window(engine, window);
        }
        let active = self.active_object();
        let timer = self.timer();
        let cells = self.item_cells();
        let World { inventory, ui, .. } = self;
        for (id, x, y) in cells {
            let Some((_, item)) = inventory.items.iter_mut().find(|(i, _)| *i == id) else {
                continue;
            };
            let sprite = if active == Some(id) && item.def.sprite_hover.is_some() {
                item.def.sprite_hover.clone()
            } else {
                item.def.sprite.clone()
            };
            item.current_sprite = sprite.clone();
            item.x = x;
            item.y = y;
            let Some(path) = sprite else {
                item.anim = None;
                continue;
            };
            let anim = item.anim.get_or_insert_with(|| AnimState::new(&path));
            if anim.sprite != crate::scene_view::key(&path) {
                *anim = AnimState::new(&path);
            }
            if let Some(loaded) = ui.bank.sprite(&path) {
                anim.step(&loaded.sprite, timer);
            }
        }
    }

    /// A press of one of the box's buttons (`CAdInventoryBox::Listen`).
    pub(crate) fn inventory_listen(&mut self, button: &str) {
        let scroll_by = self.inventory.def.as_ref().map_or(1, |d| d.scroll_by);
        if button.eq_ignore_ascii_case("close") {
            self.inventory.visible = false;
        } else if button.eq_ignore_ascii_case("prev") {
            self.inventory.scroll_offset = (self.inventory.scroll_offset - scroll_by).max(0);
        } else if button.eq_ignore_ascii_case("next") {
            self.inventory.scroll_offset += scroll_by;
        }
    }

    /// Registers where the box and its items catch the mouse.
    pub(crate) fn inventory_hits(&mut self, hits: &mut Vec<HitArea>) {
        if !self.box_shown() {
            return;
        }
        if let Some(close) = self.inventory.close_button {
            let (w, h) = (self.settings.width as i32, self.settings.height as i32);
            hits.push(HitArea::rect(close, 0, 0, w, h));
        }
        if let Some(window) = self.inventory.window {
            self.window_hits(window, 0, 0, hits);
        }
        for (id, x, y) in self.item_cells() {
            let Some(item) = self.item(id) else { continue };
            let Some(path) = item.current_sprite.clone() else {
                continue;
            };
            let frame = item.anim.as_ref().map_or(0, |a| a.current_frame);
            if let Some(loaded) = self.ui.bank.sprite(&path) {
                World::sprite_hits(
                    id,
                    crate::input::HitBank::Ui,
                    &loaded.sprite,
                    frame,
                    x,
                    y,
                    100.0,
                    (0, 0),
                    hits,
                );
            }
        }
    }

    /// Draws the box: its window, then the items in their cells.
    pub(crate) fn draw_inventory<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        white: Option<&'a Texture>,
    ) {
        if !self.box_shown() {
            return;
        }
        if let Some(window) = self.inventory.window {
            self.draw_window(list, window, 0, 0, white, Rgba::OPAQUE_WHITE);
        }
        for (id, x, y) in self.item_cells() {
            let Some(item) = self.item(id) else { continue };
            let Some(path) = item.current_sprite.as_deref() else {
                continue;
            };
            let frame = item.anim.as_ref().map_or(0, |a| a.current_frame);
            let tint = item.def.alpha_color.unwrap_or(Rgba::OPAQUE_WHITE);
            self.draw_ui_image(list, path, frame, x, y, tint);
            if item.def.display_amount {
                let width = self.sprite_size_loaded(path).map_or(0, |(w, _)| w);
                let text = item
                    .def
                    .amount_string
                    .clone()
                    .unwrap_or_else(|| item.def.amount.to_string());
                let font = item
                    .def
                    .font
                    .clone()
                    .or_else(|| self.system_font.clone())
                    .and_then(|f| self.fonts.get(&f));
                if let Some(font) = font {
                    let (mut amount_x, mut width) = (x, width);
                    if item.def.amount_align == luma_formats::TextAlign::Right {
                        width -= item.def.amount_offset_x;
                        amount_x -= item.def.amount_offset_x;
                    }
                    amount_x += item.def.amount_offset_x;
                    font.draw(
                        list,
                        &text,
                        amount_x,
                        y + item.def.amount_offset_y,
                        width,
                        item.def.amount_align,
                        -1,
                    );
                }
            }
        }
    }

    /// Prepares the amounts' texts for drawing.
    pub(crate) fn prepare_inventory_texts(&mut self) {
        if !self.box_shown() {
            return;
        }
        for (id, _, _) in self.item_cells() {
            let Some(item) = self.item(id) else { continue };
            if !item.def.display_amount {
                continue;
            }
            let text = item
                .def
                .amount_string
                .clone()
                .unwrap_or_else(|| item.def.amount.to_string());
            let align = item.def.amount_align;
            let width = item
                .current_sprite
                .as_deref()
                .and_then(|p| self.sprite_size_loaded(p))
                .map_or(0, |(w, _)| w);
            let font = item.def.font.clone().or_else(|| self.system_font.clone());
            if let Some(f) = font.and_then(|f| self.fonts.get_mut(&f)) {
                f.prepare(&text, width, align, -1);
            }
        }
    }

    /// The cursor of the selected item (`CAdGame::ShowCursor`): drawn
    /// over the normal cursor when `CURSOR_COMBINED`, instead of it
    /// otherwise; the hover cursor over an object that can use the item.
    pub(crate) fn update_item_cursor(&mut self, engine: &Engine) {
        self.cursors.item_cursor = None;
        let Some(item_id) = self.selected_item() else {
            return;
        };
        if self.ui.frozen || !self.interactive || self.cursors.hidden {
            return;
        }
        let Some(item) = self.item(item_id) else {
            return;
        };
        let (combined, name) = (item.def.cursor_combined, item.def.name.clone());
        let (cursor, cursor_hover) = (item.def.cursor.clone(), item.def.cursor_hover.clone());
        if !combined {
            self.cursors.shown = None;
        }
        let active = self.active_object();
        let usable = active.is_some_and(|a| {
            matches!(
                self.objects.get(a).map(|n| &n.kind),
                Some(Kind::Entity(_) | Kind::Actor | Kind::Item)
            )
        });
        let hover = usable
            && cursor_hover.is_some()
            && (!self.inventory.smart_item_cursor
                || active.is_some_and(|a| engine.can_handle_event(a, &name)));
        let chosen = if hover { cursor_hover } else { cursor };
        let Some(path) = chosen else { return };
        if !self.load_cursor(&path) {
            return;
        }
        self.cursors.step_anim(&path, self.timer());
        self.cursors.item_cursor = Some(path);
    }
}
