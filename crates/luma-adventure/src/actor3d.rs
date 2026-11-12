//! 3D actors: skinned, animated `.X` models standing on the painted
//! background of a scene with geometry, driven once per frame like the 2D
//! actors (see `docs/3d-actors.md`, sections 8 and 10.3, and
//! `docs/formats/actor3d.md`).
//!
//! An actor keeps its position in the geometry's world units and the
//! scene pixel its origin projects to (`X`, `Y`), which the drawing order,
//! the regions it stands in and its speech use as a 2D actor's position.
//! It walks a [`Path3D`] from the scene's path finder, turns at its
//! angular velocity, and asks its model for the idle, walk and turn
//! animations on channel 0 as its state says.
//!
//! An actor may carry [`Attachment3d`]s, models of their own attached to
//! a bone (`AddAttachment`), and textures scripts put on its materials
//! ([`MaterialOverride`], `SetTexture`); both are handed to the drawing
//! step with the actor ([`Actor3dDraw`]).
//!
//! Written with reference to `AdActor3DX.cpp`, `AdObject3D.cpp`,
//! `AdAttach3DX.cpp`, `AdActor3D.cpp` (`CreateFromFile`), `AdObject.cpp`,
//! `BObject.cpp` (`GetMatrix`) and `XModel.cpp` (`SetMaterialSprite`,
//! `SetMaterialTheora`) of Wintermute Engine 1.x (MIT License, Copyright
//! (c) 2003-2013 Jan Nedoma); see `NOTICE`.

use luma_3d::actor3d::AnimationDef;
use luma_3d::math::{deg_to_rad, rad_to_deg};
use luma_3d::{
    Actor3dDef, AnimatedModel, AnimationState, Light, Mat4, Model, Path3D, PathFinder, Projection,
    SceneGeometry, ShadowType, Ticket, Vec3,
};
use luma_archive::Vfs;
use luma_formats::scene::{Entity, Point};
use luma_formats::{BlendMode, Rgb, Rgba, Scene};
use luma_script::{Engine, NativeId};
use serde::{Deserialize, Serialize};

use crate::actor::ActorEvent;
use crate::input::HitArea;
use crate::objects::Kind;
use crate::path::Floor;
use crate::scene_view::{FreeObject, SpriteBank, regions_at};
use crate::scene3d::Scene3d;
use crate::talk::{Sentence, SubtitleMods};
use crate::world::World;

/// What a 3D actor is doing (`TObjectState` as `CAdActor3DX` uses it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Actor3dState {
    /// Standing; `Ready` for the scripts waiting for it.
    Ready,
    /// `GoTo` asked for a path; the finder is asked every frame until it
    /// accepts the request.
    SearchingPath,
    /// The path is being computed.
    WaitingPath,
    /// Walking the path.
    FollowingPath,
    /// Turning in place toward the target angle.
    Turning,
    /// Saying a sentence (`Talk`).
    Talking,
    /// Playing a script's animation (`PlayAnim`).
    PlayingAnim,
    /// Under `DirectWalk` / `DirectTurn` control.
    DirectControl,
}

/// The walking half of direct control (`TDirectWalkMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DirectWalk {
    /// Not walking.
    #[default]
    None,
    /// `DirectWalk`: forward along the heading.
    Forward,
    /// `DirectWalkBack`: backward.
    Back,
}

/// The turning half of direct control (`TDirectTurnMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DirectTurn {
    /// Not turning.
    #[default]
    None,
    /// `DirectTurnLeft`: the angle decreases.
    Left,
    /// `DirectTurnRight`: the angle increases.
    Right,
}

/// The model of an actor, and the animation state a saved game carried
/// until the model is loaded again. A saved game keeps the state alone
/// (`CXModel::Persist`), never the model's data.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelSlot {
    /// The model, once loaded.
    pub model: Option<AnimatedModel>,
    /// The state to put on the model when it is loaded again.
    pending: Option<AnimationState>,
}

impl Serialize for ModelSlot {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let state = match (&self.model, &self.pending) {
            (Some(model), _) => Some(model.state()),
            (None, pending) => pending.clone(),
        };
        state.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ModelSlot {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let pending: Option<AnimationState> = Deserialize::deserialize(deserializer)?;
        Ok(ModelSlot {
            model: None,
            pending,
        })
    }
}

/// A texture a script put on a material of the model (`CXModelMatSprite`,
/// section 10.5 of `docs/3d-actors.md`). The drawing step loads it: an
/// image or `.sprite` file whose current frame textures the material, or
/// a Theora video played in a loop whose frame does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialTexture {
    /// `SetTexture(material, file)`: an image or a `.sprite`.
    Sprite(String),
    /// `SetTheoraTexture(material, file)`: a looping video.
    Theora(String),
}

/// A material's texture replaced by script, by material name (the texture
/// file name without path and extension, `Material::name`), compared
/// ignoring ASCII case. The model's data is not changed: the drawing step
/// applies the override to every mesh material of that name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialOverride {
    /// The material name.
    pub material: String,
    /// What textures it now.
    pub texture: MaterialTexture,
}

/// A model attached to a bone of an actor (`CAdAttach3DX`, a `CAdObject3D`
/// owned by the actor, section 8.6 of `docs/3d-actors.md`). It animates
/// on its own channels, its events go to the actor, and it is drawn with
/// `own matrix * bone combined matrix * actor world matrix`: its position,
/// angle and scale offset it from the bone.
#[derive(Debug, Serialize, Deserialize)]
pub struct Attachment3d {
    /// The script object (`GetAttachment`).
    pub id: NativeId,
    /// `NAME` as given to `AddAttachment`; the same name replaces.
    pub name: String,
    /// The frame of the actor's model it follows (`m_ParentBone`).
    pub bone: String,
    /// The `.x` file of the model, read again after a saved game.
    pub model_file: String,
    /// The model and its animation state.
    pub model: ModelSlot,
    /// `Active`: updated and drawn.
    pub active: bool,
    /// `PosX`, `PosY`, `PosZ`: the offset from the bone.
    pub position: Vec3,
    /// `Angle`: the rotation about Y relative to the bone, degrees.
    pub angle: f32,
    /// `Scale`: uniform, `/ 100` of the script value.
    pub scale: f32,
    /// `DrawBackfaces`.
    pub draw_backfaces: bool,
    /// The full matrix of the last update: own, bone, actor.
    #[serde(skip)]
    world_matrix: Mat4,
    /// The projected bounding rectangle of the last update, in scene
    /// pixels (left, top, right, bottom).
    #[serde(skip)]
    bounding_rect: Option<(i32, i32, i32, i32)>,
}

impl Attachment3d {
    /// The model, once loaded.
    pub fn model(&self) -> Option<&AnimatedModel> {
        self.model.model.as_ref()
    }

    /// The model, mutable.
    pub fn model_mut(&mut self) -> Option<&mut AnimatedModel> {
        self.model.model.as_mut()
    }

    /// The matrix the attachment is drawn with: its own `GetMatrix` times
    /// the bone's combined matrix times the actor's world matrix, as of
    /// the last update of the actor.
    pub fn world_matrix(&self) -> Mat4 {
        self.world_matrix
    }

    /// The projected bounding rectangle of the last update, `None` before
    /// the first.
    pub fn bounding_rect(&self) -> Option<(i32, i32, i32, i32)> {
        self.bounding_rect
    }

    /// Plays an animation as the attachment's scripts ask
    /// (`CAdAttach3DX::ScCallMethod`, `PlayAnim(channel, name, 0, true)`):
    /// restarted, with no stop transition; when the channel already plays
    /// something, the actor's transition table and default decide the
    /// transition time, as the model's owner does in the original.
    /// `false` when the model has no such set.
    pub fn play_anim(&mut self, owner: &AnimatedModel, channel: usize, name: &str) -> bool {
        let Some(model) = self.model.model.as_mut() else {
            return false;
        };
        let transition = model
            .current_animation(channel)
            .map_or(0, |current| owner.transition_time(current, name));
        model.play_with(channel, name, transition, 0, true)
    }

    /// Stops a channel at once (`StopAnim`, `StopAnimChannel`).
    pub fn stop_anim(&mut self, channel: usize) -> bool {
        self.model
            .model
            .as_mut()
            .is_some_and(|model| model.stop(channel, 0))
    }

    /// One frame (`CAdAttach3DX::Update`): the full matrix from the
    /// bone's combined matrix and the actor's world matrix, the model's
    /// animation, the projected rectangle. Returns the animation events
    /// crossed, which the actor raises.
    fn update(
        &mut self,
        owner_world: Mat4,
        bone: &Mat4,
        projection: &Projection,
        delta_ms: u32,
    ) -> Vec<String> {
        self.world_matrix =
            world_matrix(self.position, self.angle, self.scale) * *bone * owner_world;
        let Some(model) = self.model.model.as_mut() else {
            return Vec::new();
        };
        let events = model.update(delta_ms);
        self.bounding_rect = project_box(model, self.world_matrix, projection);
        events
    }

    /// Whether a scene pixel misses the attachment's triangles (the
    /// actor's `IsTransparentAt` through its attachments).
    fn is_transparent_at(&self, projection: &Projection, x: i32, y: i32) -> bool {
        let Some(model) = self.model.model.as_ref() else {
            return true;
        };
        let Some(inverse) = self.world_matrix.inverse() else {
            return true;
        };
        let ray = projection.pick_ray(x, y);
        model
            .pick(
                inverse.transform_coord(ray.origin),
                inverse.transform_normal(ray.direction),
            )
            .is_none()
    }

    /// The world position of a bone of the attachment's own model
    /// (`GetBonePosition3D` on the attachment), through its full matrix.
    pub fn bone_position(&self, bone: &str, offset: Vec3) -> Option<Vec3> {
        let model = self.model.model.as_ref()?;
        let matrix = *model.bone_matrix(bone)? * self.world_matrix;
        Some(matrix.transform_coord(offset))
    }
}

