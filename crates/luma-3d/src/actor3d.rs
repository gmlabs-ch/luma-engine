//! The 3D actor definition file (`.act3d`, keyword `ACTOR3DX`) and the
//! `.anim` sidecar of a merged animation file.
//!
//! The file is a [definition file](https://github.com/gmlabs-ch/luma-engine/blob/main/docs/formats/definition-files.md)
//! naming the `.X` model(s) of the actor, its speeds, its shadow and the
//! looping flag and events of its animation sets. [`Actor3dDef`] is the
//! typed result; it holds paths, not loaded files, so that the caller
//! loads the model through its own file system and then applies the
//! animation blocks with [`AnimationDef::apply`]. The keys and their
//! defaults are described in `docs/formats/actor3d.md`.
//!
//! Written with reference to the MIT-licensed Wintermute Engine 1.x
//! sources (`AdActor3DX.cpp`, `AdActor3D.cpp`, `AdObject3D.cpp`,
//! `XModel.cpp`, `BObject.cpp`, Copyright (c) 2003-2013 Jan Nedoma). See
//! `NOTICE` at the root of the repository.

use luma_formats::def::{Document, Node, Nodes, Scalar, Value};
use luma_formats::{Error, Property, Region, Result, Rgba, WaypointGroup};

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

    /// The value of a `SHADOW_TYPE` key: one of the names `none`,
    /// `simple`, `flat`, `stencil` (ignoring case), else the number it
    /// starts with (`atoi`), clamped.
    pub fn parse(text: &str) -> ShadowType {
        let t = text.trim();
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
    /// applied; 0 when absent.
    pub frame: i32,
}

/// An `ANIMATION` block: properties of a named animation set of the
/// model.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AnimationDef {
    /// The set's name (`NAME`), matched ignoring ASCII case.
    pub name: String,
    /// `LOOPING`, when the block has it.
    pub looping: Option<bool>,
    /// The events, in file order.
    pub events: Vec<AnimationEventDef>,
}

/// An `EFFECT` block: an HLSL effect on a material. Parsed and kept, not
/// applied (effects are not planned).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectDef {
    /// The material name (`MATERIAL`), a texture file stem.
    pub material: String,
    /// The `.fx` file (`EFFECT_FILE`).
    pub effect_file: String,
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
    /// Font of the actor's speech (`FONT`).
    pub font: Option<String>,
    /// Cursor sprite shown over the actor (`CURSOR`).
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
    /// The `.X` files (`MODEL`, repeatable): the first is the model, the
    /// others merge their animation sets into it.
    pub models: Vec<String>,
    /// The low-polygon model used for shadows (`SHADOW_MODEL`).
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
    /// The `ANIMATION` blocks that followed a `MODEL`.
    pub animations: Vec<AnimationDef>,
    /// The `EFFECT` blocks that followed a `MODEL`.
    pub effects: Vec<EffectDef>,
    /// The private blocked polygon for 2D path finding
    /// (`BLOCKED_REGION`).
    pub blocked_region: Option<Region>,
    /// The private waypoints for 2D path finding (`WAYPOINTS`).
    pub waypoints: Option<WaypointGroup>,
}

impl Default for Actor3dDef {
    fn default() -> Self {
        Actor3dDef {
            name: String::new(),
            caption: String::new(),
            template: None,
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
            blocked_region: None,
            waypoints: None,
        }
    }
}

