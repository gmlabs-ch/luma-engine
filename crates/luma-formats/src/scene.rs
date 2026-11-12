//! Scenes: the rooms of a game, made of layers of entities and regions.
//!
//! The format is documented in `docs/formats/scene.md` and was established
//! from the MIT-licensed WME Lite sources (`AdScene.cpp`, `AdLayer.cpp`,
//! `AdEntity.cpp`, `AdRegion.cpp`, `BRegion.cpp` and related files,
//! Copyright (c) 2011 Jan Nedoma). See `NOTICE` at the root of the
//! repository.

use crate::def::{Document, Node, keys};
use crate::error::{Error, Result};
use crate::image::{BlendMode, Rgb, Rgba};
use crate::sprite::Rect;
use crate::template::Files;

/// A point in scene coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Point {
    /// Horizontal position.
    pub x: i32,
    /// Vertical position.
    pub y: i32,
}

/// A script property set from the file (`PROPERTY { NAME VALUE }`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Property {
    /// Property name.
    pub name: String,
    /// Property value, always a string.
    pub value: String,
}

/// The direction an actor faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

    /// Every direction, in code order.
    pub const ALL: [Direction; 8] = [
        Direction::Up,
        Direction::UpRight,
        Direction::Right,
        Direction::DownRight,
        Direction::Down,
        Direction::DownLeft,
        Direction::Left,
        Direction::UpLeft,
    ];

    /// The direction of a code from 0 to 7, as scripts set `Direction`;
    /// `None` for any other value.
    pub fn from_index(code: i64) -> Option<Direction> {
        usize::try_from(code)
            .ok()
            .and_then(|i| Direction::ALL.get(i).copied())
    }

    /// The direction of an angle in degrees, `atan2` style with `y`
    /// down and `0` pointing right (`CAdActor::AngleToDirection`).
    pub fn from_angle(angle: i32) -> Direction {
        match angle {
            -111..=-67 => Direction::Up,
            -66..=-22 => Direction::UpRight,
            -21..=22 => Direction::Right,
            23..=67 => Direction::DownRight,
            68..=112 => Direction::Down,
            113..=157 => Direction::DownLeft,
            158..=180 | -180..=-157 => Direction::Left,
            -156..=-112 => Direction::UpLeft,
            _ => Direction::Down,
        }
    }

    /// The next direction counter-clockwise (one step left).
    pub fn left(self) -> Direction {
        Direction::ALL[(usize::from(self.code()) + 7) % 8]
    }

    /// The next direction clockwise (one step right).
    pub fn right(self) -> Direction {
        Direction::ALL[(usize::from(self.code()) + 1) % 8]
    }
}

/// A scene: layers, waypoints, scale and rotation levels, free entities.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Scene {
    /// Name used by scripts.
    pub name: String,
    /// Caption shown by the interface.
    pub caption: String,
    /// The last `TEMPLATE` file named; it was applied where the key stands.
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
    /// Name of the active 3D camera of the geometry (`CAMERA`); an
    /// unknown name keeps the geometry's first camera.
    pub camera: Option<String>,
    /// The `.3ds` scene geometry file (`GEOMETRY`); `None` for a 2D scene.
    pub geometry: Option<String>,
    /// Field of view in **radians** replacing the active camera's
    /// (`FOV_OVERRIDE`); absent or negative keeps the camera's.
    pub fov_override: Option<f32>,
    /// Near clipping distance in world units (`NEAR_CLIPPING_PLANE`);
    /// absent or negative means the renderer's default (90).
    pub near_clipping_plane: Option<f32>,
    /// Far clipping distance in world units (`FAR_CLIPPING_PLANE`); absent
    /// or negative means the renderer's default (10000).
    pub far_clipping_plane: Option<f32>,
    /// Height of the 3D waypoints above the floor (`WAYPOINT_HEIGHT`);
    /// absent or negative keeps the geometry's default (10).
    pub waypoint_height: Option<f32>,
    /// 3D actors path-find on the scene's 2D regions and waypoints
    /// (`2D_PATHFINDING`); the geometry then only maps 2D to 3D.
    pub pathfinding_2d: bool,
    /// Highest shadow type allowed in the scene, 0 none, 1 simple, 2 flat,
    /// 3 stencil (`MAX_SHADOW_TYPE`, clamped; default 2).
    pub max_shadow_type: u8,
    /// Pre-1.7 framing: the camera sees the top-left screenful of the
    /// scene whatever the layer size (`SCROLL_3D_COMPATIBILITY`).
    pub scroll_3d_compatibility: bool,
    /// Ambient light colour for 3D actors (`AMBIENT_LIGHT_COLOR`), black
    /// by default.
    pub ambient_light_color: Rgb,
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Layer {
    /// Name used by scripts.
    pub name: String,
    /// Caption.
    pub caption: String,
    /// The last `TEMPLATE` file named; it was applied where the key stands.
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SceneNode {
    /// An entity (boxed: entities are much larger than regions).
    Entity(Box<Entity>),
    /// A region.
    Region(Region),
}

/// The kind of an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EntityKind {
    /// A visible entity.
    #[default]
    Normal,
    /// A sound source without sprite (`SUBTYPE="sound"`).
    Sound,
}

/// An entity: a sprite placed in the scene, or an invisible hotspot.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Entity {
    /// Name used by scripts.
    pub name: String,
    /// Caption shown when the mouse is over the entity.
    pub caption: String,
    /// The last `TEMPLATE` file named; it was applied where the key stands.
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
    /// Horizontal scale in percent set by a script (`ScaleX`); with
    /// `scale_y`, it replaces `scale` and the scene's.
    pub scale_x: Option<f32>,
    /// See `scale_x`.
    pub scale_y: Option<f32>,
    /// Follows the scene's rotation levels.
    pub rotatable: bool,
    /// Fixed rotation in degrees set by a script (`Rotate`), overriding
    /// the scene's; only applies when rotatable.
    pub rotate: Option<f32>,
    /// Added to the scene's rotation, in degrees (`RelativeRotate`).
    pub relative_rotate: f32,
    /// How the sprite is blended (`BlendMode`).
    pub blend_mode: BlendMode,
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
    /// The `.entity` file the entity was loaded from (`Filename`); none
    /// for an entity written in a scene.
    pub filename: Option<String>,
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
            scale_x: None,
            scale_y: None,
            rotatable: false,
            rotate: None,
            relative_rotate: 0.0,
            blend_mode: BlendMode::Normal,
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
            filename: None,
        }
    }
}

