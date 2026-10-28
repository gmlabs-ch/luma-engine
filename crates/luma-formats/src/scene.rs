//! Scenes: the rooms of a game, made of layers of entities and regions.
//!
//! The format is documented in `docs/formats/scene.md` and was established
//! from the MIT-licensed WME Lite sources (`AdScene.cpp`, `AdLayer.cpp`,
//! `AdEntity.cpp`, `AdRegion.cpp`, `BRegion.cpp` and related files,
//! Copyright (c) 2011 Jan Nedoma). See `NOTICE` at the root of the
//! repository.

use crate::def::{Document, Node, Nodes};
use crate::error::{Error, Result};
use crate::image::Rgba;
use crate::sprite::Rect;

/// A point in scene coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Point {
    /// Horizontal position.
    pub x: i32,
    /// Vertical position.
    pub y: i32,
}

/// A script property set from the file (`PROPERTY { NAME VALUE }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    /// Property name.
    pub name: String,
    /// Property value, always a string.
    pub value: String,
}

/// The direction an actor faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Code 0.
    Up,
    /// Code 1.
    UpRight,
    /// Code 2.
    Right,
    /// Code 3.
    DownRight,
    /// Code 4.
    Down,
    /// Code 5.
    DownLeft,
    /// Code 6.
    Left,
    /// Code 7.
    UpLeft,
}

impl Direction {
    /// The direction for a `WALK_TO_DIR` code: negative codes mean `Up`,
    /// codes of 8 and above mean no direction.
    pub fn from_code(code: i64) -> Option<Direction> {
        Some(match code {
            i64::MIN..=0 => Direction::Up,
            1 => Direction::UpRight,
            2 => Direction::Right,
            3 => Direction::DownRight,
            4 => Direction::Down,
            5 => Direction::DownLeft,
            6 => Direction::Left,
            7 => Direction::UpLeft,
            _ => return None,
        })
    }

    /// The numeric code of the direction.
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// A scene: layers, waypoints, scale and rotation levels, free entities.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    /// Name used by scripts.
    pub name: String,
    /// Caption shown by the interface.
    pub caption: String,
    /// Scene file loaded first (`TEMPLATE`), to be merged by the caller.
    pub template: Option<String>,
    /// Scripts attached to the scene.
    pub scripts: Vec<String>,
    /// Script properties.
    pub properties: Vec<Property>,
    /// Keep the state of the scene's objects between visits.
    pub persistent_state: bool,
    /// With `persistent_state`, also keep the sprites objects switched to.
    pub persistent_state_sprites: bool,
    /// Cursor sprite shown over the scene.
    pub cursor: Option<String>,
    /// Part of the window the scene is drawn in.
    pub viewport: Option<Rect>,
    /// Name of the active 3D camera; meaningless for 2D scenes.
    pub camera: Option<String>,
    /// The layers, in drawing order.
    pub layers: Vec<Layer>,
    /// Waypoint groups for path finding.
    pub waypoint_groups: Vec<WaypointGroup>,
    /// Scale levels, sorted by `y`.
    pub scale_levels: Vec<ScaleLevel>,
    /// Rotation levels, sorted by `x`.
    pub rotation_levels: Vec<RotationLevel>,
    /// Entities that belong to the scene rather than to a layer.
    pub free_entities: Vec<Entity>,
}

/// A layer of a scene: an ordered list of entities and regions.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Name used by scripts.
    pub name: String,
    /// Caption.
    pub caption: String,
    /// Layer file loaded first (`TEMPLATE`).
    pub template: Option<String>,
    /// The main layer: gives the scene its size and holds the floor.
    pub main: bool,
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
    /// Drawn and updated.
    pub active: bool,
    /// Close-up layer with a shield under it.
    pub close_up: bool,
    /// Scripts attached to the layer.
    pub scripts: Vec<String>,
    /// Script properties.
    pub properties: Vec<Property>,
    /// Entities and regions, in drawing order.
    pub nodes: Vec<SceneNode>,
}

impl Default for Layer {
    fn default() -> Self {
        Layer {
            name: String::new(),
            caption: String::new(),
            template: None,
            main: false,
            width: 0,
            height: 0,
            active: true,
            close_up: false,
            scripts: Vec::new(),
            properties: Vec::new(),
            nodes: Vec::new(),
        }
    }
}

/// One node of a layer.
#[derive(Debug, Clone, PartialEq)]
pub enum SceneNode {
    /// An entity (boxed: entities are much larger than regions).
    Entity(Box<Entity>),
    /// A region.
    Region(Region),
}

/// The kind of an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EntityKind {
    /// A visible entity.
    #[default]
    Normal,
    /// A sound source without sprite (`SUBTYPE="sound"`).
    Sound,
}