/// Projects the eight corners of a model's box through a matrix into
/// scene pixels (`CXModel::UpdateBoundingRect`).
fn project_box(
    model: &AnimatedModel,
    matrix: Mat4,
    projection: &Projection,
) -> Option<(i32, i32, i32, i32)> {
    let mut rect: Option<(i32, i32, i32, i32)> = None;
    for corner in model.bounding_box().corners() {
        let (x, y) = projection.project(matrix.transform_coord(corner));
        rect = Some(match rect {
            None => (x, y, x, y),
            Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(x), b.max(y)),
        });
    }
    rect
}

/// A loaded 3D actor and its state (`CAdActor3DX` with its `CAdObject3D`
/// and `CAdObject` parts).
#[derive(Debug, Serialize, Deserialize)]
pub struct Actor3d {
    /// `NAME`, which scripts change.
    pub name: String,
    /// `CAPTION`, expanded through the string table.
    pub caption: String,
    /// Path the actor was loaded from.
    pub filename: String,
    /// Whether the actor goes with the scene (`Scene.LoadActor3D`).
    pub scene_bound: bool,
    /// `ACTIVE`: drawn and updated.
    pub active: bool,
    /// `INTERACTIVE` / `REGISTRABLE`: clickable.
    pub interactive: bool,
    /// The speech font (`FONT`, `SetFont`).
    pub font: Option<String>,
    /// The actor's own cursor (`CURSOR`, `SetCursor`).
    pub cursor: Option<String>,
    /// `IgnoreItems`: not under the mouse while an item is selected.
    pub ignore_items: bool,
    /// `SoundPanning`.
    pub sound_panning: bool,
    /// `AlphaColor`, kept for the drawing step; `None` is untinted.
    pub tint: Option<Rgba>,
    /// `BlendMode`, kept for the drawing step.
    pub blend_mode: BlendMode,
    /// The private polygon for the 2D path finder (`BLOCKED_REGION`).
    pub blocked_region: Option<luma_formats::Region>,
    /// The private waypoints for the 2D path finder (`WAYPOINTS`).
    pub waypoints: Option<luma_formats::WaypointGroup>,
    /// The `MODEL` files: the first is the model, the others merge their
    /// animation sets into it.
    pub model_files: Vec<String>,
    /// The `.x` files `MergeAnims` added, replayed after a saved game.
    pub merged_files: Vec<String>,
    /// The animation sets `UnloadAnim` removed, removed again after a
    /// saved game.
    pub unloaded_sets: Vec<String>,
    /// The `SHADOW_MODEL` file, for the shadow drawing step.
    pub shadow_model_file: Option<String>,
    /// The model and its animation state.
    pub model: ModelSlot,
    /// The models attached to bones (`AddAttachment`), in order.
    pub attachments: Vec<Attachment3d>,
    /// The textures scripts put on materials (`SetTexture`,
    /// `SetTheoraTexture`), one per material name.
    pub materials: Vec<MaterialOverride>,
    /// The bone the particle emitter follows (`CreateParticleEmitterBone`),
    /// with `part_offset`; `None` when the emitter follows the actor's
    /// 2D position as any object's does.
    pub part_bone: Option<String>,
    /// The offset from that bone, in the bone's space.
    pub part_offset: Vec3,
    /// Position in world units (`PosX`, `PosY`, `PosZ`).
    pub position: Vec3,
    /// The position the 2D position was last computed from.
    last_position: Vec3,
    /// Heading in degrees (`Angle`); 0 faces `-Z`.
    pub angle: f32,
    /// The scene pixel the origin projects to (`X`).
    pub x: i32,
    /// See `x`.
    pub y: i32,
    /// Walking speed, world units per second, times the scale.
    pub velocity: f32,
    /// Turning speed, degrees per second.
    pub angular_velocity: f32,
    /// Uniform scale, `SCALE / 100`.
    pub scale: f32,
    /// Re-seat the actor on the walk plane every update.
    pub drop_to_floor: bool,
    /// Draw both sides of the triangles.
    pub draw_backfaces: bool,
    /// The actor's own shadow type; the one drawn is the lowest of this,
    /// the game's and the scene's.
    pub shadow_type: ShadowType,
    /// `0xAARRGGBB` of flat and stencil shadows.
    pub shadow_color: i32,
    /// The image of the simple shadow (`SHADOW_IMAGE`, `SetShadowImage`);
    /// the game's when `None`.
    pub shadow_image: Option<String>,
    /// Half-size of the simple shadow quad, world units.
    pub shadow_size: f32,
    /// The shadow-casting light, relative to the actor, scaled.
    pub light_position: Vec3,
    /// `AmbientLightColor`: the per-actor ambient override.
    pub ambient_light_color: Option<i32>,
    /// Lights that do not light this actor (`AddIgnoredLight`).
    pub ignored_lights: Vec<String>,
    /// What the actor is doing.
    pub state: Actor3dState,
    /// The state to return to when the current one ends.
    pub next_state: Actor3dState,
    target_point_3d: Vec3,
    target_point_2d: Point,
    target_angle: f32,
    turning_left: bool,
    /// The angle to turn to after a walk, `-1` for none.
    after_walk_angle: f32,
    /// The path being walked.
    pub path: Path3D,
    /// The open request of the scene's path finder.
    #[serde(skip)]
    ticket: Option<Ticket>,
    /// The 2D path in a `2D_PATHFINDING` scene, with its cursor.
    path_2d: Vec<Point>,
    path_2d_index: usize,
    path_2d_ready: bool,
    /// Name of the talk animation (`TalkAnimName`).
    pub talk_anim: String,
    /// Name of the idle animation (`IdleAnimName`).
    pub idle_anim: String,
    /// Name of the walk animation (`WalkAnimName`).
    pub walk_anim: String,
    /// Name of the turn-left animation (`TurnLeftAnimName`).
    pub turn_left_anim: String,
    /// Name of the turn-right animation (`TurnRightAnimName`).
    pub turn_right_anim: String,
    /// The channel of the talk animation (`TalkAnimChannel`).
    pub talk_anim_channel: i32,
    /// The channel of the script's animation while `PlayingAnim`, -1
    /// otherwise.
    state_anim_channel: i32,
    direct_walk: DirectWalk,
    direct_turn: DirectTurn,
    direct_walk_anim: Option<String>,
    direct_turn_anim: Option<String>,
    direct_walk_velocity: f32,
    direct_turn_velocity: f32,
    /// `GoToTolerance`: pixels within which a `GoTo` is not needed.
    pub go_to_tolerance: i32,
    /// The sentence being said.
    pub sentence: Option<Sentence>,
    /// The stance animation of the sentence being played on channel 0.
    temp_skel_anim: Option<String>,
    /// Where the actor's speech goes.
    pub subtitles: SubtitleMods,
    /// Whether the sentence is shown this frame.
    pub show_sentence: bool,
    /// Whether no walk, turn or animation is in progress (`Ready`).
    pub ready: bool,
    current_regions: Vec<(usize, usize)>,
    /// The world matrix of the last update.
    #[serde(skip)]
    world_matrix: Mat4,
    /// The projected bounding rectangle of the last update, in scene
    /// pixels: left, top, right, bottom (`CXModel::m_BoundingRect`).
    #[serde(skip)]
    bounding_rect: Option<(i32, i32, i32, i32)>,
    /// An empty bank: a 3D actor has no sprite, but it is drawn among the
    /// free objects, whose draw items name one.
    #[serde(skip)]
    pub bank: SpriteBank,
}

/// An actor as loaded from its file, with what the world does with the
/// file beyond the actor: the scripts to attach and the script
/// properties to set.
pub struct Loaded3d {
    /// The actor.
    pub actor: Actor3d,
    /// Messages to log.
    pub warnings: Vec<String>,
    /// `SCRIPT` keys.
    pub scripts: Vec<String>,
    /// `PROPERTY` blocks.
    pub properties: Vec<luma_formats::scene::Property>,
}

/// What an update needs from the world: the scene, its geometry, the
/// path finder, the camera's projection, the floor of a `2D_PATHFINDING`
/// scene, the game timer and how much it advanced.
pub struct Update3d<'a> {
    /// The scene, for the regions the actor stands in.
    pub scene: &'a Scene,
    /// The scene's geometry.
    pub geometry: &'a SceneGeometry,
    /// The scene's path finder.
    pub finder: &'a mut PathFinder,
    /// The projection of the frame.
    pub projection: &'a Projection,
    /// The 2D floor when the scene path-finds in 2D (`2D_PATHFINDING`).
    pub floor: Option<Floor<'a>>,
    /// The game timer, in milliseconds.
    pub timer: u64,
    /// How much the timer advanced this frame (`m_TimerDelta`).
    pub delta_ms: u64,
}

/// `CBUtils::NormalizeAngle`: into `0..=360` (360 stays 360).
pub fn normalize_angle(mut angle: f32) -> f32 {
    while angle > 360.0 {
        angle -= 360.0;
    }
    while angle < 0.0 {
        angle += 360.0;
    }
    angle
}

/// `CAdActor3DX::DirToAngle`: the heading of a direction code; `-1` for
/// `DI_NONE` (8), 0 for any other code outside the eight.
pub fn dir_to_angle(code: i32) -> f32 {
    match code {
        0 => 180.0,
        1 => 225.0,
        2 => 270.0,
        3 => 315.0,
        4 => 0.0,
        5 => 45.0,
        6 => 90.0,
        7 => 135.0,
        8 => -1.0,
        _ => 0.0,
    }
}

/// `CAdActor3DX::AngleToDir`: the direction code of a heading, eight
/// 45-degree sectors with boundaries at 22, 67, ... 337; 8 (`DI_NONE`)
/// is never reached for a normalised angle.
pub fn angle_to_dir(angle: f32) -> i32 {
    if !(22.0..337.0).contains(&angle) {
        4
    } else if angle < 67.0 {
        5
    } else if angle < 112.0 {
        6
    } else if angle < 157.0 {
        7
    } else if angle < 202.0 {
        0
    } else if angle < 247.0 {
        1
    } else if angle < 292.0 {
        2
    } else {
        3
    }
}

