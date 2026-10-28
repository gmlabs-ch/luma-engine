//! A scene loaded with its sprites and textures, composed into a draw
//! list the way the engine draws it (see `docs/formats/scene.md`,
//! section "Drawing").

use std::collections::HashMap;

use luma_archive::Vfs;
use luma_formats::scene::{Entity, EntityKind, Layer, SceneNode};
use luma_formats::{Rgb, Rgba, Scene, Sprite, SubFrame};
use luma_render::{DrawList, Quad, Rect, Renderer, Texture};

use crate::error::{Error, Result};

/// A sprite with its images uploaded.
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

/// A scene and everything needed to draw it.
#[derive(Debug)]
pub struct SceneView {
    /// The scene definition.
    pub scene: Scene,
    /// Path the scene was loaded from.
    pub path: String,
    sprites: HashMap<String, LoadedSprite>,
    textures: HashMap<String, Texture>,
    /// Files referenced by the scene that could not be loaded, with the
    /// reason; the scene is still shown without them.
    pub missing: Vec<(String, String)>,
}

/// Case-insensitive key of a game path.
fn key(path: &str) -> String {
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
    /// Loads a scene file from the game and the sprites and images of its
    /// entities. A missing or broken sprite is recorded in `missing`, not
    /// an error.
    pub fn load(vfs: &Vfs, renderer: &Renderer, path: &str) -> Result<SceneView> {
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
        };
        let paths: Vec<String> = view
            .scene
            .entities()
            .filter(|e| e.kind == EntityKind::Normal)
            .filter_map(|e| e.sprite.clone())
            .collect();
        for sprite_path in paths {
            view.load_sprite(vfs, renderer, &sprite_path);
        }
        Ok(view)
    }

    fn load_sprite(&mut self, vfs: &Vfs, renderer: &Renderer, path: &str) {
        let k = key(path);
        if self.sprites.contains_key(&k) {
            return;
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
                    return;
                }
            }
        };
        for sub in sprite.frames.iter().flat_map(|f| &f.subframes) {
            if let Some(image) = &sub.image {
                self.load_image(vfs, renderer, image, sub.color_key);
            }
        }
        self.sprites.insert(k, LoadedSprite { sprite });
    }

    fn load_image(&mut self, vfs: &Vfs, renderer: &Renderer, path: &str, color_key: Rgb) {
        let k = texture_key(path, color_key);
        if self.textures.contains_key(&k) {
            return;
        }
        let decoded = vfs.read(path).map_err(|e| e.to_string()).and_then(|bytes| {
            luma_formats::image::decode(&bytes, color_key).map_err(|e| e.to_string())
        });
        match decoded {
            Ok(image) => {
                self.textures.insert(k, renderer.texture(path, &image));
            }
            Err(reason) => self.missing.push((path.to_string(), reason)),
        }
    }

    /// The loaded sprite of a path, if it loaded.
    pub fn sprite(&self, path: &str) -> Option<&LoadedSprite> {
        self.sprites.get(&key(path))
    }

    /// The texture of an image, if it loaded.
    pub fn texture(&self, path: &str, color_key: Rgb) -> Option<&Texture> {
        self.textures.get(&texture_key(path, color_key))
    }

    /// Number of textures uploaded.
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// Fills a draw list with the scene as seen by `camera`, `elapsed_ms`
    /// after the scene started (which selects the frame of animated
    /// sprites; `0` shows every first frame).
    pub fn draw<'a>(&'a self, list: &mut DrawList<'a>, camera: &Camera, elapsed_ms: u64) {
        let scene = &self.scene;
        let mut drawn = vec![false; scene.free_entities.len()];
        for layer in &scene.layers {
            if !layer.active {
                continue;
            }
            let (ox, oy) = camera.layer_offset(scene, layer);
            list.set_offset(-ox, -oy);
            for node in &layer.nodes {
                match node {
                    SceneNode::Entity(entity) => self.draw_entity(list, entity, elapsed_ms),
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
                            self.draw_entity(list, &scene.free_entities[i], elapsed_ms);
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
                    self.draw_entity(list, &scene.free_entities[i], elapsed_ms);
                }
            }
        }
        list.set_offset(0.0, 0.0);
    }

    fn draw_entity<'a>(&'a self, list: &mut DrawList<'a>, entity: &Entity, elapsed_ms: u64) {
        if !entity.active || entity.kind != EntityKind::Normal {
            return;
        }
        let Some(loaded) = entity.sprite.as_deref().and_then(|p| self.sprite(p)) else {
            return;
        };
        let Some(frame) = loaded.sprite.frame_index_at(elapsed_ms) else {
            return;
        };
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
