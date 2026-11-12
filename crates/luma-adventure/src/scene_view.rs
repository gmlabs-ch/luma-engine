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
use luma_formats::{BlendMode, Frame, Rgb, Rgba, Scene, Sprite, SubFrame};
use luma_render::{Attributes, Draw3d, DrawList, Quad, Rect, Renderer, Texture};

use crate::error::{Error, Result};
use crate::talk::{Sentence, SubtitleMods};

/// A sprite definition loaded from the game.
#[derive(Debug)]
pub struct LoadedSprite {
    /// The sprite definition.
    pub sprite: Sprite,
    /// Whether a script changed the definition since it was loaded
    /// (frames added, a delay or a hotspot changed...). An edited sprite
    /// is written to a saved game instead of being read again from its
    /// file.
    pub edited: bool,
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
    /// Where the view's top-left corner lands on the screen: the scene
    /// viewport's origin (`SCENE_VIEWPORT`), usually 0.
    pub origin_x: f32,
    /// See `origin_x`.
    pub origin_y: f32,
}

impl Camera {
    /// A camera showing the top-left corner of the scene.
    pub fn new(width: f32, height: f32) -> Camera {
        Camera {
            width,
            height,
            offset_x: 0.0,
            offset_y: 0.0,
            origin_x: 0.0,
            origin_y: 0.0,
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
    /// The view's origin on the screen is taken off, so that the layer
    /// is drawn at `-offset` inside the viewport.
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
            (rx * (layer.width as f32 - self.width)).trunc() - self.origin_x,
            (ry * (layer.height as f32 - self.height)).trunc() - self.origin_y,
        )
    }
}

/// Identifies an entity of a scene: a node of a layer, or a free entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    /// An entity shown by a window's entity container, by index in the
    /// world's list; it lives outside the scene.
    Ui(usize),
    /// An entity of the game rather than of a scene (`Game.CreateEntity`,
    /// `Game.LoadEntity`, an attachment), by index in the game's own
    /// view; it survives scene changes.
    Game(usize),
}

/// The animation state of a sprite shown by an object, advanced like the
/// engine's `CBSprite::GetCurrentFrame`: a frame lasts its delay from the
/// moment the previous one was shown; a non-looping sprite stops on its
/// last frame and is then `finished`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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

/// The opaque pixels of an image, for hit tests (`IsTransparentAt`): a
/// pixel is opaque when its alpha is above 128.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    opaque: Vec<bool>,
}

impl Mask {
    /// The mask of a decoded image.
    pub fn of(image: &luma_formats::Image) -> Mask {
        Mask {
            width: image.width,
            height: image.height,
            opaque: image.pixels.chunks(4).map(|p| p[3] > 128).collect(),
        }
    }

    /// Whether the pixel is opaque; outside the image, it is not.
    pub fn is_opaque(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return false;
        }
        self.opaque[(y as u32 * self.width + x as u32) as usize]
    }
}

/// Sprites loaded from the game and the textures of their images, keyed
/// by path without regard to case or separator.
///
/// A saved game keeps only the sprites scripts edited (see
/// [`LoadedSprite::edited`]); the others are read again from their files
/// when the game loads, like the textures.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SpriteBank {
    #[serde(
        serialize_with = "serialize_edited",
        deserialize_with = "deserialize_edited"
    )]
    sprites: HashMap<String, LoadedSprite>,
    #[serde(skip)]
    textures: HashMap<String, Texture>,
    #[serde(skip)]
    masks: HashMap<String, Mask>,
    #[serde(skip)]
    missing: Vec<(String, String)>,
}

/// Writes the edited sprites of a bank, by key.
fn serialize_edited<S: serde::Serializer>(
    sprites: &HashMap<String, LoadedSprite>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    let mut edited: Vec<(&String, &Sprite)> = sprites
        .iter()
        .filter(|(_, s)| s.edited)
        .map(|(k, s)| (k, &s.sprite))
        .collect();
    edited.sort_by(|a, b| a.0.cmp(b.0));
    serde::Serialize::serialize(&edited, serializer)
}

/// Reads the edited sprites of a bank written by [`serialize_edited`].
fn deserialize_edited<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<HashMap<String, LoadedSprite>, D::Error> {
    let edited: Vec<(String, Sprite)> = serde::Deserialize::deserialize(deserializer)?;
    Ok(edited
        .into_iter()
        .map(|(k, sprite)| {
            (
                k,
                LoadedSprite {
                    sprite,
                    edited: true,
                },
            )
        })
        .collect())
}

