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
    /// `None` when its size is unknown or empty.
    pub fn center(&self, mask: Option<&Mask>) -> Option<(i32, i32)> {
        let (dx, dy, dw, dh) = self.rect(mask);
        if dw <= 0 || dh <= 0 {
            return None;
        }
        let (cx, cy) = (dx as f32 + dw as f32 / 2.0, dy as f32 + dh as f32 / 2.0);
        if self.rotation == 0.0 {
            return Some((cx as i32, cy as i32));
        }
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let (px, py) = self.pivot;
        let (ox, oy) = (cx - px, cy - py);
        Some((
            (px + ox * cos - oy * sin) as i32,
            (py + ox * sin + oy * cos) as i32,
        ))
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

    /// Moves the mouse to a point in game pixels, kept inside the lock
    /// rectangle when one applies (`CBGame::GetMousePos`).
    pub fn set_mouse(&mut self, x: i32, y: i32) {
        let (x, y) = match self.mouse_lock() {
            Some((left, top, right, bottom)) => (x.clamp(left, right), y.clamp(top, bottom)),
            None => (x, y),
        };
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
        World::sprite_hits_with(
            id,
            HitBank::Scene,
            &loaded.sprite,
            view.frame_of(r),
            (entity.x, entity.y),
            Transform::of_entity(&view.scene, entity),
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
                HitShape::Pixels {
                    bank,
                    image,
                    color_key,
                    area,
                } => {
                    // The size may come from the image, as in `object_at`.
                    let mask = self.hit_mask(*bank, image, *color_key);
                    let Some((cx, cy)) = area.center(mask.as_ref()) else {
                        continue;
                    };
                    found = Some((cx - hit.layer_offset.0, cy - hit.layer_offset.1));
                    break;
                }
            };
            if w <= 0 || h <= 0 {
                continue;
            }
            found = Some((
                x + w / 2 - hit.layer_offset.0,
                y + h / 2 - hit.layer_offset.1,
            ));
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
        }
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