/// The heading toward a point: `degrees(-atan2(dz, dx)) - 90`.
fn heading_to(from: Vec3, to: Vec3) -> f32 {
    rad_to_deg(-(to.z - from.z).atan2(to.x - from.x)) - 90.0
}

/// The world matrix of an object (`CBObject::GetMatrix`): a rotation
/// about Y by the angle, a uniform scale, then the position.
pub fn world_matrix(position: Vec3, angle: f32, scale: f32) -> Mat4 {
    Mat4::rotation_yaw_pitch_roll(deg_to_rad(angle), 0.0, 0.0)
        * Mat4::uniform_scaling(scale)
        * Mat4::translation(position)
}

/// Reads an `.X` model and the files merged into it.
fn load_model(vfs: &Vfs, files: &[String]) -> Result<Model, String> {
    let (first, rest) = files
        .split_first()
        .ok_or_else(|| "Error: No model has been loaded for 3D actor".to_string())?;
    let bytes = vfs
        .read(first)
        .map_err(|e| format!("Error loading model '{first}': {e}"))?;
    let mut model =
        Model::parse(&bytes, first).map_err(|e| format!("Error loading model '{first}': {e}"))?;
    for file in rest {
        let bytes = vfs
            .read(file)
            .map_err(|e| format!("Error merging model '{file}': {e}"))?;
        model
            .merge_animations(&bytes, file)
            .map_err(|e| format!("Error merging model '{file}': {e}"))?;
    }
    Ok(model)
}

impl Actor3d {
    /// Loads an `.act3d` file (`CAdActor3DX::LoadFile`, with its
    /// `TEMPLATE` chain), its model and the files merged into it, and
    /// applies the `ANIMATION` blocks. Returns the actor with the
    /// warnings to log (animation blocks naming no set of the model) and
    /// the scripts and properties of the file, which the world attaches.
    pub fn load(vfs: &Vfs, path: &str, scene_bound: bool) -> Result<Loaded3d, String> {
        let bytes = vfs
            .read(path)
            .map_err(|e| format!("CAdActor3DX::LoadFile failed for file '{path}': {e}"))?;
        let def = Actor3dDef::parse_with(&bytes, &mut |template| vfs.read(template).ok())
            .map_err(|e| format!("Error parsing ACTOR3D file '{path}': {e}"))?;
        let mut model = load_model(vfs, &def.models)?;
        let warnings = def
            .apply_animations(&mut model)
            .into_iter()
            .map(|name| format!("Error: animation '{name}' not found in model (file: {path})"))
            .collect();
        let scripts = def.scripts.clone();
        let properties = def.properties.clone();
        Ok(Loaded3d {
            actor: Actor3d::from_definition(def, path, scene_bound, model),
            warnings,
            scripts,
            properties,
        })
    }

    /// An actor from its parsed definition and its loaded model.
    pub fn from_definition(
        def: Actor3dDef,
        path: &str,
        scene_bound: bool,
        model: Model,
    ) -> Actor3d {
        let shadow_color = rgba_int(def.shadow_color);
        let position = def.position;
        Actor3d {
            name: def.name,
            caption: def.caption,
            filename: path.to_string(),
            scene_bound,
            active: def.active,
            interactive: def.interactive,
            font: def.font,
            cursor: def.cursor,
            ignore_items: false,
            // `CBObject::m_AutoSoundPanning` starts true.
            sound_panning: true,
            tint: None,
            blend_mode: BlendMode::Normal,
            blocked_region: def.blocked_region,
            waypoints: def.waypoints,
            model_files: def.models,
            merged_files: Vec::new(),
            unloaded_sets: Vec::new(),
            shadow_model_file: def.shadow_model,
            model: ModelSlot {
                model: Some(AnimatedModel::new(model)),
                pending: None,
            },
            attachments: Vec::new(),
            materials: Vec::new(),
            part_bone: None,
            part_offset: Vec3::ZERO,
            position,
            // Forces the first update to compute the 2D position.
            last_position: Vec3::new(f32::NAN, 0.0, 0.0),
            angle: def.angle,
            x: 0,
            y: 0,
            velocity: def.velocity,
            angular_velocity: def.angular_velocity,
            scale: def.scale,
            drop_to_floor: def.drop_to_floor,
            draw_backfaces: def.draw_backfaces,
            shadow_type: def.shadow_type,
            shadow_color,
            shadow_image: def.shadow_image,
            shadow_size: def.shadow_size,
            light_position: def.light_position,
            ambient_light_color: None,
            ignored_lights: Vec::new(),
            state: Actor3dState::Ready,
            next_state: Actor3dState::Ready,
            target_point_3d: Vec3::ZERO,
            target_point_2d: Point::default(),
            target_angle: 0.0,
            turning_left: false,
            after_walk_angle: -1.0,
            path: Path3D::new(),
            ticket: None,
            path_2d: Vec::new(),
            path_2d_index: 0,
            path_2d_ready: false,
            talk_anim: "talk".to_string(),
            idle_anim: "idle".to_string(),
            walk_anim: "walk".to_string(),
            turn_left_anim: "turnleft".to_string(),
            turn_right_anim: "turnright".to_string(),
            talk_anim_channel: 0,
            state_anim_channel: -1,
            direct_walk: DirectWalk::None,
            direct_turn: DirectTurn::None,
            direct_walk_anim: None,
            direct_turn_anim: None,
            direct_walk_velocity: 0.0,
            direct_turn_velocity: 0.0,
            go_to_tolerance: 2,
            sentence: None,
            temp_skel_anim: None,
            subtitles: SubtitleMods::default(),
            show_sentence: false,
            ready: true,
            current_regions: Vec::new(),
            world_matrix: world_matrix(position, def.angle, def.scale),
            bounding_rect: None,
            bank: SpriteBank::new(),
        }
    }

