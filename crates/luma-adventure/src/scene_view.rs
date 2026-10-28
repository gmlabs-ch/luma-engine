//! A scene loaded with its sprites and the animation state of its
//! entities, composed into a draw list the way the engine draws it (see
//! `docs/formats/scene.md`, section "Drawing"). Textures are uploaded
//! separately, so that a scene can be loaded and run without a GPU.
//!
//! Written with reference to `BSprite.cpp` and `AdScene.cpp` of WME Lite
//! (MIT License, Copyright (c) 2011 Jan Nedoma); see `NOTICE`.

use std::collections::HashMap;

use luma_archive::Vfs;
use luma_formats::scene::{Entity, EntityKind, Layer, Region, SceneNode};
use luma_formats::{Rgb, Rgba, Scene, Sprite, SubFrame};
use luma_render::{DrawList, Quad, Rect, Renderer, Texture};

use crate::error::{Error, Result};
use crate::talk::{Sentence, SubtitleMods};

/// A sprite definition loaded from the game.
#[derive(Debug)]
pub struct LoadedSprite {
    /// The sprite definition.
    pub sprite: Sprite,
}

/// The part of the scene shown on screen: the size of the view and the
/// scene's scroll offset, in scene pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Width of the view (the game's resolution).
    pub width: f32,
    /// Height of the view.
    pub height: f32,
    /// Scroll offset of the main layer.
    pub offset_x: f32,
    /// See `offset_x`.
    pub offset_y: f32,
}

impl Camera {
    /// A camera showing the top-left corner of the scene.
    pub fn new(width: f32, height: f32) -> Camera {
        Camera {
            width,
            height,
            offset_x: 0.0,
            offset_y: 0.0,
        }
    }

    /// Clamps the offset so that the view stays inside the scene.
    pub fn clamp_to(&mut self, scene: &Scene) {
        let max_x = (scene.width() as f32 - self.width).max(0.0);
        let max_y = (scene.height() as f32 - self.height).max(0.0);
        self.offset_x = self.offset_x.clamp(0.0, max_x);
        self.offset_y = self.offset_y.clamp(0.0, max_y);
    }

    /// The offset of a layer, scrolled in proportion to its size
    /// (parallax): a layer as wide as the scene scrolls by `offset_x`.
    pub fn layer_offset(&self, scene: &Scene, layer: &Layer) -> (f32, f32) {
        let ratio = |offset: f32, scene_size: i32, view: f32| {
            let scrollable = scene_size as f32 - view;
            if scrollable <= 0.0 {
                0.0
            } else {
                offset / scrollable
            }
        };
        let rx = ratio(self.offset_x, scene.width(), self.width);
        let ry = ratio(self.offset_y, scene.height(), self.height);
        (
            (rx * (layer.width as f32 - self.width)).trunc(),
            (ry * (layer.height as f32 - self.height)).trunc(),
        )
    }
}

/// Identifies an entity of a scene: a node of a layer, or a free entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityRef {
    /// The node `index` of layer `layer`.
    Node {
        /// The layer's index.
        layer: usize,
        /// The node's index in the layer.
        index: usize,
    },
    /// A free entity of the scene, by index.
    Free(usize),
}

/// The animation state of a sprite shown by an object, advanced like the
/// engine's `CBSprite::GetCurrentFrame`: a frame lasts its delay from the
/// moment the previous one was shown; a non-looping sprite stops on its
/// last frame and is then `finished`.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimState {
    /// The sprite path this state belongs to (normalised).
    pub sprite: String,
    /// The frame shown.
    pub current_frame: usize,
    /// When the current frame started, on the game timer; 0 before the
    /// first update.
    pub last_frame_time: u64,
    /// Whether a non-looping sprite has played to its end.
    pub finished: bool,
    /// Whether the animation is paused.
    pub paused: bool,
    /// A `Looping` set by a script, overriding the sprite's.
    pub looping: Option<bool>,
    /// Whether the last [`AnimState::step`] showed a new frame.
    pub changed: bool,
    /// The `MOVE` offset of the frame that became current at the last
    /// change.
    pub move_x: i32,
    /// See `move_x`.
    pub move_y: i32,
}

