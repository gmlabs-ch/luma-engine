//! The 3D actor definition file (`.act3d`, keyword `ACTOR3DX`) and the
//! `.anim` sidecar of a merged animation file.
//!
//! The file is a [definition file](https://github.com/gmlabs-ch/luma-engine/blob/main/docs/formats/definition-files.md)
//! naming the `.X` model(s) of the actor, its speeds, its shadow and the
//! looping flag and events of its animation sets. [`Actor3dDef`] is the
//! typed result; it holds paths, not loaded files. The keys that act on
//! the model in the original (`MODEL`, `ANIMATION`, `SHADOW_MODEL`) are
//! also kept in file order ([`Actor3dDef::steps`]), and
//! [`Actor3dDef::build_models`] carries them out through a closure that
//! reads a file, so that the caller keeps its own file system. The keys
//! and their defaults are described in `docs/formats/actor3d.md`.
//!
//! Written with reference to the MIT-licensed Wintermute Engine 1.x
//! sources (`AdActor3DX.cpp`, `AdActor3D.cpp`, `AdObject3D.cpp`,
//! `XModel.cpp`, `BObject.cpp`, Copyright (c) 2003-2013 Jan Nedoma). See
//! `NOTICE` at the root of the repository.

use luma_formats::def::{Document, Node, Scalar, document_keys, keys, scan_floats};
use luma_formats::{Error, Property, Region, Result, Rgba, WaypointGroup};

use crate::animation::ShadowModel;
use crate::math::Vec3;
use crate::model::Model;

/// The keyword of a `.X` actor file.
pub const KEYWORD: &str = "ACTOR3DX";

/// The keyword of the legacy Milkshape actor files, which are not
/// supported.
pub const LEGACY_KEYWORD: &str = "ACTOR3D";

/// The extension of the animation sidecar, without the dot.
pub const ANIM_EXTENSION: &str = "anim";

/// How an actor casts its shadow (`TShadowType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ShadowType {
    /// No shadow.
    None = 0,
    /// A textured quad under the actor.
    Simple = 1,
    /// The silhouette projected on the floor plane.
    Flat = 2,
    /// Stencil shadow volumes.
    #[default]
    Stencil = 3,
}

impl ShadowType {
    /// A type from its number, clamped to `0..=3`.
    pub fn from_index(index: i64) -> ShadowType {
        match index {
            i64::MIN..=0 => ShadowType::None,
            1 => ShadowType::Simple,
            2 => ShadowType::Flat,
            _ => ShadowType::Stencil,
        }
    }

    /// The value of a `SHADOW_TYPE` key: exactly one of the names `none`,
    /// `simple`, `flat`, `stencil` (ignoring case, `stricmp`), else the
    /// number it starts with (`atoi`, which skips leading blanks),
    /// clamped. A name with a blank around it, as a value written in
    /// braces has, is therefore not a name and reads as 0.
    pub fn parse(text: &str) -> ShadowType {
        let t = text;
        if t.eq_ignore_ascii_case("none") {
            ShadowType::None
        } else if t.eq_ignore_ascii_case("simple") {
            ShadowType::Simple
        } else if t.eq_ignore_ascii_case("flat") {
            ShadowType::Flat
        } else if t.eq_ignore_ascii_case("stencil") {
            ShadowType::Stencil
        } else {
            let scalar = Scalar {
                text: t.to_string(),
                quoted: false,
            };
            ShadowType::from_index(scalar.as_int().unwrap_or(0))
        }
    }
}

/// An `EVENT` of an `ANIMATION` block: a script event raised when the
/// animation reaches a frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimationEventDef {
    /// The event name (`NAME`).
    pub name: String,
    /// The 1-based frame (`FRAME`), clamped to the set's frames when
    /// applied; -1 when absent, as the original's event starts.
    pub frame: i32,
}

/// One thing `CXModel::ParseAnim` does while it reads an `ANIMATION`
/// block, in the order of the block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnimationOp {
    /// A `NAME` key: the set the events that follow attach to, and the
    /// set `LOOPING` applies to when it is the last `NAME` of the block.
    /// The set is looked up at once and reported when the model lacks it.
    Name(String),
    /// A well-formed `EVENT` block, for the set of the `NAME` read last.
    Event(AnimationEventDef),
    /// An `EVENT` block that gives no event (a key other than `NAME` and
    /// `FRAME`, or no `NAME`). It still needs a `NAME` before it.
    DroppedEvent,
}

/// An `ANIMATION` block: properties of named animation sets of the
/// model, as `CXModel::ParseAnim` reads them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AnimationDef {
    /// The set's name: the last `NAME` of the block, empty without one.
    pub name: String,
    /// The `LOOPING` value applied to the set of the last `NAME` once the
    /// block is read: the last `LOOPING` key. `None` when the block has
    /// no such key and when the block stops on an error, since the
    /// original then returns before applying it.
    pub looping: Option<bool>,
    /// The well-formed events that follow a `NAME`, in file order,
    /// whatever set each one attaches to (see [`Self::ops`]).
    pub events: Vec<AnimationEventDef>,
    /// What the block does, in order.
    pub ops: Vec<AnimationOp>,
    /// Why the block stopped before its end, when it did: a key other
    /// than `NAME`, `LOOPING` and `EVENT`, or a syntax error. What comes
    /// before it in [`Self::ops`] is applied, `LOOPING` is not.
    pub error: Option<String>,
}

/// What applying an `ANIMATION` block, or a file of them, did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    /// Whether the original's function returns success.
    pub ok: bool,
    /// The lines the original writes to its log, in order.
    pub log: Vec<String>,
}

/// An `.anim` sidecar as `CAdActor3DX::MergeAnimations2` reads it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AnimationFile {
    /// The `ANIMATION` blocks read before the end of the file or the
    /// first statement that is not one.
    pub animations: Vec<AnimationDef>,
    /// Why reading stopped before the end of the file, when it did: a
    /// top-level key other than `ANIMATION`, or a syntax error. The
    /// blocks before it are applied all the same, and the merge fails.
    pub error: Option<String>,
}

/// An `EFFECT` block: an HLSL effect on a material. Parsed and kept, not
/// applied (Direct3D 9 material effects are excluded from the port).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectDef {
    /// The material name (`MATERIAL`), a texture file stem.
    pub material: String,
    /// The `.fx` file (`EFFECT_FILE`).
    pub effect_file: String,
}

/// A key of the actor file that acts on the model at its place in the
/// file (`CAdActor3DX::LoadBuffer`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelStep {
    /// `MODEL`: loads the model when the actor has none yet, else merges
    /// the file's animation sets into it.
    Model(String),
    /// `ANIMATION`: applies the block to the model as loaded so far.
    Animation(AnimationDef),
    /// `SHADOW_MODEL`: loads a second model with the model as its parent.
    ShadowModel(String),
}

/// The models of an actor, built by [`Actor3dDef::build_models`].
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltModels {
    /// The model: the first `MODEL` that loaded, with the sets of the
    /// later ones merged and the `ANIMATION` blocks applied in order.
    pub model: Model,
    /// The file [`Self::model`] was loaded from.
    pub model_file: String,
    /// The `MODEL` files merged into it without error, in order.
    pub merged_files: Vec<String>,
    /// The `SHADOW_MODEL`, when the last one loaded.
    pub shadow: Option<ShadowModel>,
    /// The file [`Self::shadow`] was loaded from.
    pub shadow_file: Option<String>,
    /// The lines the original writes to its log on the way.
    pub warnings: Vec<String>,
}