    /// Loads the model again after a saved game: the actor file's
    /// `MODEL`s and `ANIMATION` blocks, the files `MergeAnims` added
    /// (with their `.anim` sidecars), the sets `UnloadAnim` removed, then
    /// the saved animation state.
    pub fn reload(&mut self, vfs: &Vfs) -> Result<Vec<String>, String> {
        if self.model.model.is_some() {
            return Ok(Vec::new());
        }
        let bytes = vfs.read(&self.filename).map_err(|e| {
            format!(
                "CAdActor3DX::LoadFile failed for file '{}': {e}",
                self.filename
            )
        })?;
        let def = Actor3dDef::parse_with(&bytes, &mut |template| vfs.read(template).ok())
            .map_err(|e| format!("Error parsing ACTOR3D file '{}': {e}", self.filename))?;
        let mut model = load_model(vfs, &self.model_files)?;
        let mut warnings: Vec<String> = def
            .apply_animations(&mut model)
            .into_iter()
            .map(|name| {
                format!(
                    "Error: animation '{name}' not found in model (file: {})",
                    self.filename
                )
            })
            .collect();
        let mut animated = AnimatedModel::new(model);
        for file in self.merged_files.clone() {
            if let Err(err) = Self::merge_into(&mut animated, vfs, &file) {
                warnings.push(err);
            }
        }
        for name in &self.unloaded_sets {
            animated.remove_animation_set(name);
        }
        if let Some(state) = self.model.pending.take() {
            animated.restore(&state);
        }
        self.model.model = Some(animated);
        for attachment in &mut self.attachments {
            if attachment.model.model.is_some() {
                continue;
            }
            let model = vfs
                .read(&attachment.model_file)
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    Model::parse(&bytes, &attachment.model_file).map_err(|e| e.to_string())
                });
            match model {
                Ok(model) => {
                    let mut animated = AnimatedModel::new(model);
                    if let Some(state) = attachment.model.pending.take() {
                        animated.restore(&state);
                    }
                    attachment.model.model = Some(animated);
                }
                Err(err) => warnings.push(format!(
                    "Error loading attachment '{}' of '{}' from '{}': {err}",
                    attachment.name, self.name, attachment.model_file
                )),
            }
        }
        Ok(warnings)
    }

    /// Attaches a model to a bone (`AddAttachment`, `CAdAttach3DX::Init`):
    /// the bone must be a frame of the actor's model, the file a model.
    /// An attachment of the same name (ignoring ASCII case) is replaced in
    /// place; the new one is given `id` as its script object. Returns the
    /// handle of the attachment replaced, if any, so the caller can drop
    /// it. Errors are the original's runtime error texts.
    pub fn add_attachment(
        &mut self,
        vfs: &Vfs,
        id: NativeId,
        file: &str,
        name: &str,
        bone: &str,
    ) -> Result<Option<NativeId>, String> {
        let Some(model) = self.model.model.as_ref() else {
            return Err("Error adding attachment".to_string());
        };
        if model.bone_matrix(bone).is_none() {
            return Err(format!("Bone '{bone}' cannot be found"));
        }
        let model = vfs
            .read(file)
            .ok()
            .and_then(|bytes| Model::parse(&bytes, file).ok())
            .ok_or_else(|| "Error adding attachment".to_string())?;
        let attachment = Attachment3d {
            id,
            name: name.to_string(),
            bone: bone.to_string(),
            model_file: file.to_string(),
            model: ModelSlot {
                model: Some(AnimatedModel::new(model)),
                pending: None,
            },
            active: true,
            position: Vec3::ZERO,
            angle: 0.0,
            scale: 1.0,
            draw_backfaces: false,
            world_matrix: self.world_matrix,
            bounding_rect: None,
        };
        match self
            .attachments
            .iter()
            .position(|a| a.name.eq_ignore_ascii_case(name))
        {
            Some(index) => {
                let old = std::mem::replace(&mut self.attachments[index], attachment);
                Ok(Some(old.id))
            }
            None => {
                self.attachments.push(attachment);
                Ok(None)
            }
        }
    }

    /// Removes an attachment by name (`RemoveAttachment`); the handle of
    /// its script object, for the caller to drop, or `None` when there
    /// is no such attachment.
    pub fn remove_attachment(&mut self, name: &str) -> Option<NativeId> {
        let index = self
            .attachments
            .iter()
            .position(|a| a.name.eq_ignore_ascii_case(name))?;
        Some(self.attachments.remove(index).id)
    }

    /// An attachment by name, ignoring ASCII case (`GetAttachment`).
    pub fn attachment(&self, name: &str) -> Option<&Attachment3d> {
        self.attachments
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
    }

    /// An attachment by script object.
    pub fn attachment_by_id(&self, id: NativeId) -> Option<&Attachment3d> {
        self.attachments.iter().find(|a| a.id == id)
    }

    /// An attachment by script object, mutable.
    pub fn attachment_by_id_mut(&mut self, id: NativeId) -> Option<&mut Attachment3d> {
        self.attachments.iter_mut().find(|a| a.id == id)
    }

    /// Puts a texture on a material (`SetTexture`, `SetTheoraTexture`,
    /// `CXModel::SetMaterialSprite/Theora`): the override of that
    /// material name is replaced, or added. The original does not check
    /// that the model has the material; a name no mesh uses has no
    /// effect.
    pub fn set_material_texture(&mut self, material: &str, texture: MaterialTexture) {
        match self
            .materials
            .iter_mut()
            .find(|m| m.material.eq_ignore_ascii_case(material))
        {
            Some(existing) => existing.texture = texture,
            None => self.materials.push(MaterialOverride {
                material: material.to_string(),
                texture,
            }),
        }
    }

    /// Merges the animation sets of an `.x` file into a model, then the
    /// `ANIMATION` blocks of its `.anim` sidecar when there is one
    /// (`CAdActor3DX::MergeAnimations`).
    fn merge_into(model: &mut AnimatedModel, vfs: &Vfs, file: &str) -> Result<(), String> {
        let bytes = vfs
            .read(file)
            .map_err(|e| format!("Error: MergeAnims failed for file '{file}': {e}"))?;
        model
            .model_mut()
            .merge_animations(&bytes, file)
            .map_err(|e| format!("Error: MergeAnims failed for file '{file}': {e}"))?;
        let sidecar = Actor3dDef::anim_sidecar(file);
        if let Ok(bytes) = vfs.read(&sidecar) {
            let defs = AnimationDef::parse_file(&bytes)
                .map_err(|e| format!("Error loading animation definition file '{sidecar}': {e}"))?;
            for def in defs {
                def.apply(model.model_mut());
            }
        }
        Ok(())
    }

    /// Adds the animation sets of a file (`MergeAnims`); whether it
    /// loaded.
    pub fn merge_anims(&mut self, vfs: &Vfs, file: &str) -> Result<(), String> {
        let model = self.model.model.as_mut().ok_or("no model")?;
        Self::merge_into(model, vfs, file)?;
        self.merged_files.push(file.to_string());
        Ok(())
    }

    /// Removes an animation set (`UnloadAnim`); whether the model had it.
    pub fn unload_anim(&mut self, name: &str) -> bool {
        let Some(model) = self.model.model.as_mut() else {
            return false;
        };
        if model.remove_animation_set(name) {
            self.unloaded_sets.push(name.to_string());
            true
        } else {
            false
        }
    }

    /// The model, once loaded.
    pub fn model(&self) -> Option<&AnimatedModel> {
        self.model.model.as_ref()
    }

    /// The model, mutable.
    pub fn model_mut(&mut self) -> Option<&mut AnimatedModel> {
        self.model.model.as_mut()
    }

    /// The world matrix of the last update (`GetMatrix`): rotation about
    /// Y, uniform scale, position.
    pub fn world_matrix(&self) -> Mat4 {
        self.world_matrix
    }

    /// The projected bounding rectangle of the last update, in scene
    /// pixels, as `(left, top, right, bottom)`; `None` before the first
    /// update.
    pub fn bounding_rect(&self) -> Option<(i32, i32, i32, i32)> {
        self.bounding_rect
    }

    /// The actor's height (`CAdActor3DX::GetHeight`): `Y - top - 5`
    /// of the projected box, 0 before the first update.
    pub fn height(&self) -> i32 {
        self.bounding_rect
            .map_or(0, |(_, top, _, _)| self.y - top - 5)
    }

    /// The position of the hotspot in scene pixels.
    pub fn position_2d(&self) -> Point {
        Point {
            x: self.x,
            y: self.y,
        }
    }

    /// Whether the actor is following a path (`IsWalking`).
    pub fn is_walking(&self) -> bool {
        self.state == Actor3dState::FollowingPath
    }

    /// Whether the actor is saying a sentence.
    pub fn is_talking(&self) -> bool {
        self.state == Actor3dState::Talking
    }

    /// The direction code of the heading (`Direction`).
    pub fn direction(&self) -> i32 {
        angle_to_dir(self.angle)
    }

    /// Where the actor is drawn among the free objects.
    pub(crate) fn free_object(&self, stick: Option<(usize, usize)>) -> FreeObject {
        FreeObject {
            x: self.x,
            y: self.y,
            stick,
        }
    }

    /// Whether a `GoTo` to a scene point is worth starting
    /// (`IsGoToNeeded`): the point is more than `GoToTolerance` pixels
    /// away on either axis.
    pub fn is_go_to_needed(&self, x: i32, y: i32) -> bool {
        !((x - self.x).abs() <= self.go_to_tolerance && (y - self.y).abs() <= self.go_to_tolerance)
    }

    /// Starts a walk to a 3D point (`GoTo3D`); `after_angle` is the
    /// heading to turn to on arrival, `-1` for none. A walk already
    /// going to the same point goes on.
    pub fn go_to_3d(&mut self, target: Vec3, after_angle: f32) {
        self.after_walk_angle = after_angle;
        if target == self.target_point_3d && self.state == Actor3dState::FollowingPath {
            return;
        }
        self.path.reset();
        self.ticket = None;
        self.target_point_3d = target;
        self.state = Actor3dState::SearchingPath;
        self.ready = false;
    }

    /// Starts a walk to a scene point in a `2D_PATHFINDING` scene
    /// (`GoTo2D`): the target is corrected to a walkable point.
    pub fn go_to_2d(&mut self, x: i32, y: i32, after_angle: f32, floor: &Floor<'_>) {
        self.after_walk_angle = after_angle;
        if x == self.target_point_2d.x
            && y == self.target_point_2d.y
            && self.state == Actor3dState::FollowingPath
        {
            return;
        }
        self.path_2d.clear();
        self.path_2d_index = 0;
        self.path_2d_ready = false;
        self.target_point_2d = floor.correct_target(self.position_2d(), Point { x, y });
        self.state = Actor3dState::SearchingPath;
        self.ready = false;
    }

    /// Drops the paths (`GoTo` within the tolerance, `TurnTo`).
    pub fn reset_paths(&mut self) {
        self.path.reset();
        self.ticket = None;
        self.path_2d.clear();
        self.path_2d_index = 0;
        self.path_2d_ready = false;
    }

    /// Prepares a turn to an angle (`PrepareTurn`): both angles are
    /// normalised and the shorter way chosen; returns whether the turn
    /// goes left (the angle decreases).
    fn prepare_turn(&mut self, target: f32) -> bool {
        self.angle = normalize_angle(self.angle);
        let target = normalize_angle(target);
        if self.angle == target {
            self.target_angle = self.angle;
            return true;
        }
        let d1 = target - self.angle;
        let d2 = target + 360.0 - self.angle;
        let d3 = target - 360.0 - self.angle;
        let d1 = if d1.abs() <= d2.abs() { d1 } else { d2 };
        let delta = if d1.abs() <= d3.abs() { d1 } else { d3 };
        self.target_angle = self.angle + delta;
        delta < 0.0
    }

    /// Turns toward an angle (`CAdActor3DX::TurnTo`): no turn when the
    /// actor faces it already.
    pub fn turn_to(&mut self, angle: f32) {
        self.turning_left = self.prepare_turn(angle);
        if self.target_angle == self.angle {
            self.state = self.next_state;
            self.next_state = Actor3dState::Ready;
        } else {
            self.state = Actor3dState::Turning;
            self.ready = false;
        }
    }

    /// Turns toward a 3D point (`TurnTo(object)`).
    pub fn turn_to_point(&mut self, target: Vec3) {
        self.turn_to(heading_to(self.position, target));
    }

    /// One step of a turn (`TurnToStep`): rotates toward the target
    /// angle at `velocity` degrees per second; true when done.
    fn turn_to_step(&mut self, velocity: f32, delta_ms: u64) -> bool {
        let step = velocity * delta_ms as f32 / 1000.0;
        if self.turning_left {
            self.angle -= step;
            if self.angle < self.target_angle {
                self.angle = self.target_angle;
            }
        } else {
            self.angle += step;
            if self.angle > self.target_angle {
                self.angle = self.target_angle;
            }
        }
        if self.angle == self.target_angle {
            self.angle = normalize_angle(self.angle);
            self.target_angle = self.angle;
            true
        } else {
            false
        }
    }

    /// Jumps to a scene point (`CAdObject3D::SkipTo`) with the exact or
    /// the tolerant conversion; false, and nothing changed, when the
    /// point is off the walk planes.
    pub fn skip_to(
        &mut self,
        geometry: &SceneGeometry,
        projection: &Projection,
        x: i32,
        y: i32,
        tolerant: bool,
    ) -> bool {
        let converted = if tolerant {
            geometry.convert_2d_to_3d_tolerant(projection, x, y)
        } else {
            geometry.convert_2d_to_3d(projection, x, y)
        };
        match converted {
            Some(position) => {
                self.position = position;
                self.x = x;
                self.y = y;
                true
            }
            None => false,
        }
    }

    /// Sets the 2D position (`X` or `Y` set by a script): the 3D
    /// position follows with the exact conversion when it succeeds.
    pub fn set_position_2d(
        &mut self,
        geometry: Option<(&SceneGeometry, &Projection)>,
        x: i32,
        y: i32,
    ) {
        self.x = x;
        self.y = y;
        if let Some(position) =
            geometry.and_then(|(geometry, projection)| geometry.convert_2d_to_3d(projection, x, y))
        {
            self.position = position;
        }
    }

    /// Ends the current walk, turn or animation (`CAdObject::Reset`).
    pub fn reset(&mut self) {
        if let Some(sentence) = &mut self.sentence {
            sentence.finish();
        }
        self.state = Actor3dState::Ready;
        self.next_state = Actor3dState::Ready;
    }

    /// Starts saying a sentence (`CAdObject::Talk`), already placed by
    /// the world.
    pub fn talk(&mut self, sentence: Sentence) {
        self.sentence = Some(sentence);
        self.temp_skel_anim = None;
        self.state = Actor3dState::Talking;
        self.ready = false;
    }

    /// Ends the sentence (`StopTalk`); whether one was playing.
    pub fn stop_talk(&mut self) -> bool {
        if let Some(sentence) = &mut self.sentence {
            sentence.finish();
        }
        if self.state == Actor3dState::Talking {
            self.state = self.next_state;
            self.next_state = Actor3dState::Ready;
            true
        } else {
            false
        }
    }

    /// Plays an animation on a channel as a script asks
    /// (`CAdActor3DX::PlayAnim`): restarted, with the actor's transition
    /// times; with `set_state` the actor plays it until it ends. False
    /// when the model has no such set.
    pub fn play_anim(&mut self, channel: usize, name: &str, set_state: bool) -> bool {
        let Some(model) = self.model.model.as_mut() else {
            return false;
        };
        if !model.play(channel, name, true) {
            return false;
        }
        if set_state {
            self.state = Actor3dState::PlayingAnim;
            self.state_anim_channel = channel as i32;
            self.ready = false;
        }
        true
    }

    /// Puts the actor under direct control (`DirectWalk`,
    /// `DirectWalkBack`): `velocity` 0 means the actor's own, `anim`
    /// `None` the walk animation.
    pub fn direct_walk(&mut self, back: bool, velocity: f32, anim: Option<String>) {
        self.direct_walk_velocity = velocity;
        self.direct_walk_anim = anim;
        self.direct_walk = if back {
            DirectWalk::Back
        } else {
            DirectWalk::Forward
        };
        self.state = Actor3dState::DirectControl;
        self.ready = false;
    }

    /// Ends the walking half of direct control (`DirectWalkStop`).
    pub fn direct_walk_stop(&mut self) {
        self.direct_walk = DirectWalk::None;
    }

    /// Puts the actor under direct control (`DirectTurnLeft`,
    /// `DirectTurnRight`).
    pub fn direct_turn(&mut self, right: bool, velocity: f32, anim: Option<String>) {
        self.direct_turn_velocity = velocity;
        self.direct_turn_anim = anim;
        self.direct_turn = if right {
            DirectTurn::Right
        } else {
            DirectTurn::Left
        };
        self.state = Actor3dState::DirectControl;
        self.ready = false;
    }

    /// Ends the turning half of direct control (`DirectTurnStop`).
    pub fn direct_turn_stop(&mut self) {
        self.direct_turn = DirectTurn::None;
    }

    /// Forgets the regions the actor stood in (a scene change).
    pub fn invalidate_regions(&mut self) {
        self.current_regions.clear();
    }

    /// Renumbers the regions the actor stands in, after a node was
    /// inserted in or deleted from a layer.
    pub(crate) fn remap_regions(&mut self, f: impl Fn((usize, usize)) -> (usize, usize)) {
        for region in &mut self.current_regions {
            *region = f(*region);
        }
    }

    /// Raises `ActorEntry` and `ActorLeave` for the regions of the main
    /// layer the actor entered and left (`CAdObject::AfterMove`).
    fn after_move(&mut self, scene: &Scene, events: &mut Vec<ActorEvent>) {
        let new = regions_at(scene, self.x, self.y);
        for &(layer, index) in &new {
            if !self.current_regions.contains(&(layer, index)) {
                events.push(ActorEvent::OnRegion {
                    layer,
                    index,
                    event: "ActorEntry",
                });
            }
        }
        for &(layer, index) in &self.current_regions {
            if !new.contains(&(layer, index)) {
                events.push(ActorEvent::OnRegion {
                    layer,
                    index,
                    event: "ActorLeave",
                });
            }
        }
        self.current_regions = new;
    }

    /// Requests an animation on channel 0 without restarting it when it
    /// already plays, as the state machine does every frame.
    fn request(&mut self, name: &str) {
        if let Some(model) = self.model.model.as_mut() {
            model.play(0, name, false);
        }
    }

    /// One frame (`CAdActor3DX::Update`): the state machine, the 2D
    /// position, the model's animation. The region events are pushed to
    /// `events`; the animation events crossed are returned by name.
    pub fn update(&mut self, ctx: &mut Update3d<'_>, events: &mut Vec<ActorEvent>) -> Vec<String> {
        if self.model.model.is_none() {
            return Vec::new();
        }
        if self.state == Actor3dState::Ready {
            self.state_anim_channel = -1;
        }
        if self.sentence.is_some() && self.state != Actor3dState::Talking {
            self.sentence = None;
            self.show_sentence = false;
            if self.talk_anim_channel > 0 {
                if let Some(model) = self.model.model.as_mut() {
                    let stop = model.default_stop_transition_ms();
                    model.stop(self.talk_anim_channel as usize, stop);
                }
            }
        }
        match self.state {
            Actor3dState::DirectControl => self.direct_step(ctx),
            Actor3dState::Turning => {
                let anim = if self.turning_left {
                    self.turn_left_anim.clone()
                } else {
                    self.turn_right_anim.clone()
                };
                self.request(&anim);
                if self.turn_to_step(self.angular_velocity, ctx.delta_ms) {
                    self.state = self.next_state;
                    self.next_state = Actor3dState::Ready;
                }
            }
            Actor3dState::SearchingPath => {
                let idle = self.idle_anim.clone();
                self.request(&idle);
                match &ctx.floor {
                    Some(floor) => {
                        self.path_2d = floor.find_path(self.position_2d(), self.target_point_2d);
                        self.path_2d_index = 0;
                        self.path_2d_ready = true;
                        self.state = Actor3dState::WaitingPath;
                    }
                    None => {
                        if let Some(ticket) =
                            ctx.finder
                                .request(ctx.geometry, self.position, self.target_point_3d)
                        {
                            self.ticket = Some(ticket);
                            self.state = Actor3dState::WaitingPath;
                        }
                    }
                }
            }
            Actor3dState::WaitingPath => {
                let idle = self.idle_anim.clone();
                self.request(&idle);
                if ctx.floor.is_some() {
                    if self.path_2d_ready {
                        self.follow_path_2d(ctx);
                    }
                } else if let Some(path) = self.ticket.and_then(|t| ctx.finder.take_path(t)) {
                    self.ticket = None;
                    self.path = path;
                    self.follow_path_3d();
                }
            }
            Actor3dState::FollowingPath => {
                if ctx.floor.is_some() {
                    self.next_step_2d(ctx);
                } else {
                    self.next_step_3d(ctx.delta_ms);
                }
                let walk = self.walk_anim.clone();
                self.request(&walk);
            }
            Actor3dState::Talking => self.talk_step(ctx.timer),
            Actor3dState::PlayingAnim => {
                if self.state_anim_channel != 0 {
                    let idle = self.idle_anim.clone();
                    self.request(&idle);
                }
            }
            Actor3dState::Ready => {
                let idle = self.idle_anim.clone();
                self.request(&idle);
            }
        }
        if self.state == Actor3dState::PlayingAnim {
            let playing = usize::try_from(self.state_anim_channel)
                .ok()
                .zip(self.model.model.as_ref())
                .is_some_and(|(channel, model)| model.is_playing(channel, None));
            if !playing {
                self.state = self.next_state;
                self.next_state = Actor3dState::Ready;
            }
        }
        self.ready = self.state == Actor3dState::Ready;

        // `CAdObject3D::Update`: the floor, the world matrix, the 2D
        // position when the 3D one changed.
        if self.drop_to_floor {
            if let Some(height) = ctx.geometry.height_at(self.position, 5.0) {
                self.position.y = height;
            }
        }
        self.world_matrix = world_matrix(self.position, self.angle, self.scale);
        if self.position != self.last_position {
            self.last_position = self.position;
            let (x, y) = ctx.projection.project(self.position);
            if (x, y) != (self.x, self.y) {
                self.x = x;
                self.y = y;
                self.after_move(ctx.scene, events);
            }
        }

        let delta_ms = ctx.delta_ms.min(u64::from(u32::MAX)) as u32;
        let mut anim_events = match self.model.model.as_mut() {
            Some(model) => model.update(delta_ms),
            None => Vec::new(),
        };
        self.update_bounding_rect(ctx.projection);
        // `UpdateAttachments`: the active attachments animate, their events
        // go to the actor; one whose bone is gone keeps its last matrix.
        if let Some(model) = self.model.model.as_ref() {
            let owner_world = self.world_matrix;
            for attachment in &mut self.attachments {
                if !attachment.active {
                    continue;
                }
                let Some(bone) = model.bone_matrix(&attachment.bone) else {
                    continue;
                };
                anim_events.extend(attachment.update(owner_world, bone, ctx.projection, delta_ms));
            }
        }
        anim_events
    }

    /// Projects the eight corners of the model's box into scene pixels
    /// (`CXModel::UpdateBoundingRect`).
    fn update_bounding_rect(&mut self, projection: &Projection) {
        let Some(model) = self.model.model.as_ref() else {
            return;
        };
        self.bounding_rect = project_box(model, self.world_matrix, projection);
    }

    /// The direct-control step (section 8.4 of `docs/3d-actors.md`):
    /// turn at the turning velocity, walk a step in the heading
    /// direction when the geometry (or the 2D floor) allows it, and
    /// request the walk animation on success, idle otherwise. Turning
    /// alone requests the turn animation (or idle) and does not walk in
    /// place, a reading the original's code leaves open.
    fn direct_step(&mut self, ctx: &mut Update3d<'_>) {
        if self.direct_walk == DirectWalk::None && self.direct_turn == DirectTurn::None {
            self.state = self.next_state;
            self.next_state = Actor3dState::Ready;
            return;
        }
        let dt = ctx.delta_ms as f32 / 1000.0;
        let turn_velocity = if self.direct_turn_velocity == 0.0 {
            self.angular_velocity
        } else {
            self.direct_turn_velocity
        };
        match self.direct_turn {
            DirectTurn::Right => self.angle = normalize_angle(self.angle + turn_velocity * dt),
            DirectTurn::Left => self.angle = normalize_angle(self.angle - turn_velocity * dt),
            DirectTurn::None => {}
        }
        if self.direct_walk == DirectWalk::None {
            let anim = self
                .direct_turn_anim
                .clone()
                .unwrap_or_else(|| self.idle_anim.clone());
            self.request(&anim);
            return;
        }
        let walk_velocity = if self.direct_walk_velocity == 0.0 {
            self.velocity
        } else {
            self.direct_walk_velocity
        };
        let step = walk_velocity * self.scale * dt;
        let heading = deg_to_rad(self.angle);
        let mut new_position = self.position;
        let sign = if self.direct_walk == DirectWalk::Back {
            -1.0
        } else {
            1.0
        };
        new_position.x += -heading.sin() * step * sign;
        new_position.z += -heading.cos() * step * sign;
        let can_walk = match &ctx.floor {
            Some(floor) => {
                let matrix = world_matrix(new_position, self.angle, self.scale);
                let (x, y) = ctx.projection.project(matrix.translation_part());
                !floor.is_blocked_at(x, y, false)
            }
            None => ctx.geometry.direct_path_exists(self.position, new_position),
        };
        if can_walk {
            let anim = self
                .direct_walk_anim
                .clone()
                .unwrap_or_else(|| self.walk_anim.clone());
            self.request(&anim);
            self.position = new_position;
        } else {
            let idle = self.idle_anim.clone();
            self.request(&idle);
        }
    }

    /// Starts walking the 3D path found (`FollowPath3D`).
    fn follow_path_3d(&mut self) {
        self.path.first();
        match self.path.current() {
            Some(first) => {
                self.state = Actor3dState::FollowingPath;
                self.init_line(self.position, first, true);
            }
            None => self.arrive(),
        }
    }

    /// The end of a walk: the after-walk turn, else the next state.
    fn arrive(&mut self) {
        if self.after_walk_angle != -1.0 {
            self.turn_to(self.after_walk_angle);
        } else {
            self.state = self.next_state;
            self.next_state = Actor3dState::Ready;
        }
    }

    /// One frame of walking a 3D path (`GetNextStep3D`): turn toward the
    /// target heading, step along it; when the step would not get closer
    /// to the point, snap to it and take the next one.
    fn next_step_3d(&mut self, delta_ms: u64) {
        if self.angle != self.target_angle {
            self.turn_to_step(self.angular_velocity, delta_ms);
        }
        let new_position = self.step_toward_target(delta_ms);
        let current = self.path.current();
        let closer = current.is_some_and(|point| {
            (point - self.position).length() < (point - new_position).length()
        });
        if current.is_none() || closer {
            if let Some(point) = current {
                self.position = point;
            }
            if self.path.advance().is_none() {
                self.path.reset();
                self.arrive();
            } else if let Some(next) = self.path.current() {
                self.init_line(self.position, next, false);
            }
        } else {
            self.position = new_position;
        }
    }

    /// The position a frame's step along the target heading reaches.
    fn step_toward_target(&self, delta_ms: u64) -> Vec3 {
        let step = self.velocity * self.scale * delta_ms as f32 / 1000.0;
        let heading = deg_to_rad(self.target_angle);
        let mut new_position = self.position;
        new_position.x += -heading.sin() * step;
        new_position.z += -heading.cos() * step;
        new_position
    }

    /// Sets up the walk toward a point (`InitLine3D`): the first point
    /// turns the actor in place first; the others only set the target
    /// heading, which the walk turns toward while moving.
    fn init_line(&mut self, start: Vec3, end: Vec3, first_step: bool) {
        let heading = heading_to(start, end);
        if first_step {
            self.next_state = Actor3dState::FollowingPath;
            self.turn_to(heading);
        } else {
            self.turning_left = self.prepare_turn(heading);
        }
    }

    /// The current point of the 2D path as a 3D point (tolerant
    /// conversion).
    fn current_2d_point(&self, ctx: &Update3d<'_>) -> Option<Vec3> {
        let point = self.path_2d.get(self.path_2d_index)?;
        ctx.geometry
            .convert_2d_to_3d_tolerant(ctx.projection, point.x, point.y)
    }

    /// Starts walking the 2D path found (`FollowPath2D`): the points at
    /// the actor's own position are skipped.
    fn follow_path_2d(&mut self, ctx: &Update3d<'_>) {
        self.path_2d_index = 0;
        while self
            .path_2d
            .get(self.path_2d_index)
            .is_some_and(|p| *p == self.position_2d())
        {
            self.path_2d_index += 1;
        }
        match self.current_2d_point(ctx) {
            Some(first) => {
                self.state = Actor3dState::FollowingPath;
                self.init_line(self.position, first, true);
            }
            None => self.arrive(),
        }
    }

    /// One frame of walking a 2D path (`GetNextStep2D`): like the 3D
    /// step, each point converted as it is reached.
    fn next_step_2d(&mut self, ctx: &Update3d<'_>) {
        let Some(current) = self.current_2d_point(ctx) else {
            self.state = self.next_state;
            self.next_state = Actor3dState::Ready;
            return;
        };
        if self.angle != self.target_angle {
            self.turn_to_step(self.angular_velocity, ctx.delta_ms);
        }
        let new_position = self.step_toward_target(ctx.delta_ms);
        if (current - self.position).length() < (current - new_position).length() {
            self.position = current;
            self.path_2d_index += 1;
            match self.current_2d_point(ctx) {
                None => {
                    self.path_2d.clear();
                    self.path_2d_index = 0;
                    self.arrive();
                }
                Some(next) => self.init_line(self.position, next, false),
            }
        } else {
            self.position = new_position;
        }
    }

    /// One frame of talking (section 8.2, `STATE_TALKING`): the
    /// sentence's stances play on channel 0, restarted at each change;
    /// without a stance the talk animation plays on its channel, else
    /// idle.
    fn talk_step(&mut self, timer: u64) {
        self.show_sentence = false;
        let Some(time_is_up) = self.sentence.as_ref().map(|s| s.time_is_up(timer)) else {
            self.state = Actor3dState::Ready;
            return;
        };
        let stance_done = match (&self.temp_skel_anim, self.model.model.as_ref()) {
            (Some(stance), Some(model)) => !model.is_playing(0, Some(stance)),
            _ => true,
        };
        if stance_done || time_is_up {
            if time_is_up {
                if let Some(sentence) = &mut self.sentence {
                    sentence.finish();
                }
                self.temp_skel_anim = None;
                self.state = self.next_state;
                self.next_state = Actor3dState::Ready;
                if self.talk_anim_channel > 0 {
                    if let Some(model) = self.model.model.as_mut() {
                        let stop = model.default_stop_transition_ms();
                        model.stop(self.talk_anim_channel as usize, stop);
                    }
                }
            } else {
                self.temp_skel_anim = self.sentence.as_mut().and_then(|s| s.next_stance());
                let (talk, idle) = (self.talk_anim.clone(), self.idle_anim.clone());
                let channel = usize::try_from(self.talk_anim_channel).unwrap_or(0);
                match (self.temp_skel_anim.clone(), self.model.model.as_mut()) {
                    (Some(stance), Some(model)) => {
                        model.play(0, &stance, true);
                    }
                    (None, Some(model)) => {
                        if model.model().animation_set(&talk).is_some() {
                            model.play(channel, &talk, false);
                        } else {
                            model.play(0, &idle, false);
                        }
                    }
                    _ => {}
                }
                self.show_sentence = true;
            }
        } else {
            if let (Some(stance), Some(model)) =
                (self.temp_skel_anim.clone(), self.model.model.as_mut())
            {
                model.play(0, &stance, false);
            }
            self.show_sentence = true;
        }
    }

    /// Whether a scene pixel inside the bounding rectangle misses the
    /// model's triangles (`CXModel::IsTransparentAt`): the pick ray of
    /// the pixel is moved to model space by the inverse world matrix and
    /// tested against every posed triangle, front faces only.
    pub fn is_transparent_at(&self, projection: &Projection, x: i32, y: i32) -> bool {
        let Some(model) = self.model.model.as_ref() else {
            return true;
        };
        let hits_model = self.world_matrix.inverse().is_some_and(|inverse| {
            let ray = projection.pick_ray(x, y);
            let origin = inverse.transform_coord(ray.origin);
            let direction = inverse.transform_normal(ray.direction);
            model.pick(origin, direction).is_some()
        });
        if hits_model {
            return false;
        }
        // The actor is clickable through its attachments (section 8.6).
        !self.attachments.iter().any(|a| {
            a.active
                && a.bounding_rect
                    .is_some_and(|(l, t, r, b)| (l..=r).contains(&x) && (t..=b).contains(&y))
                && !a.is_transparent_at(projection, x, y)
        })
    }

    /// The projected rectangles the actor is clickable in: its model's
    /// and its active attachments' (`CAdAttach3DX::Display` registers the
    /// owner), in that order; none before the first update.
    pub fn hit_rects(&self) -> Vec<(i32, i32, i32, i32)> {
        self.bounding_rect
            .into_iter()
            .chain(
                self.attachments
                    .iter()
                    .filter(|a| a.active)
                    .filter_map(|a| a.bounding_rect),
            )
            .collect()
    }

    /// The world position of a bone (`GetBonePosition3D`): the origin
    /// (or `offset`) through the bone's combined matrix and the world
    /// matrix; `None` for an unknown bone.
    pub fn bone_position(&self, bone: &str, offset: Vec3) -> Option<Vec3> {
        let model = self.model.model.as_ref()?;
        let matrix = *model.bone_matrix(bone)? * self.world_matrix;
        Some(matrix.transform_coord(offset))
    }

    /// The polygon the actor blocks for the 2D path finder, in scene
    /// coordinates (`UpdateBlockRegion`), at the actor's 2D position
    /// and the scene's scale at its `y`.
    pub fn block_polygon(&self, scene: &Scene) -> Option<Vec<Point>> {
        let region = self.blocked_region.as_ref()?;
        Some(crate::actor::mimic(
            &region.points,
            scene.scale_at(self.y),
            self.x,
            self.y,
        ))
    }
}

