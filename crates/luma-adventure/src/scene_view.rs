//! A scene loaded with its sprites and the animation state of its
//! entities, composed into a draw list the way the engine draws it (see
//! `docs/formats/scene.md`, section "Drawing"). Textures are uploaded
//! separately, so that a scene can be loaded and run without a GPU.

use std::collections::HashMap;

use luma_archive::Vfs;
use luma_formats::scene::{Entity, EntityKind, Layer, Region, SceneNode};
use luma_formats::{Rgb, Rgba, Scene, Sprite, SubFrame};
use luma_render::{DrawList, Quad, Rect, Renderer, Texture};

use crate::error::{Error, Result};

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

/// The animation state of an entity's sprite, advanced like the engine's
/// `CBSprite::GetCurrentFrame`: a frame lasts its delay from the moment
/// the previous one was shown; a non-looping sprite stops on its last
/// frame and is then `finished`.
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
}

impl AnimState {
    fn new(sprite: String) -> AnimState {
        AnimState {
            sprite,
            current_frame: 0,
            last_frame_time: 0,
            finished: false,
            paused: false,
            looping: None,
        }
    }

    /// Back to the first frame (`CBSprite::Reset`).
    pub fn reset(&mut self) {
        self.current_frame = 0;
        self.last_frame_time = 0;
        self.finished = false;
    }
}

/// A scene and everything needed to run and draw it.
#[derive(Debug)]
pub struct SceneView {
    /// The scene definition, which scripts modify.
    pub scene: Scene,
    /// Path the scene was loaded from.
    pub path: String,
    sprites: HashMap<String, LoadedSprite>,
    textures: HashMap<String, Texture>,
    /// Files referenced by the scene that could not be loaded, with the
    /// reason; the scene is still shown without them.
    pub missing: Vec<(String, String)>,
    anims: HashMap<EntityRef, AnimState>,
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

impl SceneView {
    /// Loads a scene file from the game and the sprites of its entities.
    /// A missing or broken sprite is recorded in `missing`, not an error.
    /// Images are decoded and uploaded later by [`SceneView::upload`].
    pub fn load(vfs: &Vfs, path: &str) -> Result<SceneView> {
        let bytes = vfs.read(path)?;
        let scene = Scene::parse(&bytes).map_err(|source| Error::Format {
            path: path.to_string(),
            source,
        })?;
        let mut view = SceneView {
            scene,
            path: path.to_string(),
            sprites: HashMap::new(),
            textures: HashMap::new(),
            missing: Vec::new(),
            anims: HashMap::new(),
        };
        let paths: Vec<String> = view
            .scene
            .entities()
            .filter(|e| e.kind == EntityKind::Normal)
            .filter_map(|e| e.sprite.clone())
            .collect();
        for sprite_path in paths {
            view.add_sprite(vfs, &sprite_path);
        }
        Ok(view)
    }

    /// Loads a sprite definition if it is not loaded yet, for an entity
    /// whose sprite a script changed. Returns whether the sprite is
    /// available.
    pub fn add_sprite(&mut self, vfs: &Vfs, path: &str) -> bool {
        let k = key(path);
        if self.sprites.contains_key(&k) {
            return true;
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

    /// The animation state of an entity, if it has been shown.
    pub fn anim(&self, r: EntityRef) -> Option<&AnimState> {
        self.anims.get(&r)
    }

    /// The animation state of an entity, created for its current sprite.
    pub fn anim_mut(&mut self, r: EntityRef) -> Option<&mut AnimState> {
        let sprite = key(self.entity(r)?.sprite.as_deref()?);
        let state = self
            .anims
            .entry(r)
            .or_insert_with(|| AnimState::new(sprite.clone()));
        if state.sprite != sprite {
            *state = AnimState::new(sprite);
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
    /// game timer, in milliseconds).
    pub fn advance(&mut self, timer: u64) {
        for r in self.entity_refs() {
            let Some(entity) = self.entity(r) else {
                continue;
            };
            if !entity.active || entity.kind != EntityKind::Normal {
                continue;
            }
            let Some(path) = entity.sprite.clone() else {
                continue;
            };
            let (frames, looping, delay_of): (usize, bool, Vec<u64>) = match self.sprite(&path) {
                Some(loaded) => (
                    loaded.sprite.frames.len(),
                    loaded.sprite.looping,
                    loaded
                        .sprite
                        .frames
                        .iter()
                        .map(|f| u64::from(f.delay_ms))
                        .collect(),
                ),
                None => continue,
            };
            if frames == 0 {
                continue;
            }
            let Some(state) = self.anim_mut(r) else {
                continue;
            };
            let looping = state.looping.unwrap_or(looping);
            state.current_frame = state.current_frame.min(frames - 1);
            if !state.paused
                && !state.finished
                && state.last_frame_time != 0
                && timer >= state.last_frame_time + delay_of[state.current_frame]
            {
                if state.current_frame < frames - 1 {
                    state.current_frame += 1;
                } else if looping {
                    state.current_frame = 0;
                } else {
                    state.finished = true;
                }
                state.last_frame_time = timer;
            }
            if state.last_frame_time == 0 {
                state.last_frame_time = timer;
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
        let scene = &self.scene;
        let mut drawn = vec![false; scene.free_entities.len()];
        for (li, layer) in scene.layers.iter().enumerate() {
            if !layer.active {
                continue;
            }
            let (ox, oy) = camera.layer_offset(scene, layer);
            list.set_offset(-ox, -oy);
            for (ni, node) in layer.nodes.iter().enumerate() {
                match node {
                    SceneNode::Entity(entity) => self.draw_entity(
                        list,
                        EntityRef::Node {
                            layer: li,
                            index: ni,
                        },
                        entity,
                    ),
                    SceneNode::Region(region) => {
                        if region.blocked || region.decoration || !region.active {
                            continue;
                        }
                        let mut inside: Vec<usize> = (0..drawn.len())
                            .filter(|&i| {
                                let e = &scene.free_entities[i];
                                !drawn[i] && e.active && region.contains(e.x, e.y)
                            })
                            .collect();
                        inside.sort_by_key(|&i| scene.free_entities[i].y);
                        for i in inside {
                            drawn[i] = true;
                            self.draw_entity(list, EntityRef::Free(i), &scene.free_entities[i]);
                        }
                    }
                }
            }
            if layer.main {
                let mut rest: Vec<usize> = (0..drawn.len())
                    .filter(|&i| !drawn[i] && scene.free_entities[i].active)
                    .collect();
                rest.sort_by_key(|&i| scene.free_entities[i].y);
                for i in rest {
                    drawn[i] = true;
                    self.draw_entity(list, EntityRef::Free(i), &scene.free_entities[i]);
                }
            }
        }
        list.set_offset(0.0, 0.0);
    }

    fn draw_entity<'a>(&'a self, list: &mut DrawList<'a>, r: EntityRef, entity: &Entity) {
        if !entity.active || entity.kind != EntityKind::Normal {
            return;
        }
        let Some(loaded) = entity.sprite.as_deref().and_then(|p| self.sprite(p)) else {
            return;
        };
        if loaded.sprite.frames.is_empty() {
            return;
        }
        let frame = self.frame_of(r).min(loaded.sprite.frames.len() - 1);
        let scale = self.scene.entity_scale(entity) / 100.0;
        let tint = self.scene.entity_tint(entity);
        for sub in &loaded.sprite.frames[frame].subframes {
            let Some(image) = &sub.image else { continue };
            let Some(texture) = self.texture(image, sub.color_key) else {
                continue;
            };
            list.push(place(sub, texture, entity.x, entity.y, scale, tint));
        }
    }
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