/// An entity: a sprite placed in the scene, or an invisible hotspot.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    /// Name used by scripts.
    pub name: String,
    /// Caption shown when the mouse is over the entity.
    pub caption: String,
    /// Entity file loaded first (`TEMPLATE`).
    pub template: Option<String>,
    /// Normal or sound.
    pub kind: EntityKind,
    /// Drawn, updated and clickable.
    pub active: bool,
    /// Position where the sprite's hotspot is placed.
    pub x: i32,
    /// See `x`.
    pub y: i32,
    /// Sprite or image drawn at the position.
    pub sprite: Option<String>,
    /// Sprites played while talking.
    pub talk_sprites: Vec<String>,
    /// Alternative talk sprites.
    pub talk_special_sprites: Vec<String>,
    /// Font used for speech.
    pub font: Option<String>,
    /// Inventory item the entity represents.
    pub item: Option<String>,
    /// Follows the scene's scale.
    pub scalable: bool,
    /// Fixed scale in percent, overriding the scene's.
    pub scale: Option<f32>,
    /// Added to the scene's scale, in percent.
    pub relative_scale: f32,
    /// Follows the scene's rotation levels.
    pub rotatable: bool,
    /// Reacts to the mouse (`INTERACTIVE`).
    pub interactive: bool,
    /// Takes the tint of the region it stands in (`COLORABLE`).
    pub colorable: bool,
    /// Fixed tint and opacity, overriding the scene's.
    pub tint: Option<Rgba>,
    /// Cursor sprite shown over the entity.
    pub cursor: Option<String>,
    /// Clickable area; without it the sprite's pixels are.
    pub region: Option<Region>,
    /// Polygon that blocks actors while the entity is active.
    pub blocked_region: Option<Region>,
    /// Waypoints available while the entity is active.
    pub waypoints: Option<WaypointGroup>,
    /// Where the actor walks before interacting.
    pub walk_to_x: i32,
    /// See `walk_to_x`.
    pub walk_to_y: i32,
    /// Direction the actor turns to on arrival.
    pub walk_to_dir: Option<Direction>,
    /// Scripts attached to the entity.
    pub scripts: Vec<String>,
    /// Script properties.
    pub properties: Vec<Property>,
    /// Not clickable while an inventory item is selected.
    pub ignore_items: bool,
    /// Included in the scene's persistent state.
    pub save_state: bool,
    /// Sound started when the entity loads.
    pub sound: Option<String>,
    /// Start offset of the sound, in milliseconds.
    pub sound_start_ms: i32,
    /// Volume of the sound, in percent.
    pub sound_volume: i32,
    /// Pan the sound with the position on screen.
    pub sound_panning: bool,
}

impl Default for Entity {
    fn default() -> Self {
        Entity {
            name: String::new(),
            caption: String::new(),
            template: None,
            kind: EntityKind::Normal,
            active: true,
            x: 0,
            y: 0,
            sprite: None,
            talk_sprites: Vec::new(),
            talk_special_sprites: Vec::new(),
            font: None,
            item: None,
            scalable: true,
            scale: None,
            relative_scale: 0.0,
            rotatable: false,
            interactive: true,
            colorable: true,
            tint: None,
            cursor: None,
            region: None,
            blocked_region: None,
            waypoints: None,
            walk_to_x: 0,
            walk_to_y: 0,
            walk_to_dir: None,
            scripts: Vec::new(),
            properties: Vec::new(),
            ignore_items: false,
            save_state: true,
            sound: None,
            sound_start_ms: 0,
            sound_volume: 100,
            sound_panning: true,
        }
    }
}

/// A polygon with a role in the scene.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    /// Name used by scripts.
    pub name: String,
    /// Caption.
    pub caption: String,
    /// Region file loaded first (`TEMPLATE`).
    pub template: Option<String>,
    /// Takes part in blocking, scaling, tinting and hit tests.
    pub active: bool,
    /// The vertices, in order.
    pub points: Vec<Point>,
    /// Scripts attached to the region.
    pub scripts: Vec<String>,
    /// Script properties.
    pub properties: Vec<Property>,
    /// Actors cannot walk into it (scene regions only).
    pub blocked: bool,
    /// Exists only for scripts (scene regions only).
    pub decoration: bool,
    /// Scale applied inside, in percent; `None` means "use the levels".
    pub zoom: Option<f32>,
    /// Tint applied inside.
    pub tint: Rgba,
}

impl Default for Region {
    fn default() -> Self {
        Region {
            name: String::new(),
            caption: String::new(),
            template: None,
            active: true,
            points: Vec::new(),
            scripts: Vec::new(),
            properties: Vec::new(),
            blocked: false,
            decoration: false,
            zoom: None,
            tint: Rgba::OPAQUE_WHITE,
        }
    }
}

impl Region {
    /// Bounding rectangle of the vertices, right and bottom exclusive;
    /// `None` without vertices.
    pub fn bounds(&self) -> Option<Rect> {
        let first = self.points.first()?;
        let mut rect = Rect {
            left: first.x,
            top: first.y,
            right: first.x,
            bottom: first.y,
        };
        for p in &self.points {
            rect.left = rect.left.min(p.x);
            rect.top = rect.top.min(p.y);
            rect.right = rect.right.max(p.x);
            rect.bottom = rect.bottom.max(p.y);
        }
        Some(rect)
    }

    /// Whether the point is inside the polygon, with the engine's test:
    /// inside the bounding rectangle, then an even-odd crossing count.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        if self.points.len() < 3 {
            return false;
        }
        let Some(bounds) = self.bounds() else {
            return false;
        };
        if x < bounds.left || x >= bounds.right || y < bounds.top || y >= bounds.bottom {
            return false;
        }
        let (px, py) = (f64::from(x), f64::from(y));
        let mut crossings = 0;
        let n = self.points.len();
        let mut p1 = self.points[0];
        for i in 1..=n {
            let p2 = self.points[i % n];
            let (x1, y1) = (f64::from(p1.x), f64::from(p1.y));
            let (x2, y2) = (f64::from(p2.x), f64::from(p2.y));
            if py > y1.min(y2) && py <= y1.max(y2) && px <= x1.max(x2) && y1 != y2 {
                let xinters = (py - y1) * (x2 - x1) / (y2 - y1) + x1;
                if x1 == x2 || px <= xinters {
                    crossings += 1;
                }
            }
            p1 = p2;
        }
        crossings % 2 == 1
    }
}