/// `0xAARRGGBB` of a colour.
fn rgba_int(c: Rgba) -> i32 {
    luma_script::rgba(
        i32::from(c.r),
        i32::from(c.g),
        i32::from(c.b),
        i32::from(c.a),
    )
}

/// What the drawing step needs for one 3D actor this frame.
pub struct Actor3dDraw<'a> {
    /// The actor's handle.
    pub id: NativeId,
    /// Its index among the free objects (`DrawItem::Other`), to place it
    /// in the scene's drawing order.
    pub other_index: usize,
    /// The posed, skinned model.
    pub model: &'a AnimatedModel,
    /// The world matrix.
    pub world: Mat4,
    /// The position in world units (the feet), where the shadows lie.
    pub position: Vec3,
    /// The heading in degrees, which turns the simple shadow.
    pub angle: f32,
    /// The lights that light the actor, at most [`crate::scene3d::MAX_LIGHTS`].
    pub lights: Vec<&'a Light>,
    /// The ambient colour: the actor's override, else the scene's.
    pub ambient: i32,
    /// Whether both sides of the triangles are drawn.
    pub draw_backfaces: bool,
    /// The shadow to draw: the lowest of the actor's, the game's and the
    /// scene's shadow types.
    pub shadow_type: ShadowType,
    /// `0xAARRGGBB` of the shadow.
    pub shadow_color: i32,
    /// The shadow light, in world units (the actor's position plus its
    /// relative light position times the scale).
    pub shadow_light: Vec3,
    /// Half-size of the simple shadow quad, times the scale.
    pub shadow_size: f32,
    /// The simple shadow's image: the actor's, else the game's.
    pub shadow_image: Option<&'a str>,
    /// The tint and the blend mode, as for a 2D object.
    pub tint: Option<Rgba>,
    /// See `tint`.
    pub blend_mode: BlendMode,
    /// The textures scripts put on the model's materials, by material
    /// name (section 10.5): the drawing step textures every mesh material
    /// of that name with the file's current frame instead of the `.x`
    /// texture.
    pub materials: &'a [MaterialOverride],
    /// The active attachments whose bone exists, in order (section 8.6):
    /// drawn after the model with their own matrices, lit as the actor,
    /// and part of its stencil shadow volume (10.4). Nothing else is
    /// needed from the actor to draw them.
    pub attachments: Vec<AttachmentDraw<'a>>,
}