/// A parsed `.act3d` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Actor3dDef {
    /// Name used by scripts (`NAME`).
    pub name: String,
    /// Caption shown when the mouse is over the actor (`CAPTION`).
    pub caption: String,
    /// The last `TEMPLATE` key, recorded by [`Actor3dDef::parse`] and
    /// loaded in place by [`Actor3dDef::parse_with`].
    pub template: Option<String>,
    /// The path of the last template file that could be read, when there
    /// is one: the original's `LoadFile` on a template overwrites the
    /// actor's file name, so this is what the actor's `Filename` answers.
    pub filename_override: Option<String>,
    /// Font of the actor's speech (`FONT`).
    pub font: Option<String>,
    /// Cursor sprite shown over the actor (the last `CURSOR`). The
    /// original loads it at the key; when it cannot be loaded the actor
    /// has no cursor and the load goes on.
    pub cursor: Option<String>,
    /// Scripts attached to the actor (`SCRIPT`, repeatable).
    pub scripts: Vec<String>,
    /// Script properties (`PROPERTY`).
    pub properties: Vec<Property>,
    /// Drawn and updated (`ACTIVE`).
    pub active: bool,
    /// Clickable (`INTERACTIVE` or `REGISTRABLE`).
    pub interactive: bool,
    /// Initial 3D position (`X`, `Y`, `Z`).
    pub position: Vec3,
    /// Initial heading in degrees (`ANGLE`), as written.
    pub angle: f32,
    /// Uniform scale (`SCALE`, a percentage in the file, stored divided
    /// by 100).
    pub scale: f32,
    /// Walking speed in world units per second (`VELOCITY`).
    pub velocity: f32,
    /// Turning speed in degrees per second (`ANGULAR_VELOCITY`).
    pub angular_velocity: f32,
    /// Re-seat the actor on the walk plane every update
    /// (`DROP_TO_FLOOR`).
    pub drop_to_floor: bool,
    /// Draw both sides of the triangles (`DRAW_BACKFACES`).
    pub draw_backfaces: bool,
    /// The `.X` files (`MODEL`, repeatable), in file order. See
    /// [`Self::steps`] for what the original does with each.
    pub models: Vec<String>,
    /// The low-polygon model used for shadows: the last `SHADOW_MODEL`
    /// that follows a `MODEL` key.
    pub shadow_model: Option<String>,
    /// The actor's own shadow type (`SHADOW_TYPE`, `SHADOW`,
    /// `SIMPLE_SHADOW`).
    pub shadow_type: ShadowType,
    /// Colour of flat and stencil shadows (`SHADOW_COLOR`).
    pub shadow_color: Rgba,
    /// Image of the simple shadow (`SHADOW_IMAGE`); the game's when
    /// absent.
    pub shadow_image: Option<String>,
    /// Half-size of the simple shadow quad in world units
    /// (`SHADOW_SIZE`).
    pub shadow_size: f32,
    /// Position of the shadow-casting light relative to the actor
    /// (`LIGHT_POSITION`).
    pub light_position: Vec3,
    /// The `ANIMATION` blocks with a `NAME` that follow a `MODEL` key.
    pub animations: Vec<AnimationDef>,
    /// The `EFFECT` blocks that followed a `MODEL`.
    pub effects: Vec<EffectDef>,
    /// The `MODEL`, `ANIMATION` and `SHADOW_MODEL` keys in file order,
    /// since the last point where the original drops the model (the
    /// start of the file and of every template that could be read):
    /// what [`Self::build_models`] carries out.
    pub steps: Vec<ModelStep>,
    /// The private blocked polygon for 2D path finding
    /// (`BLOCKED_REGION`).
    pub blocked_region: Option<Region>,
    /// The private waypoints for 2D path finding (`WAYPOINTS`).
    pub waypoints: Option<WaypointGroup>,
    /// The lines the original writes to its log while it reads the file:
    /// a template that cannot be read or loaded.
    pub log: Vec<String>,
}

impl Default for Actor3dDef {
    fn default() -> Self {
        Actor3dDef {
            name: luma_formats::UNNAMED.to_string(),
            caption: String::new(),
            template: None,
            filename_override: None,
            font: None,
            cursor: None,
            scripts: Vec::new(),
            properties: Vec::new(),
            active: true,
            interactive: true,
            position: Vec3::ZERO,
            angle: 0.0,
            scale: 1.0,
            velocity: 1.0,
            angular_velocity: 1.0,
            drop_to_floor: true,
            draw_backfaces: true,
            models: Vec::new(),
            shadow_model: None,
            shadow_type: ShadowType::Stencil,
            shadow_color: Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 128,
            },
            shadow_image: None,
            shadow_size: 10.0,
            light_position: Vec3::new(-40.0, 200.0, -40.0),
            animations: Vec::new(),
            effects: Vec::new(),
            steps: Vec::new(),
            blocked_region: None,
            waypoints: None,
            log: Vec::new(),
        }
    }
}

/// A source of template files for [`Actor3dDef::parse_with`]: the bytes
/// of a path, `None` when it cannot be read.
pub type TemplateSource<'a> = dyn FnMut(&str) -> Option<Vec<u8>> + 'a;

/// The keys of `CAdActor3DX::LoadBuffer`, in the order of its table.
const ACTOR3DX_KEYS: &[&str] = &[
    "ACTOR3DX",
    "X",
    "Y",
    "Z",
    "ANGLE",
    "VELOCITY",
    "ANGULAR_VELOCITY",
    "TEMPLATE",
    "NAME",
    "REGISTRABLE",
    "INTERACTIVE",
    "ACTIVE",
    "MODEL",
    "EVENTS",
    "FONT",
    "CURSOR",
    "DROP_TO_FLOOR",
    "SCRIPT",
    "CAPTION",
    "PROPERTY",
    "ANIMATION",
    "EDITOR_PROPERTY",
    "SHADOW_IMAGE",
    "SHADOW_SIZE",
    "SIMPLE_SHADOW",
    "SHADOW_COLOR",
    "SHADOW_MODEL",
    "SHADOW_TYPE",
    "LIGHT_POSITION",
    "SHADOW",
    "SCALE",
    "DRAW_BACKFACES",
    "BLOCKED_REGION",
    "WAYPOINTS",
    "EFFECT",
];

/// The keys of `CXModel::ParseAnim`.
const ANIMATION_KEYS: &[&str] = &["NAME", "LOOPING", "EVENT"];

/// The keys of `CXModel::ParseEvent`.
const EVENT_KEYS: &[&str] = &["NAME", "FRAME"];

/// The keys of `CAdActor3DX::ParseEffect`.
const EFFECT_KEYS: &[&str] = &["MATERIAL", "EFFECT_FILE"];

/// The keys of `CAdActor3DX::MergeAnimations2`.
const ANIMATION_FILE_KEYS: &[&str] = &["ANIMATION"];

/// How deep `TEMPLATE` files may load one another. The original has no
/// bound and a file naming itself overflows its stack.
const MAX_TEMPLATE_DEPTH: u32 = 16;

/// How many template files one actor file may load in all, so that
/// files naming one another several times cannot multiply without end.
const MAX_TEMPLATE_LOADS: u32 = 64;

impl Actor3dDef {
    /// Whether a file is a `.X` actor definition: its first keyword is
    /// `ACTOR3DX` (`CAdActor3D::IsActorX`). Anything else, including the
    /// legacy `ACTOR3D` files, is not.
    pub fn is_actor_x(bytes: &[u8]) -> bool {
        Document::parse(bytes)
            .ok()
            .and_then(|doc| {
                doc.nodes
                    .first()
                    .map(|n| n.key.eq_ignore_ascii_case(KEYWORD))
            })
            .unwrap_or(false)
    }

    /// Parses an `.act3d` file. A `TEMPLATE` is recorded in
    /// [`Self::template`] and not loaded; the keys of the file that
    /// follow it are applied as the original would apply them over the
    /// template (the model keys written before it are dropped). Fails on
    /// a syntax error, a root other than `ACTOR3DX`, a key the original
    /// does not know, and, when there is no template, on a file without
    /// `MODEL`.
    pub fn parse(bytes: &[u8]) -> Result<Actor3dDef> {
        let mut def = Actor3dDef::default();
        def.apply_file(bytes, None, 0, &mut 0)?;
        Ok(def)
    }

    /// Parses an `.act3d` file and loads its `TEMPLATE` files in place
    /// through `templates`, as the original does: at the key, the
    /// template file (itself an `ACTOR3DX` file, with templates of its
    /// own) is loaded over the values read so far by the same function,
    /// which first drops the model and the shadow model.
    ///
    /// A template that cannot be read changes nothing; one that fails
    /// (a syntax error, an unknown key, no `MODEL`) keeps what it had
    /// applied before failing. Neither fails the actor: the original
    /// sets an error code there that the next statement overwrites. Both
    /// are reported in [`Self::log`].
    ///
    /// Fails on a syntax error or an unknown key of the file itself, on
    /// a root other than `ACTOR3DX`, and when no `MODEL` key is left
    /// (whether a model really loads is known by
    /// [`Self::build_models`]).
    pub fn parse_with(bytes: &[u8], templates: &mut TemplateSource<'_>) -> Result<Actor3dDef> {
        let mut def = Actor3dDef::default();
        def.apply_file(bytes, Some(templates), 0, &mut 0)?;
        Ok(def)
    }