/// A group of waypoints for path finding.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WaypointGroup {
    /// Name used by scripts.
    pub name: String,
    /// Waypoint file loaded first (`TEMPLATE`).
    pub template: Option<String>,
    /// The waypoints.
    pub points: Vec<Point>,
    /// Script properties.
    pub properties: Vec<Property>,
}

/// A scale level: the scale at a `y`, interpolated between levels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleLevel {
    /// Vertical position.
    pub y: i32,
    /// Scale in percent.
    pub scale: f32,
}

/// A rotation level: the rotation at an `x`, interpolated between levels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotationLevel {
    /// Horizontal position.
    pub x: i32,
    /// Rotation in degrees.
    pub rotation: f32,
}

impl Scene {
    /// Parses a `.scene` file.
    pub fn parse(bytes: &[u8]) -> Result<Scene> {
        Scene::from_document(&Document::parse(bytes)?)
    }

    /// Builds a scene from a parsed document with a `SCENE` root block.
    pub fn from_document(doc: &Document) -> Result<Scene> {
        let root = doc.get("SCENE").ok_or_else(|| Error::Invalid {
            reason: "'SCENE' keyword expected".into(),
        })?;
        let mut scene = Scene {
            persistent_state_sprites: true,
            ..Scene::default()
        };
        for node in root.nodes() {
            match node.key.to_ascii_uppercase().as_str() {
                "NAME" => scene.name = text(node),
                "CAPTION" => scene.caption = text(node),
                "TEMPLATE" => scene.template = node.as_str().map(str::to_string),
                "SCRIPT" => scene.scripts.extend(node.as_str().map(str::to_string)),
                "PROPERTY" => scene.properties.extend(Property::from_node(node)?),
                "PERSISTENT_STATE" => scene.persistent_state = boolean(node),
                "PERSISTENT_STATE_SPRITES" => scene.persistent_state_sprites = boolean(node),
                "CURSOR" => scene.cursor = node.as_str().map(str::to_string),
                "CAMERA" => scene.camera = node.as_str().map(str::to_string),
                "VIEWPORT" => {
                    let v = ints(node);
                    scene.viewport = Some(Rect {
                        left: get(&v, 0, 0),
                        top: get(&v, 1, 0),
                        right: get(&v, 2, 0),
                        bottom: get(&v, 3, 0),
                    });
                }
                "LAYER" => scene.layers.push(Layer::from_node(node)?),
                "ENTITY" => scene.free_entities.push(Entity::from_node(node, true)?),
                "WAYPOINTS" => scene.waypoint_groups.push(WaypointGroup::from_node(node)?),
                "SCALE_LEVEL" => scene.scale_levels.push(ScaleLevel::from_node(node)),
                "ROTATION_LEVEL" => scene.rotation_levels.push(RotationLevel::from_node(node)),
                _ => {}
            }
        }
        scene.scale_levels.sort_by_key(|level| level.y);
        scene.rotation_levels.sort_by_key(|level| level.x);
        Ok(scene)
    }

    /// The main layer: the last one flagged `MAIN`, if any.
    pub fn main_layer(&self) -> Option<&Layer> {
        self.layers.iter().rev().find(|layer| layer.main)
    }

    /// Width of the scene: that of the main layer, `0` without one.
    pub fn width(&self) -> i32 {
        self.main_layer().map_or(0, |layer| layer.width)
    }

    /// Height of the scene: that of the main layer, `0` without one.
    pub fn height(&self) -> i32 {
        self.main_layer().map_or(0, |layer| layer.height)
    }

    /// Scale in percent at a `y` from the scale levels alone: linear
    /// interpolation between the surrounding levels, `100` outside them.
    pub fn scale_at(&self, y: i32) -> f32 {
        interpolate(self.scale_levels.iter().map(|l| (l.y, l.scale)), y, 100.0)
    }

    /// Rotation in degrees at an `x` from the rotation levels: linear
    /// interpolation between the surrounding levels, `0` outside them.
    pub fn rotation_at(&self, x: i32) -> f32 {
        interpolate(
            self.rotation_levels.iter().map(|l| (l.x, l.rotation)),
            x,
            0.0,
        )
    }

    /// Scale in percent of a scalable object at a point: the `SCALE` of
    /// the last region of the main layer containing it, else the scale
    /// levels.
    pub fn zoom_at(&self, x: i32, y: i32) -> f32 {
        self.main_regions_containing(x, y)
            .find_map(|region| region.zoom)
            .unwrap_or_else(|| self.scale_at(y))
    }

    /// Tint of a colorable object at a point: that of the last region of
    /// the main layer containing it, else opaque white.
    pub fn tint_at(&self, x: i32, y: i32) -> Rgba {
        self.main_regions_containing(x, y)
            .next()
            .map_or(Rgba::OPAQUE_WHITE, |region| region.tint)
    }

    /// Whether actors are blocked at a point by the main layer's regions:
    /// the first `BLOCKED` region containing it blocks; otherwise the
    /// point is free when a walkable region contains it, and blocked
    /// (off the floor) when none does. Entities' blocked regions are not
    /// considered.
    pub fn is_blocked_at(&self, x: i32, y: i32) -> bool {
        let mut blocked = true;
        for region in self.main_layer().into_iter().flat_map(Layer::regions) {
            if region.active && !region.decoration && region.contains(x, y) {
                if region.blocked {
                    return true;
                }
                blocked = false;
            }
        }
        blocked
    }

    /// Scale in percent an entity is drawn at: its own `SCALE`, else the
    /// scene's scale at its position plus `RELATIVE_SCALE`; `100` when it
    /// is not scalable.
    pub fn entity_scale(&self, entity: &Entity) -> f32 {
        if !entity.scalable {
            return 100.0;
        }
        entity
            .scale
            .unwrap_or_else(|| self.zoom_at(entity.x, entity.y) + entity.relative_scale)
    }