/// What the drawing step needs for one attachment of a 3D actor.
pub struct AttachmentDraw<'a> {
    /// The attachment's script object.
    pub id: NativeId,
    /// Its name.
    pub name: &'a str,
    /// The bone it follows.
    pub bone: &'a str,
    /// The posed, skinned model.
    pub model: &'a AnimatedModel,
    /// The full matrix: the attachment's own, the bone's combined matrix,
    /// the actor's world matrix. Used as the actor's `world` is.
    pub world: Mat4,
    /// Whether both sides of the triangles are drawn.
    pub draw_backfaces: bool,
}

/// What the drawing step needs for the scene this frame.
pub struct ThreeDFrame<'a> {
    /// The camera's projection for the frame.
    pub projection: &'a Projection,
    /// The scene's ambient light.
    pub ambient: Rgb,
    /// The fog, when enabled.
    pub fog: Option<crate::scene3d::Fog>,
    /// The geometry, for the shadow receivers.
    pub geometry: &'a SceneGeometry,
    /// Whether the game and the scene allow stencil shadows
    /// (`CAdGame::GetMaxShadowType()`): the receivers are then written
    /// to the depth buffer before the main layer's nodes.
    pub stencil_shadows: bool,
}

impl World {
    /// A 3D actor by handle.
    pub fn actor3d(&self, id: NativeId) -> Option<&Actor3d> {
        self.actors3d.iter().find(|(i, _)| *i == id).map(|(_, a)| a)
    }