/// A polygon with a role in the scene.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Region {
    /// Name used by scripts.
    pub name: String,
    /// Caption.
    pub caption: String,
    /// The last `TEMPLATE` file named; it was applied where the key stands.
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
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WaypointGroup {
    /// Name used by scripts.
    pub name: String,
    /// The last `TEMPLATE` file named; it was applied where the key stands.
    pub template: Option<String>,
    /// The waypoints.
    pub points: Vec<Point>,
    /// Script properties.
    pub properties: Vec<Property>,
    /// Whether path finding uses the group (`WaypointGroup.Active`); a
    /// runtime flag scripts set, not a file key.
    pub active: bool,
}

impl Default for WaypointGroup {
    fn default() -> Self {
        WaypointGroup {
            name: String::new(),
            template: None,
            points: Vec::new(),
            properties: Vec::new(),
            active: true,
        }
    }
}

/// A scale level: the scale at a `y`, interpolated between levels.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScaleLevel {
    /// Vertical position.
    pub y: i32,
    /// Scale in percent.
    pub scale: f32,
}

/// A rotation level: the rotation at an `x`, interpolated between levels.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RotationLevel {
    /// Horizontal position.
    pub x: i32,
    /// Rotation in degrees.
    pub rotation: f32,
}

/// The keys of `CAdScene::LoadBuffer`, in the order of its table.
const SCENE_KEYS: &[&str] = &[
    "SCENE",
    "TEMPLATE",
    "NAME",
    "LAYER",
    "WAYPOINTS",
    "EVENTS",
    "CURSOR",
    "GEOMETRY",
    "CAMERA",
    "ENTITY",
    "SCALE_LEVEL",
    "ROTATION_LEVEL",
    "EDITOR_MARGIN_H",
    "EDITOR_MARGIN_V",
    "EDITOR_COLOR_FRAME",
    "EDITOR_COLOR_ENTITY_SEL",
    "EDITOR_COLOR_REGION_SEL",
    "EDITOR_COLOR_DECORATION_SEL",
    "EDITOR_COLOR_BLOCKED_SEL",
    "EDITOR_COLOR_WAYPOINTS_SEL",
    "EDITOR_COLOR_REGION",
    "EDITOR_COLOR_DECORATION",
    "EDITOR_COLOR_BLOCKED",
    "EDITOR_COLOR_ENTITY",
    "EDITOR_COLOR_WAYPOINTS",
    "EDITOR_COLOR_SCALE",
    "EDITOR_SHOW_REGIONS",
    "EDITOR_SHOW_DECORATION",
    "EDITOR_SHOW_BLOCKED",
    "EDITOR_SHOW_ENTITIES",
    "EDITOR_SHOW_SCALE",
    "SCRIPT",
    "CAPTION",
    "PROPERTY",
    "VIEWPORT",
    "PERSISTENT_STATE_SPRITES",
    "PERSISTENT_STATE",
    "EDITOR_PROPERTY",
    "EDITOR_SHOW_GEOMETRY",
    "EDITOR_RESOLUTION_WIDTH",
    "EDITOR_RESOLUTION_HEIGHT",
    "FOV_OVERRIDE",
    "WAYPOINT_HEIGHT",
    "NEAR_CLIPPING_PLANE",
    "FAR_CLIPPING_PLANE",
    "2D_PATHFINDING",
    "MAX_SHADOW_TYPE",
    "SCROLL_3D_COMPATIBILITY",
    "AMBIENT_LIGHT_COLOR",
];

/// The keys of `CAdLayer::LoadBuffer`, in the order of its table.
const LAYER_KEYS: &[&str] = &[
    "LAYER",
    "TEMPLATE",
    "NAME",
    "WIDTH",
    "HEIGHT",
    "MAIN",
    "ENTITY",
    "REGION",
    "ACTIVE",
    "EDITOR_SELECTED",
    "SCRIPT",
    "CAPTION",
    "PROPERTY",
    "CLOSE_UP",
    "EDITOR_PROPERTY",
];

/// The keys of `CAdEntity::LoadBuffer`, in the order of its table.
const ENTITY_KEYS: &[&str] = &[
    "ENTITY",
    "SPRITE",
    "X",
    "Y",
    "TEMPLATE",
    "NAME",
    "SCALABLE",
    "REGISTRABLE",
    "INTERACTIVE",
    "SHADOWABLE",
    "COLORABLE",
    "ACTIVE",
    "EVENTS",
    "FONT",
    "TALK_SPECIAL",
    "TALK",
    "CURSOR",
    "REGION",
    "BLOCKED_REGION",
    "EDITOR_SELECTED",
    "SCRIPT",
    "SOUND_START_TIME",
    "SOUND_VOLUME",
    "SOUND_PANNING",
    "SOUND",
    "SUBTYPE",
    "CAPTION",
    "PROPERTY",
    "WAYPOINTS",
    "IGNORE_ITEMS",
    "ROTABLE",
    "ROTATABLE",
    "ALPHA_COLOR",
    "SCALE",
    "RELATIVE_SCALE",
    "ALPHA",
    "EDITOR_PROPERTY",
    "ITEM",
    "WALK_TO_X",
    "WALK_TO_Y",
    "WALK_TO_DIR",
    "SAVE_STATE",
];

/// The keys of `CBRegion::LoadBuffer` (the region of an entity, a blocked
/// region), in the order of its table.
const BASE_REGION_KEYS: &[&str] = &[
    "REGION",
    "TEMPLATE",
    "NAME",
    "ACTIVE",
    "POINT",
    "CAPTION",
    "SCRIPT",
    "EDITOR_SELECTED_POINT",
    "PROPERTY",
];

/// The keys of `CAdRegion::LoadBuffer` (a region of a layer), in the
/// order of its table.
const SCENE_REGION_KEYS: &[&str] = &[
    "REGION",
    "TEMPLATE",
    "NAME",
    "ACTIVE",
    "ZOOM",
    "SCALE",
    "BLOCKED",
    "DECORATION",
    "POINT",
    "ALPHA_COLOR",
    "ALPHA",
    "EDITOR_SELECTED_POINT",
    "EDITOR_SELECTED",
    "SCRIPT",
    "CAPTION",
    "PROPERTY",
    "EDITOR_PROPERTY",
];

/// The keys of `CAdWaypointGroup::LoadBuffer`, in the order of its table.
const WAYPOINTS_KEYS: &[&str] = &[
    "WAYPOINTS",
    "TEMPLATE",
    "NAME",
    "POINT",
    "EDITOR_SELECTED_POINT",
    "EDITOR_SELECTED",
    "PROPERTY",
    "EDITOR_PROPERTY",
];

/// The keys of `CAdScaleLevel::LoadBuffer`, in the order of its table.
const SCALE_LEVEL_KEYS: &[&str] = &["SCALE_LEVEL", "TEMPLATE", "Y", "SCALE", "EDITOR_PROPERTY"];