    /// Tint an entity is drawn with: its own, else the scene's tint at
    /// its position when colorable, else opaque white.
    pub fn entity_tint(&self, entity: &Entity) -> Rgba {
        match entity.tint {
            Some(tint) => tint,
            None if entity.colorable => self.tint_at(entity.x, entity.y),
            None => Rgba::OPAQUE_WHITE,
        }
    }

    /// Rotation in degrees an entity is drawn with: the scene's rotation
    /// at its position when rotatable, else `0`.
    pub fn entity_rotation(&self, entity: &Entity) -> f32 {
        if entity.rotatable {
            self.rotation_at(entity.x)
        } else {
            0.0
        }
    }

    /// Every entity of the scene: layer nodes in order, then free
    /// entities.
    pub fn entities(&self) -> impl Iterator<Item = &Entity> {
        self.layers
            .iter()
            .flat_map(Layer::entities)
            .chain(self.free_entities.iter())
    }

    /// Every sprite file the scene refers to (entity sprites, talk
    /// sprites, cursors), in order of appearance, without duplicates.
    pub fn sprite_paths(&self) -> Vec<&str> {
        let candidates = self.cursor.iter().chain(self.entities().flat_map(|entity| {
            entity
                .sprite
                .iter()
                .chain(&entity.talk_sprites)
                .chain(&entity.talk_special_sprites)
                .chain(entity.cursor.iter())
        }));
        let mut paths: Vec<&str> = Vec::new();
        for path in candidates {
            if !paths.iter().any(|known| known.eq_ignore_ascii_case(path)) {
                paths.push(path);
            }
        }
        paths
    }

    /// Active, non-blocked regions of the main layer that contain the
    /// point, last node first.
    fn main_regions_containing(&self, x: i32, y: i32) -> impl Iterator<Item = &Region> {
        self.main_layer()
            .into_iter()
            .flat_map(|layer| layer.regions().rev())
            .filter(move |r| r.active && !r.blocked && r.contains(x, y))
    }
}

impl Layer {
    fn from_node(node: &Node) -> Result<Layer> {
        let mut layer = Layer::default();
        for child in node.nodes() {
            match child.key.to_ascii_uppercase().as_str() {
                "NAME" => layer.name = text(child),
                "CAPTION" => layer.caption = text(child),
                "TEMPLATE" => layer.template = child.as_str().map(str::to_string),
                "MAIN" => layer.main = boolean(child),
                "WIDTH" => layer.width = integer(child),
                "HEIGHT" => layer.height = integer(child),
                "ACTIVE" => layer.active = boolean(child),
                "CLOSE_UP" => layer.close_up = boolean(child),
                "SCRIPT" => layer.scripts.extend(child.as_str().map(str::to_string)),
                "PROPERTY" => layer.properties.extend(Property::from_node(child)?),
                "ENTITY" => layer
                    .nodes
                    .push(SceneNode::Entity(Box::new(Entity::from_node(
                        child, false,
                    )?))),
                "REGION" => layer
                    .nodes
                    .push(SceneNode::Region(Region::from_node(child, true)?)),
                _ => {}
            }
        }
        Ok(layer)
    }

    /// The entities of the layer, in order.
    pub fn entities(&self) -> impl DoubleEndedIterator<Item = &Entity> {
        self.nodes.iter().filter_map(|node| match node {
            SceneNode::Entity(entity) => Some(entity.as_ref()),
            SceneNode::Region(_) => None,
        })
    }

    /// The regions of the layer, in order.
    pub fn regions(&self) -> impl DoubleEndedIterator<Item = &Region> {
        self.nodes.iter().filter_map(|node| match node {
            SceneNode::Region(region) => Some(region),
            SceneNode::Entity(_) => None,
        })
    }
}

impl Entity {
    /// Parses an `.entity` file: an `ENTITY` root block.
    pub fn parse(bytes: &[u8]) -> Result<Entity> {
        Entity::from_document(&Document::parse(bytes)?)
    }

    /// Builds an entity from a parsed document with an `ENTITY` root
    /// block, scalable by default.
    pub fn from_document(doc: &Document) -> Result<Entity> {
        let root = doc.get("ENTITY").ok_or_else(|| Error::Invalid {
            reason: "'ENTITY' keyword expected".into(),
        })?;
        Entity::from_node(root, true)
    }