/// A source of template files for [`Actor3dDef::parse_with`]: the bytes
/// of a path, `None` when it cannot be read.
pub type TemplateSource<'a> = dyn FnMut(&str) -> Option<Vec<u8>> + 'a;

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
        def.apply_file(bytes, None)?;
        if def.models.is_empty() && def.template.is_none() {
            return Err(no_model());
        }
        Ok(def)
    }

    /// Parses an `.act3d` file and loads its `TEMPLATE` files in place
    /// through `templates`, as the original does: at the key, the
    /// template file (itself an `ACTOR3DX` file, with templates of its
    /// own) is applied over the values read so far, after dropping the
    /// models, shadow model, animation and effect blocks collected so
    /// far. A template that cannot be read or parsed fails the load, and
    /// so does a result without `MODEL`.
    pub fn parse_with(bytes: &[u8], templates: &mut TemplateSource<'_>) -> Result<Actor3dDef> {
        let mut def = Actor3dDef::default();
        def.apply_file(bytes, Some(templates))?;
        if def.models.is_empty() {
            return Err(no_model());
        }
        Ok(def)
    }

    fn apply_file(
        &mut self,
        bytes: &[u8],
        mut templates: Option<&mut TemplateSource<'_>>,
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
        for child in root.nodes() {
            match child.key.to_ascii_uppercase().as_str() {
                "TEMPLATE" => {
                    let path = text(child);
                    self.models.clear();
                    self.shadow_model = None;
                    self.animations.clear();
                    self.effects.clear();
                    self.template = Some(path.clone());
                    if let Some(source) = templates.as_deref_mut() {
                        let bytes = source(&path).ok_or_else(|| Error::Invalid {
                            reason: format!(
                                "line {}: template '{path}' cannot be read",
                                child.line
                            ),
                        })?;
                        self.apply_file(&bytes, Some(source))?;
                    }
                }
                "NAME" => self.name = text(child),
                "CAPTION" => self.caption = text(child),
                "FONT" => self.font = Some(text(child)),
                "CURSOR" => self.cursor = Some(text(child)),
                "SCRIPT" => self.scripts.push(text(child)),
                "PROPERTY" => self.properties.extend(Property::from_node(child)?),
                "EDITOR_PROPERTY" | "EVENTS" => {}
                "ACTIVE" => self.active = boolean(child),
                "INTERACTIVE" | "REGISTRABLE" => self.interactive = boolean(child),
                "X" => self.position.x = float(child),
                "Y" => self.position.y = float(child),
                "Z" => self.position.z = float(child),
                // The original computes the normalised angle and discards
                // it, so the value stays as written.
                "ANGLE" => self.angle = float(child),
                "SCALE" => self.scale = float(child) / 100.0,
                "VELOCITY" => self.velocity = float(child),
                "ANGULAR_VELOCITY" => self.angular_velocity = float(child),
                "DROP_TO_FLOOR" => self.drop_to_floor = boolean(child),
                "DRAW_BACKFACES" => self.draw_backfaces = boolean(child),
                "MODEL" => self.models.push(text(child)),
                "SHADOW_MODEL" => {
                    // Without a model before it, the original logs an
                    // error and ignores the key.
                    if !self.models.is_empty() {
                        self.shadow_model = Some(text(child));
                    }
                }
                "SHADOW_TYPE" => self.shadow_type = ShadowType::parse(child.as_str().unwrap_or("")),
                "SHADOW" => {
                    if !boolean(child) {
                        self.shadow_type = ShadowType::None;
                    }
                }
                "SIMPLE_SHADOW" => {
                    if boolean(child) {
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
                "SHADOW_IMAGE" => self.shadow_image = Some(text(child)),
                "SHADOW_SIZE" => self.shadow_size = float(child).max(0.0),
                "LIGHT_POSITION" => {
                    let v = floats(child);
                    let get = |i: usize| v.get(i).copied().unwrap_or(0.0);
                    self.light_position = Vec3::new(get(0), get(1), get(2));
                }
                "ANIMATION" => {
                    // The original needs a model to attach the block to.
                    if !self.models.is_empty() {
                        if let Some(animation) = AnimationDef::from_node(child) {
                            self.animations.push(animation);
                        }
                    }
                }
                "EFFECT" => {
                    if !self.models.is_empty() {
                        if let Some(effect) = EffectDef::from_node(child) {
                            self.effects.push(effect);
                        }
                    }
                }
                "BLOCKED_REGION" => self.blocked_region = Some(Region::from_node(child, false)?),
                "WAYPOINTS" => self.waypoints = Some(WaypointGroup::from_node(child)?),
                _ => {
                    return Err(Error::Invalid {
                        reason: format!(
                            "line {}: unknown key '{}' in {KEYWORD} definition",
                            child.line, child.key
                        ),
                    });
                }
            }
        }
        Ok(())
    }

    /// Applies the `ANIMATION` blocks to a loaded model: the looping flag
    /// and the events of each named set. Returns the names of the blocks
    /// whose set the model does not have (the original logs them).
    pub fn apply_animations(&self, model: &mut Model) -> Vec<String> {
        self.animations
            .iter()
            .filter(|animation| !animation.apply(model))
            .map(|animation| animation.name.clone())
            .collect()
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
    /// Builds a block from an `ANIMATION` node (`CXModel::ParseAnim`):
    /// `NAME`, `LOOPING`, and `EVENT` blocks, which must come **after**
    /// the `NAME` (earlier ones are dropped) and need a `NAME` of their
    /// own. `None` without a `NAME`, since the block then names no set.
    /// Other keys are ignored.
    pub fn from_node(node: &Node) -> Option<AnimationDef> {
        let mut animation = AnimationDef::default();
        let mut named = false;
        for child in node.nodes() {
            match child.key.to_ascii_uppercase().as_str() {
                "NAME" => {
                    animation.name = text(child);
                    named = true;
                }
                "LOOPING" => animation.looping = Some(boolean(child)),
                "EVENT" if named => {
                    let name = child.get("NAME").map(text);
                    let frame = child.get("FRAME").map(integer).unwrap_or(0);
                    if let Some(name) = name {
                        animation.events.push(AnimationEventDef { name, frame });
                    }
                }
                _ => {}
            }
        }
        named.then_some(animation)
    }

    /// Parses an `.anim` sidecar: top-level `ANIMATION` blocks and nothing
    /// else (`MergeAnimations2`). Fails on a syntax error.
    pub fn parse_file(bytes: &[u8]) -> Result<Vec<AnimationDef>> {
        let doc = Document::parse(bytes)?;
        Ok(doc
            .nodes
            .iter()
            .filter(|node| node.key.eq_ignore_ascii_case("ANIMATION"))
            .filter_map(AnimationDef::from_node)
            .collect())
    }

    /// Applies the block to the model's animation set of that name
    /// (ignoring ASCII case): sets `looping` when the block has it and
    /// adds the events, clamped to the set's frames. `false` when the
    /// model has no such set.
    pub fn apply(&self, model: &mut Model) -> bool {
        let ticks = model.ticks_per_second;
        let Some(set) = model.animation_set_mut(&self.name) else {
            return false;
        };
        if let Some(looping) = self.looping {
            set.looping = looping;
        }
        for event in &self.events {
            set.add_event(&event.name, event.frame, ticks);
        }
        true
    }
}

impl EffectDef {
    /// Builds a block from an `EFFECT` node (`ParseEffect`): kept only
    /// when both `MATERIAL` and `EFFECT_FILE` are present.
    pub fn from_node(node: &Node) -> Option<EffectDef> {
        let material = node.get("MATERIAL").map(text)?;
        let effect_file = node.get("EFFECT_FILE").map(text)?;
        Some(EffectDef {
            material,
            effect_file,
        })
    }
}

fn no_model() -> Error {
    Error::Invalid {
        reason: "no MODEL has been given for the 3D actor".into(),
    }
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

fn float(node: &Node) -> f32 {
    node.as_float().unwrap_or(0.0) as f32
}

/// The value as floats: a list block `{ a, b, c }` or a comma-separated
/// scalar; a part that is not a number ends the list.
fn floats(node: &Node) -> Vec<f32> {
    match &node.value {
        Value::List(items) => items
            .iter()
            .map_while(|s| s.as_float().map(|f| f as f32))
            .collect(),
        Value::Scalar(s) => s
            .text
            .split(',')
            .map_while(|part| {
                Scalar {
                    text: part.trim().to_string(),
                    quoted: false,
                }
                .as_float()
                .map(|f| f as f32)
            })
            .collect(),
        _ => Vec::new(),
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
Frame Bone { }
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
        // events without a NAME are dropped; FRAME defaults to 0.
        assert_eq!(def.animations.len(), 1);
        assert_eq!(def.animations[0].name, "walk");
        assert_eq!(def.animations[0].looping, None);
        assert_eq!(
            def.animations[0].events,
            vec![AnimationEventDef {
                name: "late".into(),
                frame: 0
            }]
        );
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

        // A template that cannot be read fails the load.
        let mut none = |_: &str| None;
        let err = Actor3dDef::parse_with(outer.as_bytes(), &mut none).unwrap_err();
        assert!(
            err.to_string()
                .contains("template 'base.act3d' cannot be read")
        );
        // A template without MODEL, and nothing after it, fails too.
        let mut empty = |_: &str| Some(b"ACTOR3DX { NAME = \"e\" }".to_vec());
        let only_template = b"ACTOR3DX { TEMPLATE = \"e.act3d\" }";
        assert!(Actor3dDef::parse_with(only_template, &mut empty).is_err());
        assert!(
            Actor3dDef::parse(only_template).is_ok(),
            "recorded, not loaded"
        );
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
        assert!(err.to_string().contains("line 2: unknown key 'COLOR'"));
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
OTHER { }
ANIMATION { LOOPING = TRUE }
"#;
        let defs = AnimationDef::parse_file(anim.as_bytes()).unwrap();
        assert_eq!(defs.len(), 2);
        let mut missing = Vec::new();
        for d in &defs {
            if !d.apply(&mut model) {
                missing.push(d.name.clone());
            }
        }
        assert_eq!(missing, vec!["run"]);
        let walk = model.animation_set("walk").unwrap();
        assert!(!walk.looping);
        assert_eq!(walk.events.len(), 3);
        assert_eq!(walk.events[2].frame, 1);
        assert!(AnimationDef::parse_file(b"ANIMATION { NAME = ").is_err());
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