impl AnimState {
    /// A state at the first frame of a sprite path.
    pub fn new(sprite: &str) -> AnimState {
        AnimState {
            sprite: key(sprite),
            current_frame: 0,
            last_frame_time: 0,
            finished: false,
            paused: false,
            looping: None,
            changed: false,
            move_x: 0,
            move_y: 0,
        }
    }

    /// Back to the first frame (`CBSprite::Reset`).
    pub fn reset(&mut self) {
        self.current_frame = 0;
        self.last_frame_time = 0;
        self.finished = false;
        self.move_x = 0;
        self.move_y = 0;
    }

    /// Advances the state to `timer` (the game timer, in milliseconds)
    /// for the sprite it belongs to, and returns whether a new frame
    /// became current, in which case `move_x` and `move_y` hold that
    /// frame's `MOVE` (`CBSprite::GetCurrentFrame`).
    pub fn step(&mut self, sprite: &Sprite, timer: u64) -> bool {
        let frames = sprite.frames.len();
        if frames == 0 {
            self.changed = false;
            return false;
        }
        let looping = self.looping.unwrap_or(sprite.looping);
        self.current_frame = self.current_frame.min(frames - 1);
        let last = self.current_frame;
        let delay = u64::from(sprite.frames[self.current_frame].delay_ms);
        if !self.paused
            && !self.finished
            && self.last_frame_time != 0
            && timer >= self.last_frame_time + delay
        {
            if self.current_frame < frames - 1 {
                self.current_frame += 1;
            } else if looping {
                self.current_frame = 0;
            } else {
                self.finished = true;
            }
            self.last_frame_time = timer;
        }
        self.changed = last != self.current_frame || (looping && frames == 1);
        if self.last_frame_time == 0 {
            self.last_frame_time = timer;
            self.changed = true;
        }
        if self.changed {
            self.move_x = sprite.frames[self.current_frame].move_x;
            self.move_y = sprite.frames[self.current_frame].move_y;
        }
        self.changed
    }
}

/// Sprites loaded from the game and the textures of their images, keyed
/// by path without regard to case or separator.
#[derive(Debug, Default)]
pub struct SpriteBank {
    sprites: HashMap<String, LoadedSprite>,
    textures: HashMap<String, Texture>,
    missing: Vec<(String, String)>,
}

/// Case-insensitive key of a game path.
pub(crate) fn key(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

fn texture_key(path: &str, color_key: Rgb) -> String {
    format!(
        "{}|{},{},{}",
        key(path),
        color_key.r,
        color_key.g,
        color_key.b
    )
}

impl SpriteBank {
    /// An empty bank.
    pub fn new() -> SpriteBank {
        SpriteBank::default()
    }

    /// Loads a sprite definition if it is not loaded yet. A missing or
    /// broken sprite is recorded in [`SpriteBank::missing`], not an
    /// error. Returns whether the sprite is available.
    pub fn add_sprite(&mut self, vfs: &Vfs, path: &str) -> bool {
        let k = key(path);
        if self.sprites.contains_key(&k) {
            return true;
        }
        if self.missing.iter().any(|(p, _)| key(p) == k) {
            return false;
        }
        let sprite = if Sprite::is_image_path(path) {
            Sprite::from_image(path)
        } else {
            match vfs
                .read(path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| Sprite::parse(&bytes).map_err(|e| e.to_string()))
            {
                Ok(sprite) => sprite,
                Err(reason) => {
                    self.missing.push((path.to_string(), reason));
                    return false;
                }
            }
        };
        self.sprites.insert(k, LoadedSprite { sprite });
        true
    }

    /// Decodes and uploads the images of every loaded sprite that has no
    /// texture yet. Safe to call every frame.
    pub fn upload(&mut self, vfs: &Vfs, renderer: &Renderer) {
        let images: Vec<(String, Rgb)> = self
            .sprites
            .values()
            .flat_map(|s| s.sprite.frames.iter())
            .flat_map(|f| f.subframes.iter())
            .filter_map(|sub| sub.image.as_ref().map(|i| (i.clone(), sub.color_key)))
            .collect();
        for (image, color_key) in images {
            let k = texture_key(&image, color_key);
            if self.textures.contains_key(&k) || self.missing.iter().any(|(p, _)| *p == image) {
                continue;
            }
            let decoded = vfs
                .read(&image)
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    luma_formats::image::decode(&bytes, color_key).map_err(|e| e.to_string())
                });
            match decoded {
                Ok(decoded) => {
                    self.textures.insert(k, renderer.texture(&image, &decoded));
                }
                Err(reason) => self.missing.push((image, reason)),
            }
        }
    }

    /// Adds an already parsed sprite under a path, replacing any loaded
    /// one.
    pub fn insert(&mut self, path: &str, sprite: Sprite) {
        self.sprites.insert(key(path), LoadedSprite { sprite });
    }

    /// The loaded sprite of a path, if it loaded.
    pub fn sprite(&self, path: &str) -> Option<&LoadedSprite> {
        self.sprites.get(&key(path))
    }

    /// The texture of an image, if it was uploaded.
    pub fn texture(&self, path: &str, color_key: Rgb) -> Option<&Texture> {
        self.textures.get(&texture_key(path, color_key))
    }

    /// Number of textures uploaded.
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// Files that could not be loaded, with the reason.
    pub fn missing(&self) -> &[(String, String)] {
        &self.missing
    }
}