    fn from_node(node: &Node, scalable: bool) -> Result<Entity> {
        let mut entity = Entity {
            scalable,
            ..Entity::default()
        };
        let (mut r, mut g, mut b, mut alpha) = (0i64, 0i64, 0i64, 0i64);
        for child in node.nodes() {
            match child.key.to_ascii_uppercase().as_str() {
                "NAME" => entity.name = text(child),
                "CAPTION" => entity.caption = text(child),
                "TEMPLATE" => entity.template = child.as_str().map(str::to_string),
                "ACTIVE" => entity.active = boolean(child),
                "X" => entity.x = integer(child),
                "Y" => entity.y = integer(child),
                "SPRITE" => entity.sprite = child.as_str().map(str::to_string),
                "TALK" => entity
                    .talk_sprites
                    .extend(child.as_str().map(str::to_string)),
                "TALK_SPECIAL" => entity
                    .talk_special_sprites
                    .extend(child.as_str().map(str::to_string)),
                "FONT" => entity.font = child.as_str().map(str::to_string),
                "ITEM" => entity.item = child.as_str().map(str::to_string),
                "SCALABLE" => entity.scalable = boolean(child),
                "SCALE" => entity.scale = Some(integer(child) as f32),
                "RELATIVE_SCALE" => entity.relative_scale = integer(child) as f32,
                "ROTATABLE" | "ROTABLE" => entity.rotatable = boolean(child),
                "INTERACTIVE" | "REGISTRABLE" => entity.interactive = boolean(child),
                "COLORABLE" | "SHADOWABLE" => entity.colorable = boolean(child),
                "ALPHA_COLOR" => {
                    let v = ints(child);
                    r = v.first().copied().unwrap_or(0);
                    g = v.get(1).copied().unwrap_or(0);
                    b = v.get(2).copied().unwrap_or(0);
                }
                "ALPHA" => alpha = child.as_int().unwrap_or(0),
                "CURSOR" => entity.cursor = child.as_str().map(str::to_string),
                "REGION" => entity.region = Some(Region::from_node(child, false)?),
                "BLOCKED_REGION" => {
                    entity.blocked_region = Some(Region::from_node(child, false)?);
                }
                "WAYPOINTS" => entity.waypoints = Some(WaypointGroup::from_node(child)?),
                "WALK_TO_X" => entity.walk_to_x = integer(child),
                "WALK_TO_Y" => entity.walk_to_y = integer(child),
                "WALK_TO_DIR" => {
                    entity.walk_to_dir = Direction::from_code(child.as_int().unwrap_or(0));
                }
                "SCRIPT" => entity.scripts.extend(child.as_str().map(str::to_string)),
                "PROPERTY" => entity.properties.extend(Property::from_node(child)?),
                "IGNORE_ITEMS" => entity.ignore_items = boolean(child),
                "SAVE_STATE" => entity.save_state = boolean(child),
                "SUBTYPE" => {
                    if child
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case("sound"))
                    {
                        entity.kind = EntityKind::Sound;
                        entity.sprite = None;
                        entity.scalable = false;
                        entity.rotatable = false;
                        entity.interactive = false;
                        entity.colorable = false;
                    }
                }
                "SOUND" => entity.sound = child.as_str().map(str::to_string),
                "SOUND_START_TIME" => entity.sound_start_ms = integer(child),
                "SOUND_VOLUME" => entity.sound_volume = integer(child),
                "SOUND_PANNING" => entity.sound_panning = boolean(child),
                _ => {}
            }
        }
        if alpha != 0 && r == 0 && g == 0 && b == 0 {
            (r, g, b) = (255, 255, 255);
        }
        if (r, g, b, alpha) != (0, 0, 0, 0) {
            entity.tint = Some(Rgba {
                r: clamp_channel(r),
                g: clamp_channel(g),
                b: clamp_channel(b),
                a: clamp_channel(alpha),
            });
        }
        Ok(entity)
    }
}

impl Region {
    /// Parses a region block. Scene regions (`scene == true`) read the
    /// blocking, decoration, scale and tint keys; the hotspot of an entity
    /// reads only the base keys, like the engine.
    fn from_node(node: &Node, scene: bool) -> Result<Region> {
        let mut region = Region::default();
        let (mut r, mut g, mut b, mut alpha) = (255i64, 255i64, 255i64, 255i64);
        for child in node.nodes() {
            match child.key.to_ascii_uppercase().as_str() {
                "NAME" => region.name = text(child),
                "CAPTION" => region.caption = text(child),
                "TEMPLATE" => region.template = child.as_str().map(str::to_string),
                "ACTIVE" => region.active = boolean(child),
                "POINT" => {
                    let v = ints(child);
                    region.points.push(Point {
                        x: get(&v, 0, 0),
                        y: get(&v, 1, 0),
                    });
                }
                "SCRIPT" => region.scripts.extend(child.as_str().map(str::to_string)),
                "PROPERTY" => region.properties.extend(Property::from_node(child)?),
                "BLOCKED" if scene => region.blocked = boolean(child),
                "DECORATION" if scene => region.decoration = boolean(child),
                "SCALE" | "ZOOM" if scene => {
                    let zoom = integer(child);
                    region.zoom = (zoom != 0).then_some(zoom as f32);
                }
                "ALPHA_COLOR" if scene => {
                    let v = ints(child);
                    r = v.first().copied().unwrap_or(255);
                    g = v.get(1).copied().unwrap_or(255);
                    b = v.get(2).copied().unwrap_or(255);
                }
                "ALPHA" if scene => alpha = child.as_int().unwrap_or(255),
                _ => {}
            }
        }
        region.tint = Rgba {
            r: clamp_channel(r),
            g: clamp_channel(g),
            b: clamp_channel(b),
            a: clamp_channel(alpha),
        };
        Ok(region)
    }
}

impl WaypointGroup {
    fn from_node(node: &Node) -> Result<WaypointGroup> {
        let mut group = WaypointGroup::default();
        for child in node.nodes() {
            match child.key.to_ascii_uppercase().as_str() {
                "NAME" => group.name = text(child),
                "TEMPLATE" => group.template = child.as_str().map(str::to_string),
                "POINT" => {
                    let v = ints(child);
                    group.points.push(Point {
                        x: get(&v, 0, 0),
                        y: get(&v, 1, 0),
                    });
                }
                "PROPERTY" => group.properties.extend(Property::from_node(child)?),
                _ => {}
            }
        }
        Ok(group)
    }
}

impl ScaleLevel {
    fn from_node(node: &Node) -> ScaleLevel {
        ScaleLevel {
            y: node.get_int("Y").unwrap_or(0) as i32,
            scale: node.get_int("SCALE").unwrap_or(100) as f32,
        }
    }
}