    /// `CAdActor3DX::LoadBuffer`: the keys of the root block in order,
    /// over what the definition holds. On an error what was read before
    /// it stays applied.
    fn apply_file(
        &mut self,
        bytes: &[u8],
        mut templates: Option<&mut TemplateSource<'_>>,
        depth: u32,
        loads: &mut u32,
    ) -> Result<()> {
        let doc = Document::parse(bytes)?;
        let root = doc.nodes.first().ok_or_else(|| Error::Invalid {
            reason: format!("'{KEYWORD}' keyword expected"),
        })?;
        if root.key.eq_ignore_ascii_case(LEGACY_KEYWORD) {
            return Err(Error::Invalid {
                reason: format!(
                    "'{LEGACY_KEYWORD}' is a legacy Milkshape actor, which is not supported; '{KEYWORD}' expected"
                ),
            });
        }
        if !root.key.eq_ignore_ascii_case(KEYWORD) {
            return Err(Error::Invalid {
                reason: format!("'{KEYWORD}' keyword expected"),
            });
        }
        // The original deletes the model and the shadow model here.
        self.drop_models();
        for item in keys(root, ACTOR3DX_KEYS, KEYWORD) {
            let (key, child) = item?;
            match key {
                "TEMPLATE" => {
                    let path = child.text();
                    self.template = Some(path.clone());
                    match templates.as_deref_mut() {
                        // Not loaded: what a template would drop is
                        // dropped.
                        None => self.drop_models(),
                        Some(_) if depth >= MAX_TEMPLATE_DEPTH || *loads >= MAX_TEMPLATE_LOADS => {
                            self.log.push(format!(
                                "Error: template '{path}' is nested too deep and is not loaded"
                            ));
                        }
                        Some(source) => match source(&path) {
                            None => self
                                .log
                                .push(format!("CAdActor3DX::LoadFile failed for file '{path}'")),
                            Some(bytes) => {
                                *loads += 1;
                                self.filename_override = Some(path.clone());
                                if let Err(err) =
                                    self.apply_file(&bytes, Some(source), depth + 1, loads)
                                {
                                    self.log.push(err.to_string());
                                    self.log
                                        .push(format!("Error parsing ACTOR3D file '{path}'"));
                                }
                            }
                        },
                    }
                }
                "NAME" => self.name = child.text(),
                "CAPTION" => self.caption = child.text(),
                "FONT" => self.font = Some(child.text()),
                "CURSOR" => self.cursor = Some(child.text()),
                "SCRIPT" => self.scripts.push(child.text()),
                "PROPERTY" => self.properties.extend(Property::from_node(child)),
                "ACTIVE" => child.read_bool(&mut self.active),
                "INTERACTIVE" | "REGISTRABLE" => child.read_bool(&mut self.interactive),
                "X" => child.read_float(&mut self.position.x),
                "Y" => child.read_float(&mut self.position.y),
                "Z" => child.read_float(&mut self.position.z),
                // The original computes the normalised angle and discards
                // it, so the value stays as written.
                "ANGLE" => child.read_float(&mut self.angle),
                // The division follows the scan whether or not it read a
                // value.
                "SCALE" => {
                    child.read_float(&mut self.scale);
                    self.scale /= 100.0;
                }
                "VELOCITY" => child.read_float(&mut self.velocity),
                "ANGULAR_VELOCITY" => child.read_float(&mut self.angular_velocity),
                "DROP_TO_FLOOR" => child.read_bool(&mut self.drop_to_floor),
                "DRAW_BACKFACES" => child.read_bool(&mut self.draw_backfaces),
                "MODEL" => {
                    let path = child.text();
                    self.models.push(path.clone());
                    self.steps.push(ModelStep::Model(path));
                }
                "SHADOW_MODEL" => {
                    let path = child.text();
                    // Without a model before it, the original logs an
                    // error and ignores the key.
                    if !self.models.is_empty() {
                        self.shadow_model = Some(path.clone());
                    }
                    self.steps.push(ModelStep::ShadowModel(path));
                }
                "SHADOW_TYPE" => self.shadow_type = ShadowType::parse(child.as_str().unwrap_or("")),
                // An empty value leaves an uninitialised flag in the
                // original; it changes nothing here.
                "SHADOW" => {
                    if child.as_bool() == Some(false) {
                        self.shadow_type = ShadowType::None;
                    }
                }
                "SIMPLE_SHADOW" => {
                    if child.as_bool() == Some(true) {
                        self.shadow_type = ShadowType::Simple;
                    }
                }
                "SHADOW_COLOR" => {
                    let v = child.as_ints().unwrap_or_default();
                    let channel = |i: usize| v.get(i).copied().unwrap_or(0).clamp(0, 255) as u8;
                    self.shadow_color = Rgba {
                        r: channel(0),
                        g: channel(1),
                        b: channel(2),
                        a: channel(3),
                    };
                }
                "SHADOW_IMAGE" => self.shadow_image = Some(child.text()),
                "SHADOW_SIZE" => {
                    child.read_float(&mut self.shadow_size);
                    self.shadow_size = self.shadow_size.max(0.0);
                }
                // `%f,%f,%f`: the coordinates the scan reaches are set,
                // the others keep their value.
                "LIGHT_POSITION" => {
                    let values = scan_floats(child.as_str().unwrap_or(""));
                    let light = &mut self.light_position;
                    for (slot, value) in [&mut light.x, &mut light.y, &mut light.z]
                        .into_iter()
                        .zip(values)
                    {
                        *slot = value as f32;
                    }
                }
                "ANIMATION" => {
                    let animation = AnimationDef::from_node(child);
                    // The original needs a model to attach the block to.
                    if !self.models.is_empty() && animation.has_name() {
                        self.animations.push(animation.clone());
                    }
                    self.steps.push(ModelStep::Animation(animation));
                }
                "EFFECT" => {
                    if !self.models.is_empty() {
                        if let Some(effect) = EffectDef::from_node(child) {
                            self.effects.push(effect);
                        }
                    }
                }
                // A region or a group that cannot be read leaves the
                // actor without one, and the load goes on.
                "BLOCKED_REGION" => self.blocked_region = Region::from_node(child, false).ok(),
                "WAYPOINTS" => self.waypoints = WaypointGroup::from_node(child).ok(),
                // `ACTOR3DX` and `EVENTS` are in the table and have no
                // case; `EDITOR_PROPERTY` only acts in the editor.
                _ => {}
            }
        }
        if self.models.is_empty() && (templates.is_some() || self.template.is_none()) {
            return Err(no_model());
        }
        Ok(())
    }

    /// What `SAFE_DELETE(m_ModelX)` and `SAFE_DELETE(m_ShadowModel)` at
    /// the start of `LoadBuffer` undo: everything that acted on them.
    fn drop_models(&mut self) {
        self.models.clear();
        self.shadow_model = None;
        self.animations.clear();
        self.effects.clear();
        self.steps.clear();
    }

    /// Loads the models as the original does while it reads the file:
    /// the steps of [`Self::steps`] in order, reading each file through
    /// `read` (`None` when it cannot be read).
    ///
    /// - `MODEL` loads the model when there is none yet; a file that
    ///   fails leaves the actor without a model, and the next `MODEL` is
    ///   then the one loaded. With a model, it merges the animation sets
    ///   of the file ([`Model::merge_animations`]); a failure is logged
    ///   and the sets read before it stay.
    /// - `ANIMATION` applies its block to the model as loaded so far
    ///   ([`AnimationDef::apply_to`]): a set a later `MODEL` brings is
    ///   reported as missing. Without a model the block is only logged.
    /// - `SHADOW_MODEL` drops the shadow model and loads the file with
    ///   the model as its parent ([`ShadowModel::parse`]); a failure
    ///   leaves the actor without a shadow model. Without a model the
    ///   key is only logged.
    ///
    /// None of these failures stops the load in the original (it sets an
    /// error code that the next statement overwrites). The load fails
    /// when, at the end, **no model is loaded**; the error is then the
    /// log, its last line saying so. `file_name` is the actor's file,
    /// for the messages.
    pub fn build_models(
        &self,
        file_name: &str,
        read: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
    ) -> std::result::Result<BuiltModels, Vec<String>> {
        let mut log = Vec::new();
        let mut model: Option<(Model, String)> = None;
        let mut merged_files = Vec::new();
        let mut shadow: Option<(ShadowModel, String)> = None;
        for step in &self.steps {
            match step {
                ModelStep::Model(path) => match (&mut model, read(path)) {
                    (None, None) => log.push(format!("Error loading model '{path}'")),
                    (None, Some(bytes)) => match Model::parse(&bytes, path) {
                        Ok(loaded) => model = Some((loaded, path.clone())),
                        Err(err) => log.push(format!("Error loading model '{path}': {err}")),
                    },
                    (Some(_), None) => log.push(format!("Error merging model '{path}'")),
                    (Some((model, _)), Some(bytes)) => match model.merge_animations(&bytes, path) {
                        Ok(_) => merged_files.push(path.clone()),
                        Err(err) => log.push(format!("Error merging model '{path}': {err}")),
                    },
                },
                ModelStep::Animation(animation) => match &mut model {
                    Some((model, _)) => log.extend(animation.apply_to(model).log),
                    None => log.push(format!(
                        "Error: a MODEL= line must precede any animation definitions (file: {file_name})"
                    )),
                },
                ModelStep::ShadowModel(path) => match &model {
                    Some((model, _)) => {
                        shadow = None;
                        match read(path) {
                            None => log.push(format!("Error loading shadow model '{path}'")),
                            Some(bytes) => match ShadowModel::parse(&bytes, path, model) {
                                Ok(loaded) => shadow = Some((loaded, path.clone())),
                                Err(err) => {
                                    log.push(format!("Error loading shadow model '{path}': {err}"));
                                }
                            },
                        }
                    }
                    None => log.push(format!(
                        "Error: a MODEL= line must precede shadow model assignment (file: {file_name})"
                    )),
                },
            }
        }
        let Some((model, model_file)) = model else {
            log.push("Error: No model has been loaded for 3D actor".to_string());
            return Err(log);
        };
        let (shadow, shadow_file) = match shadow {
            Some((shadow, file)) => (Some(shadow), Some(file)),
            None => (None, None),
        };
        Ok(BuiltModels {
            model,
            model_file,
            merged_files,
            shadow,
            shadow_file,
            warnings: log,
        })
    }