/// Case-insensitive key of a game path.
pub(crate) fn key(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

/// The file an image is read from: itself, or the engine's placeholder
/// when it does not exist (`CBSurfaceStorage::AddSurface`).
fn placeholder_for<'a>(vfs: &Vfs, image: &'a str) -> &'a str {
    if !vfs.exists(image) && vfs.exists(crate::resources::INVALID_IMAGE) {
        crate::resources::INVALID_IMAGE
    } else {
        image
    }
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

    /// Loads a sprite definition if it is not loaded yet
    /// (`CBSprite::LoadFile`). A file that does not exist is replaced by
    /// the engine's placeholder image and the sprite loads; the miss is
    /// recorded in [`SpriteBank::missing`] all the same, for the log. A
    /// sprite file that cannot be parsed is recorded there too, and is
    /// not available. Returns whether the sprite is available.
    pub fn add_sprite(&mut self, vfs: &Vfs, path: &str) -> bool {
        let k = key(path);
        if self.sprites.contains_key(&k) {
            return true;
        }
        if self.missing.iter().any(|(p, _)| key(p) == k) {
            return false;
        }
        let sprite = if !vfs.exists(path) && vfs.exists(crate::resources::INVALID_IMAGE) {
            self.missing.push((
                path.to_string(),
                "file not found, the placeholder image is shown".to_string(),
            ));
            Sprite::from_image(crate::resources::INVALID_IMAGE)
        } else if Sprite::is_image_path(path) || crate::save::is_thumbnail_path(path) {
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
        self.sprites.insert(
            k,
            LoadedSprite {
                sprite,
                edited: false,
            },
        );
        true
    }

    /// Looks again at the images whose name starts with `prefix` (the
    /// thumbnails of saved games, `savegame:<slot>`, after a slot was
    /// written or emptied): a sprite made of such an image shows it
    /// when it exists now and the placeholder when it does not, and the
    /// textures made of them are returned, to be released; the next
    /// upload decodes the images again.
    pub(crate) fn refresh_prefixed(&mut self, vfs: &Vfs, prefix: &str) -> Vec<Texture> {
        let prefix = key(prefix);
        for (k, loaded) in &mut self.sprites {
            if !k.starts_with(&prefix) || loaded.edited {
                continue;
            }
            if vfs.exists(k) {
                loaded.sprite = Sprite::from_image(k.clone());
            } else if vfs.exists(crate::resources::INVALID_IMAGE) {
                loaded.sprite = Sprite::from_image(crate::resources::INVALID_IMAGE);
            }
        }
        self.missing
            .retain(|(path, _)| !key(path).starts_with(&prefix));
        self.masks.retain(|k, _| !k.starts_with(&prefix));
        let stale: Vec<String> = self
            .textures
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        stale
            .into_iter()
            .filter_map(|k| self.textures.remove(&k))
            .collect()
    }

    /// Whether a loaded sprite has this `NAME` (the key of its `.sprite`
    /// file), without regard to case. A sprite without a name, or one
    /// that is not loaded, has none.
    pub fn sprite_name_is(&self, path: &str, name: &str) -> bool {
        self.sprites.get(&key(path)).is_some_and(|loaded| {
            !loaded.sprite.name.is_empty() && loaded.sprite.name.eq_ignore_ascii_case(name)
        })
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
                .read(placeholder_for(vfs, &image))
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
        self.sprites.insert(
            key(path),
            LoadedSprite {
                sprite,
                edited: false,
            },
        );
    }

    /// The loaded sprite of a path, if it loaded.
    pub fn sprite(&self, path: &str) -> Option<&LoadedSprite> {
        self.sprites.get(&key(path))
    }

    /// The definition of a loaded sprite, for a script to change it; the
    /// sprite is then kept in saved games (see [`LoadedSprite::edited`]).
    pub fn edit_sprite(&mut self, path: &str) -> Option<&mut Sprite> {
        let loaded = self.sprites.get_mut(&key(path))?;
        loaded.edited = true;
        Some(&mut loaded.sprite)
    }

    /// The opaque pixels of an image, decoded on first request (no GPU
    /// needed); `None` when the image cannot be decoded.
    pub fn mask(&mut self, vfs: &Vfs, path: &str, color_key: Rgb) -> Option<&Mask> {
        let k = texture_key(path, color_key);
        if !self.masks.contains_key(&k) {
            if self.missing.iter().any(|(p, _)| *p == path) {
                return None;
            }
            let decoded = vfs
                .read(placeholder_for(vfs, path))
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    luma_formats::image::decode(&bytes, color_key).map_err(|e| e.to_string())
                });
            match decoded {
                Ok(image) => {
                    self.masks.insert(k.clone(), Mask::of(&image));
                }
                Err(reason) => {
                    self.missing.push((path.to_string(), reason));
                    return None;
                }
            }
        }
        self.masks.get(&k)
    }

    /// The opaque pixels of an image, when already decoded.
    pub fn mask_if_loaded(&self, path: &str, color_key: Rgb) -> Option<&Mask> {
        self.masks.get(&texture_key(path, color_key))
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

/// A free object of another list drawn among the scene's (an actor):
/// where it stands, and the region it sticks to, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreeObject {
    /// Position of the hotspot.
    pub x: i32,
    /// See `x`.
    pub y: i32,
    /// The region node of the main layer the object is drawn in whatever
    /// its position (`StickToRegion`), as `(layer, node)`.
    pub stick: Option<(usize, usize)>,
}

/// How an object's sprite is transformed when drawn: its scale per axis
/// in percent (`CAdObject::GetScale`) and its rotation in degrees
/// (`CAdEntity::Display`, `CAdActor::Display`), both around the hotspot,
/// which stays at the object's position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Horizontal scale in percent.
    pub scale_x: f32,
    /// Vertical scale in percent.
    pub scale_y: f32,
    /// Rotation in degrees, positive clockwise on screen.
    pub rotation: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Transform::IDENTITY
    }
}

impl Transform {
    /// Unscaled and unrotated.
    pub const IDENTITY: Transform = Transform {
        scale_x: 100.0,
        scale_y: 100.0,
        rotation: 0.0,
    };

    /// The same scale on both axes, no rotation.
    pub fn uniform(scale: f32) -> Transform {
        Transform {
            scale_x: scale,
            scale_y: scale,
            rotation: 0.0,
        }
    }

    /// The transform of an entity in a scene.
    pub fn of_entity(scene: &Scene, entity: &Entity) -> Transform {
        let (scale_x, scale_y) = scene.entity_scale_xy(entity);
        Transform {
            scale_x,
            scale_y,
            rotation: scene.entity_rotation(entity),
        }
    }

