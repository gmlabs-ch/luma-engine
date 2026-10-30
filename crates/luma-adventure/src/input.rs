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
use crate::scene_view::{AnimState, DrawItem, EntityRef, SpriteBank, key};
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
}

/// The keyboard as scripts see it (`Game.Keyboard`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Keyboard {
    /// The code of the last key pressed (`KeyCode`): the Windows virtual
    /// key code, or the ASCII code of a printable key.
    pub key_code: i32,
    /// The character of the last key when it is printable (`Key`).
    pub key: String,
    /// Whether the last key was printable (`Printable`).
    pub printable: bool,
    /// Modifier state at the last key.
    pub shift: bool,
    /// See `shift`.
    pub alt: bool,
    /// See `shift`.
    pub control: bool,
    /// The keys held, by code.
    pub down: HashSet<i32>,
}

impl Keyboard {
    /// Whether a key is held (`IsKeyDown`), by code; a letter's code may
    /// be given in either case.
    pub fn is_key_down(&self, code: i32) -> bool {
        let code = match u8::try_from(code) {
            Ok(c) if c.is_ascii_lowercase() => i32::from(c.to_ascii_uppercase()),
            _ => code,
        };
        self.down.contains(&code)
    }
}

/// Which bank holds the image of a hit area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitBank {
    Scene,
    Actor(NativeId),
    Ui,
}

/// Where an object can be hit, as the engine registers it while drawing
/// (`CBActiveRect`).
#[derive(Debug, Clone, PartialEq)]
enum HitShape {
    /// A sub-frame drawn at `dst` (layer coordinates), from `src` of its
    /// image, scaled by `zoom` percent, possibly mirrored; when `precise`,
    /// only its opaque pixels count.
    Pixels {
        bank: HitBank,
        image: String,
        color_key: Rgb,
        dst: (i32, i32, i32, i32),
        src: Option<(i32, i32, i32, i32)>,
        zoom: f32,
        mirror_x: bool,
        mirror_y: bool,
        precise: bool,
    },
    /// A polygon in scene coordinates.
    Polygon(Vec<Point>),
    /// A rectangle on screen (a window, a button).
    Rect(i32, i32, i32, i32),
}

/// An object's hit area.
#[derive(Debug, Clone, PartialEq)]
pub struct HitArea {
    owner: NativeId,
    /// The scroll offset of the layer the object is drawn in.
    layer_offset: (i32, i32),
    shape: HitShape,
}