    /// A 3D actor by handle, mutable.
    pub fn actor3d_mut(&mut self, id: NativeId) -> Option<&mut Actor3d> {
        self.actors3d
            .iter_mut()
            .find(|(i, _)| *i == id)
            .map(|(_, a)| a)
    }

    /// The handles of the 3D actors, in load order.
    pub fn actor3d_ids(&self) -> Vec<NativeId> {
        self.actors3d.iter().map(|(id, _)| *id).collect()
    }

    /// Whether the main object is a 3D actor (the auto-scroll ignores
    /// changes under 5 pixels for one, `CAdScene::ScrollTo`).
    pub(crate) fn main_object_is_3d(&self) -> bool {
        self.main_object()
            .is_some_and(|id| matches!(self.objects.get(id).map(|n| &n.kind), Some(Kind::Actor3D)))
    }

    /// Loads a 3D actor (`Game.LoadActor3D`, `Scene.LoadActor3D`, and
    /// `LoadActor` given an `ACTOR3DX` file) and attaches its scripts;
    /// `None` when the file cannot be loaded.
    pub fn load_actor3d(
        &mut self,
        engine: &mut Engine,
        path: &str,
        scene_bound: bool,
    ) -> Option<NativeId> {
        let Loaded3d {
            mut actor,
            warnings,
            scripts,
            properties,
        } = match Actor3d::load(&self.vfs, path, scene_bound) {
            Ok(loaded) => loaded,
            Err(err) => {
                self.log(err);
                return None;
            }
        };
        for warning in warnings {
            self.log(warning);
        }
        actor.caption = self.expand(&actor.caption);
        let id = self.objects.insert(Kind::Actor3D);
        // As in the original, `X` and `Y` read 0 until the first update
        // projects the position.
        self.actors3d.push((id, actor));
        self.apply_properties(id, &properties);
        for script in scripts {
            self.attach(engine, id, &script);
        }
        Some(id)
    }