/// An object drawn among the free objects of a scene: an actor, or an
/// entity of another bank.
pub struct Drawable<'a> {
    /// Position of the hotspot.
    pub x: i32,
    /// See `x`.
    pub y: i32,
    /// The sprite shown, from `bank`.
    pub sprite: Option<&'a LoadedSprite>,
    /// Where the sprite's textures are.
    pub bank: &'a SpriteBank,
    /// The frame shown.
    pub frame: usize,
    /// Scale in percent.
    pub scale: f32,
    /// Tint and opacity.
    pub tint: Rgba,
}

/// An entity saying a sentence.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityTalk {
    /// The sentence.
    pub sentence: Sentence,
    /// The talk sprite being played.
    pub sprite: Option<String>,
    /// Whether the sentence is shown this frame.
    pub show: bool,
}

/// A scene and everything needed to run and draw it.
#[derive(Debug)]
pub struct SceneView {
    /// The scene definition, which scripts modify.
    pub scene: Scene,
    /// Path the scene was loaded from.
    pub path: String,
    /// The sprites of the scene's entities.
    pub bank: SpriteBank,
    anims: HashMap<EntityRef, AnimState>,
    talks: HashMap<EntityRef, EntityTalk>,
    /// The subtitle placement of entities scripts changed it on.
    pub subtitles: HashMap<EntityRef, SubtitleMods>,
}

impl SceneView {
    /// Loads a scene file from the game and the sprites of its entities.
    /// A missing or broken sprite is recorded in [`SceneView::missing`],
    /// not an error. Images are decoded and uploaded later by
    /// [`SceneView::upload`].
    pub fn load(vfs: &Vfs, path: &str) -> Result<SceneView> {
        let bytes = vfs.read(path)?;
        let scene = Scene::parse(&bytes).map_err(|source| Error::Format {
            path: path.to_string(),
            source,
        })?;
        let mut view = SceneView {
            scene,
            path: path.to_string(),
            bank: SpriteBank::new(),
            anims: HashMap::new(),
            talks: HashMap::new(),
            subtitles: HashMap::new(),
        };
        let paths: Vec<String> = view
            .scene
            .entities()
            .filter(|e| e.kind == EntityKind::Normal)
            .filter_map(|e| e.sprite.clone())
            .collect();
        for sprite_path in paths {
            view.bank.add_sprite(vfs, &sprite_path);
        }
        Ok(view)
    }

    /// Loads a sprite definition if it is not loaded yet, for an entity
    /// whose sprite a script changed. Returns whether the sprite is
    /// available.
    pub fn add_sprite(&mut self, vfs: &Vfs, path: &str) -> bool {
        self.bank.add_sprite(vfs, path)
    }

    /// Decodes and uploads the images of every loaded sprite that has no
    /// texture yet. Safe to call every frame.
    pub fn upload(&mut self, vfs: &Vfs, renderer: &Renderer) {
        self.bank.upload(vfs, renderer);
    }