impl HitArea {
    /// A rectangle on screen.
    pub(crate) fn rect(owner: NativeId, x: i32, y: i32, w: i32, h: i32) -> HitArea {
        HitArea {
            owner,
            layer_offset: (0, 0),
            shape: HitShape::Rect(x, y, w, h),
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

    /// Moves the mouse to a point in game pixels.
    pub fn set_mouse(&mut self, x: i32, y: i32) {
        self.mouse.x = x;
        self.mouse.y = y;
    }

    /// The keyboard state.
    pub fn keyboard(&self) -> &Keyboard {
        &self.keyboard
    }

    /// Records a key going down (`CBKeyboardState::ReadKey`): its code,
    /// its character when printable, and the modifiers.
    pub fn key_down(
        &mut self,
        code: i32,
        text: Option<char>,
        shift: bool,
        alt: bool,
        control: bool,
    ) {
        self.keyboard.key_code = code;
        self.keyboard.printable = text.is_some();
        self.keyboard.key = text.map(String::from).unwrap_or_default();
        self.keyboard.shift = shift;
        self.keyboard.alt = alt;
        self.keyboard.control = control;
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

    /// The cursor of an object (`CURSOR` of entities and actors).
    fn object_cursor(&self, id: NativeId) -> Option<String> {
        match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Actor) => self.actor(id).and_then(|a| a.def.cursor.clone()),
            Some(Kind::Entity(r)) => self
                .scene()
                .and_then(|v| v.entity(*r))
                .and_then(|e| e.cursor.clone()),
            Some(Kind::Control) => self.control(id).and_then(|c| c.cursor.clone()),
            _ => None,
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
            if self.cursors.active.is_some() {
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
        for sub in &frame.subframes {
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
        self.ui_hits_in_game(&mut hits);
        self.inventory_hits(&mut hits);
        self.ui_hits_normal(&mut hits);
        self.responses_hits(&mut hits);
        self.hits = hits;
    }

    /// The hit areas of the scene's objects, in drawing order.
    fn scene_hits(&self, hits: &mut Vec<HitArea>) {
        let Some(view) = &self.scene else {
            return;
        };
        let camera = self.camera();
        let actor_ids = self.actor_ids();
        let positions: Vec<(i32, i32)> = actor_ids
            .iter()
            .filter_map(|id| self.actor(*id))
            .filter(|a| a.def.active)
            .map(|a| (a.x, a.y))
            .collect();
        let active_actors: Vec<NativeId> = actor_ids
            .iter()
            .copied()
            .filter(|id| self.actor(*id).is_some_and(|a| a.def.active))
            .collect();
        for item in view.draw_order(&positions) {
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
                    if let Some(id) = active_actors.get(index) {
                        self.actor_hits(*id, layer_offset, hits);
                    }
                }
            }
        }
    }

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
        let Some(frame) = sprite
            .frames
            .get(frame.min(sprite.frames.len().saturating_sub(1)))
        else {
            return;
        };
        for sub in &frame.subframes {
            let Some(image) = &sub.image else { continue };
            if sub.decoration {
                continue;
            }
            let src = sub
                .rect
                .filter(|r| !r.is_empty())
                .map(|r| (r.left, r.top, r.width() as i32, r.height() as i32));
            // The size is known from the sub-frame's rectangle; without
            // one, from the image, read at hit time.
            let (w, h) = src.map_or((0, 0), |(_, _, w, h)| (w, h));
            let zoom = scale / 100.0;
            let dst = if (scale - 100.0).abs() < f32::EPSILON {
                (
                    x - sub.hotspot_x + src.map_or(0, |s| s.0),
                    y - sub.hotspot_y + src.map_or(0, |s| s.1),
                    w,
                    h,
                )
            } else {
                (
                    (x as f32 - (sub.hotspot_x + src.map_or(0, |s| s.0)) as f32 * zoom) as i32,
                    (y as f32 - (sub.hotspot_y + src.map_or(0, |s| s.1)) as f32 * zoom) as i32,
                    (w as f32 * zoom) as i32,
                    (h as f32 * zoom) as i32,
                )
            };
            hits.push(HitArea {
                owner,
                layer_offset,
                shape: HitShape::Pixels {
                    bank,
                    image: image.clone(),
                    color_key: sub.color_key,
                    dst,
                    src,
                    zoom: scale,
                    mirror_x: sub.mirror_x,
                    mirror_y: sub.mirror_y,
                    precise: sprite.precise,
                },
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
        let Some(view) = &self.scene else { return };
        let Some(entity) = view.entity(r) else { return };
        if !entity.active || !entity.interactive {
            return;
        }
        if entity.ignore_items && self.selected_item.is_some() {
            return;
        }
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
            });
        }
        let Some(path) = view.shown_sprite(r) else {
            return;
        };
        let Some(loaded) = view.sprite(&path) else {
            return;
        };
        World::sprite_hits(
            id,
            HitBank::Scene,
            &loaded.sprite,
            view.frame_of(r),
            entity.x,
            entity.y,
            view.scene.entity_scale(entity),
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
        let Some(path) = actor.current_sprite() else {
            return;
        };
        let Some(loaded) = actor.bank.sprite(path) else {
            return;
        };
        World::sprite_hits(
            id,
            HitBank::Actor(id),
            &loaded.sprite,
            actor.frame_of(path),
            actor.x,
            actor.y,
            actor.scale(&view.scene),
            layer_offset,
            hits,
        );
    }

    /// The object at a point of the screen (`CBRenderer::GetObjectAt`):
    /// the last hit area drawn that contains it, by its polygon or by an
    /// opaque pixel of its sprite.
    pub fn object_at(&mut self, x: i32, y: i32) -> Option<NativeId> {
        let hits = std::mem::take(&mut self.hits);
        let mut found = None;
        for hit in hits.iter().rev() {
            let (lx, ly) = (x + hit.layer_offset.0, y + hit.layer_offset.1);
            let inside = match &hit.shape {
                HitShape::Polygon(points) => crate::path::polygon_contains(points, lx, ly),
                HitShape::Rect(rx, ry, rw, rh) => {
                    lx >= *rx && ly >= *ry && lx < rx + rw && ly < ry + rh
                }
                HitShape::Pixels {
                    bank,
                    image,
                    color_key,
                    dst,
                    src,
                    zoom,
                    mirror_x,
                    mirror_y,
                    precise,
                } => {
                    let mask = match bank {
                        HitBank::Scene => self
                            .scene
                            .as_mut()
                            .and_then(|v| v.bank.mask(&self.vfs, image, *color_key).cloned()),
                        HitBank::Actor(actor) => self
                            .actor_and_vfs(*actor)
                            .and_then(|(vfs, a)| a.bank.mask(vfs, image, *color_key).cloned()),
                        HitBank::Ui => self.ui.bank.mask(&self.vfs, image, *color_key).cloned(),
                    };
                    let Some(mask) = mask else { continue };
                    let (sx, sy, sw, sh) =
                        src.unwrap_or((0, 0, mask.width as i32, mask.height as i32));
                    let (dx, dy, mut dw, mut dh) = *dst;
                    if dw == 0 || dh == 0 {
                        let z = zoom / 100.0;
                        dw = (sw as f32 * z) as i32;
                        dh = (sh as f32 * z) as i32;
                    }
                    if lx < dx || ly < dy || lx >= dx + dw || ly >= dy + dh {
                        false
                    } else if !*precise {
                        true
                    } else {
                        let z = zoom / 100.0;
                        let mut xx = ((sx + lx - dx) as f32 / z) as i32;
                        let mut yy = ((sy + ly - dy) as f32 / z) as i32;
                        if *mirror_x {
                            xx = sw - xx;
                        }
                        if *mirror_y {
                            yy = sh - yy;
                        }
                        mask.is_opaque(xx, yy)
                    }
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
}