impl RotationLevel {
    fn from_node(node: &Node) -> RotationLevel {
        RotationLevel {
            x: node.get_int("X").unwrap_or(0) as i32,
            rotation: node.get_int("ROTATION").unwrap_or(0) as f32,
        }
    }
}

impl Property {
    /// A `PROPERTY` block needs both `NAME` and `VALUE`.
    fn from_node(node: &Node) -> Result<Option<Property>> {
        let name = node.get_str("NAME");
        let value = node.get_str("VALUE");
        match (name, value) {
            (Some(name), Some(value)) => Ok(Some(Property {
                name: name.to_string(),
                value: value.to_string(),
            })),
            _ => Err(Error::Invalid {
                reason: format!("PROPERTY at line {} needs both NAME and VALUE", node.line),
            }),
        }
    }
}

/// Linear interpolation between the level before `at` (strictly) and the
/// level at or after it; `outside` when either is missing. `levels` must
/// be sorted.
fn interpolate(levels: impl Iterator<Item = (i32, f32)>, at: i32, outside: f32) -> f32 {
    let mut prev = None;
    let mut next = None;
    for (pos, value) in levels {
        if pos < at {
            prev = Some((pos, value));
        } else {
            next = Some((pos, value));
            break;
        }
    }
    let (Some((p0, v0)), Some((p1, v1))) = (prev, next) else {
        return outside;
    };
    let span = (p1 - p0) as f32;
    if span == 0.0 {
        return v0;
    }
    v0 + (v1 - v0) * ((at - p0) as f32 / span)
}

fn text(node: &Node) -> String {
    node.as_str().unwrap_or("").to_string()
}

fn boolean(node: &Node) -> bool {
    node.as_bool().unwrap_or(false)
}