    /// Whether the sprite is drawn at its own size (the engine's
    /// `ZoomX == 100 && ZoomY == 100` test, which skips the scaling
    /// arithmetic and its truncation).
    pub fn is_unscaled(&self) -> bool {
        self.scale_x == 100.0 && self.scale_y == 100.0
    }

    /// Whether the sprite is turned.
    pub fn is_rotated(&self) -> bool {
        self.rotation != 0.0
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
    /// Scale and rotation.
    pub transform: Transform,
    /// Tint and opacity.
    pub tint: Rgba,
    /// How the sprite is blended.
    pub blend: BlendMode,
    /// The region node of the main layer the object is drawn in whatever
    /// its position (`StickToRegion`), as `(layer, node)`.
    pub stick: Option<(usize, usize)>,
    /// The attachments drawn before the object (`AddAttachment` with
    /// `preDisplay`), placed and scaled with it already.
    pub before: Vec<Drawable<'a>>,
    /// The attachments drawn after the object.
    pub after: Vec<Drawable<'a>>,
}

/// The attachments of a scene entity, drawn around it.
#[derive(Default)]
pub struct AttachedDraws<'a> {
    /// Drawn before the entity.
    pub before: Vec<Drawable<'a>>,
    /// Drawn after it.
    pub after: Vec<Drawable<'a>>,
}

/// A video drawn in an entity's place (`Entity.PlayTheora`): the frame
/// shown, at the entity's position, scaled and tinted like the entity.
pub struct VideoOverlay<'a> {
    /// The entity whose video it is.
    pub entity: EntityRef,
    /// The frame shown.
    pub texture: &'a Texture,
    /// Scale in percent.
    pub zoom: f32,
    /// Where the picture is from the entity's position: its place in
    /// the video's coded frame, zoomed.
    pub offset: (f32, f32),
    /// Tint and opacity.
    pub tint: Rgba,
}

/// An entity saying a sentence.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EntityTalk {
    /// The sentence.
    pub sentence: Sentence,
    /// The talk sprite being played.
    pub sprite: Option<String>,
    /// Whether the sentence is shown this frame.
    pub show: bool,
    /// The sprite `ForceTalkAnim` asked for, played as the first stance
    /// in place of the talk sprites.
    pub forced: Option<String>,
}

/// One object in the drawing order of a scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawItem {
    /// An entity node of a layer.
    Node {
        /// The layer.
        layer: usize,
        /// The node in the layer.
        index: usize,
    },
    /// A free entity of the scene, drawn in a layer.
    Free {
        /// The free entity.
        index: usize,
        /// The layer it is drawn in.
        layer: usize,
    },
    /// One of the other objects given to the drawing (an actor).
    Other {
        /// Its index among them.
        index: usize,
        /// The layer it is drawn in.
        layer: usize,
    },
}

impl DrawItem {
    /// The layer the item is drawn in.
    pub fn layer(self) -> usize {
        match self {
            DrawItem::Node { layer, .. }
            | DrawItem::Free { layer, .. }
            | DrawItem::Other { layer, .. } => layer,
        }
    }
}

/// Quads drawn right after an object of the scene, in its layer: the
/// particles of its emitter (`CAdEntity::Display`, `CAdActor::Display`
/// draw the emitter after the sprite).
#[derive(Default)]
pub struct Overlays<'a> {
    /// By entity, for the scene's entities: the quads with their
    /// attributes (a particle's rotation, the emitter's blend mode).
    pub entities: HashMap<EntityRef, Vec<(Quad<'a>, Attributes)>>,
    /// By index among the other objects given to the drawing (actors).
    pub others: HashMap<usize, Vec<(Quad<'a>, Attributes)>>,
    /// The videos drawn in entities' places instead of their sprites.
    pub videos: Vec<VideoOverlay<'a>>,
    /// The 3D items drawn in place of other objects (the 3D actors), by
    /// index among them: the shadow, then the model (`CAdActor3DX::
    /// Display`), before the object's particles.
    pub three_d: HashMap<usize, Vec<Draw3d<'a>>>,
    /// The 3D items drawn before the nodes of the main layer: the shadow
    /// receivers written to the depth buffer (`CAdScene::TraverseNodes`,
    /// `RenderShadowGeometry`).
    pub before_main: Vec<Draw3d<'a>>,
}

/// A scene and everything needed to run and draw it.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SceneView {
    /// The scene definition, which scripts modify.
    pub scene: Scene,
    /// Path the scene was loaded from.
    pub path: String,
    /// The sprites of the scene's entities; loaded again after a saved
    /// game, except those scripts edited.
    pub bank: SpriteBank,
    anims: HashMap<EntityRef, AnimState>,
    talks: HashMap<EntityRef, EntityTalk>,
    /// The sprite an entity plays once (`PlayAnim`), shown instead of
    /// its own until it finishes (`STATE_PLAYING_ANIM`).
    playing: HashMap<EntityRef, String>,
    /// The subtitle placement of entities scripts changed it on.
    pub subtitles: HashMap<EntityRef, SubtitleMods>,
    /// Whether this view holds the game's own entities rather than a
    /// scene: its free entities are then `EntityRef::Game`, not `Free`.
    pub(crate) game_level: bool,
    /// The 3D side of the scene when it has `GEOMETRY` (and the `3d`
    /// feature is on): the geometry, the path finder, the camera's
    /// projection; see `docs/3d-actors.md`.
    pub three_d: Option<crate::scene3d::Scene3d>,
    /// What loading the geometry logged, taken by the world.
    #[serde(skip)]
    pub(crate) three_d_log: Vec<String>,
    /// The frames that became current at the last [`SceneView::advance`]:
    /// the entity, its sprite's key and the frame's index. The world takes
    /// them to play the frames' sounds and raise their events.
    #[serde(skip)]
    shown: Vec<(EntityRef, String, usize)>,
}