    /// The 3D actors that go with the scene, for a scene change.
    pub(crate) fn scene_bound_actors3d(&self) -> Vec<NativeId> {
        self.actors3d
            .iter()
            .filter(|(_, a)| a.scene_bound)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Reads the geometry and the models again after a saved game
    /// (`InitAfterLoad`), logging what fails.
    pub(crate) fn reload_3d(&mut self) {
        let vfs = &self.vfs;
        let mut messages = Vec::new();
        if let Some(scene3d) = self.scene.as_mut().and_then(|v| v.three_d.as_mut()) {
            if let Err(err) = scene3d.reload(vfs) {
                messages.push(err);
            }
        }
        for (_, actor) in &mut self.actors3d {
            match actor.reload(vfs) {
                Ok(warnings) => messages.extend(warnings),
                Err(err) => messages.push(err),
            }
        }
        for message in messages {
            self.log(message);
        }
    }

    /// One frame of every active 3D actor (`CAdScene::UpdateFreeObjects`
    /// for the 3D objects): the path finder runs its open request, then
    /// each actor's state machine, position and model advance. The
    /// events raised are queued.
    pub(crate) fn update_actors3d(&mut self) {
        if self.actors3d.is_empty() {
            return;
        }
        let Some(projection) = self.projection_3d() else {
            return;
        };
        self.run_path_finder_3d();
        let (timer, delta_ms) = (self.timer, self.timer_delta);
        let ids = self.actor3d_ids();
        let obstacles: Vec<_> = ids.iter().map(|id| self.obstacles_for(*id)).collect();
        let mut raised: Vec<(NativeId, Vec<ActorEvent>, Vec<String>)> = Vec::new();
        {
            let Some(view) = self.scene.as_mut() else {
                return;
            };
            let Some(scene3d) = view.three_d.as_mut() else {
                return;
            };
            scene3d.set_projection(Some(projection.clone()));
            let Scene3d {
                geometry, finder, ..
            } = scene3d;
            let scene = &view.scene;
            for (index, (id, actor)) in self.actors3d.iter_mut().enumerate() {
                if !actor.active {
                    continue;
                }
                let (blockers, waypoints) = &obstacles[index];
                let floor = scene.pathfinding_2d.then(|| Floor {
                    scene,
                    blockers,
                    waypoints,
                });
                let mut ctx = Update3d {
                    scene,
                    geometry,
                    finder,
                    projection: &projection,
                    floor,
                    timer,
                    delta_ms,
                };
                let mut events = Vec::new();
                let anim_events = actor.update(&mut ctx, &mut events);
                raised.push((*id, events, anim_events));
            }
        }
        for (id, events, anim_events) in raised {
            self.queue_events(id, events);
            for event in anim_events {
                self.queue_event(id, event);
            }
        }
    }

    /// The hit areas of a 3D actor: its projected bounding rectangle and
    /// its attachments', confirmed at test time by a pick into the models
    /// (section 10.3).
    pub(crate) fn actor3d_hits(
        &self,
        id: NativeId,
        layer_offset: (i32, i32),
        hits: &mut Vec<HitArea>,
    ) {
        let Some(actor) = self.actor3d(id) else {
            return;
        };
        if !actor.interactive || (actor.ignore_items && self.selected_item.is_some()) {
            return;
        }
        for rect in actor.hit_rects() {
            hits.push(HitArea::model3d(id, layer_offset, rect));
        }
    }

    /// Whether a scene pixel inside a 3D actor's rectangle hits its
    /// model (`CBActiveRect::IsTransparentAt` through `CXModel`).
    pub(crate) fn actor3d_hit_at(&self, id: NativeId, x: i32, y: i32) -> bool {
        let Some(actor) = self.actor3d(id) else {
            return false;
        };
        let Some(projection) = self
            .scene3d()
            .and_then(|s| s.projection().cloned())
            .or_else(|| self.projection_3d())
        else {
            return false;
        };
        !actor.is_transparent_at(&projection, x, y)
    }

    /// Everything the drawing step needs for the 3D actors of the frame:
    /// the frame's camera and lights, and for each active 3D actor with
    /// a model its handle, its index among the free objects (the
    /// `DrawItem::Other` the scene's order places it at), its skinned
    /// model, world matrix, lights and shadow. `None` without geometry
    /// or before the first update of the frame.
    pub fn three_d_draws(&self) -> Option<(ThreeDFrame<'_>, Vec<Actor3dDraw<'_>>)> {
        let view = self.scene.as_ref()?;
        let scene3d = view.three_d.as_ref()?;
        let projection = scene3d.projection()?;
        let max_shadow = self.max_shadow_type.min(view.scene.max_shadow_type);
        let frame = ThreeDFrame {
            projection,
            ambient: view.scene.ambient_light_color,
            fog: scene3d.fog,
            geometry: &scene3d.geometry,
            stencil_shadows: max_shadow >= ShadowType::Stencil as u8,
        };
        let scene_ambient = luma_script::rgba(
            i32::from(view.scene.ambient_light_color.r),
            i32::from(view.scene.ambient_light_color.g),
            i32::from(view.scene.ambient_light_color.b),
            255,
        );
        let mut draws = Vec::new();
        for (index, (id, _)) in self.free_objects().into_iter().enumerate() {
            let Some(actor) = self.actor3d(id) else {
                continue;
            };
            let Some(model) = actor.model() else {
                continue;
            };
            let shadow_type =
                ShadowType::from_index(i64::from((actor.shadow_type as u8).min(max_shadow)));
            draws.push(Actor3dDraw {
                id,
                other_index: index,
                model,
                world: actor.world_matrix(),
                position: actor.position,
                angle: actor.angle,
                lights: Scene3d::lights_for(
                    &scene3d.geometry,
                    actor.position,
                    &actor.ignored_lights,
                ),
                ambient: actor.ambient_light_color.unwrap_or(scene_ambient),
                draw_backfaces: actor.draw_backfaces,
                shadow_type,
                shadow_color: actor.shadow_color,
                shadow_light: actor.position + actor.light_position * actor.scale,
                shadow_size: actor.shadow_size * actor.scale,
                shadow_image: actor
                    .shadow_image
                    .as_deref()
                    .or(self.shadow_image.as_deref()),
                tint: actor.tint,
                blend_mode: actor.blend_mode,
                materials: &actor.materials,
                attachments: actor
                    .attachments
                    .iter()
                    .filter(|a| a.active && model.bone_matrix(&a.bone).is_some())
                    .filter_map(|a| {
                        Some(AttachmentDraw {
                            id: a.id,
                            name: &a.name,
                            bone: &a.bone,
                            model: a.model()?,
                            world: a.world_matrix(),
                            draw_backfaces: a.draw_backfaces,
                        })
                    })
                    .collect(),
            });
        }
        Some((frame, draws))
    }

    /// Attaches a model to a bone of a 3D actor (`AddAttachment`): the
    /// attachment gets a script object of its own; the one it replaces
    /// loses its handle, as the original deletes it. Errors are the
    /// original's runtime error texts, for the caller to raise.
    pub fn add_attachment3d(
        &mut self,
        id: NativeId,
        file: &str,
        name: &str,
        bone: &str,
    ) -> Result<(), String> {
        if self.actor3d(id).is_none() {
            return Err("Error adding attachment".to_string());
        }
        let handle = self.objects.insert(Kind::Attachment3D(id));
        let vfs = &self.vfs;
        let result = self
            .actors3d
            .iter_mut()
            .find(|(i, _)| *i == id)
            .map(|(_, a)| a.add_attachment(vfs, handle, file, name, bone))
            .unwrap_or_else(|| Err("Error adding attachment".to_string()));
        match result {
            Ok(replaced) => {
                if let Some(old) = replaced {
                    self.objects.remove(old);
                }
                Ok(())
            }
            Err(err) => {
                self.objects.remove(handle);
                Err(err)
            }
        }
    }

    /// Removes an attachment of a 3D actor by name (`RemoveAttachment`),
    /// with its script object; whether there was one.
    pub fn remove_attachment3d(&mut self, id: NativeId, name: &str) -> bool {
        match self.actor3d_mut(id).and_then(|a| a.remove_attachment(name)) {
            Some(handle) => {
                self.objects.remove(handle);
                true
            }
            None => false,
        }
    }

    /// The attachment a script object stands for, with its owner.
    pub fn attachment3d(&self, handle: NativeId) -> Option<(NativeId, &Attachment3d)> {
        let Kind::Attachment3D(owner) = self.native(handle)?.kind else {
            return None;
        };
        let attachment = self.actor3d(owner)?.attachment_by_id(handle)?;
        Some((owner, attachment))
    }

    /// The attachment a script object stands for, mutable.
    pub(crate) fn attachment3d_mut(&mut self, handle: NativeId) -> Option<&mut Attachment3d> {
        let Kind::Attachment3D(owner) = self.native(handle)?.kind else {
            return None;
        };
        self.actor3d_mut(owner)?.attachment_by_id_mut(handle)
    }

    /// Where a 3D actor's particle emitter goes when it follows a bone
    /// (`CAdActor3DX::UpdatePartEmitter` with `m_PartBone`): the bone's
    /// position, offset, projected to scene pixels; `None` when the actor
    /// has no bone emitter, the bone is unknown or there is no camera.
    pub(crate) fn actor3d_bone_emitter_position(&self, id: NativeId) -> Option<(i32, i32)> {
        let actor = self.actor3d(id)?;
        let bone = actor.part_bone.as_deref()?;
        let position = actor.bone_position(bone, actor.part_offset)?;
        let projection = self
            .scene3d()
            .and_then(|s| s.projection().cloned())
            .or_else(|| self.projection_3d())?;
        Some(projection.project(position))
    }

    /// Walks a 3D actor to an entity (`GoToObject`): to its `WALK_TO`
    /// point and direction, or to its position when the point is `0,0`
    /// (the original converts the `0,0` itself, a slip no scene relies
    /// on). Returns whether a walk or a turn started, for the script to
    /// wait on.
    pub(crate) fn actor3d_go_to_entity(&mut self, id: NativeId, entity: &Entity) -> bool {
        let (x, y, after) = if entity.walk_to_x == 0 && entity.walk_to_y == 0 {
            (entity.x, entity.y, -1.0)
        } else {
            (
                entity.walk_to_x,
                entity.walk_to_y,
                entity
                    .walk_to_dir
                    .map_or(-1.0, |d| dir_to_angle(i32::from(d.code()))),
            )
        };
        let needed = self.actor3d(id).is_some_and(|a| a.is_go_to_needed(x, y));
        if !needed {
            let turns = after != -1.0;
            if let Some(actor) = self.actor3d_mut(id) {
                actor.reset_paths();
                if turns {
                    actor.turn_to(after);
                }
            }
            return turns;
        }
        self.actor3d_go_to(id, x, y, after)
    }

    /// Walks a 3D actor to a scene point (`GoTo`): in a `2D_PATHFINDING`
    /// scene on the 2D floor, else to the tolerant conversion of the
    /// point. Returns whether a walk started.
    pub(crate) fn actor3d_go_to(&mut self, id: NativeId, x: i32, y: i32, after: f32) -> bool {
        let Some(view) = self.scene.as_ref() else {
            return false;
        };
        if view.scene.pathfinding_2d {
            let (blockers, waypoints) = self.obstacles_for(id);
            let scene = self.scene.as_ref().map(|v| &v.scene);
            let Some(scene) = scene else { return false };
            let floor = Floor {
                scene,
                blockers: &blockers,
                waypoints: &waypoints,
            };
            let Some(actor) = self
                .actors3d
                .iter_mut()
                .find(|(i, _)| *i == id)
                .map(|(_, a)| a)
            else {
                return false;
            };
            actor.go_to_2d(x, y, after, &floor);
            return true;
        }
        let Some(projection) = self.projection_3d() else {
            return false;
        };
        let Some(target) = self
            .scene3d()
            .and_then(|s| s.geometry.convert_2d_to_3d_tolerant(&projection, x, y))
        else {
            return false;
        };
        match self.actor3d_mut(id) {
            Some(actor) => {
                actor.go_to_3d(target, after);
                true
            }
            None => false,
        }
    }

    /// The 3D position of an object's 2D position, for `TurnTo(object)`
    /// (the exact conversion).
    pub(crate) fn point_3d_of(&self, x: i32, y: i32) -> Option<Vec3> {
        let projection = self.projection_3d()?;
        self.scene3d()?.geometry.convert_2d_to_3d(&projection, x, y)
    }
}