    /// Applies every block of [`Self::animations`] to a loaded model,
    /// whatever its place among the `MODEL` keys, and returns the names
    /// the blocks give that the model has no set for.
    /// [`Self::build_models`] is the original's order, where a block
    /// only sees the sets of the `MODEL` keys before it.
    pub fn apply_animations(&self, model: &mut Model) -> Vec<String> {
        let mut missing = Vec::new();
        for animation in &self.animations {
            for op in &animation.ops {
                if let AnimationOp::Name(name) = op {
                    if model.animation_set(name).is_none() {
                        missing.push(name.clone());
                    }
                }
            }
            animation.apply_to(model);
        }
        missing
    }

    /// The `.anim` sidecar of a model file: the same directory and base
    /// name with the extension `anim` (`actors\x\throw.x` gives
    /// `actors\x\throw.anim`).
    pub fn anim_sidecar(model_path: &str) -> String {
        let stem_end = match model_path.rfind('.') {
            Some(dot) if !model_path[dot..].contains(['/', '\\']) => dot,
            _ => model_path.len(),
        };
        format!("{}.{ANIM_EXTENSION}", &model_path[..stem_end])
    }
}

impl AnimationDef {
    /// Reads an `ANIMATION` block as `CXModel::ParseAnim` does: the keys
    /// `NAME`, `LOOPING` and `EVENT` in order. Any other key, or a place
    /// where the block is not a sequence of statements, stops the block
    /// there ([`Self::error`]). An `EVENT` block knows `NAME` and
    /// `FRAME` (`ParseEvent`); with another key, or without a `NAME`, it
    /// gives no event, and the block goes on.
    pub fn from_node(node: &Node) -> AnimationDef {
        let mut animation = AnimationDef::default();
        // `ScanStr("%b")` leaves the flag alone on an empty value.
        let mut looping = false;
        let mut looping_set = false;
        let mut named = false;
        for item in keys(node, ANIMATION_KEYS, "ANIMATION") {
            let (key, child) = match item {
                Ok(item) => item,
                Err(err) => {
                    animation.error = Some(err.to_string());
                    break;
                }
            };
            match key {
                "NAME" => {
                    animation.name = child.text();
                    animation.ops.push(AnimationOp::Name(child.text()));
                    named = true;
                }
                "LOOPING" => {
                    child.read_bool(&mut looping);
                    looping_set = true;
                }
                _ => match AnimationEventDef::from_node(child) {
                    Some(event) => {
                        if named {
                            animation.events.push(event.clone());
                        }
                        animation.ops.push(AnimationOp::Event(event));
                    }
                    None => animation.ops.push(AnimationOp::DroppedEvent),
                },
            }
        }
        if animation.error.is_none() && looping_set {
            animation.looping = Some(looping);
        }
        animation
    }

    /// Whether the block has a `NAME` key.
    pub fn has_name(&self) -> bool {
        self.ops.iter().any(|op| matches!(op, AnimationOp::Name(_)))
    }

    /// Parses an `.anim` sidecar and returns its blocks, or the error
    /// that stopped the reading. The original applies the blocks read
    /// before such an error; [`AnimationFile::parse`] keeps them.
    pub fn parse_file(bytes: &[u8]) -> Result<Vec<AnimationDef>> {
        let file = AnimationFile::parse(bytes);
        match file.error {
            Some(reason) => Err(Error::Invalid { reason }),
            None => Ok(file.animations),
        }
    }

    /// Applies the block to a model as `CXModel::ParseAnim` does while it
    /// reads it, and returns what the original logs and answers:
    ///
    /// - at each `NAME`, a set the model lacks (looked up ignoring ASCII
    ///   case) is logged;
    /// - each event is added to the set of the `NAME` read last, when the
    ///   model has it, its frame clamped to the set's frames; an `EVENT`
    ///   before any `NAME` is logged and dropped;
    /// - a block that stopped on an error fails here, the events before
    ///   the error added and `LOOPING` not applied;
    /// - else the last `LOOPING` is applied to the set of the last
    ///   `NAME`, when the model has it.
    pub fn apply_to(&self, model: &mut Model) -> Applied {
        let ticks = model.ticks_per_second;
        let mut log = Vec::new();
        let mut current: Option<&str> = None;
        for op in &self.ops {
            match op {
                AnimationOp::Name(name) => {
                    current = Some(name);
                    if model.animation_set(name).is_none() {
                        log.push(format!(
                            "Error: Animation '{name}' cannot be found in the model."
                        ));
                    }
                }
                AnimationOp::Event(_) | AnimationOp::DroppedEvent => {
                    let Some(name) = current else {
                        log.push(
                            "Error: NAME filed must precede any EVENT fields in actor definition files."
                                .to_string(),
                        );
                        continue;
                    };
                    if let (AnimationOp::Event(event), Some(set)) =
                        (op, model.animation_set_mut(name))
                    {
                        set.add_event(&event.name, event.frame, ticks);
                    }
                }
            }
        }
        if self.error.is_some() {
            return Applied { ok: false, log };
        }
        if let (Some(name), Some(looping)) = (current, self.looping) {
            if let Some(set) = model.animation_set_mut(name) {
                set.looping = looping;
            }
        }
        Applied { ok: true, log }
    }

    /// [`Self::apply_to`] without its log: whether the original's
    /// `ParseAnim` succeeds.
    pub fn apply(&self, model: &mut Model) -> bool {
        self.apply_to(model).ok
    }
}

impl AnimationEventDef {
    /// `CXModel::ParseEvent`: the keys `NAME` and `FRAME`, the last of
    /// each counting. `None` when the block has another key or no
    /// `NAME`: the original then drops the event.
    fn from_node(node: &Node) -> Option<AnimationEventDef> {
        let mut name = None;
        let mut frame = -1;
        for item in keys(node, EVENT_KEYS, "EVENT") {
            let (key, child) = item.ok()?;
            match key {
                "NAME" => name = Some(child.text()),
                _ => child.read_int(&mut frame),
            }
        }
        Some(AnimationEventDef { name: name?, frame })
    }
}

impl AnimationFile {
    /// Reads an `.anim` sidecar as `CAdActor3DX::MergeAnimations2` does:
    /// top-level `ANIMATION` blocks, up to the end of the file or to the
    /// first statement that is something else, where the original stops
    /// with `Syntax error in animation definition file`.
    pub fn parse(bytes: &[u8]) -> AnimationFile {
        let mut file = AnimationFile::default();
        let doc = match Document::parse(bytes) {
            Ok(doc) => doc,
            Err(err) => {
                // Not a statement to begin with.
                file.error = Some(format!("Syntax error in animation definition file: {err}"));
                return file;
            }
        };
        for item in document_keys(&doc, ANIMATION_FILE_KEYS, "animation") {
            match item {
                Ok((_, node)) => file.animations.push(AnimationDef::from_node(node)),
                Err(err) => {
                    file.error = Some(format!("Syntax error in animation definition file: {err}"));
                    break;
                }
            }
        }
        file
    }

    /// Applies the blocks to a model in order and returns what the
    /// original logs and answers. A block that fails does not fail the
    /// file (the error code it sets is overwritten when the next
    /// statement is read); the file fails only when its reading stopped
    /// on [`Self::error`], after the blocks before it were applied.
    pub fn apply_to(&self, model: &mut Model) -> Applied {
        let mut log = Vec::new();
        for animation in &self.animations {
            log.extend(animation.apply_to(model).log);
        }
        if let Some(error) = &self.error {
            log.push(error.clone());
        }
        Applied {
            ok: self.error.is_none(),
            log,
        }
    }
}