    /// The loaded sprite of a path, if it loaded.
    pub fn sprite(&self, path: &str) -> Option<&LoadedSprite> {
        self.bank.sprite(path)
    }

    /// The texture of an image, if it was uploaded.
    pub fn texture(&self, path: &str, color_key: Rgb) -> Option<&Texture> {
        self.bank.texture(path, color_key)
    }

    /// Number of textures uploaded.
    pub fn texture_count(&self) -> usize {
        self.bank.texture_count()
    }

    /// Files referenced by the scene that could not be loaded, with the
    /// reason; the scene is still shown without them.
    pub fn missing(&self) -> &[(String, String)] {
        self.bank.missing()
    }

    /// Every entity of the scene: layer nodes in order, then free ones.
    pub fn entity_refs(&self) -> Vec<EntityRef> {
        let mut refs = Vec::new();
        for (layer, l) in self.scene.layers.iter().enumerate() {
            for (index, node) in l.nodes.iter().enumerate() {
                if matches!(node, SceneNode::Entity(_)) {
                    refs.push(EntityRef::Node { layer, index });
                }
            }
        }
        refs.extend((0..self.scene.free_entities.len()).map(EntityRef::Free));
        refs
    }

    /// A node of a layer.
    pub fn node(&self, layer: usize, index: usize) -> Option<&SceneNode> {
        self.scene.layers.get(layer)?.nodes.get(index)
    }

    /// An entity.
    pub fn entity(&self, r: EntityRef) -> Option<&Entity> {
        match r {
            EntityRef::Node { layer, index } => match self.node(layer, index)? {
                SceneNode::Entity(e) => Some(e),
                SceneNode::Region(_) => None,
            },
            EntityRef::Free(i) => self.scene.free_entities.get(i),
        }
    }

    /// An entity, mutable.
    pub fn entity_mut(&mut self, r: EntityRef) -> Option<&mut Entity> {
        match r {
            EntityRef::Node { layer, index } => {
                match self.scene.layers.get_mut(layer)?.nodes.get_mut(index)? {
                    SceneNode::Entity(e) => Some(e),
                    SceneNode::Region(_) => None,
                }
            }
            EntityRef::Free(i) => self.scene.free_entities.get_mut(i),
        }
    }

    /// A region node of a layer.
    pub fn region(&self, layer: usize, index: usize) -> Option<&Region> {
        match self.node(layer, index)? {
            SceneNode::Region(r) => Some(r),
            SceneNode::Entity(_) => None,
        }
    }

    /// A region node of a layer, mutable.
    pub fn region_mut(&mut self, layer: usize, index: usize) -> Option<&mut Region> {
        match self.scene.layers.get_mut(layer)?.nodes.get_mut(index)? {
            SceneNode::Region(r) => Some(r),
            SceneNode::Entity(_) => None,
        }
    }

    /// The first layer node (entity or region) with this name, ignoring
    /// case, as `(layer, index)`.
    pub fn find_node(&self, name: &str) -> Option<(usize, usize)> {
        for (li, layer) in self.scene.layers.iter().enumerate() {
            for (ni, node) in layer.nodes.iter().enumerate() {
                let node_name = match node {
                    SceneNode::Entity(e) => &e.name,
                    SceneNode::Region(r) => &r.name,
                };
                if node_name.eq_ignore_ascii_case(name) {
                    return Some((li, ni));
                }
            }
        }
        None
    }

    /// The first free entity with this name, ignoring case.
    pub fn find_free(&self, name: &str) -> Option<usize> {
        self.scene
            .free_entities
            .iter()
            .position(|e| e.name.eq_ignore_ascii_case(name))
    }

    /// The regions of the main layer containing a point, last node
    /// first, up to the engine's limit of nine (`GetRegionsAt`).
    pub fn regions_at(&self, x: i32, y: i32) -> Vec<(usize, usize)> {
        regions_at(&self.scene, x, y)
    }

    /// The animation state of an entity, if it has been shown.
    pub fn anim(&self, r: EntityRef) -> Option<&AnimState> {
        self.anims.get(&r)
    }