fn integer(node: &Node) -> i32 {
    node.as_int()
        .unwrap_or(0)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn ints(node: &Node) -> Vec<i64> {
    node.as_ints().unwrap_or_default()
}

fn get(values: &[i64], index: usize, default: i32) -> i32 {
    values.get(index).map_or(default, |&v| {
        v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    })
}

fn clamp_channel(v: i64) -> u8 {
    v.clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: &str = r#"; generated by SceneEdit
SCENE {
  NAME="StoreRoom"
  CAPTION=""
  PERSISTENT_STATE=TRUE
  SCRIPT="scenes\storeroom\scr\scene_init.script"
  SCRIPT="scripts\scene.script"
  PROPERTY {
    NAME="music"
    VALUE="hum"
  }
  CURSOR="interface\cursors\scene.sprite"
  EDITOR_MARGIN_H=100
  EDITOR_COLOR_FRAME { 136,136,136,224 }

  WAYPOINTS {
    NAME="new waypoints"
    EDITOR_SELECTED=FALSE
    POINT {547,707}
    POINT {702,662}
  }

  LAYER {
    NAME="back"
    MAIN=FALSE
    WIDTH=1200
    HEIGHT=768
    ENTITY {
      NAME="sky"
      SPRITE="sky.png"
    }
  }

  LAYER {
    NAME="main"
    CAPTION=""
    MAIN=TRUE
    WIDTH=1859
    HEIGHT=768
    ACTIVE=TRUE
    ENTITY {
      NAME="background"
      X=0
      Y=0
      SCALABLE=FALSE
      INTERACTIVE=FALSE
      COLORABLE=FALSE
      SPRITE="scenes\storeroom\artwork\background.png"
    }
    REGION {
      NAME="floor"
      BLOCKED=FALSE
      DECORATION=FALSE
      ACTIVE=TRUE
      SCALE=0
      ALPHA_COLOR { 200,210,220 }
      ALPHA = 128
      PROPERTY {
        NAME="xSurface"
        VALUE="metal"
      }
      POINT {0,400}
      POINT {1000,400}
      POINT {1000,768}
      POINT {0,768}
    }
    REGION {
      NAME="DoorBlock"
      BLOCKED=TRUE
      ACTIVE=TRUE
      POINT {600,500}
      POINT {700,500}
      POINT {700,600}
      POINT {600,600}
    }
    REGION {
      NAME="zoomed"
      SCALE=55
      POINT {800,500}
      POINT {900,500}
      POINT {900,600}
      POINT {800,600}
    }
    ENTITY {
      NAME="MESConsole"
      CAPTION="/STORER0002/Console"
      X=1184
      Y=230
      SCALABLE=TRUE
      INTERACTIVE=TRUE
      COLORABLE=TRUE
      WALK_TO_X=1195
      WALK_TO_Y=379
      WALK_TO_DIR=6
      CURSOR="interface\cursors\cursorup.sprite"
      SCRIPT="scenes\storeroom\scr\mesconsole.script"
      ALPHA = 200
      REGION {
        NAME="<unnamed>"
        ACTIVE=TRUE
        BLOCKED=TRUE
        POINT {1150,184}
        POINT {1186,176}
        POINT {1220,192}
        POINT {1216,221}
      }
    }
    ENTITY {
      NAME="hum"
      SUBTYPE="sound"
      X=10
      Y=10
      SOUND="audio\hum.ogg"
      SOUND_VOLUME=40
    }
  }

  SCALE_LEVEL {
    Y=768
    SCALE=70
  }
  SCALE_LEVEL {
    Y=274
    SCALE=40
  }
  ROTATION_LEVEL {
    X=0
    ROTATION=-10
  }
  ROTATION_LEVEL {
    X=100
    ROTATION=10
  }

  ENTITY {
    NAME="Coffin"
    X=458
    Y=614
    WALK_TO_DIR=7
    SPRITE="scenes\blackchamber\sprites\coffin.sprite"
  }
}
"#;

    fn room() -> Scene {
        Scene::parse(ROOM.as_bytes()).unwrap()
    }

    #[test]
    fn parses_a_full_scene() {
        let scene = room();
        assert_eq!(scene.name, "StoreRoom");
        assert!(scene.persistent_state);
        assert!(scene.persistent_state_sprites);
        assert_eq!(scene.scripts.len(), 2);
        assert_eq!(
            scene.properties,
            vec![Property {
                name: "music".into(),
                value: "hum".into()
            }]
        );
        assert_eq!(
            scene.cursor.as_deref(),
            Some("interface\\cursors\\scene.sprite")
        );
        assert_eq!(scene.waypoint_groups.len(), 1);
        assert_eq!(scene.waypoint_groups[0].points[1], Point { x: 702, y: 662 });

        assert_eq!(scene.layers.len(), 2);
        let main = scene.main_layer().unwrap();
        assert_eq!(main.name, "main");
        assert_eq!((scene.width(), scene.height()), (1859, 768));
        assert_eq!(main.nodes.len(), 6);
        assert_eq!(main.entities().count(), 3);
        assert_eq!(main.regions().count(), 3);
        assert!(!scene.layers[0].main);
        assert_eq!(scene.layers[0].width, 1200);

        let background = main.entities().next().unwrap();
        assert_eq!(background.name, "background");
        assert!(!background.scalable);
        assert!(!background.interactive);
        assert!(!background.colorable);
        assert_eq!(background.tint, None);

        let floor = main.regions().next().unwrap();
        assert_eq!(floor.name, "floor");
        assert!(!floor.blocked);
        assert_eq!(floor.zoom, None);
        assert_eq!(
            floor.tint,
            Rgba {
                r: 200,
                g: 210,
                b: 220,
                a: 128
            }
        );
        assert_eq!(floor.properties[0].name, "xSurface");
        assert_eq!(floor.points.len(), 4);

        let console = main.entities().nth(1).unwrap();
        assert_eq!(console.caption, "/STORER0002/Console");
        assert!(console.scalable);
        assert_eq!((console.walk_to_x, console.walk_to_y), (1195, 379));
        assert_eq!(console.walk_to_dir, Some(Direction::Left));
        assert_eq!(
            console.cursor.as_deref(),
            Some("interface\\cursors\\cursorup.sprite")
        );
        assert_eq!(
            console.tint,
            Some(Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 200
            }),
            "ALPHA alone means white with that alpha"
        );
        let hotspot = console.region.as_ref().unwrap();
        assert_eq!(hotspot.points.len(), 4);
        assert!(!hotspot.blocked, "entity regions ignore BLOCKED");

        let hum = main.entities().nth(2).unwrap();
        assert_eq!(hum.kind, EntityKind::Sound);
        assert_eq!(hum.sprite, None);
        assert!(!hum.interactive);
        assert_eq!(hum.sound.as_deref(), Some("audio\\hum.ogg"));
        assert_eq!(hum.sound_volume, 40);
        assert_eq!(hum.sound_start_ms, 0);
        assert!(hum.sound_panning);

        assert_eq!(scene.free_entities.len(), 1);
        let coffin = &scene.free_entities[0];
        assert!(coffin.scalable, "free entities are scalable by default");
        assert_eq!(coffin.walk_to_dir, Some(Direction::UpLeft));

        assert_eq!(
            scene.scale_levels.iter().map(|l| l.y).collect::<Vec<_>>(),
            vec![274, 768],
            "scale levels are sorted"
        );
        assert_eq!(scene.entities().count(), 5);
        assert_eq!(
            scene.sprite_paths(),
            vec![
                "interface\\cursors\\scene.sprite",
                "sky.png",
                "scenes\\storeroom\\artwork\\background.png",
                "interface\\cursors\\cursorup.sprite",
                "scenes\\blackchamber\\sprites\\coffin.sprite",
            ]
        );
    }

    #[test]
    fn interpolates_scale_and_rotation_levels() {
        let scene = room();
        assert_eq!(scene.scale_at(100), 100.0, "above the first level");
        assert_eq!(scene.scale_at(274), 100.0, "on the first level: no prev");
        assert_eq!(scene.scale_at(521), 55.0);
        assert_eq!(scene.scale_at(768), 70.0);
        assert_eq!(scene.scale_at(900), 100.0, "below the last level");
        assert_eq!(scene.rotation_at(50), 0.0);
        assert_eq!(scene.rotation_at(75), 5.0);
        assert_eq!(scene.rotation_at(200), 0.0);

        let empty = Scene::default();
        assert_eq!(empty.scale_at(10), 100.0);
        assert_eq!(empty.rotation_at(10), 0.0);
    }

    #[test]
    fn looks_up_regions_at_a_point() {
        let scene = room();
        let floor = scene.main_layer().unwrap().regions().next().unwrap();
        assert!(floor.contains(500, 500));
        assert!(!floor.contains(500, 100));
        assert!(!floor.contains(1500, 500));
        assert_eq!(
            floor.bounds(),
            Some(Rect {
                left: 0,
                top: 400,
                right: 1000,
                bottom: 768
            })
        );

        assert_eq!(scene.zoom_at(500, 521), 55.0, "floor has no scale: levels");
        assert_eq!(scene.zoom_at(850, 550), 55.0, "zoomed region");
        assert_eq!(scene.zoom_at(650, 521), 55.0, "blocked regions are skipped");
        assert_eq!(scene.tint_at(500, 500).a, 128);
        assert_eq!(scene.tint_at(1500, 500), Rgba::OPAQUE_WHITE);

        assert!(!scene.is_blocked_at(500, 500));
        assert!(scene.is_blocked_at(650, 550), "blocked region wins");
        assert!(scene.is_blocked_at(1500, 500), "off the floor");

        let mut triangle = Region::default();
        assert!(!triangle.contains(0, 0));
        triangle.points = vec![
            Point { x: 0, y: 0 },
            Point { x: 10, y: 0 },
            Point { x: 0, y: 10 },
        ];
        assert!(triangle.contains(2, 2));
        assert!(!triangle.contains(8, 8));
    }

    #[test]
    fn derives_entity_scale_tint_and_rotation() {
        let scene = room();
        let main = scene.main_layer().unwrap();
        let background = main.entities().next().unwrap();
        assert_eq!(scene.entity_scale(background), 100.0);
        assert_eq!(scene.entity_tint(background), Rgba::OPAQUE_WHITE);

        let console = main.entities().nth(1).unwrap();
        assert_eq!(scene.entity_scale(console), 100.0, "above the levels");
        assert_eq!(scene.entity_tint(console).a, 200);
        assert_eq!(scene.entity_rotation(console), 0.0);

        let actor = Entity {
            x: 75,
            y: 521,
            relative_scale: 5.0,
            rotatable: true,
            ..Entity::default()
        };
        assert_eq!(scene.entity_scale(&actor), 60.0);
        assert_eq!(scene.entity_tint(&actor).a, 128);
        assert_eq!(scene.entity_rotation(&actor), 5.0);

        let fixed = Entity {
            scale: Some(33.0),
            ..actor.clone()
        };
        assert_eq!(scene.entity_scale(&fixed), 33.0);
    }

    #[test]
    fn defaults_match_the_engine() {
        let scene = Scene::parse(b"SCENE { LAYER { ENTITY { } REGION { } } }").unwrap();
        assert!(!scene.persistent_state);
        assert!(scene.persistent_state_sprites);
        assert_eq!(scene.main_layer(), None);
        assert_eq!((scene.width(), scene.height()), (0, 0));
        let layer = &scene.layers[0];
        assert!(layer.active && !layer.main && !layer.close_up);
        let entity = layer.entities().next().unwrap();
        assert!(!entity.scalable, "layer entities are not scalable");
        assert!(entity.active && entity.interactive && entity.colorable);
        assert!(!entity.rotatable && !entity.ignore_items && entity.save_state);
        assert_eq!(entity.walk_to_dir, None);
        let region = layer.regions().next().unwrap();
        assert!(region.active && !region.blocked && !region.decoration);
        assert_eq!(region.zoom, None);
        assert_eq!(region.tint, Rgba::OPAQUE_WHITE);
        assert!(!region.contains(0, 0));

        let entity = Entity::parse(b"ENTITY { NAME=\"x\" WALK_TO_DIR=-3 }").unwrap();
        assert!(entity.scalable, ".entity files are scalable");
        assert_eq!(entity.walk_to_dir, Some(Direction::Up));
        assert_eq!(Direction::from_code(8), None);
        assert_eq!(Direction::from_code(9), None);
        assert_eq!(Direction::Down.code(), 4);
    }

    #[test]
    fn rejects_bad_files() {
        let err = Scene::parse(b"SPRITE { }").unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }), "{err}");
        let err = Scene::parse(b"SCENE { PROPERTY { NAME=\"a\" } }").unwrap_err();
        assert!(err.to_string().contains("PROPERTY"), "{err}");
        let err = Scene::parse(b"SCENE { LAYER {").unwrap_err();
        assert!(matches!(err, Error::Syntax(_)), "{err}");
        let err = Entity::parse(b"SCENE { }").unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }), "{err}");
    }

    #[test]
    #[ignore = "needs the local test game in testdata/local"]
    fn loads_every_local_scene() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/local/white-chamber/unpacked");
        if !root.is_dir() {
            eprintln!("skipped: {} is absent", root.display());
            return;
        }
        let mut files = std::collections::BTreeMap::new();
        let mut scenes = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let key = path
                        .strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .to_ascii_lowercase();
                    if path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("scene"))
                    {
                        scenes.push(path.clone());
                    }
                    files.insert(key, path);
                }
            }
        }
        let mut entities = 0;
        let mut regions = 0;
        let mut sprites = 0;
        let mut missing = Vec::new();
        for path in &scenes {
            let scene = Scene::parse(&std::fs::read(path).unwrap())
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let main = scene
                .main_layer()
                .unwrap_or_else(|| panic!("{}: no main layer", path.display()));
            assert!(main.width > 0 && main.height > 0, "{}", path.display());
            entities += scene.entities().count();
            regions += scene
                .layers
                .iter()
                .map(|l| l.regions().count())
                .sum::<usize>();
            for sprite in scene.sprite_paths() {
                sprites += 1;
                let key = sprite.replace('\\', "/").to_ascii_lowercase();
                match files.get(&key) {
                    Some(file) => {
                        if !crate::sprite::Sprite::is_image_path(sprite) {
                            crate::sprite::Sprite::parse(&std::fs::read(file).unwrap())
                                .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
                        }
                    }
                    None => missing.push(format!("{}: {sprite}", path.display())),
                }
            }
        }
        eprintln!(
            "parsed {} scenes, {entities} entities, {regions} regions, {sprites} sprite references, {} missing",
            scenes.len(),
            missing.len()
        );
        assert!(
            missing.is_empty(),
            "missing sprites:\n{}",
            missing.join("\n")
        );
    }
}