impl EffectDef {
    /// Builds a block from an `EFFECT` node (`ParseEffect`): the keys
    /// `MATERIAL` and `EFFECT_FILE`, the last of each counting; kept
    /// only when both are present and the block has no other key.
    pub fn from_node(node: &Node) -> Option<EffectDef> {
        let mut material = None;
        let mut effect_file = None;
        for item in keys(node, EFFECT_KEYS, "EFFECT") {
            let (key, child) = item.ok()?;
            match key {
                "MATERIAL" => material = Some(child.text()),
                _ => effect_file = Some(child.text()),
            }
        }
        Some(EffectDef {
            material: material?,
            effect_file: effect_file?,
        })
    }
}

fn no_model() -> Error {
    Error::Invalid {
        reason: "no MODEL has been given for the 3D actor".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRINITY: &str = r#"ACTOR3DX
{
  NAME = "trinity"
  CAPTION = "Trinity"
  INTERACTIVE = TRUE
  ACTIVE = TRUE
  SCRIPT = "actors\trinity\trinity.script"
  SHADOW_IMAGE = "actors\trinity\shadow.png"
  SHADOW_SIZE = 12.0
  LIGHT_POSITION { -40, 200, -40 }
  SHADOW_TYPE = "stencil"
;  SHADOW_COLOR { 0, 0, 0, 128 }
  SCALE = 180

  ;--- velocity
  VELOCITY = 70.0
  ANGULAR_VELOCITY = 400.0

  ;--- external data
  MODEL = "actors\trinity\trinity.x"
  FONT = "fonts\outline_white.font"

  ANIMATION
  {
    NAME="idle"
    LOOPING=TRUE
  }

  ANIMATION
  {
    NAME="walk"
    LOOPING=TRUE

    EVENT
    {
      FRAME = 4
      NAME = "footstep"
    }

    EVENT
    {
      FRAME = 16
      NAME = "footstep"
    }
  }
}
"#;

    const MODEL: &str = r#"xof 0303txt 0032
Frame Bone { FrameTransformMatrix { 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1;; } }
AnimationSet walk {
 Animation {
  { Bone }
  AnimationKey {
   2;
   5;
   0;3;0,0,0;;,
   1200;3;0,1,0;;,
   2400;3;0,2,0;;,
   3600;3;0,3,0;;,
   4800;3;0,4,0;;;
  }
 }
}
AnimationSet idle {
 Animation {
  { Bone }
  AnimationKey { 2; 1; 0;3;0,0,0;;; }
 }
}
"#;

    #[test]
    fn parses_the_demo_actor() {
        let def = Actor3dDef::parse(TRINITY.as_bytes()).unwrap();
        assert_eq!(def.name, "trinity");
        assert_eq!(def.caption, "Trinity");
        assert!(def.interactive && def.active);
        assert_eq!(def.scripts, vec!["actors\\trinity\\trinity.script"]);
        assert_eq!(
            def.shadow_image.as_deref(),
            Some("actors\\trinity\\shadow.png")
        );
        assert_eq!(def.shadow_size, 12.0);
        assert_eq!(def.light_position, Vec3::new(-40.0, 200.0, -40.0));
        assert_eq!(def.shadow_type, ShadowType::Stencil);
        assert_eq!(def.shadow_color.a, 128);
        assert_eq!(def.scale, 1.8);
        assert_eq!(def.velocity, 70.0);
        assert_eq!(def.angular_velocity, 400.0);
        assert_eq!(def.models, vec!["actors\\trinity\\trinity.x"]);
        assert_eq!(def.font.as_deref(), Some("fonts\\outline_white.font"));
        assert_eq!(def.template, None);
        assert_eq!(def.animations.len(), 2);
        assert_eq!(def.animations[0].name, "idle");
        assert_eq!(def.animations[0].looping, Some(true));
        assert!(def.animations[0].events.is_empty());
        assert_eq!(
            def.animations[1].events,
            vec![
                AnimationEventDef {
                    name: "footstep".into(),
                    frame: 4
                },
                AnimationEventDef {
                    name: "footstep".into(),
                    frame: 16
                }
            ]
        );
        // Defaults the file does not set.
        assert_eq!(def.position, Vec3::ZERO);
        assert_eq!(def.angle, 0.0);
        assert!(def.drop_to_floor && def.draw_backfaces);
        assert!(def.blocked_region.is_none() && def.waypoints.is_none());
        assert!(Actor3dDef::is_actor_x(TRINITY.as_bytes()));
    }

    #[test]
    fn applies_the_defaults_and_the_odd_keys() {
        let text = r#"ACTOR3DX
{
  MODEL = "a.x"
  MODEL = "b.x"
  SHADOW_MODEL = "low.x"
  X = 1.5
  Y = -2
  Z = 3
  ANGLE = 725
  SCALE = 50
  SHADOW_SIZE = -4
  SHADOW_COLOR { 300, 10, 20, -1 }
  LIGHT_POSITION { 1.5, 2, 3.25 }
  DROP_TO_FLOOR = FALSE
  DRAW_BACKFACES = FALSE
  REGISTRABLE = FALSE
  ACTIVE = NO
  CURSOR = "c.sprite"
  PROPERTY { NAME = "mood"  VALUE = "happy" }
  EDITOR_PROPERTY { NAME = "x"  VALUE = "y" }
  SHADOW = FALSE
  EFFECT { MATERIAL = "skin"  EFFECT_FILE = "fx\glow.fx" }
  EFFECT { MATERIAL = "skin" }
  BLOCKED_REGION { POINT { 0, 0 }  POINT { 10, 0 }  POINT { 10, 10 } }
  WAYPOINTS { POINT { 5, 5 } }
  ANIMATION { LOOPING = TRUE }
  ANIMATION { EVENT { NAME = "early" FRAME = 1 }  NAME = "walk"  EVENT { FRAME = 3 }  EVENT { NAME = "late" } }
}
"#;
        let def = Actor3dDef::parse(text.as_bytes()).unwrap();
        assert_eq!(def.models, vec!["a.x", "b.x"]);
        assert_eq!(def.shadow_model.as_deref(), Some("low.x"));
        assert_eq!(def.position, Vec3::new(1.5, -2.0, 3.0));
        assert_eq!(def.angle, 725.0, "the angle is kept as written");
        assert_eq!(def.scale, 0.5);
        assert_eq!(def.shadow_size, 0.0);
        assert_eq!(
            def.shadow_color,
            Rgba {
                r: 255,
                g: 10,
                b: 20,
                a: 0
            }
        );
        assert_eq!(def.light_position, Vec3::new(1.5, 2.0, 3.25));
        assert!(!def.drop_to_floor && !def.draw_backfaces);
        assert!(!def.interactive && !def.active);
        assert_eq!(def.cursor.as_deref(), Some("c.sprite"));
        assert_eq!(def.properties.len(), 1);
        assert_eq!(def.properties[0].value, "happy");
        assert_eq!(def.shadow_type, ShadowType::None);
        assert_eq!(
            def.effects,
            vec![EffectDef {
                material: "skin".into(),
                effect_file: "fx\\glow.fx".into()
            }]
        );
        assert_eq!(def.blocked_region.as_ref().unwrap().points.len(), 3);
        assert_eq!(def.waypoints.as_ref().unwrap().points.len(), 1);
        // A block without NAME names no set; events before the NAME and
        // events without a NAME are dropped; FRAME starts at -1.
        assert_eq!(def.animations.len(), 1);
        assert_eq!(def.animations[0].name, "walk");
        assert_eq!(def.animations[0].looping, None);
        let late = AnimationEventDef {
            name: "late".into(),
            frame: -1,
        };
        assert_eq!(def.animations[0].events, vec![late.clone()]);
        assert_eq!(
            def.animations[0].ops,
            vec![
                AnimationOp::Event(AnimationEventDef {
                    name: "early".into(),
                    frame: 1
                }),
                AnimationOp::Name("walk".into()),
                AnimationOp::DroppedEvent,
                AnimationOp::Event(late),
            ]
        );
        // Every model key is a step, in file order, the block without a
        // name included.
        assert_eq!(def.steps.len(), 5);
        assert_eq!(def.steps[0], ModelStep::Model("a.x".into()));
        assert_eq!(def.steps[1], ModelStep::Model("b.x".into()));
        assert_eq!(def.steps[2], ModelStep::ShadowModel("low.x".into()));
        assert!(matches!(&def.steps[3], ModelStep::Animation(a) if !a.has_name()));
    }

    #[test]
    fn shadow_type_keys_follow_the_original() {
        assert_eq!(ShadowType::parse("None"), ShadowType::None);
        assert_eq!(ShadowType::parse("SIMPLE"), ShadowType::Simple);
        assert_eq!(ShadowType::parse("flat"), ShadowType::Flat);
        assert_eq!(ShadowType::parse("stencil"), ShadowType::Stencil);
        assert_eq!(ShadowType::parse("0"), ShadowType::None);
        assert_eq!(ShadowType::parse("2"), ShadowType::Flat);
        assert_eq!(ShadowType::parse("7"), ShadowType::Stencil);
        assert_eq!(ShadowType::parse("-3"), ShadowType::None);
        assert_eq!(ShadowType::parse("soft"), ShadowType::None);
        assert_eq!(ShadowType::parse("1x"), ShadowType::Simple);

        let parse = |keys: &str| {
            Actor3dDef::parse(format!("ACTOR3DX {{ MODEL = \"m.x\" {keys} }}").as_bytes())
                .unwrap()
                .shadow_type
        };
        assert_eq!(parse(""), ShadowType::Stencil);
        assert_eq!(parse("SHADOW_TYPE = 1"), ShadowType::Simple);
        assert_eq!(parse("SIMPLE_SHADOW = TRUE"), ShadowType::Simple);
        assert_eq!(parse("SIMPLE_SHADOW = FALSE"), ShadowType::Stencil);
        assert_eq!(parse("SHADOW = TRUE"), ShadowType::Stencil);
        assert_eq!(parse("SHADOW = FALSE"), ShadowType::None);
        assert_eq!(
            parse("SHADOW_TYPE = \"flat\" SHADOW = FALSE"),
            ShadowType::None
        );
    }

    #[test]
    fn templates_reset_the_model_keys() {
        let outer = r#"ACTOR3DX
{
  MODEL = "lost.x"
  ANIMATION { NAME = "lost" LOOPING = TRUE }
  NAME = "outer"
  TEMPLATE = "base.act3d"
  MODEL = "extra.x"
  ANIMATION { NAME = "walk" LOOPING = FALSE }
  VELOCITY = 5
}
"#;
        let base = r#"ACTOR3DX
{
  NAME = "base"
  CAPTION = "Base"
  MODEL = "base.x"
  SHADOW_MODEL = "base_low.x"
  ANIMATION { NAME = "idle" LOOPING = TRUE }
  VELOCITY = 1
  ANGULAR_VELOCITY = 90
}
"#;
        // Without a template source: the file's own keys after the
        // template, the model keys before it dropped.
        let def = Actor3dDef::parse(outer.as_bytes()).unwrap();
        assert_eq!(def.template.as_deref(), Some("base.act3d"));
        assert_eq!(def.name, "outer");
        assert_eq!(def.models, vec!["extra.x"]);
        assert_eq!(def.animations.len(), 1);
        assert_eq!(def.animations[0].name, "walk");
        assert_eq!(def.velocity, 5.0);

        // With the template loaded in place: the template's values, then
        // the outer file's later keys over them; the outer MODEL merges
        // into the template's.
        let mut reads = Vec::new();
        let mut source = |path: &str| {
            reads.push(path.to_string());
            (path == "base.act3d").then(|| base.as_bytes().to_vec())
        };
        let def = Actor3dDef::parse_with(outer.as_bytes(), &mut source).unwrap();
        assert_eq!(reads, vec!["base.act3d"]);
        assert_eq!(
            def.name, "base",
            "the template's NAME comes after the outer one"
        );
        assert_eq!(def.caption, "Base");
        assert_eq!(def.models, vec!["base.x", "extra.x"]);
        assert_eq!(def.shadow_model.as_deref(), Some("base_low.x"));
        assert_eq!(
            def.animations
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            vec!["idle", "walk"]
        );
        assert_eq!(def.velocity, 5.0);
        assert_eq!(def.angular_velocity, 90.0);

        assert_eq!(def.filename_override.as_deref(), Some("base.act3d"));
        assert!(def.log.is_empty());
        // The steps start again at the template: `lost.x` is gone.
        assert_eq!(
            def.steps
                .iter()
                .filter_map(|step| match step {
                    ModelStep::Model(path) => Some(path.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["base.x", "extra.x"]
        );

        // A template that cannot be read changes nothing and does not
        // fail the load: the model keys before it stay.
        let mut none = |_: &str| None;
        let def = Actor3dDef::parse_with(outer.as_bytes(), &mut none).unwrap();
        assert_eq!(def.models, vec!["lost.x", "extra.x"]);
        assert_eq!(def.name, "outer");
        assert_eq!(def.filename_override, None);
        assert_eq!(
            def.log,
            vec!["CAdActor3DX::LoadFile failed for file 'base.act3d'"]
        );
        // A template without MODEL fails, which the file that names it
        // does not see: its own MODEL, after the key, is the model.
        let mut empty = |_: &str| Some(b"ACTOR3DX { NAME = \"e\" VELOCITY = 3 }".to_vec());
        let def = Actor3dDef::parse_with(outer.as_bytes(), &mut empty).unwrap();
        assert_eq!(def.models, vec!["extra.x"]);
        assert_eq!(def.name, "e");
        assert_eq!(def.filename_override.as_deref(), Some("base.act3d"));
        assert_eq!(def.log.len(), 2);
        assert!(def.log[0].contains("no MODEL"), "{:?}", def.log);
        assert_eq!(def.log[1], "Error parsing ACTOR3D file 'base.act3d'");
        // With nothing after it, the actor has no model.
        let only_template = b"ACTOR3DX { TEMPLATE = \"e.act3d\" }";
        assert!(Actor3dDef::parse_with(only_template, &mut empty).is_err());
        assert!(
            Actor3dDef::parse(only_template).is_ok(),
            "recorded, not loaded"
        );
    }

    /// A template that stops on an error keeps what it applied before
    /// it, the model included; a template that is not an actor file
    /// leaves the model alone; templates cannot nest without end.
    #[test]
    fn a_failing_template_keeps_what_it_read() {
        let outer = b"ACTOR3DX { MODEL = \"first.x\" TEMPLATE = \"t.act3d\" VELOCITY = 9 }";
        let mut broken = |_: &str| {
            Some(b"ACTOR3DX { MODEL = \"t.x\" SCALE = 50 BOGUS = 1 ANGLE = 90 }".to_vec())
        };
        let def = Actor3dDef::parse_with(outer, &mut broken).unwrap();
        assert_eq!(def.models, vec!["t.x"]);
        assert_eq!(def.scale, 0.5);
        assert_eq!(def.angle, 0.0, "not reached");
        assert_eq!(def.velocity, 9.0);
        assert!(def.log[0].contains("unknown key `BOGUS`"), "{:?}", def.log);
        assert_eq!(def.log[1], "Error parsing ACTOR3D file 't.act3d'");

        // The root keyword is checked before the model is dropped.
        let mut other = |_: &str| Some(b"ENTITY { NAME = \"x\" }".to_vec());
        let def = Actor3dDef::parse_with(outer, &mut other).unwrap();
        assert_eq!(def.models, vec!["first.x"]);
        assert!(def.log[0].contains("'ACTOR3DX' keyword expected"));

        // A template that names itself stops at a bound.
        let mut reads = 0;
        let mut itself = |_: &str| {
            reads += 1;
            Some(b"ACTOR3DX { MODEL = \"m.x\" TEMPLATE = \"self.act3d\" }".to_vec())
        };
        let def =
            Actor3dDef::parse_with(b"ACTOR3DX { TEMPLATE = \"self.act3d\" }", &mut itself).unwrap();
        assert_eq!(reads, MAX_TEMPLATE_DEPTH);
        assert_eq!(def.models, vec!["m.x"]);
        assert!(def.log.iter().any(|line| line.contains("nested too deep")));
        // Nor multiply: each file names itself twice.
        let mut reads = 0;
        let mut twice = |_: &str| {
            reads += 1;
            Some(b"ACTOR3DX { TEMPLATE = \"t\" TEMPLATE = \"t\" MODEL = \"m.x\" }".to_vec())
        };
        Actor3dDef::parse_with(b"ACTOR3DX { TEMPLATE = \"t\" }", &mut twice).unwrap();
        assert_eq!(reads, MAX_TEMPLATE_LOADS);
    }

    /// The values are read as `CAdActor3DX::LoadBuffer` scans them.
    #[test]
    fn values_are_scanned_as_the_original_does() {
        let parse = |keys: &str| {
            Actor3dDef::parse(format!("ACTOR3DX {{ MODEL = \"m.x\" {keys} }}").as_bytes())
        };
        // An empty value leaves the variable; SCALE is divided all the
        // same.
        let def =
            parse("X { } VELOCITY { } ACTIVE { } SCALE { } SHADOW { } SIMPLE_SHADOW { }").unwrap();
        assert_eq!(def.position.x, 0.0);
        assert_eq!(def.velocity, 1.0);
        assert!(def.active);
        assert_eq!(def.scale, 0.01);
        assert_eq!(def.shadow_type, ShadowType::Stencil);
        // Text that is not a number reads as 0.
        assert_eq!(parse("VELOCITY = fast").unwrap().velocity, 0.0);
        // The light keeps the coordinates the scan does not reach.
        let def = parse("LIGHT_POSITION { 1, 2 }").unwrap();
        assert_eq!(def.light_position, Vec3::new(1.0, 2.0, -40.0));
        // A name in braces carries its blanks and is no name.
        assert_eq!(
            parse("SHADOW_TYPE { flat }").unwrap().shadow_type,
            ShadowType::None
        );
        assert_eq!(
            parse("SHADOW_TYPE { 2 }").unwrap().shadow_type,
            ShadowType::Flat
        );
        // A region or waypoint group that cannot be read is left out and
        // the load goes on; the same key replaces what was there.
        let def = parse(
            "BLOCKED_REGION { POINT { 0, 0 } } BLOCKED_REGION { COLOR = 1 } WAYPOINTS { BOGUS = 1 } NAME = \"after\"",
        )
        .unwrap();
        assert!(def.blocked_region.is_none() && def.waypoints.is_none());
        assert_eq!(def.name, "after");
        // The table is matched by prefix, in its order: `NAMES` is
        // `NAME` followed by something that is no key.
        assert!(parse("NAMES = \"x\"").is_err());
        assert!(parse("ZOOM = 1").is_err());
        // The root keyword and EVENTS are in the table and do nothing.
        assert!(parse("ACTOR3DX { NAME = \"inner\" } EVENTS { X = 1 }").is_ok());
        // An EFFECT block with another key is dropped alone.
        let def = parse("EFFECT { MATERIAL = \"m\" EFFECT_FILE = \"f\" COLOR = 1 }").unwrap();
        assert!(def.effects.is_empty());
    }

    #[test]
    fn rejects_bad_files() {
        let err = Actor3dDef::parse(b"ACTOR3D { NAME = \"old\" MODEL = \"m.ms3d\" }").unwrap_err();
        assert!(err.to_string().contains("legacy Milkshape"));
        assert!(!Actor3dDef::is_actor_x(b"ACTOR3D { }"));
        assert!(!Actor3dDef::is_actor_x(b"ACTOR { }"));
        assert!(!Actor3dDef::is_actor_x(b""));
        assert!(Actor3dDef::parse(b"ACTOR { }").is_err());
        assert!(Actor3dDef::parse(b"").is_err());
        assert!(Actor3dDef::parse(b"ACTOR3DX { NAME = ").is_err());
        let err = Actor3dDef::parse(b"ACTOR3DX { NAME = \"x\" }").unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid definition: no MODEL has been given for the 3D actor"
        );
        let err = Actor3dDef::parse(b"ACTOR3DX { MODEL = \"m.x\"\n COLOR = 1 }").unwrap_err();
        assert!(err.to_string().contains("line 2"), "{err}");
        assert!(err.to_string().contains("unknown key `COLOR`"), "{err}");
        // A shadow model, animation or effect before any model is ignored.
        let def = Actor3dDef::parse(
            b"ACTOR3DX { SHADOW_MODEL = \"l.x\" ANIMATION { NAME = \"a\" } EFFECT { MATERIAL = \"m\" EFFECT_FILE = \"f\" } MODEL = \"m.x\" }",
        )
        .unwrap();
        assert_eq!(def.shadow_model, None);
        assert!(def.animations.is_empty() && def.effects.is_empty());
    }

    #[test]
    fn applies_animation_blocks_to_a_model() {
        let mut model = Model::parse(MODEL.as_bytes(), "m.x").unwrap();
        let def = Actor3dDef::parse(TRINITY.as_bytes()).unwrap();
        let missing = def.apply_animations(&mut model);
        assert!(missing.is_empty());
        let walk = model.animation_set("walk").unwrap();
        assert!(walk.looping);
        // Frame time 1200 ticks = 249 ms, total 999 ms: 5 frames. The
        // event at frame 16 is clamped to 5.
        assert_eq!(walk.total_frames(4800), 5);
        assert_eq!(
            walk.events.iter().map(|e| e.frame).collect::<Vec<_>>(),
            vec![4, 5]
        );
        let idle = model.animation_set("idle").unwrap();
        assert!(idle.looping);

        let anim = r#"
ANIMATION { NAME = "WALK" LOOPING = FALSE EVENT { NAME = "step" FRAME = 0 } }
ANIMATION { NAME = "run" }
ANIMATION { LOOPING = TRUE }
"#;
        let defs = AnimationDef::parse_file(anim.as_bytes()).unwrap();
        assert_eq!(defs.len(), 3);
        let file = AnimationFile::parse(anim.as_bytes());
        assert_eq!(file.animations, defs);
        let applied = file.apply_to(&mut model);
        assert!(applied.ok);
        assert_eq!(
            applied.log,
            vec!["Error: Animation 'run' cannot be found in the model."]
        );
        let walk = model.animation_set("walk").unwrap();
        assert!(!walk.looping);
        assert_eq!(walk.events.len(), 3);
        assert_eq!(walk.events[2].frame, 1);
        // A text that is not closed runs to the end (`CBParser::GetSubText`);
        // one that does not start with a key is a syntax error.
        assert!(AnimationDef::parse_file(b"ANIMATION { NAME = ").is_ok());
        assert!(AnimationDef::parse_file(b"{ ANIMATION").is_err());
        assert!(AnimationDef::parse_file(b"").unwrap().is_empty());
    }

    /// `CXModel::ParseAnim`, key by key.
    #[test]
    fn an_animation_block_acts_in_the_order_of_its_keys() {
        let block = |text: &str| {
            let doc = Document::parse(format!("ANIMATION {{ {text} }}").as_bytes()).unwrap();
            AnimationDef::from_node(&doc.nodes[0])
        };
        let events = |model: &Model, set: &str| -> Vec<String> {
            model
                .animation_set(set)
                .unwrap()
                .events
                .iter()
                .map(|e| e.name.clone())
                .collect()
        };
        let fresh = || Model::parse(MODEL.as_bytes(), "m.x").unwrap();

        // Each EVENT goes to the set of the NAME read so far; LOOPING,
        // the last one, goes to the set of the last NAME.
        let def = block(
            "EVENT { NAME = \"lost\" } NAME = \"walk\" LOOPING = TRUE EVENT { NAME = \"a\" FRAME = 2 } NAME = \"IDLE\" EVENT { NAME = \"b\" } LOOPING = FALSE",
        );
        assert_eq!(def.name, "IDLE");
        assert_eq!(def.looping, Some(false));
        let mut model = fresh();
        model.animation_set_mut("idle").unwrap().looping = true;
        let applied = def.apply_to(&mut model);
        assert!(applied.ok);
        assert_eq!(
            applied.log,
            vec!["Error: NAME filed must precede any EVENT fields in actor definition files."]
        );
        assert_eq!(events(&model, "walk"), ["a"]);
        assert_eq!(events(&model, "idle"), ["b"]);
        assert!(!model.animation_set("walk").unwrap().looping);
        assert!(!model.animation_set("idle").unwrap().looping);

        // A set the model lacks is reported at its NAME; its events and
        // LOOPING go nowhere.
        let def = block("NAME = \"walk\" NAME = \"jump\" EVENT { NAME = \"x\" } LOOPING = TRUE");
        let mut model = fresh();
        let applied = def.apply_to(&mut model);
        assert!(applied.ok);
        assert_eq!(
            applied.log,
            vec!["Error: Animation 'jump' cannot be found in the model."]
        );
        assert!(events(&model, "walk").is_empty());
        assert!(!model.animation_set("walk").unwrap().looping);

        // An unknown key stops the block: the events before it stay,
        // LOOPING is not applied, what follows is not read.
        let def = block(
            "NAME = \"walk\" LOOPING = TRUE EVENT { NAME = \"kept\" } SPEED = 2 EVENT { NAME = \"never\" }",
        );
        assert!(def.error.as_deref().unwrap().contains("SPEED"));
        assert_eq!(def.looping, None);
        let mut model = fresh();
        let applied = def.apply_to(&mut model);
        assert!(!applied.ok);
        assert!(!def.apply(&mut fresh()));
        assert!(applied.log.is_empty());
        assert_eq!(events(&model, "walk"), ["kept"]);
        assert!(!model.animation_set("walk").unwrap().looping);

        // An EVENT with a key of its own kind is dropped alone; an empty
        // LOOPING keeps the value read before it.
        let def = block(
            "NAME = \"walk\" EVENT { NAME = \"bad\" TIME = 3 } EVENT { FRAME = 2 } LOOPING = TRUE LOOPING { } EVENT { NAME = \"good\" FRAME = 2 NAME = \"better\" }",
        );
        assert_eq!(def.error, None);
        assert_eq!(def.looping, Some(true));
        let mut model = fresh();
        assert!(def.apply(&mut model));
        assert_eq!(events(&model, "walk"), ["better"]);
        assert_eq!(model.animation_set("walk").unwrap().events[0].frame, 2);
        assert!(model.animation_set("walk").unwrap().looping);
    }

    /// `CAdActor3DX::MergeAnimations2`.
    #[test]
    fn an_anim_file_stops_at_what_is_not_an_animation() {
        let text = r#"
ANIMATION { NAME = "walk" LOOPING = TRUE }
ANIMATION { NAME = "idle" BOGUS = 1 LOOPING = TRUE }
ANIMATION { NAME = "idle" EVENT { NAME = "blink" } }
OTHER { }
ANIMATION { NAME = "idle" LOOPING = TRUE }
"#;
        let file = AnimationFile::parse(text.as_bytes());
        assert_eq!(file.animations.len(), 3);
        assert!(file.error.as_deref().unwrap().contains("OTHER"));
        assert!(AnimationDef::parse_file(text.as_bytes()).is_err());
        let mut model = Model::parse(MODEL.as_bytes(), "m.x").unwrap();
        let applied = file.apply_to(&mut model);
        // The file fails on OTHER, after the blocks before it were
        // applied; the block that failed did not fail it.
        assert!(!applied.ok);
        assert_eq!(applied.log.len(), 1);
        assert!(applied.log[0].starts_with("Syntax error in animation definition file"));
        assert!(model.animation_set("walk").unwrap().looping);
        let idle = model.animation_set("idle").unwrap();
        assert!(!idle.looping);
        assert_eq!(idle.events.len(), 1);

        // A file whose blocks all load succeeds even when one fails.
        let file = AnimationFile::parse(b"ANIMATION { BOGUS = 1 } ANIMATION { NAME = \"walk\" }");
        assert_eq!(file.error, None);
        assert!(file.apply_to(&mut model).ok);
    }

    /// `CAdActor3DX::LoadBuffer` acts on the model at each key's place.
    #[test]
    fn builds_the_models_in_the_order_of_the_file() {
        const RUN: &str = r#"xof 0303txt 0032
AnimationSet run {
 Animation {
  { Bone }
  AnimationKey { 2; 1; 0;3;0,0,0;;; }
 }
}
"#;
        const LOW: &str = r#"xof 0303txt 0032
Frame LowRoot {
 Mesh low {
  3; 0;0;0;, 1;0;0;, 0;0;1;; 1; 3;0,1,2;;
  SkinWeights { "Bone"; 3; 0,1,2; 1.0,1.0,1.0; 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1;; }
 }
}
"#;
        let mut read = |path: &str| match path {
            "m.x" => Some(MODEL.as_bytes().to_vec()),
            "run.x" => Some(RUN.as_bytes().to_vec()),
            "low.x" => Some(LOW.as_bytes().to_vec()),
            "broken.x" => Some(b"not an x file".to_vec()),
            _ => None,
        };
        let text = r#"ACTOR3DX
{
  ANIMATION { NAME = "walk" LOOPING = TRUE }
  SHADOW_MODEL = "low.x"
  MODEL = "broken.x"
  MODEL = "m.x"
  ANIMATION { NAME = "run" LOOPING = TRUE }
  MODEL = "run.x"
  MODEL = "missing.x"
  SHADOW_MODEL = "low.x"
  ANIMATION { NAME = "idle" LOOPING = TRUE }
}
"#;
        let def = Actor3dDef::parse(text.as_bytes()).unwrap();
        let built = def.build_models("a.act3d", &mut read).unwrap();
        // The first MODEL that loads is the model; the later ones merge.
        assert_eq!(built.model_file, "m.x");
        assert_eq!(built.merged_files, vec!["run.x"]);
        assert_eq!(built.model.file_name, "m.x");
        // The block for `run` came before the file that brings the set.
        assert!(!built.model.animation_set("run").unwrap().looping);
        assert!(built.model.animation_set("idle").unwrap().looping);
        // The block before any model did nothing.
        assert!(!built.model.animation_set("walk").unwrap().looping);
        // The shadow model found its bone in the model.
        let shadow = built.shadow.as_ref().unwrap();
        assert_eq!(built.shadow_file.as_deref(), Some("low.x"));
        assert_eq!(
            shadow.model().meshes[0].bones[0].frame,
            built.model.find_frame("Bone")
        );
        assert_eq!(built.warnings.len(), 5, "{:#?}", built.warnings);
        assert_eq!(
            built.warnings[0],
            "Error: a MODEL= line must precede any animation definitions (file: a.act3d)"
        );
        assert_eq!(
            built.warnings[1],
            "Error: a MODEL= line must precede shadow model assignment (file: a.act3d)"
        );
        assert!(built.warnings[2].starts_with("Error loading model 'broken.x'"));
        assert_eq!(
            built.warnings[3],
            "Error: Animation 'run' cannot be found in the model."
        );
        assert_eq!(built.warnings[4], "Error merging model 'missing.x'");

        // The whole-file application the steps replace finds `run`.
        let mut model = Model::parse(MODEL.as_bytes(), "m.x").unwrap();
        model.merge_animations(RUN.as_bytes(), "run.x").unwrap();
        assert!(def.apply_animations(&mut model).is_empty());
        assert!(model.animation_set("run").unwrap().looping);

        // A shadow model that fails to load leaves none, the earlier one
        // included; the actor loads.
        let def = Actor3dDef::parse(
            b"ACTOR3DX { MODEL = \"m.x\" SHADOW_MODEL = \"low.x\" SHADOW_MODEL = \"broken.x\" }",
        )
        .unwrap();
        let built = def.build_models("a.act3d", &mut read).unwrap();
        assert!(built.shadow.is_none() && built.shadow_file.is_none());
        assert_eq!(built.warnings.len(), 1);

        // No model at the end fails the actor.
        let def =
            Actor3dDef::parse(b"ACTOR3DX { MODEL = \"broken.x\" MODEL = \"missing.x\" }").unwrap();
        let log = def.build_models("a.act3d", &mut read).unwrap_err();
        assert_eq!(log.len(), 3);
        assert_eq!(log[2], "Error: No model has been loaded for 3D actor");
    }

    #[test]
    fn names_the_anim_sidecar() {
        assert_eq!(
            Actor3dDef::anim_sidecar("actors\\trinity\\throw.x"),
            "actors\\trinity\\throw.anim"
        );
        assert_eq!(Actor3dDef::anim_sidecar("a/b.c/model"), "a/b.c/model.anim");
        assert_eq!(Actor3dDef::anim_sidecar("Model.X"), "Model.anim");
    }

    /// Checks the demo's actor file against its model when both are
    /// present under `testdata/local/wme_demo_3d/`.
    #[test]
    fn loads_the_demo_actor_when_present() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/local/wme_demo_3d");
        let act3d = dir.join("trinity.act3d");
        let model_path = dir.join("trinity.x");
        if !act3d.is_file() || !model_path.is_file() {
            eprintln!("skipped: {} is absent", act3d.display());
            return;
        }
        let def = Actor3dDef::parse(&std::fs::read(&act3d).unwrap()).unwrap();
        assert_eq!(def.models, vec!["actors\\trinity\\trinity.x"]);
        assert_eq!(def.animations.len(), 3);
        let mut model = Model::parse(&std::fs::read(&model_path).unwrap(), &def.models[0]).unwrap();
        assert!(def.apply_animations(&mut model).is_empty());
        let walk = model.animation_set("walk").unwrap();
        assert!(walk.looping);
        assert_eq!(
            walk.events.iter().map(|e| e.frame).collect::<Vec<_>>(),
            vec![4, 16]
        );
        assert!(model.animation_set("hold").unwrap().looping);
        assert!(!model.animation_set("take_up").unwrap().looping);
        if let Ok(teapot) = std::fs::read(dir.join("teapot.act3d")) {
            let def = Actor3dDef::parse(&teapot).unwrap();
            assert_eq!(def.scale, 1.8);
            assert!(def.animations.is_empty());
        }
    }
}