/// The keys of `CAdRotLevel::LoadBuffer`, in the order of its table.
const ROTATION_LEVEL_KEYS: &[&str] = &[
    "ROTATION_LEVEL",
    "TEMPLATE",
    "X",
    "ROTATION",
    "EDITOR_PROPERTY",
];

/// The keys of `CBScriptHolder::ParseProperty`, in the order of its
/// table.
const PROPERTY_KEYS: &[&str] = &["PROPERTY", "NAME", "VALUE"];

/// The root block a file must start with, or the original's
/// `'<KEY>' keyword expected` error.
fn root_of<'a>(doc: &'a Document, key: &str) -> Result<&'a Node> {
    doc.root(key).ok_or_else(|| Error::Invalid {
        reason: format!("'{key}' keyword expected"),
    })
}

impl Scene {
    /// Parses a `.scene` file. No other file is read: a `TEMPLATE` key
    /// fails to load, which the original engine ignores.
    pub fn parse(bytes: &[u8]) -> Result<Scene> {
        Scene::parse_with(bytes, &mut Files::none())
    }

    /// Parses a `.scene` file, reading the `TEMPLATE` files it names
    /// through `files`.
    pub fn parse_with(bytes: &[u8], files: &mut Files<'_>) -> Result<Scene> {
        Scene::from_document_with(&Document::parse(bytes)?, files)
    }

    /// Builds a scene from a parsed document with a `SCENE` root block.
    pub fn from_document(doc: &Document) -> Result<Scene> {
        Scene::from_document_with(doc, &mut Files::none())
    }

    /// Builds a scene from a parsed document whose first statement is the
    /// `SCENE` block, reading the `TEMPLATE` files it names through
    /// `files`.
    pub fn from_document_with(doc: &Document, files: &mut Files<'_>) -> Result<Scene> {
        let mut scene = Scene::default();
        scene.load(root_of(doc, "SCENE")?, files)?;
        Ok(scene)
    }

    /// What `CAdScene::Cleanup` leaves of a scene: its name and its
    /// script properties.
    fn cleanup(&mut self) {
        *self = Scene {
            name: std::mem::take(&mut self.name),
            properties: std::mem::take(&mut self.properties),
            persistent_state_sprites: true,
            max_shadow_type: 2,
            ..Scene::default()
        };
    }