    /// The sprite an entity shows: its talk sprite while talking, else
    /// its own.
    pub fn shown_sprite(&self, r: EntityRef) -> Option<String> {
        if let Some(talk) = self.talks.get(&r) {
            if let Some(sprite) = &talk.sprite {
                return Some(sprite.clone());
            }
        }
        self.entity(r)?.sprite.clone()
    }

    /// Starts a sentence on an entity (`CAdObject::Talk`).
    pub fn talk(&mut self, r: EntityRef, sentence: Sentence) {
        self.talks.insert(
            r,
            EntityTalk {
                sentence,
                sprite: None,
                show: false,
            },
        );
    }

    /// Ends an entity's sentence (`StopTalk`); whether one was playing.
    pub fn stop_talk(&mut self, r: EntityRef) -> bool {
        self.talks.remove(&r).is_some()
    }

    /// Whether an entity is saying a sentence.
    pub fn is_talking(&self, r: EntityRef) -> bool {
        self.talks.contains_key(&r)
    }

    /// The sentences shown this frame, with their entities.
    pub fn sentences(&self) -> impl Iterator<Item = (EntityRef, &Sentence)> {
        self.talks
            .iter()
            .filter(|(_, t)| t.show)
            .map(|(r, t)| (*r, &t.sentence))
    }

    /// Ends every sentence older than 300 ms (`FinishSentences`).
    pub fn skip_sentences(&mut self, timer: u64) {
        for talk in self.talks.values_mut() {
            if talk.sentence.can_skip(timer) {
                talk.sentence.finish();
            }
        }
    }

    /// The talk sprite of an entity for a stance
    /// (`CAdTalkHolder::GetTalkStance`): the special or generic sprite of
    /// that name, else a random generic one, else the entity's sprite.
    fn talk_stance(&self, r: EntityRef, stance: Option<&str>, random: u32) -> Option<String> {
        let entity = self.entity(r)?;
        if let Some(name) = stance {
            let found = entity
                .talk_special_sprites
                .iter()
                .chain(&entity.talk_sprites)
                .find(|s| sprite_name(s).eq_ignore_ascii_case(name));
            if let Some(path) = found {
                return Some(path.clone());
            }
        }
        if entity.talk_sprites.is_empty() {
            entity.sprite.clone()
        } else {
            Some(entity.talk_sprites[random as usize % entity.talk_sprites.len()].clone())
        }
    }

    /// One frame of each talking entity (`CAdEntity::Update`,
    /// `STATE_TALKING`).
    fn talk_step(&mut self, timer: u64, random: u32) {
        let talking: Vec<EntityRef> = self.talks.keys().copied().collect();
        for r in talking {
            let (time_is_up, sprite_done) = {
                let talk = &self.talks[&r];
                let done = talk.sprite.as_deref().is_none_or(|p| {
                    self.anims
                        .get(&r)
                        .is_some_and(|a| a.sprite == key(p) && a.finished)
                });
                (talk.sentence.time_is_up(timer), done)
            };
            if time_is_up {
                self.talks.remove(&r);
                continue;
            }
            if sprite_done {
                let stance = self
                    .talks
                    .get_mut(&r)
                    .and_then(|t| t.sentence.next_stance());
                let sprite = self.talk_stance(r, stance.as_deref(), random);
                if let Some(talk) = self.talks.get_mut(&r) {
                    talk.sprite = sprite.clone();
                    talk.show = true;
                }
                if let Some(path) = sprite {
                    let state = self.anims.entry(r).or_insert_with(|| AnimState::new(&path));
                    *state = AnimState::new(&path);
                }
            } else if let Some(talk) = self.talks.get_mut(&r) {
                talk.show = true;
            }
        }
    }

    /// The animation state of an entity, created for its current sprite.
    pub fn anim_mut(&mut self, r: EntityRef) -> Option<&mut AnimState> {
        let sprite = key(&self.shown_sprite(r)?);
        let state = self
            .anims
            .entry(r)
            .or_insert_with(|| AnimState::new(&sprite));
        if state.sprite != sprite {
            *state = AnimState::new(&sprite);
        }
        Some(state)
    }

    /// Restarts an entity's animation (`Reset`).
    pub fn reset_anim(&mut self, r: EntityRef) {
        if let Some(state) = self.anim_mut(r) {
            state.reset();
        }
    }