impl SceneView {
    /// Loads a scene file from the game and the sprites of its entities.
    /// A missing or broken sprite is recorded in [`SceneView::missing`],
    /// not an error. Images are decoded and uploaded later by
    /// [`SceneView::upload`].
    pub fn load(vfs: &Vfs, path: &str) -> Result<SceneView> {
        let bytes = vfs.read(path)?;
        // The `TEMPLATE` files of the scene and of its parts are read
        // where the keys stand (`CAdScene::LoadBuffer`).
        let mut read = |file: &str| vfs.read(file).ok();
        let scene = Scene::parse_with(&bytes, &mut luma_formats::Files::new(&mut read)).map_err(
            |source| Error::Format {
                path: path.to_string(),
                source,
            },
        )?;
        let mut view = SceneView {
            scene,
            path: path.to_string(),
            bank: SpriteBank::new(),
            anims: HashMap::new(),
            talks: HashMap::new(),
            playing: HashMap::new(),
            subtitles: HashMap::new(),
            game_level: false,
            three_d: None,
            three_d_log: Vec::new(),
            shown: Vec::new(),
        };
        view.load_geometry(vfs);
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

    /// The view holding the game's own entities (`Game.CreateEntity`,
    /// `Game.LoadEntity`, attachments): an empty scene whose free
    /// entities are the game's, named by `EntityRef::Game`, with their
    /// sprites and states, kept across scene changes.
    pub fn game_level() -> SceneView {
        SceneView {
            scene: Scene::default(),
            path: String::new(),
            bank: SpriteBank::new(),
            anims: HashMap::new(),
            talks: HashMap::new(),
            playing: HashMap::new(),
            subtitles: HashMap::new(),
            game_level: true,
            three_d: None,
            three_d_log: Vec::new(),
            shown: Vec::new(),
        }
    }

    /// Loads the scene's 3D geometry (`GEOMETRY`) with the `3d` feature;
    /// without it, a scene with geometry is logged and runs as a 2D scene.
    #[cfg(feature = "3d")]
    fn load_geometry(&mut self, vfs: &Vfs) {
        match crate::scene3d::Scene3d::load(vfs, &self.scene) {
            Some(Ok((three_d, warnings))) => {
                self.three_d_log.extend(warnings);
                self.three_d = Some(three_d);
            }
            Some(Err(err)) => self.three_d_log.push(err),
            None => {}
        }
    }

    /// See the `3d` version.
    #[cfg(not(feature = "3d"))]
    fn load_geometry(&mut self, _vfs: &Vfs) {
        if let Some(file) = &self.scene.geometry {
            self.three_d_log.push(format!(
                "Scene geometry '{file}' ignored: this build of Luma Engine was made without the `3d` feature"
            ));
        }
    }

    /// The reference naming free entity `index` of this view.
    pub fn free_ref(&self, index: usize) -> EntityRef {
        if self.game_level {
            EntityRef::Game(index)
        } else {
            EntityRef::Free(index)
        }
    }

    /// The index in `free_entities` a reference names in this view.
    fn free_index(&self, r: EntityRef) -> Option<usize> {
        match r {
            EntityRef::Free(i) if !self.game_level => Some(i),
            EntityRef::Game(i) if self.game_level => Some(i),
            _ => None,
        }
    }

    /// Loads a sprite definition if it is not loaded yet, for an entity
    /// whose sprite a script changed. Returns whether the sprite is
    /// available.
    pub fn add_sprite(&mut self, vfs: &Vfs, path: &str) -> bool {
        self.bank.add_sprite(vfs, path)
    }

    /// Looks at the saved games' thumbnails again
    /// ([`SpriteBank::refresh_prefixed`]).
    pub(crate) fn refresh_thumbnails(&mut self, vfs: &Vfs) -> Vec<Texture> {
        self.bank
            .refresh_prefixed(vfs, crate::save::THUMBNAIL_PREFIX)
    }

    /// Loads the sprites the scene's state names, after a saved game:
    /// the entities' own and the talk sprites of the talking ones.
    pub(crate) fn reload_sprites(&mut self, vfs: &Vfs) {
        let mut paths: Vec<String> = self
            .scene
            .entities()
            .filter(|e| e.kind == EntityKind::Normal)
            .filter_map(|e| e.sprite.clone())
            .collect();
        for (r, talk) in &self.talks {
            paths.extend(talk.sprite.clone());
            paths.extend(talk.forced.clone());
            if let Some(playback) = &talk.sentence.talk {
                paths.extend(playback.def.sprite_paths().into_iter().map(str::to_string));
            }
            if let Some(e) = self.entity(*r) {
                paths.extend(e.talk_sprites.iter().cloned());
                paths.extend(e.talk_special_sprites.iter().cloned());
            }
        }
        // The sprites `PlayAnim` is showing instead of the entities' own.
        paths.extend(self.playing.values().cloned());
        for path in paths {
            self.bank.add_sprite(vfs, &path);
        }
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

    /// Renumbers the states kept per entity, after a node or a free
    /// entity was inserted or deleted.
    pub(crate) fn remap_refs(&mut self, f: impl Fn(EntityRef) -> EntityRef) {
        self.anims = std::mem::take(&mut self.anims)
            .into_iter()
            .map(|(r, v)| (f(r), v))
            .collect();
        self.talks = std::mem::take(&mut self.talks)
            .into_iter()
            .map(|(r, v)| (f(r), v))
            .collect();
        self.playing = std::mem::take(&mut self.playing)
            .into_iter()
            .map(|(r, v)| (f(r), v))
            .collect();
        self.subtitles = std::mem::take(&mut self.subtitles)
            .into_iter()
            .map(|(r, v)| (f(r), v))
            .collect();
    }

    /// Drops the states kept for an entity that left the scene.
    pub(crate) fn forget_entity(&mut self, r: EntityRef) {
        self.anims.remove(&r);
        self.talks.remove(&r);
        self.playing.remove(&r);
        self.subtitles.remove(&r);
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
        refs.extend((0..self.scene.free_entities.len()).map(|i| self.free_ref(i)));
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
            EntityRef::Free(_) | EntityRef::Game(_) => {
                self.scene.free_entities.get(self.free_index(r)?)
            }
            EntityRef::Ui(_) => None,
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
            EntityRef::Free(_) | EntityRef::Game(_) => {
                let i = self.free_index(r)?;
                self.scene.free_entities.get_mut(i)
            }
            EntityRef::Ui(_) => None,
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
        if let Some(sprite) = self.playing.get(&r) {
            return Some(sprite.clone());
        }
        self.entity(r)?.sprite.clone()
    }

    /// Shows a sprite in place of the entity's own (`PlayAnim`): a
    /// sprite that does not loop gives the entity its own back when it
    /// ends; a looping one plays until `Reset`, a sentence or another
    /// `PlayAnim`, moving the entity by its frames' `MOVE` all along.
    /// `false` when the file cannot be loaded.
    pub fn play_anim(&mut self, vfs: &Vfs, r: EntityRef, path: &str) -> bool {
        if self.entity(r).is_none() || !self.bank.add_sprite(vfs, path) {
            return false;
        }
        // The sprite plays as it is: a looping one goes on until `Reset`,
        // a sentence or another `PlayAnim` (a wandering character).
        self.playing.insert(r, path.to_string());
        self.anims.insert(r, AnimState::new(path));
        true
    }

    /// Whether an entity is playing a `PlayAnim` sprite; scripts waiting
    /// for the entity go on when it is done.
    pub fn is_playing_anim(&self, r: EntityRef) -> bool {
        self.playing.contains_key(&r)
    }

    /// Starts a sentence on an entity (`CAdObject::Talk`): talking is the
    /// entity's state now, so a sprite `PlayAnim` was showing is dropped
    /// (`CAdEntity::Update` frees it once the entity is ready again) and
    /// the entity is ready as soon as the sentence ends. `forced` is the
    /// sprite `ForceTalkAnim` named, already loaded, played as the first
    /// stance.
    pub fn talk(&mut self, r: EntityRef, sentence: Sentence, forced: Option<String>) {
        self.playing.remove(&r);
        self.talks.insert(
            r,
            EntityTalk {
                sentence,
                sprite: None,
                show: false,
                forced,
            },
        );
    }

    /// Adds a talk sprite to an entity (`AddTalkSprite`): to the special
    /// set with `special`, else to the generic one. `false` when the
    /// file does not load.
    pub fn add_talk_sprite(&mut self, vfs: &Vfs, r: EntityRef, path: &str, special: bool) -> bool {
        if self.entity(r).is_none() || !self.bank.add_sprite(vfs, path) {
            return false;
        }
        if let Some(entity) = self.entity_mut(r) {
            if special {
                entity.talk_special_sprites.push(path.to_string());
            } else {
                entity.talk_sprites.push(path.to_string());
            }
        }
        true
    }

    /// Removes a talk sprite from an entity (`RemoveTalkSprite`), by file
    /// name; an entity playing it shows its own sprite in its place
    /// (`m_TempSprite2 = m_Sprite`) until that sprite has played or the
    /// sentence ends.
    pub fn remove_talk_sprite(&mut self, r: EntityRef, path: &str, special: bool) {
        let Some(entity) = self.entity_mut(r) else {
            return;
        };
        let list = if special {
            &mut entity.talk_special_sprites
        } else {
            &mut entity.talk_sprites
        };
        let Some(i) = list.iter().position(|p| key(p) == key(path)) else {
            return;
        };
        list.remove(i);
        let own = entity.sprite.clone();
        if let Some(talk) = self.talks.get_mut(&r) {
            if talk.sprite.as_deref().is_some_and(|p| key(p) == key(path)) {
                talk.sprite = own;
            }
        }
    }

    /// Replaces an entity's talk sprites of a set with one
    /// (`SetTalkSprite`); an entity playing one of the old ones plays the
    /// new one at once. `false` when the file does not load.
    pub fn set_talk_sprite(&mut self, vfs: &Vfs, r: EntityRef, path: &str, special: bool) -> bool {
        if self.entity(r).is_none() || !self.bank.add_sprite(vfs, path) {
            return false;
        }
        let current = self.talks.get(&r).and_then(|t| t.sprite.clone());
        let Some(entity) = self.entity_mut(r) else {
            return false;
        };
        let list = if special {
            &mut entity.talk_special_sprites
        } else {
            &mut entity.talk_sprites
        };
        let playing_old =
            current.is_some_and(|current| list.iter().any(|p| key(p) == key(&current)));
        list.clear();
        list.push(path.to_string());
        if playing_old {
            if let Some(talk) = self.talks.get_mut(&r) {
                talk.sprite = Some(path.to_string());
            }
            self.anims.insert(r, AnimState::new(path));
        }
        true
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

    /// Every sentence being said, mutable.
    pub fn sentences_mut(&mut self) -> impl Iterator<Item = &mut Sentence> {
        self.talks.values_mut().map(|t| &mut t.sentence)
    }

    /// The speech sound of the sentence an entity is saying.
    pub(crate) fn speech_sound(&self, r: EntityRef) -> Option<luma_audio::SoundId> {
        self.talks.get(&r)?.sentence.sound
    }

    /// The entities saying a sentence with a speech sound.
    pub(crate) fn speaking_entities(&self) -> Vec<EntityRef> {
        self.talks
            .iter()
            .filter(|(_, t)| t.sentence.sound.is_some())
            .map(|(r, _)| *r)
            .collect()
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
    /// The name of a talk sprite is the `NAME` of its `.sprite` file; a
    /// sprite without one (a plain image) answers to no stance.
    fn talk_stance(&self, r: EntityRef, stance: Option<&str>, random: u32) -> Option<String> {
        let entity = self.entity(r)?;
        if let Some(name) = stance {
            let found = entity
                .talk_special_sprites
                .iter()
                .chain(&entity.talk_sprites)
                .find(|s| self.bank.sprite_name_is(s, name));
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
            let mut definition_sprite_done = false;
            // `m_Sentence->Update()`: the sprite the `.talk` definition
            // gives for this moment replaces the talk stance
            // (`m_TempSprite2`); an entity faces down for it.
            if let Some(talk) = self.talks.get_mut(&r) {
                let shown_done = talk.sprite.as_deref().filter(|p| {
                    self.anims
                        .get(&r)
                        .is_some_and(|a| a.sprite == key(p) && a.finished)
                });
                if let Some(path) = shown_done {
                    talk.sentence.note_sprite_finished(path);
                }
                let frame = talk
                    .sentence
                    .update_talk(timer, luma_formats::Direction::Down);
                if let Some(crate::talk::TalkFrame {
                    sprite: Some(path),
                    reset,
                    finished,
                }) = frame
                {
                    if finished {
                        // Played to its end: the stances take the frame.
                        definition_sprite_done = true;
                    } else if reset || self.anims.get(&r).is_none_or(|a| a.sprite != key(&path)) {
                        self.anims.insert(r, AnimState::new(&path));
                    }
                    talk.sprite = Some(path);
                }
            }
            let (time_is_up, sprite_done) = {
                let talk = &self.talks[&r];
                let done = talk.sprite.as_deref().is_none_or(|p| {
                    self.anims
                        .get(&r)
                        .is_some_and(|a| a.sprite == key(p) && a.finished)
                });
                (
                    talk.sentence.time_is_up(timer),
                    done || definition_sprite_done,
                )
            };
            if time_is_up {
                self.talks.remove(&r);
                continue;
            }
            if sprite_done {
                // The forced sprite is the first stance (`GetTalkStance`);
                // the sentence's stance list still advances.
                let (stance, forced) = match self.talks.get_mut(&r) {
                    Some(t) => (t.sentence.next_stance(), t.forced.take()),
                    None => (None, None),
                };
                let sprite = forced.or_else(|| self.talk_stance(r, stance.as_deref(), random));
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
        // `Reset` ends a `PlayAnim` too (`STATE_READY`).
        self.playing.remove(&r);
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
            let (changed, frame, move_x, move_y) = {
                let Some(loaded) = self.bank.sprites.get(&sprite_key) else {
                    continue;
                };
                let Some(state) = self.anims.get_mut(&r) else {
                    continue;
                };
                let changed = state.step(&loaded.sprite, timer);
                (changed, state.current_frame, state.move_x, state.move_y)
            };
            if changed {
                self.shown.push((r, sprite_key, frame));
                if move_x != 0 || move_y != 0 {
                    if let Some(entity) = self.entity_mut(r) {
                        entity.x += move_x;
                        entity.y += move_y;
                    }
                }
            }
            // A played sprite that reached its end gives the entity back
            // its own sprite.
            if let Some(playing) = self.playing.get(&r) {
                let done = self
                    .anims
                    .get(&r)
                    .is_some_and(|a| a.sprite == key(playing) && a.finished);
                if done {
                    self.playing.remove(&r);
                    self.anims.remove(&r);
                }
            }
        }
    }

    /// The frame an entity shows.
    pub fn frame_of(&self, r: EntityRef) -> usize {
        self.anims.get(&r).map_or(0, |a| a.current_frame)
    }

    /// Takes the frames that became current since the last call: the
    /// entity, its sprite's key and the frame's index, in the order they
    /// changed (see [`SceneView::advance`]).
    pub fn take_shown(&mut self) -> Vec<(EntityRef, String, usize)> {
        std::mem::take(&mut self.shown)
    }

    /// Fills a draw list with the scene as seen by `camera`, showing the
    /// frames the animations are at (see [`SceneView::advance`]).
    pub fn draw<'a>(&'a self, list: &mut DrawList<'a>, camera: &Camera) {
        self.draw_with(list, camera, &[], &HashMap::new(), |_| None);
    }

    /// The order the scene's objects are drawn in (`CAdScene::
    /// TraverseNodes`): layer by layer, each layer's nodes in order; a
    /// walkable region of a layer is followed by the free objects
    /// standing in it, or stuck to it (`StickToRegion`), sorted by `y`;
    /// the main layer ends with the free objects not drawn yet, sorted
    /// by `y`. The free objects are the `others` (actors, at their
    /// positions) first, then the scene's free entities, as the engine
    /// lists them; `free_stick` gives the region a free entity sticks
    /// to.
    pub fn draw_order(
        &self,
        others: &[FreeObject],
        free_stick: impl Fn(usize) -> Option<(usize, usize)>,
    ) -> Vec<DrawItem> {
        let scene = &self.scene;
        let mut objects: Vec<(FreeObject, DrawItem)> = Vec::new();
        for (i, object) in others.iter().enumerate() {
            objects.push((*object, DrawItem::Other { index: i, layer: 0 }));
        }
        for (i, e) in scene.free_entities.iter().enumerate() {
            if e.active && e.kind == EntityKind::Normal {
                objects.push((
                    FreeObject {
                        x: e.x,
                        y: e.y,
                        stick: free_stick(i),
                    },
                    DrawItem::Free { index: i, layer: 0 },
                ));
            }
        }
        let mut drawn = vec![false; objects.len()];
        let mut order = Vec::new();
        // The objects of a region (`DisplayRegionContent`): those stuck to
        // it, and those standing in it that stick to none; with no region,
        // every one left.
        let mut draw_in = |order: &mut Vec<DrawItem>,
                           layer: usize,
                           region: Option<(usize, &Region)>| {
            let mut inside: Vec<usize> = (0..objects.len())
                .filter(|&i| {
                    let object = objects[i].0;
                    !drawn[i]
                        && match region {
                            None => true,
                            Some((node, r)) => {
                                object.stick == Some((layer, node))
                                    || (object.stick.is_none() && r.contains(object.x, object.y))
                            }
                        }
                })
                .collect();
            inside.sort_by_key(|&i| objects[i].0.y);
            for i in inside {
                drawn[i] = true;
                order.push(match objects[i].1 {
                    DrawItem::Other { index, .. } => DrawItem::Other { index, layer },
                    DrawItem::Free { index, .. } => DrawItem::Free { index, layer },
                    DrawItem::Node { layer, index } => DrawItem::Node { layer, index },
                });
            }
        };
        for (li, layer) in scene.layers.iter().enumerate() {
            if !layer.active {
                continue;
            }
            for (ni, node) in layer.nodes.iter().enumerate() {
                match node {
                    SceneNode::Entity(_) => order.push(DrawItem::Node {
                        layer: li,
                        index: ni,
                    }),
                    SceneNode::Region(region) => {
                        if region.blocked || region.decoration || !region.active {
                            continue;
                        }
                        draw_in(&mut order, li, Some((ni, region)));
                    }
                }
            }
            if layer.main {
                draw_in(&mut order, li, None);
            }
        }
        order
    }

    /// Like [`SceneView::draw`], with other objects (actors) drawn among
    /// the free entities, in the order of [`SceneView::draw_order`], and
    /// `videos` drawn in their entities' places instead of the sprites.
    pub fn draw_with<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        camera: &Camera,
        others: &[Drawable<'a>],
        attached: &HashMap<EntityRef, AttachedDraws<'a>>,
        free_stick: impl Fn(usize) -> Option<(usize, usize)>,
    ) {
        self.draw_with_overlays(
            list,
            camera,
            others,
            attached,
            &Overlays::default(),
            free_stick,
        );
    }

    /// Like [`SceneView::draw_with`], with the `overlays` of the objects
    /// (their particles) pushed right after each object, in its layer.
    pub fn draw_with_overlays<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        camera: &Camera,
        others: &[Drawable<'a>],
        attached: &HashMap<EntityRef, AttachedDraws<'a>>,
        overlays: &Overlays<'a>,
        free_stick: impl Fn(usize) -> Option<(usize, usize)>,
    ) {
        let scene = &self.scene;
        let positions: Vec<FreeObject> = others
            .iter()
            .map(|d| FreeObject {
                x: d.x,
                y: d.y,
                stick: d.stick,
            })
            .collect();
        // An entity's attachments are drawn around it (`CAdEntity::Display`).
        let draw_entity = |list: &mut DrawList<'a>, r: EntityRef, d: &Drawable<'a>| {
            let around = attached.get(&r);
            for a in around.iter().flat_map(|a| &a.before) {
                draw_object(list, a);
            }
            draw_object(list, d);
            for a in around.iter().flat_map(|a| &a.after) {
                draw_object(list, a);
            }
        };
        let mut current_layer = usize::MAX;
        let mut main_started = false;
        // An entity's video, where it plays, takes the sprite's place,
        // the attachments around it (`CAdEntity::Display`).
        let video_of = |r: EntityRef| overlays.videos.iter().find(|v| v.entity == r);
        let draw_video =
            |list: &mut DrawList<'a>, r: EntityRef, entity: &Entity, video: &VideoOverlay<'a>| {
                let around = attached.get(&r);
                for a in around.iter().flat_map(|a| &a.before) {
                    draw_object(list, a);
                }
                let zoom = video.zoom / 100.0;
                list.push(Quad {
                    texture: video.texture,
                    dst: Rect::new(
                        entity.x as f32 + video.offset.0,
                        entity.y as f32 + video.offset.1,
                        (video.texture.width as f32 * zoom).trunc(),
                        (video.texture.height as f32 * zoom).trunc(),
                    ),
                    src: None,
                    tint: video.tint,
                    flip_x: false,
                    flip_y: false,
                });
                for a in around.iter().flat_map(|a| &a.after) {
                    draw_object(list, a);
                }
            };
        for item in self.draw_order(&positions, free_stick) {
            let layer = item.layer();
            if layer != current_layer {
                current_layer = layer;
                let (ox, oy) = camera.layer_offset(scene, &scene.layers[layer]);
                list.set_offset(-ox, -oy);
                if scene.layers[layer].main && !main_started {
                    main_started = true;
                    for draw in &overlays.before_main {
                        list.push_3d(draw.clone());
                    }
                }
            }
            let overlay = match item {
                DrawItem::Node { layer, index } => {
                    let r = EntityRef::Node { layer, index };
                    if let Some(SceneNode::Entity(entity)) = self.node(layer, index) {
                        match video_of(r) {
                            Some(video) if entity.active && entity.kind == EntityKind::Normal => {
                                draw_video(list, r, entity, video);
                            }
                            _ => {
                                if let Some(d) = self.drawable(r, entity) {
                                    draw_entity(list, r, &d);
                                }
                            }
                        }
                    }
                    overlays.entities.get(&r)
                }
                DrawItem::Free { index, .. } => {
                    let r = EntityRef::Free(index);
                    if let Some(entity) = scene.free_entities.get(index) {
                        match video_of(r) {
                            Some(video) if entity.active && entity.kind == EntityKind::Normal => {
                                draw_video(list, r, entity, video);
                            }
                            _ => {
                                if let Some(d) = self.drawable(r, entity) {
                                    draw_entity(list, r, &d);
                                }
                            }
                        }
                    }
                    overlays.entities.get(&r)
                }
                DrawItem::Other { index, .. } => {
                    for draw in overlays.three_d.get(&index).into_iter().flatten() {
                        list.push_3d(draw.clone());
                    }
                    if let Some(d) = others.get(index) {
                        draw_object(list, d);
                    }
                    overlays.others.get(&index)
                }
            };
            for (quad, attributes) in overlay.into_iter().flatten() {
                list.push_with(*quad, *attributes);
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
            transform: Transform::of_entity(&self.scene, entity),
            tint: self.scene.entity_tint(entity),
            stick: None,
            before: Vec::new(),
            after: Vec::new(),
            blend: entity.blend_mode,
        })
    }
}

/// Whether a sprite file loads, as `CBSprite::LoadFile` answers: a file
/// that is not there does (the `invalid.bmp` placeholder takes its
/// place), an image does, and a sprite definition does when it parses.
pub(crate) fn sprite_file_loads(vfs: &Vfs, path: &str) -> bool {
    if Sprite::is_image_path(path) || crate::save::is_thumbnail_path(path) {
        return true;
    }
    match vfs.read(path) {
        Ok(bytes) => Sprite::parse(&bytes).is_ok(),
        Err(_) => true,
    }
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

/// Whether a sub-frame is drawn and hit-tested (`CBFrame::Draw`): the
/// engine's `2D_ONLY` sub-frames are for its software renderer and its
/// `3D_ONLY` ones for its accelerated renderer. Luma always draws with
/// the GPU, so a `2D_ONLY` sub-frame is never drawn and a `3D_ONLY` one
/// always is.
pub fn is_drawn(sub: &SubFrame) -> bool {
    !sub.only_2d
}

/// The sub-frames of a frame that are drawn, see [`is_drawn`].
pub fn drawn_subframes(frame: &Frame) -> impl Iterator<Item = &SubFrame> {
    frame.subframes.iter().filter(|sub| is_drawn(sub))
}

/// Draws an object's current frame (`CBSprite::Display`,
/// `CBSubFrame::Draw`): each drawn sub-frame placed by its hotspot,
/// scaled, turned around the hotspot and blended as the object says,
/// between the attachments drawn before and after the object.
pub fn draw_object<'a>(list: &mut DrawList<'a>, d: &Drawable<'a>) {
    for a in &d.before {
        draw_object(list, a);
    }
    if let Some(loaded) = d.sprite.filter(|l| !l.sprite.frames.is_empty()) {
        let frame = d.frame.min(loaded.sprite.frames.len() - 1);
        for sub in drawn_subframes(&loaded.sprite.frames[frame]) {
            let Some(image) = &sub.image else { continue };
            let Some(texture) = d.bank.texture(image, sub.color_key) else {
                continue;
            };
            let (quad, attributes) = place_transformed(sub, texture, d.x, d.y, d.transform, d.tint);
            list.push_with(
                quad,
                Attributes {
                    blend: d.blend,
                    ..attributes
                },
            );
        }
    }
    for a in &d.after {
        draw_object(list, a);
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
/// engine. See [`place_transformed`] for a scale per axis and a rotation.
pub fn place<'a>(
    sub: &SubFrame,
    texture: &'a Texture,
    x: i32,
    y: i32,
    scale: f32,
    owner_tint: Rgba,
) -> Quad<'a> {
    place_transformed(
        sub,
        texture,
        x,
        y,
        Transform::uniform(scale * 100.0),
        owner_tint,
    )
    .0
}

/// Like [`place`], with a [`Transform`]: the sub-frame is scaled by a
/// factor per axis around the hotspot, which stays at `(x, y)`, and the
/// returned attributes turn it around that point by the transform's
/// rotation (`CBSubFrame::Draw`: the hotspot is the centre handed to
/// `DisplayTransform`). The attributes' blend mode is the default; the
/// caller sets the owner's.
pub fn place_transformed<'a>(
    sub: &SubFrame,
    texture: &'a Texture,
    x: i32,
    y: i32,
    transform: Transform,
    owner_tint: Rgba,
) -> (Quad<'a>, Attributes) {
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
    let (zx, zy) = (transform.scale_x / 100.0, transform.scale_y / 100.0);
    let dst = if transform.is_unscaled() {
        Rect::new((x - sub.hotspot_x) as f32, (y - sub.hotspot_y) as f32, w, h)
    } else {
        Rect::new(
            (x as f32 - sub.hotspot_x as f32 * zx).trunc(),
            (y as f32 - sub.hotspot_y as f32 * zy).trunc(),
            (w * zx).trunc(),
            (h * zy).trunc(),
        )
    };
    let quad = Quad {
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
    };
    let attributes = Attributes {
        rotation: transform.rotation,
        // The centre `DisplayTransform` turns around: the hotspot, from
        // the truncated top-left corner.
        pivot: (
            dst.x + sub.hotspot_x as f32 * zx,
            dst.y + sub.hotspot_y as f32 * zy,
        ),
        blend: BlendMode::Normal,
    };
    (quad, attributes)
}