    /// `CAdScene::LoadBuffer`: empties the scene, then applies the keys of
    /// the block in order. A key that is not in the table is a syntax
    /// error that fails the load. A layer, waypoint group, level or
    /// entity that cannot be read is dropped alone, and so is a
    /// `TEMPLATE` that cannot be loaded: the original sets an error code
    /// that the next statement overwrites.
    fn load(&mut self, root: &Node, files: &mut Files<'_>) -> Result<()> {
        self.cleanup();
        let mut camera: Option<String> = None;
        let mut waypoint_height = -1.0f32;
        for item in keys(root, SCENE_KEYS, "SCENE") {
            let (key, node) = item?;
            match key {
                "TEMPLATE" => {
                    // `LoadFile` on the scene itself: everything read so
                    // far is dropped, the template's keys apply, and the
                    // keys that follow go over them.
                    let name = node.text();
                    let _ = files
                        .template(&name, |doc, files| self.load(root_of(doc, "SCENE")?, files));
                    self.template = Some(name);
                }
                "NAME" => self.name = node.text(),
                "CAPTION" => self.caption = node.text(),
                "SCRIPT" => self.scripts.push(node.text()),
                "PROPERTY" => self.properties.extend(Property::from_node(node)),
                "VIEWPORT" => {
                    let mut rect = self.viewport.unwrap_or_default();
                    node.read_ints(&mut [
                        &mut rect.left,
                        &mut rect.top,
                        &mut rect.right,
                        &mut rect.bottom,
                    ]);
                    self.viewport = Some(rect);
                    // The original's `case` has no `break` and goes on to
                    // read the viewport's text as `PERSISTENT_STATE`: a
                    // defect, not reproduced.
                }
                "PERSISTENT_STATE" => node.read_bool(&mut self.persistent_state),
                "PERSISTENT_STATE_SPRITES" => node.read_bool(&mut self.persistent_state_sprites),
                "CURSOR" => self.cursor = Some(node.text()),
                "CAMERA" => camera = Some(node.text()),
                "GEOMETRY" => {
                    // A new geometry starts with its own camera and
                    // waypoint height.
                    self.geometry = Some(node.text());
                    self.camera = None;
                    self.waypoint_height = None;
                }
                "FOV_OVERRIDE" => self.fov_override = float(node).or(self.fov_override),
                "NEAR_CLIPPING_PLANE" => {
                    self.near_clipping_plane = float(node).or(self.near_clipping_plane);
                }
                "FAR_CLIPPING_PLANE" => {
                    self.far_clipping_plane = float(node).or(self.far_clipping_plane);
                }
                "WAYPOINT_HEIGHT" => node.read_float(&mut waypoint_height),
                "2D_PATHFINDING" => node.read_bool(&mut self.pathfinding_2d),
                "MAX_SHADOW_TYPE" => {
                    let mut kind = 0;
                    node.read_int(&mut kind);
                    self.max_shadow_type = kind.clamp(0, 3) as u8;
                }
                "SCROLL_3D_COMPATIBILITY" => node.read_bool(&mut self.scroll_3d_compatibility),
                "AMBIENT_LIGHT_COLOR" => {
                    let (mut r, mut g, mut b) = (0, 0, 0);
                    node.read_ints(&mut [&mut r, &mut g, &mut b]);
                    self.ambient_light_color = Rgb::from_ints(r, g, b);
                }
                "LAYER" => self.layers.extend(Layer::from_node(node, files).ok()),
                "ENTITY" => {
                    let mut entity = Entity::default();
                    if entity.load(node, files).is_ok() {
                        self.free_entities.push(entity);
                    }
                }
                "WAYPOINTS" => {
                    let mut group = WaypointGroup::default();
                    if group.load(node, files).is_ok() {
                        self.waypoint_groups.push(group);
                    }
                }
                "SCALE_LEVEL" => {
                    let mut level = ScaleLevel { y: 0, scale: 100.0 };
                    if level.load(node, files).is_ok() {
                        self.scale_levels.push(level);
                    }
                }
                "ROTATION_LEVEL" => {
                    let mut level = RotationLevel {
                        x: 0,
                        rotation: 0.0,
                    };
                    if level.load(node, files).is_ok() {
                        self.rotation_levels.push(level);
                    }
                }
                // `SCENE`, `EVENTS` and the editor's keys are accepted
                // and mean nothing to the engine.
                _ => {}
            }
        }
        // The camera and the waypoint height named by this file reach the
        // geometry only when there is one by the end of the file.
        if self.geometry.is_some() {
            if let Some(camera) = camera.filter(|name| !name.is_empty()) {
                self.camera = Some(camera);
            }
            if waypoint_height >= 0.0 {
                self.waypoint_height = Some(waypoint_height);
            }
        }
        self.scale_levels.sort_by_key(|level| level.y);
        self.rotation_levels.sort_by_key(|level| level.x);
        Ok(())
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

    /// Scale in percent an entity is drawn at when it has no per-axis
    /// scale: its own `SCALE`, else the scene's scale at its position
    /// plus `RELATIVE_SCALE`; `100` when it is not scalable. See
    /// [`Scene::entity_scale_xy`] for the whole rule.
    pub fn entity_scale(&self, entity: &Entity) -> f32 {
        if !entity.scalable {
            return 100.0;
        }
        entity
            .scale
            .unwrap_or_else(|| self.zoom_at(entity.x, entity.y) + entity.relative_scale)
    }

    /// Scale per axis, in percent, an entity is drawn at
    /// (`CAdObject::GetScale`): `ScaleX` and `ScaleY` when either is set
    /// (the other axis at 100), else [`Scene::entity_scale`]; `100` on
    /// both axes when the entity is not scalable.
    pub fn entity_scale_xy(&self, entity: &Entity) -> (f32, f32) {
        self.object_scale(
            (entity.x, entity.y),
            entity.scalable,
            entity.scale,
            (entity.scale_x, entity.scale_y),
            entity.relative_scale,
        )
    }

    /// Scale per axis, in percent, of an object at `position` with the
    /// scale settings of `CBObject` (`CAdObject::GetScale`): `(100, 100)`
    /// unless `scalable`; `scale_xy` when either axis is set, the other
    /// axis at 100; else `scale`; else the scene's scale at the position
    /// plus `relative_scale`.
    pub fn object_scale(
        &self,
        position: (i32, i32),
        scalable: bool,
        scale: Option<f32>,
        scale_xy: (Option<f32>, Option<f32>),
        relative_scale: f32,
    ) -> (f32, f32) {
        if !scalable {
            return (100.0, 100.0);
        }
        match scale_xy {
            (None, None) => {
                let s =
                    scale.unwrap_or_else(|| self.zoom_at(position.0, position.1) + relative_scale);
                (s, s)
            }
            (x, y) => (x.unwrap_or(100.0), y.unwrap_or(100.0)),
        }
    }

    /// Rotation in degrees of an object at `x` with the rotation
    /// settings of `CBObject` (`CAdEntity::Display`, `CAdActor::Display`):
    /// `0` unless `rotatable`; `rotate` when set; else the scene's
    /// rotation at `x` plus `relative_rotate`.
    pub fn object_rotation(
        &self,
        x: i32,
        rotatable: bool,
        rotate: Option<f32>,
        relative_rotate: f32,
    ) -> f32 {
        if !rotatable {
            return 0.0;
        }
        rotate.unwrap_or_else(|| self.rotation_at(x) + relative_rotate)
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

    /// Rotation in degrees an entity is drawn with: its `Rotate` when
    /// set, else the scene's rotation at its position plus
    /// `RelativeRotate`, when rotatable; else `0`.
    pub fn entity_rotation(&self, entity: &Entity) -> f32 {
        self.object_rotation(
            entity.x,
            entity.rotatable,
            entity.rotate,
            entity.relative_rotate,
        )
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
    /// Parses a layer file: a `LAYER` root block.
    pub fn parse_with(bytes: &[u8], files: &mut Files<'_>) -> Result<Layer> {
        let doc = Document::parse(bytes)?;
        Layer::from_node(root_of(&doc, "LAYER")?, files)
    }

    fn from_node(node: &Node, files: &mut Files<'_>) -> Result<Layer> {
        let mut layer = Layer::default();
        layer.load(node, files)?;
        Ok(layer)
    }

    /// `CAdLayer::LoadBuffer`: the keys of the block in order, over what
    /// the layer holds. A region or an entity that cannot be read is
    /// dropped alone.
    fn load(&mut self, node: &Node, files: &mut Files<'_>) -> Result<()> {
        for item in keys(node, LAYER_KEYS, "LAYER") {
            let (key, child) = item?;
            match key {
                "TEMPLATE" => {
                    // `LoadFile` on the layer itself: the template's keys
                    // go over what was read, its nodes are appended.
                    let name = child.text();
                    let _ = files
                        .template(&name, |doc, files| self.load(root_of(doc, "LAYER")?, files));
                    self.template = Some(name);
                }
                "NAME" => self.name = child.text(),
                "CAPTION" => self.caption = child.text(),
                "MAIN" => child.read_bool(&mut self.main),
                "WIDTH" => child.read_int(&mut self.width),
                "HEIGHT" => child.read_int(&mut self.height),
                "ACTIVE" => child.read_bool(&mut self.active),
                "CLOSE_UP" => child.read_bool(&mut self.close_up),
                "SCRIPT" => self.scripts.push(child.text()),
                "PROPERTY" => self.properties.extend(Property::from_node(child)),
                "ENTITY" => {
                    // The entities of a layer do not follow the scene's
                    // scale unless they say so.
                    let mut entity = Entity {
                        scalable: false,
                        ..Entity::default()
                    };
                    if entity.load(child, files).is_ok() {
                        self.nodes.push(SceneNode::Entity(Box::new(entity)));
                    }
                }
                "REGION" => {
                    let mut region = Region::default();
                    if region.load(child, true, files).is_ok() {
                        self.nodes.push(SceneNode::Region(region));
                    }
                }
                _ => {}
            }
        }
        Ok(())
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
    /// Parses an `.entity` file: an `ENTITY` root block. No other file is
    /// read: a `TEMPLATE` key fails to load, which the original ignores.
    pub fn parse(bytes: &[u8]) -> Result<Entity> {
        Entity::parse_with(bytes, &mut Files::none())
    }

    /// Parses an `.entity` file, reading the `TEMPLATE` files it names
    /// through `files`.
    pub fn parse_with(bytes: &[u8], files: &mut Files<'_>) -> Result<Entity> {
        Entity::from_document_with(&Document::parse(bytes)?, files)
    }

    /// Builds an entity from a parsed document with an `ENTITY` root
    /// block, scalable by default.
    pub fn from_document(doc: &Document) -> Result<Entity> {
        Entity::from_document_with(doc, &mut Files::none())
    }

    /// Builds an entity from a parsed document whose first statement is
    /// the `ENTITY` block, reading `TEMPLATE` files through `files`.
    pub fn from_document_with(doc: &Document, files: &mut Files<'_>) -> Result<Entity> {
        let mut entity = Entity::default();
        entity.load(root_of(doc, "ENTITY")?, files)?;
        Ok(entity)
    }

    /// `CAdEntity::LoadBuffer`: the keys of the block in order, over what
    /// the entity holds. A region or a waypoint group that cannot be read
    /// leaves the entity without one.
    fn load(&mut self, node: &Node, files: &mut Files<'_>) -> Result<()> {
        let (mut r, mut g, mut b, mut alpha) = (0, 0, 0, 0);
        // Whether this block names a tint of its own.
        let mut tinted = false;
        for item in keys(node, ENTITY_KEYS, "ENTITY") {
            let (key, child) = item?;
            match key {
                "TEMPLATE" => {
                    // `LoadFile` on the entity itself. (The original then
                    // answers the template's name as the entity's file;
                    // the entity keeps its own here.)
                    let name = child.text();
                    let _ = files.template(&name, |doc, files| {
                        self.load(root_of(doc, "ENTITY")?, files)
                    });
                    self.template = Some(name);
                }
                "NAME" => self.name = child.text(),
                "CAPTION" => self.caption = child.text(),
                "ACTIVE" => child.read_bool(&mut self.active),
                "X" => child.read_int(&mut self.x),
                "Y" => child.read_int(&mut self.y),
                "SPRITE" => self.sprite = Some(child.text()),
                "TALK" => self.talk_sprites.push(child.text()),
                "TALK_SPECIAL" => self.talk_special_sprites.push(child.text()),
                "FONT" => self.font = Some(child.text()),
                "ITEM" => self.item = Some(child.text()),
                "SCALABLE" => child.read_bool(&mut self.scalable),
                "SCALE" => {
                    let mut scale = 0;
                    child.read_int(&mut scale);
                    self.scale = Some(scale as f32);
                }
                "RELATIVE_SCALE" => {
                    let mut scale = 0;
                    child.read_int(&mut scale);
                    self.relative_scale = scale as f32;
                }
                "ROTABLE" | "ROTATABLE" => child.read_bool(&mut self.rotatable),
                "REGISTRABLE" | "INTERACTIVE" => child.read_bool(&mut self.interactive),
                "SHADOWABLE" | "COLORABLE" => child.read_bool(&mut self.colorable),
                "ALPHA_COLOR" => {
                    child.read_ints(&mut [&mut r, &mut g, &mut b]);
                    tinted = true;
                }
                "ALPHA" => {
                    child.read_int(&mut alpha);
                    tinted = true;
                }
                "CURSOR" => self.cursor = Some(child.text()),
                "REGION" => {
                    let mut region = Region::default();
                    self.region = region.load(child, false, files).is_ok().then_some(region);
                }
                "BLOCKED_REGION" => {
                    let mut region = Region::default();
                    self.blocked_region =
                        region.load(child, false, files).is_ok().then_some(region);
                }
                "WAYPOINTS" => {
                    let mut group = WaypointGroup::default();
                    self.waypoints = group.load(child, files).is_ok().then_some(group);
                }
                "WALK_TO_X" => child.read_int(&mut self.walk_to_x),
                "WALK_TO_Y" => child.read_int(&mut self.walk_to_y),
                "WALK_TO_DIR" => {
                    let mut dir = 0;
                    child.read_int(&mut dir);
                    self.walk_to_dir = Direction::from_code(i64::from(dir));
                }
                "SCRIPT" => self.scripts.push(child.text()),
                "PROPERTY" => self.properties.extend(Property::from_node(child)),
                "IGNORE_ITEMS" => child.read_bool(&mut self.ignore_items),
                "SAVE_STATE" => child.read_bool(&mut self.save_state),
                "SUBTYPE" => {
                    if child
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case("sound"))
                    {
                        self.kind = EntityKind::Sound;
                        self.sprite = None;
                        self.scalable = false;
                        self.rotatable = false;
                        self.interactive = false;
                        self.colorable = false;
                    }
                }
                "SOUND" => self.sound = Some(child.text()),
                "SOUND_START_TIME" => child.read_int(&mut self.sound_start_ms),
                "SOUND_VOLUME" => child.read_int(&mut self.sound_volume),
                "SOUND_PANNING" => child.read_bool(&mut self.sound_panning),
                _ => {}
            }
        }
        // A block that names no tint keeps the one it has: a template's.
        // (The original computes it again from this block alone and
        // loses the template's.)
        if tinted {
            if alpha != 0 && r == 0 && g == 0 && b == 0 {
                (r, g, b) = (255, 255, 255);
            }
            let tint = Rgba::from_ints(r, g, b, alpha);
            self.tint = (tint != Rgba::from_ints(0, 0, 0, 0)).then_some(tint);
        }
        Ok(())
    }
}

impl Region {
    /// Parses a region file: a `REGION` root block, with the keys of a
    /// layer's region (`scene == true`) or of an entity's.
    pub fn parse_with(bytes: &[u8], scene: bool, files: &mut Files<'_>) -> Result<Region> {
        let doc = Document::parse(bytes)?;
        let mut region = Region::default();
        region.load(root_of(&doc, "REGION")?, scene, files)?;
        Ok(region)
    }

    /// Parses a region block. Regions of a layer (`scene == true`) know
    /// the blocking, decoration, scale and tint keys
    /// (`CAdRegion::LoadBuffer`); the regions of an entity only the base
    /// keys (`CBRegion::LoadBuffer`), and any other key is a syntax
    /// error. No `TEMPLATE` file is read.
    pub fn from_node(node: &Node, scene: bool) -> Result<Region> {
        let mut region = Region::default();
        region.load(node, scene, &mut Files::none())?;
        Ok(region)
    }

    /// `LoadBuffer`: the points are dropped first, then the keys of the
    /// block apply in order.
    fn load(&mut self, node: &Node, scene: bool, files: &mut Files<'_>) -> Result<()> {
        self.points.clear();
        let (mut r, mut g, mut b, mut alpha) = (255, 255, 255, 255);
        // Whether this block names a tint of its own.
        let mut tinted = false;
        let (table, what) = if scene {
            (SCENE_REGION_KEYS, "REGION")
        } else {
            (BASE_REGION_KEYS, "REGION")
        };
        for item in keys(node, table, what) {
            let (key, child) = item?;
            match key {
                "TEMPLATE" => {
                    // `LoadFile` on the region itself: the template's
                    // points replace those read so far.
                    let name = child.text();
                    let _ = files.template(&name, |doc, files| {
                        self.load(root_of(doc, "REGION")?, scene, files)
                    });
                    self.template = Some(name);
                }
                "NAME" => self.name = child.text(),
                "CAPTION" => self.caption = child.text(),
                "ACTIVE" => child.read_bool(&mut self.active),
                "POINT" => self.points.push(Point::from_node(child)),
                "SCRIPT" => self.scripts.push(child.text()),
                "PROPERTY" => self.properties.extend(Property::from_node(child)),
                "BLOCKED" => child.read_bool(&mut self.blocked),
                "DECORATION" => child.read_bool(&mut self.decoration),
                "SCALE" | "ZOOM" => {
                    let mut zoom = 0;
                    child.read_int(&mut zoom);
                    self.zoom = (zoom != 0).then_some(zoom as f32);
                }
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
        // A block that names no tint keeps the one it has: a template's.
        if scene && tinted {
            self.tint = Rgba::from_ints(r, g, b, alpha);
        }
        Ok(())
    }
}

impl Point {
    /// A `POINT { x, y }`; a coordinate that is missing is 0 (the
    /// original leaves it uninitialised).
    fn from_node(node: &Node) -> Point {
        let mut point = Point::default();
        node.read_ints(&mut [&mut point.x, &mut point.y]);
        point
    }
}

impl WaypointGroup {
    /// Parses a waypoint file: a `WAYPOINTS` root block.
    pub fn parse_with(bytes: &[u8], files: &mut Files<'_>) -> Result<WaypointGroup> {
        let doc = Document::parse(bytes)?;
        let mut group = WaypointGroup::default();
        group.load(root_of(&doc, "WAYPOINTS")?, files)?;
        Ok(group)
    }

    /// Parses a `WAYPOINTS` block (a scene's, or the private one of an
    /// actor definition). No `TEMPLATE` file is read.
    pub fn from_node(node: &Node) -> Result<WaypointGroup> {
        let mut group = WaypointGroup::default();
        group.load(node, &mut Files::none())?;
        Ok(group)
    }

    /// `CAdWaypointGroup::LoadBuffer`: the keys of the block in order;
    /// the points are added to those the group has.
    pub(crate) fn load(&mut self, node: &Node, files: &mut Files<'_>) -> Result<()> {
        for item in keys(node, WAYPOINTS_KEYS, "WAYPOINTS") {
            let (key, child) = item?;
            match key {
                "TEMPLATE" => {
                    let name = child.text();
                    let _ = files.template(&name, |doc, files| {
                        self.load(root_of(doc, "WAYPOINTS")?, files)
                    });
                    self.template = Some(name);
                }
                "NAME" => self.name = child.text(),
                "POINT" => self.points.push(Point::from_node(child)),
                "PROPERTY" => self.properties.extend(Property::from_node(child)),
                _ => {}
            }
        }
        Ok(())
    }
}

impl ScaleLevel {
    /// Parses a scale level file: a `SCALE_LEVEL` root block.
    pub fn parse_with(bytes: &[u8], files: &mut Files<'_>) -> Result<ScaleLevel> {
        let doc = Document::parse(bytes)?;
        let mut level = ScaleLevel { y: 0, scale: 100.0 };
        level.load(root_of(&doc, "SCALE_LEVEL")?, files)?;
        Ok(level)
    }

    /// `CAdScaleLevel::LoadBuffer`.
    fn load(&mut self, node: &Node, files: &mut Files<'_>) -> Result<()> {
        for item in keys(node, SCALE_LEVEL_KEYS, "SCALE_LEVEL") {
            let (key, child) = item?;
            match key {
                "TEMPLATE" => {
                    let _ = files.template(&child.text(), |doc, files| {
                        self.load(root_of(doc, "SCALE_LEVEL")?, files)
                    });
                }
                "Y" => child.read_int(&mut self.y),
                "SCALE" => {
                    let mut scale = 0;
                    child.read_int(&mut scale);
                    self.scale = scale as f32;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl RotationLevel {
    /// Parses a rotation level file: a `ROTATION_LEVEL` root block.
    pub fn parse_with(bytes: &[u8], files: &mut Files<'_>) -> Result<RotationLevel> {
        let doc = Document::parse(bytes)?;
        let mut level = RotationLevel {
            x: 0,
            rotation: 0.0,
        };
        level.load(root_of(&doc, "ROTATION_LEVEL")?, files)?;
        Ok(level)
    }

    /// `CAdRotLevel::LoadBuffer`.
    fn load(&mut self, node: &Node, files: &mut Files<'_>) -> Result<()> {
        for item in keys(node, ROTATION_LEVEL_KEYS, "ROTATION_LEVEL") {
            let (key, child) = item?;
            match key {
                "TEMPLATE" => {
                    let _ = files.template(&child.text(), |doc, files| {
                        self.load(root_of(doc, "ROTATION_LEVEL")?, files)
                    });
                }
                "X" => child.read_int(&mut self.x),
                "ROTATION" => {
                    let mut rotation = 0;
                    child.read_int(&mut rotation);
                    self.rotation = rotation as f32;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl Property {
    /// `CBScriptHolder::ParseProperty`: a `PROPERTY { NAME VALUE }` block.
    /// The last `NAME` and the last `VALUE` count. A block with another
    /// key, or without one of the two, sets nothing: the original logs it
    /// and the loader that called it goes on.
    pub fn from_node(node: &Node) -> Option<Property> {
        let mut name = None;
        let mut value = None;
        for item in keys(node, PROPERTY_KEYS, "PROPERTY") {
            let (key, child) = item.ok()?;
            match key {
                "NAME" => name = Some(child.text()),
                "VALUE" => value = Some(child.text()),
                _ => {}
            }
        }
        Some(Property {
            name: name?,
            value: value?,
        })
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

/// `ScanStr(params, "%f", ...)` for a value kept as an option: `None`
/// when the value is empty.
fn float(node: &Node) -> Option<f32> {
    node.as_float().map(|f| f as f32)
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
        assert!(!hotspot.blocked);

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
    fn directions_turn_and_come_from_angles() {
        assert_eq!(Direction::from_index(6), Some(Direction::Left));
        assert_eq!(Direction::from_index(8), None);
        assert_eq!(Direction::from_index(-1), None);
        assert_eq!(Direction::Up.left(), Direction::UpLeft);
        assert_eq!(Direction::UpLeft.right(), Direction::Up);
        assert_eq!(Direction::Right.right(), Direction::DownRight);
        assert_eq!(Direction::from_angle(0), Direction::Right);
        assert_eq!(Direction::from_angle(-90), Direction::Up);
        assert_eq!(Direction::from_angle(90), Direction::Down);
        assert_eq!(Direction::from_angle(180), Direction::Left);
        assert_eq!(Direction::from_angle(-180), Direction::Left);
        assert_eq!(Direction::from_angle(-45), Direction::UpRight);
        assert_eq!(Direction::from_angle(45), Direction::DownRight);
        assert_eq!(Direction::from_angle(135), Direction::DownLeft);
        assert_eq!(Direction::from_angle(-135), Direction::UpLeft);
        assert_eq!(Direction::from_angle(22), Direction::Right);
        assert_eq!(Direction::from_angle(23), Direction::DownRight);
        assert_eq!(Direction::from_angle(-112), Direction::UpLeft);
        assert_eq!(Direction::from_angle(-111), Direction::Up);
        assert_eq!(Direction::from_angle(500), Direction::Down);
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
        assert_eq!(scene.entity_scale_xy(&fixed), (33.0, 33.0));

        // A per-axis scale replaces `Scale` and the scene's; the other
        // axis is 100, as in `CAdObject::GetScale`.
        let wide = Entity {
            scale_x: Some(150.0),
            ..fixed.clone()
        };
        assert_eq!(scene.entity_scale_xy(&wide), (150.0, 100.0));
        let both = Entity {
            scale_y: Some(50.0),
            ..wide.clone()
        };
        assert_eq!(scene.entity_scale_xy(&both), (150.0, 50.0));
        let flat = Entity {
            scalable: false,
            ..both.clone()
        };
        assert_eq!(scene.entity_scale_xy(&flat), (100.0, 100.0));

        // `RelativeRotate` adds to the levels; `Rotate` replaces them;
        // neither applies to an object that is not rotatable.
        let leaning = Entity {
            relative_rotate: 2.5,
            ..actor.clone()
        };
        assert_eq!(scene.entity_rotation(&leaning), 7.5);
        let fixed_rotation = Entity {
            rotate: Some(-90.0),
            ..leaning.clone()
        };
        assert_eq!(scene.entity_rotation(&fixed_rotation), -90.0);
        let upright = Entity {
            rotatable: false,
            ..fixed_rotation.clone()
        };
        assert_eq!(scene.entity_rotation(&upright), 0.0);
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

    fn serve<'a>(files: &'a [(&'a str, &'a str)]) -> impl FnMut(&str) -> Option<Vec<u8>> + 'a {
        move |name| {
            files
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, body)| body.as_bytes().to_vec())
        }
    }

    /// `CAdScene::LoadBuffer TEMPLATE`: the template is loaded into the
    /// scene where the key stands, after dropping what was read so far
    /// (`Cleanup`, which keeps the name and the script properties).
    #[test]
    fn a_scene_template_is_loaded_in_place() {
        let files = [(
            "base.scene",
            r#"SCENE { CAPTION="Base" SCRIPT="base.script" PERSISTENT_STATE=TRUE
  LAYER { NAME="base_layer" MAIN=TRUE WIDTH=100 HEIGHT=50 }
  SCALE_LEVEL { Y=10 SCALE=50 }
}"#,
        )];
        let mut read = serve(&files);
        let scene = Scene::parse_with(
            br#"SCENE { NAME="mine" SCRIPT="dropped.script" CURSOR="dropped.sprite"
  PROPERTY { NAME="kept" VALUE="1" }
  LAYER { NAME="dropped" }
  TEMPLATE="base.scene"
  SCRIPT="mine.script"
  LAYER { NAME="top" }
}"#,
            &mut Files::new(&mut read),
        )
        .unwrap();
        assert_eq!(scene.name, "mine");
        assert_eq!(scene.caption, "Base");
        assert_eq!(scene.scripts, ["base.script", "mine.script"]);
        assert_eq!(scene.cursor, None);
        assert_eq!(scene.properties.len(), 1);
        assert!(scene.persistent_state);
        let layers: Vec<&str> = scene.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(layers, ["base_layer", "top"]);
        assert_eq!(scene.scale_levels.len(), 1);
        assert_eq!((scene.width(), scene.height()), (100, 50));

        // A template that cannot be read changes nothing.
        let scene =
            Scene::parse(br#"SCENE { LAYER { NAME="kept" } TEMPLATE="gone.scene" }"#).unwrap();
        assert_eq!(scene.layers.len(), 1);
    }

    /// `TEMPLATE` in a layer, a region, a waypoint group, a level and an
    /// entity: each loads the named file into itself where the key
    /// stands.
    #[test]
    fn templates_of_the_parts_of_a_scene() {
        let files = [
            (
                "l.layer",
                r#"LAYER { NAME="tpl" WIDTH=640 HEIGHT=480 ENTITY { NAME="from_tpl" } }"#,
            ),
            (
                "r.region",
                r#"REGION { NAME="tpl" BLOCKED=TRUE ALPHA=10 POINT {1,1} POINT {2,2} POINT {3,3} }"#,
            ),
            ("w.wpt", r#"WAYPOINTS { NAME="tpl" POINT {5,6} }"#),
            ("s.level", "SCALE_LEVEL { Y=40 SCALE=60 }"),
            ("r.level", "ROTATION_LEVEL { X=30 ROTATION=15 }"),
            (
                "e.entity",
                r#"ENTITY { NAME="tpl" X=7 Y=8 ALPHA=99 SPRITE="tpl.sprite" SCRIPT="tpl.script" }"#,
            ),
        ];
        let mut read = serve(&files);
        let scene = Scene::parse_with(
            br#"SCENE {
  LAYER { WIDTH=1 ENTITY { NAME="first" } TEMPLATE="l.layer" HEIGHT=99
    REGION { POINT {9,9} TEMPLATE="r.region" POINT {4,4} NAME="mine" }
    ENTITY { X=1 TEMPLATE="e.entity" Y=50 SCRIPT="mine.script" }
  }
  WAYPOINTS { POINT {1,2} TEMPLATE="w.wpt" POINT {7,8} }
  SCALE_LEVEL { TEMPLATE="s.level" SCALE=70 }
  ROTATION_LEVEL { X=1 TEMPLATE="r.level" }
}"#,
            &mut Files::new(&mut read),
        )
        .unwrap();
        let layer = &scene.layers[0];
        assert_eq!(layer.name, "tpl");
        assert_eq!((layer.width, layer.height), (640, 99));
        let names: Vec<&str> = layer.entities().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["first", "from_tpl", "tpl"]);

        let region = layer.regions().next().unwrap();
        assert_eq!(region.name, "mine");
        assert!(region.blocked);
        // The template's points replace those read before it.
        assert_eq!(region.points.len(), 4);
        assert_eq!(region.points[0], Point { x: 1, y: 1 });
        assert_eq!(region.tint, Rgba::from_ints(255, 255, 255, 10));

        let entity = layer.entities().nth(2).unwrap();
        assert_eq!((entity.x, entity.y), (7, 50));
        assert_eq!(entity.sprite.as_deref(), Some("tpl.sprite"));
        assert_eq!(entity.scripts, ["tpl.script", "mine.script"]);
        assert_eq!(entity.tint, Some(Rgba::from_ints(255, 255, 255, 99)));

        let group = &scene.waypoint_groups[0];
        assert_eq!(group.name, "tpl");
        assert_eq!(group.points.len(), 3);
        assert_eq!(scene.scale_levels[0], ScaleLevel { y: 40, scale: 70.0 });
        assert_eq!(
            scene.rotation_levels[0],
            RotationLevel {
                x: 30,
                rotation: 15.0
            }
        );
    }

    /// A part that cannot be read is dropped alone: the original sets an
    /// error code that the next statement overwrites.
    #[test]
    fn a_part_that_fails_is_dropped_alone() {
        let scene = Scene::parse(
            br#"SCENE {
  LAYER { NAME="bad" BOGUS=1 }
  LAYER { NAME="good"
    REGION { NAME="bad" BOGUS=1 }
    REGION { NAME="good" }
    ENTITY { NAME="bad" BOGUS=1 }
    ENTITY { NAME="hotspot"
      REGION { POINT {1,1} BLOCKED=TRUE }
      BLOCKED_REGION { POINT {1,1} }
      WAYPOINTS { BOGUS=1 }
    }
  }
  WAYPOINTS { BOGUS=1 }
  SCALE_LEVEL { BOGUS=1 }
  ROTATION_LEVEL { BOGUS=1 }
  ENTITY { BOGUS=1 }
  PROPERTY { NAME="no value" }
  PROPERTY { NAME="a" VALUE="1" OTHER="x" }
  PROPERTY { NAME="a" NAME="b" VALUE="1" VALUE="2" }
  NAME="after"
}"#,
        )
        .unwrap();
        assert_eq!(scene.name, "after");
        assert_eq!(scene.layers.len(), 1);
        let layer = &scene.layers[0];
        assert_eq!(layer.regions().count(), 1);
        assert_eq!(layer.entities().count(), 1);
        let entity = layer.entities().next().unwrap();
        // `BLOCKED` is not a key of an entity's region (`CBRegion`).
        assert!(entity.region.is_none());
        assert!(entity.blocked_region.is_some());
        assert!(entity.waypoints.is_none());
        assert!(scene.waypoint_groups.is_empty());
        assert!(scene.scale_levels.is_empty() && scene.rotation_levels.is_empty());
        assert!(scene.free_entities.is_empty());
        // A property needs a name and a value; the last of each counts.
        assert_eq!(
            scene.properties,
            vec![Property {
                name: "b".into(),
                value: "2".into()
            }]
        );
    }

    /// `CAdScene::LoadBuffer VIEWPORT`: the original's `case` has no
    /// `break` and reads the viewport as `PERSISTENT_STATE` too; that
    /// defect is not reproduced.
    #[test]
    fn a_viewport_leaves_the_persistent_state_alone() {
        let scene =
            Scene::parse(b"SCENE { PERSISTENT_STATE=TRUE VIEWPORT { 0,0,800,600 } }").unwrap();
        assert!(scene.persistent_state);
        let scene = Scene::parse(b"SCENE { VIEWPORT { 1,0,800,600 } }").unwrap();
        assert!(!scene.persistent_state);
        let rect = scene.viewport.unwrap();
        assert_eq!((rect.left, rect.right, rect.bottom), (1, 800, 600));
    }

    /// `CAdEntity::LoadBuffer SOUND`: the volume and the start time
    /// count wherever they stand (the original only takes those read
    /// before `SOUND`).
    #[test]
    fn an_entity_sound_takes_its_values_in_any_order() {
        for text in [
            "ENTITY { SOUND=\"a.ogg\" SOUND_START_TIME=500 SOUND_VOLUME=40 }",
            "ENTITY { SOUND_START_TIME=500 SOUND_VOLUME=40 SOUND=\"a.ogg\" }",
        ] {
            let entity = Entity::parse(text.as_bytes()).unwrap();
            assert_eq!(entity.sound.as_deref(), Some("a.ogg"));
            assert_eq!((entity.sound_volume, entity.sound_start_ms), (40, 500));
        }
    }

    #[test]
    fn rejects_bad_files() {
        let err = Scene::parse(b"SPRITE { }").unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }), "{err}");
        let err = Scene::parse(b"SCENE { BOGUS=1 }").unwrap_err();
        assert!(matches!(err, Error::Syntax(_)), "{err}");
        // The root block must be the first statement.
        let err = Scene::parse(b"NAME=\"x\"\nSCENE { }").unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }), "{err}");
        let err = Entity::parse(b"SCENE { }").unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }), "{err}");
    }

    #[test]
    fn reads_the_3d_keys_with_their_defaults() {
        let scene = Scene::parse(b"SCENE { NAME=\"flat\" }").unwrap();
        assert_eq!(scene.geometry, None);
        assert_eq!(scene.camera, None);
        assert_eq!(scene.fov_override, None);
        assert_eq!(scene.near_clipping_plane, None);
        assert_eq!(scene.far_clipping_plane, None);
        assert_eq!(scene.waypoint_height, None);
        assert!(!scene.pathfinding_2d);
        assert_eq!(scene.max_shadow_type, 2);
        assert!(!scene.scroll_3d_compatibility);
        assert_eq!(scene.ambient_light_color, Rgb { r: 0, g: 0, b: 0 });

        let scene = Scene::parse(
            br#"SCENE {
  NAME="Warehouse"
  GEOMETRY="scenes\front\geometry.3ds"
  CAMERA="Camera01"
  MAX_SHADOW_TYPE=7
  FOV_OVERRIDE=1.25
  NEAR_CLIPPING_PLANE=-1
  FAR_CLIPPING_PLANE=5000
  WAYPOINT_HEIGHT=12.5
  2D_PATHFINDING=TRUE
  SCROLL_3D_COMPATIBILITY=TRUE
  AMBIENT_LIGHT_COLOR { 300, 20, -5 }
  EDITOR_RESOLUTION_WIDTH=800
}"#,
        )
        .unwrap();
        assert_eq!(
            scene.geometry.as_deref(),
            Some("scenes\\front\\geometry.3ds")
        );
        assert_eq!(scene.camera.as_deref(), Some("Camera01"));
        assert_eq!(scene.max_shadow_type, 3);
        assert_eq!(scene.fov_override, Some(1.25));
        assert_eq!(scene.near_clipping_plane, Some(-1.0));
        assert_eq!(scene.far_clipping_plane, Some(5000.0));
        assert_eq!(scene.waypoint_height, Some(12.5));
        assert!(scene.pathfinding_2d);
        assert!(scene.scroll_3d_compatibility);
        assert_eq!(
            scene.ambient_light_color,
            Rgb {
                r: 255,
                g: 20,
                b: 0
            },
            "each number kept within 0 to 255"
        );
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