    /// Advances the animations of the active entities to `timer` (the
    /// game timer, in milliseconds), applying the `MOVE` of each frame
    /// that becomes current to its entity.
    pub fn advance(&mut self, timer: u64) {
        self.advance_with(timer, 0);
    }

    /// Like [`SceneView::advance`], with a random number for the talk
    /// sprites of talking entities.
    pub fn advance_with(&mut self, timer: u64, random: u32) {
        self.talk_step(timer, random);
        for r in self.entity_refs() {
            let Some(entity) = self.entity(r) else {
                continue;
            };
            if !entity.active || entity.kind != EntityKind::Normal {
                continue;
            }
            let Some(sprite_key) = self.anim_mut(r).map(|s| s.sprite.clone()) else {
                continue;
            };
            let (changed, move_x, move_y) = {
                let Some(loaded) = self.bank.sprites.get(&sprite_key) else {
                    continue;
                };
                let Some(state) = self.anims.get_mut(&r) else {
                    continue;
                };
                let changed = state.step(&loaded.sprite, timer);
                (changed, state.move_x, state.move_y)
            };
            if changed && (move_x != 0 || move_y != 0) {
                if let Some(entity) = self.entity_mut(r) {
                    entity.x += move_x;
                    entity.y += move_y;
                }
            }
        }
    }

    /// The frame an entity shows.
    pub fn frame_of(&self, r: EntityRef) -> usize {
        self.anims.get(&r).map_or(0, |a| a.current_frame)
    }

    /// Fills a draw list with the scene as seen by `camera`, showing the
    /// frames the animations are at (see [`SceneView::advance`]).
    pub fn draw<'a>(&'a self, list: &mut DrawList<'a>, camera: &Camera) {
        self.draw_with(list, camera, &[]);
    }

    /// Like [`SceneView::draw`], with other objects (actors) drawn among
    /// the free entities: an object standing in a walkable region of the
    /// main layer is drawn right after that region, in `y` order with
    /// the others; the rest follow the main layer's nodes.
    pub fn draw_with<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        camera: &Camera,
        others: &[Drawable<'a>],
    ) {
        let scene = &self.scene;
        // Free objects: the others first, then the scene's free entities,
        // as the engine lists them.
        let mut objects: Vec<Drawable<'a>> = others
            .iter()
            .map(|d| Drawable {
                x: d.x,
                y: d.y,
                sprite: d.sprite,
                bank: d.bank,
                frame: d.frame,
                scale: d.scale,
                tint: d.tint,
            })
            .collect();
        for (i, e) in scene.free_entities.iter().enumerate() {
            if let Some(d) = self.drawable(EntityRef::Free(i), e) {
                objects.push(d);
            }
        }
        let mut drawn = vec![false; objects.len()];
        let mut draw_in = |list: &mut DrawList<'a>, region: Option<&Region>| {
            let mut inside: Vec<usize> = (0..objects.len())
                .filter(|&i| {
                    !drawn[i] && region.is_none_or(|r| r.contains(objects[i].x, objects[i].y))
                })
                .collect();
            inside.sort_by_key(|&i| objects[i].y);
            for i in inside {
                drawn[i] = true;
                draw_object(list, &objects[i]);
            }
        };
        for (li, layer) in scene.layers.iter().enumerate() {
            if !layer.active {
                continue;
            }
            let (ox, oy) = camera.layer_offset(scene, layer);
            list.set_offset(-ox, -oy);
            for (ni, node) in layer.nodes.iter().enumerate() {
                match node {
                    SceneNode::Entity(entity) => {
                        let r = EntityRef::Node {
                            layer: li,
                            index: ni,
                        };
                        if let Some(d) = self.drawable(r, entity) {
                            draw_object(list, &d);
                        }
                    }
                    SceneNode::Region(region) => {
                        if region.blocked || region.decoration || !region.active {
                            continue;
                        }
                        draw_in(list, Some(region));
                    }
                }
            }
            if layer.main {
                draw_in(list, None);
            }
        }
        list.set_offset(0.0, 0.0);
    }

    fn drawable<'a>(&'a self, r: EntityRef, entity: &'a Entity) -> Option<Drawable<'a>> {
        if !entity.active || entity.kind != EntityKind::Normal {
            return None;
        }
        let loaded = self.shown_sprite(r).and_then(|p| self.sprite(&p))?;
        if loaded.sprite.frames.is_empty() {
            return None;
        }
        Some(Drawable {
            x: entity.x,
            y: entity.y,
            sprite: Some(loaded),
            bank: &self.bank,
            frame: self.frame_of(r),
            scale: self.scene.entity_scale(entity),
            tint: self.scene.entity_tint(entity),
        })
    }
}

/// The name of a sprite file, for talk stances: its file name without
/// directory and extension.
fn sprite_name(path: &str) -> &str {
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    file.rsplit_once('.').map_or(file, |(name, _)| name)
}

/// The regions of a scene's main layer containing a point, as
/// `(layer, node)` pairs, last node first, up to the engine's limit of
/// nine (`CAdScene::GetRegionsAt`).
pub fn regions_at(scene: &Scene, x: i32, y: i32) -> Vec<(usize, usize)> {
    let Some(main) = scene.layers.iter().rposition(|l| l.main) else {
        return Vec::new();
    };
    scene.layers[main]
        .nodes
        .iter()
        .enumerate()
        .rev()
        .filter_map(|(i, node)| match node {
            SceneNode::Region(r) if r.active && r.contains(x, y) => Some((main, i)),
            _ => None,
        })
        .take(9)
        .collect()
}

/// Draws an object's current frame.
pub fn draw_object<'a>(list: &mut DrawList<'a>, d: &Drawable<'a>) {
    let Some(loaded) = d.sprite else { return };
    if loaded.sprite.frames.is_empty() {
        return;
    }
    let frame = d.frame.min(loaded.sprite.frames.len() - 1);
    for sub in &loaded.sprite.frames[frame].subframes {
        let Some(image) = &sub.image else { continue };
        let Some(texture) = d.bank.texture(image, sub.color_key) else {
            continue;
        };
        list.push(place(sub, texture, d.x, d.y, d.scale / 100.0, d.tint));
    }
}

/// The height of a sprite's frame as the engine measures it
/// (`CAdObject::GetHeight`): the largest hotspot `y` of its sub-frames,
/// scaled by `zoom` percent.
pub fn frame_height(sprite: &Sprite, frame: usize, zoom: f32) -> i32 {
    let Some(frame) = sprite.frames.get(frame) else {
        return 0;
    };
    let height = frame
        .subframes
        .iter()
        .map(|s| s.hotspot_y)
        .max()
        .unwrap_or(0);
    (height as f32 * zoom / 100.0) as i32
}

/// Where and how a sub-frame is drawn for an owner at `(x, y)` with a
/// scale factor (`1.0` for 100 %) and the owner's tint: the hotspot is
/// placed at the position, scaled around it, and the sub-frame's own tint
/// replaces the owner's when it has one. Integer truncation matches the
/// engine.
pub fn place<'a>(
    sub: &SubFrame,
    texture: &'a Texture,
    x: i32,
    y: i32,
    scale: f32,
    owner_tint: Rgba,
) -> Quad<'a> {
    let src = sub.rect.filter(|r| !r.is_empty()).map(|r| {
        Rect::new(
            r.left as f32,
            r.top as f32,
            r.width() as f32,
            r.height() as f32,
        )
    });
    let (w, h) = match src {
        Some(r) => (r.w, r.h),
        None => (texture.width as f32, texture.height as f32),
    };
    let dst = if scale == 1.0 {
        Rect::new((x - sub.hotspot_x) as f32, (y - sub.hotspot_y) as f32, w, h)
    } else {
        Rect::new(
            (x as f32 - sub.hotspot_x as f32 * scale).trunc(),
            (y as f32 - sub.hotspot_y as f32 * scale).trunc(),
            (w * scale).trunc(),
            (h * scale).trunc(),
        )
    };
    Quad {
        texture,
        dst,
        src,
        tint: if sub.is_untinted() {
            owner_tint
        } else {
            sub.tint
        },
        flip_x: sub.mirror_x,
        flip_y: sub.mirror_y,
    }
}
