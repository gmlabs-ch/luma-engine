//! The properties and methods of 3D actors, as scripts call them (see
//! `docs/3d-actors.md`, section 11).
//!
//! Written with reference to the `ScGetProperty`, `ScSetProperty` and
//! `ScCallMethod` methods of `AdActor3DX.cpp`, `AdObject3D.cpp`,
//! `AdObject.cpp` and `BObject.cpp` of Wintermute Engine 1.x (MIT License,
//! Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.

use luma_3d::{ShadowType, Vec3};
use luma_formats::BlendMode;
use luma_script::{Context, Effect, NativeId, Properties, Return, Value, rgba};

use crate::actor3d::{dir_to_angle, normalize_angle};
use crate::objects::Kind;
use crate::world::World;

fn color_int(c: luma_formats::Rgba) -> i32 {
    rgba(
        i32::from(c.r),
        i32::from(c.g),
        i32::from(c.b),
        i32::from(c.a),
    )
}

fn color_of(v: i32) -> luma_formats::Rgba {
    luma_formats::Rgba {
        r: ((v >> 16) & 0xff) as u8,
        g: ((v >> 8) & 0xff) as u8,
        b: (v & 0xff) as u8,
        a: ((v >> 24) & 0xff) as u8,
    }
}

/// An object `{ X, Y }` or `{ X, Y, Z }`; an empty object when the
/// position is unknown (`GetBonePosition2D/3D` of an unknown bone leave
/// the result without members).
fn point_object(point: Option<(f64, f64, Option<f64>)>) -> Value {
    let mut props = Properties::new();
    if let Some((x, y, z)) = point {
        props.insert("X".to_string(), Value::Float(x));
        props.insert("Y".to_string(), Value::Float(y));
        if let Some(z) = z {
            props.insert("Z".to_string(), Value::Float(z));
        }
    }
    Value::Object(props)
}

impl World {
    pub(crate) fn actor3d_get(&self, id: NativeId, name: &str) -> Option<Value> {
        let actor = self.actor3d(id)?;
        let model = actor.model();
        Some(match name {
            "Type" => Value::from("actor3dx"),
            "Name" => Value::from(actor.name.clone()),
            "Caption" => Value::from(actor.caption.clone()),
            "Filename" => Value::from(actor.filename.clone()),
            "X" => Value::Int(actor.x),
            "Y" => Value::Int(actor.y),
            "Height" => Value::Int(actor.height()),
            "Ready" => Value::Bool(actor.ready),
            "Active" => Value::Bool(actor.active),
            "Registrable" | "Interactive" => Value::Bool(actor.interactive),
            "PosX" => Value::Float(f64::from(actor.position.x)),
            "PosY" => Value::Float(f64::from(actor.position.y)),
            "PosZ" => Value::Float(f64::from(actor.position.z)),
            "Angle" | "DirectionAngle" | "DirAngle" => Value::Float(f64::from(actor.angle)),
            "Direction" => Value::Int(actor.direction()),
            "Velocity" => Value::Float(f64::from(actor.velocity)),
            "AngularVelocity" | "AngVelocity" => Value::Float(f64::from(actor.angular_velocity)),
            "Scale" => Value::Float(f64::from(actor.scale * 100.0)),
            "DropToFloor" => Value::Bool(actor.drop_to_floor),
            "DrawBackfaces" => Value::Bool(actor.draw_backfaces),
            "ShadowType" => Value::Int(actor.shadow_type as i32),
            "Shadow" => Value::Bool(actor.shadow_type != ShadowType::None),
            "SimpleShadow" => Value::Bool(actor.shadow_type == ShadowType::Simple),
            "ShadowColor" => Value::Int(actor.shadow_color),
            "AmbientLightColor" => actor.ambient_light_color.map_or(Value::Null, Value::Int),
            "TalkAnimName" => Value::from(actor.talk_anim.clone()),
            "IdleAnimName" => Value::from(actor.idle_anim.clone()),
            "WalkAnimName" => Value::from(actor.walk_anim.clone()),
            "TurnLeftAnimName" => Value::from(actor.turn_left_anim.clone()),
            "TurnRightAnimName" => Value::from(actor.turn_right_anim.clone()),
            "TalkAnimChannel" => Value::Int(actor.talk_anim_channel),
            "AnimTransitionTime" => {
                Value::Int(model.map_or(0, |m| m.default_transition_ms() as i32))
            }
            "AnimStopTransitionTime" => {
                Value::Int(model.map_or(0, |m| m.default_stop_transition_ms() as i32))
            }
            "GoToTolerance" => Value::Int(actor.go_to_tolerance),
            "AlphaColor" => Value::Int(actor.tint.map_or(0, color_int)),
            "BlendMode" => Value::Int(actor.blend_mode.to_int()),
            "IgnoreItems" => Value::Bool(actor.ignore_items),
            "SoundPanning" => Value::Bool(actor.sound_panning),
            "SaveState" => Value::Bool(true),
            "SubtitlesWidth" => Value::Int(actor.subtitles.width),
            "SubtitlesPosX" => Value::Int(actor.subtitles.x),
            "SubtitlesPosY" => Value::Int(actor.subtitles.y),
            "SubtitlesPosRelative" => Value::Bool(actor.subtitles.relative),
            "SubtitlesPosXCenter" => Value::Bool(actor.subtitles.x_center),
            "NumItems" => Value::Int(self.taken_items_of(id).len() as i32),
            "ParticleEmitter" => self.emitter_of(id).map_or(Value::Null, Value::Native),
            "Item" => Value::Null,
            _ => return None,
        })
    }

    pub(crate) fn actor3d_set(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
        let (text, int, flag, float) = (
            value.get_string(self),
            value.get_int(self),
            value.get_bool(self),
            value.get_float(self) as f32,
        );
        let is_null = value.is_null();
        let caption = self.expand(&text);
        let anim_name = |default: &str| {
            if is_null {
                default.to_string()
            } else {
                text.clone()
            }
        };
        // `X` and `Y` convert the 2D point to 3D with the exact conversion.
        if name == "X" || name == "Y" {
            let projection = self.projection_3d();
            let Some(view) = self.scene.as_mut() else {
                return false;
            };
            let geometry = view.three_d.as_ref().zip(projection.as_ref());
            let Some(actor) = self
                .actors3d
                .iter_mut()
                .find(|(i, _)| *i == id)
                .map(|(_, a)| a)
            else {
                return false;
            };
            let (x, y) = if name == "X" {
                (int, actor.y)
            } else {
                (actor.x, int)
            };
            actor.set_position_2d(geometry.map(|(s, p)| (&s.geometry, p)), x, y);
            return true;
        }
        let Some(actor) = self.actor3d_mut(id) else {
            return false;
        };
        match name {
            "Name" => actor.name = text,
            "Caption" => actor.caption = caption,
            "Active" => actor.active = flag,
            "Registrable" | "Interactive" => actor.interactive = flag,
            "PosX" => actor.position.x = float,
            "PosY" => actor.position.y = float,
            "PosZ" => actor.position.z = float,
            "Angle" => actor.angle = float,
            "DirectionAngle" | "DirAngle" => actor.angle = normalize_angle(float),
            "Direction" => actor.angle = dir_to_angle(int),
            "Velocity" => actor.velocity = float,
            "AngularVelocity" | "AngVelocity" => actor.angular_velocity = float,
            "Scale" => actor.scale = float / 100.0,
            "DropToFloor" => actor.drop_to_floor = flag,
            "DrawBackfaces" => actor.draw_backfaces = flag,
            "ShadowType" => actor.shadow_type = ShadowType::from_index(i64::from(int)),
            "Shadow" => {
                actor.shadow_type = if flag {
                    ShadowType::Stencil
                } else {
                    ShadowType::None
                }
            }
            "SimpleShadow" => {
                actor.shadow_type = if flag {
                    ShadowType::Simple
                } else {
                    ShadowType::Stencil
                }
            }
            "ShadowColor" => actor.shadow_color = int,
            "AmbientLightColor" => actor.ambient_light_color = (!is_null).then_some(int),
            "TalkAnimName" => actor.talk_anim = anim_name("talk"),
            "IdleAnimName" => actor.idle_anim = anim_name("idle"),
            "WalkAnimName" => actor.walk_anim = anim_name("walk"),
            "TurnLeftAnimName" => actor.turn_left_anim = anim_name("turnleft"),
            "TurnRightAnimName" => actor.turn_right_anim = anim_name("turnright"),
            "TalkAnimChannel" => actor.talk_anim_channel = int,
            "AnimTransitionTime" => {
                if let Some(model) = actor.model_mut() {
                    model.set_default_transition_ms(int.max(0) as u32);
                }
            }
            "AnimStopTransitionTime" => {
                if let Some(model) = actor.model_mut() {
                    model.set_default_stop_transition_ms(int.max(0) as u32);
                }
            }
            "GoToTolerance" => actor.go_to_tolerance = int,
            "AlphaColor" => actor.tint = Some(color_of(int)),
            "BlendMode" => actor.blend_mode = BlendMode::from_int(int),
            "IgnoreItems" => actor.ignore_items = flag,
            "SoundPanning" => actor.sound_panning = flag,
            "SubtitlesWidth" => actor.subtitles.width = int,
            "SubtitlesPosX" => actor.subtitles.x = int,
            "SubtitlesPosY" => actor.subtitles.y = int,
            "SubtitlesPosRelative" => actor.subtitles.relative = flag,
            "SubtitlesPosXCenter" => actor.subtitles.x_center = flag,
            "SaveState" => {}
            _ => return false,
        }
        true
    }

    pub(crate) fn actor3d_call(
        &mut self,
        id: NativeId,
        name: &str,
        args: &[Value],
        ctx: Context<'_>,
    ) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        if let Some(ret) = self.object_sound_call(id, name, args) {
            return Some(ret);
        }
        let wait = |sync: bool| {
            if sync {
                Return::effect(Effect::WaitForExclusive(id))
            } else {
                Return::null()
            }
        };
        let optional_string = |v: Value, world: &World| match v {
            Value::Null => None,
            v => Some(v.get_string(world)),
        };
        Some(match name {
            "PlayAnim" | "PlayAnimAsync" => {
                let anim = arg(0).get_string(self);
                // The async form sets the state too (section 11.1).
                let started = self
                    .actor3d_mut(id)
                    .is_some_and(|a| a.play_anim(0, &anim, true));
                if started && name == "PlayAnim" {
                    Return {
                        value: Value::Bool(true),
                        effect: Effect::WaitFor(id),
                    }
                } else {
                    Return::value(started)
                }
            }
            "PlayAnimChannel" | "PlayAnimChannelAsync" => {
                let channel = usize::try_from(arg(0).get_int(self)).ok();
                let anim = arg(1).get_string(self);
                let sync = name == "PlayAnimChannel";
                let started = channel.is_some_and(|channel| {
                    self.actor3d_mut(id)
                        .is_some_and(|a| a.play_anim(channel, &anim, sync))
                });
                if started && sync {
                    Return {
                        value: Value::Bool(true),
                        effect: Effect::WaitFor(id),
                    }
                } else {
                    Return::value(started)
                }
            }
            "StopAnim" | "StopAnimChannel" => {
                let (channel, time) = if name == "StopAnim" {
                    (Some(0usize), arg(0))
                } else {
                    (usize::try_from(arg(0).get_int(self)).ok(), arg(1))
                };
                let time = (!time.is_null()).then(|| time.get_int(self).max(0) as u32);
                let stopped = self.actor3d_mut(id).is_some_and(|a| {
                    a.model_mut().is_some_and(|model| {
                        // `StopAnim` defaults to the stop transition time,
                        // `StopAnimChannel` to 0.
                        let time = time.unwrap_or(if name == "StopAnim" {
                            model.default_stop_transition_ms()
                        } else {
                            0
                        });
                        channel.is_some_and(|c| model.stop(c, time))
                    })
                });
                Return::value(stopped)
            }
            "IsAnimPlaying" | "IsAnimChannelPlaying" => {
                let (channel, anim) = if name == "IsAnimPlaying" {
                    (0usize, arg(0))
                } else {
                    (arg(0).get_int(self).max(0) as usize, arg(1))
                };
                let anim = optional_string(anim, self);
                Return::value(self.actor3d(id).is_some_and(|a| {
                    a.model()
                        .is_some_and(|m| m.is_playing(channel, anim.as_deref()))
                }))
            }
            "SetAnimTransitionTime" => {
                let (from, to) = (arg(0).get_string(self), arg(1).get_string(self));
                let time = arg(2).get_int(self);
                if let Some(model) = self.actor3d_mut(id).and_then(|a| a.model_mut()) {
                    model.set_transition_time(&from, &to, u32::try_from(time).ok());
                }
                Return::null()
            }
            "GetAnimTransitionTime" => {
                let (from, to) = (arg(0).get_string(self), arg(1).get_string(self));
                let time = self
                    .actor3d(id)
                    .and_then(|a| a.model())
                    .and_then(|m| {
                        m.transition_times()
                            .find(|(f, t, _)| {
                                f.eq_ignore_ascii_case(&from) && t.eq_ignore_ascii_case(&to)
                            })
                            .map(|(_, _, time)| time as i32)
                    })
                    .unwrap_or(-1);
                Return::value(time)
            }
            "MergeAnims" => {
                let path = arg(0).get_string(self);
                let vfs = &self.vfs;
                let result = self
                    .actors3d
                    .iter_mut()
                    .find(|(i, _)| *i == id)
                    .map(|(_, a)| a.merge_anims(vfs, &path));
                match result {
                    Some(Ok(())) => Return::value(true),
                    Some(Err(err)) => {
                        self.log(err);
                        Return::value(false)
                    }
                    None => Return::value(false),
                }
            }
            "UnloadAnim" => {
                let anim = arg(0).get_string(self);
                Return::value(self.actor3d_mut(id).is_some_and(|a| a.unload_anim(&anim)))
            }
            "GoTo" | "GoToAsync" => {
                let (x, y) = (arg(0).get_int(self), arg(1).get_int(self));
                let needed = self.actor3d(id).is_some_and(|a| a.is_go_to_needed(x, y));
                if needed {
                    let started = self.actor3d_go_to(id, x, y, -1.0);
                    wait(started && name == "GoTo")
                } else {
                    if let Some(actor) = self.actor3d_mut(id) {
                        actor.reset_paths();
                    }
                    Return::null()
                }
            }
            "GoTo3D" | "GoTo3DAsync" => {
                let target = Vec3::new(
                    arg(0).get_float(self) as f32,
                    arg(1).get_float(self) as f32,
                    arg(2).get_float(self) as f32,
                );
                if let Some(actor) = self.actor3d_mut(id) {
                    actor.go_to_3d(target, -1.0);
                }
                wait(name == "GoTo3D")
            }
            "GoToObject" | "GoToObjectAsync" => {
                let entity = arg(0).as_native().and_then(|target| {
                    match self.native(target).map(|n| n.kind.clone()) {
                        Some(Kind::Entity(r)) => self.entity_def(r).cloned(),
                        _ => None,
                    }
                });
                let Some(entity) = entity else {
                    self.runtime_error(
                        &ctx,
                        &format!("actor.{name} method accepts an entity refrence only"),
                    );
                    return Some(Return::null());
                };
                let started = self.actor3d_go_to_entity(id, &entity);
                wait(started && name == "GoToObject")
            }
            "TurnTo" | "TurnToAsync" => {
                let target = arg(0);
                let angle = match target.as_native().filter(|t| self.objects.contains(*t)) {
                    Some(object) => {
                        // Toward the object's 2D position converted to 3D;
                        // the original turns to garbage when the conversion
                        // fails, Luma does not turn.
                        let Some(point) = self
                            .object_position_2d(object)
                            .and_then(|(x, y)| self.point_3d_of(x, y))
                        else {
                            return Some(Return::null());
                        };
                        if let Some(actor) = self.actor3d_mut(id) {
                            actor.reset_paths();
                            actor.turn_to_point(point);
                        }
                        return Some(wait(name == "TurnTo"));
                    }
                    None => dir_to_angle(target.get_int(self)),
                };
                if let Some(actor) = self.actor3d_mut(id) {
                    actor.reset_paths();
                    actor.turn_to(angle);
                }
                wait(name == "TurnTo")
            }
            "TurnToAngle" | "TurnToAngleAsync" => {
                let angle = arg(0).get_float(self) as f32;
                if let Some(actor) = self.actor3d_mut(id) {
                    actor.reset_paths();
                    actor.turn_to(angle);
                }
                wait(name == "TurnToAngle")
            }
            "IsWalking" => Return::value(self.actor3d(id).is_some_and(|a| a.is_walking())),
            "DirectWalk" | "DirectWalkBack" => {
                let velocity = if arg(0).is_null() {
                    0.0
                } else {
                    arg(0).get_float(self) as f32
                };
                let anim = optional_string(arg(1), self);
                if let Some(actor) = self.actor3d_mut(id) {
                    actor.direct_walk(name == "DirectWalkBack", velocity, anim);
                }
                Return::value(true)
            }
            "DirectWalkStop" => {
                if let Some(actor) = self.actor3d_mut(id) {
                    actor.direct_walk_stop();
                }
                Return::value(true)
            }
            "DirectTurnLeft" | "DirectTurnRight" => {
                let velocity = if arg(0).is_null() {
                    0.0
                } else {
                    arg(0).get_float(self) as f32
                };
                let anim = optional_string(arg(1), self);
                if let Some(actor) = self.actor3d_mut(id) {
                    actor.direct_turn(name == "DirectTurnRight", velocity, anim);
                }
                Return::value(true)
            }
            "DirectTurnStop" => {
                if let Some(actor) = self.actor3d_mut(id) {
                    actor.direct_turn_stop();
                }
                Return::value(true)
            }
            "SkipTo" => {
                let (x, y) = (arg(0).get_int(self), arg(1).get_int(self));
                let projection = self.projection_3d();
                if let (Some(view), Some(projection)) = (self.scene.as_ref(), projection) {
                    if let Some(scene3d) = view.three_d.as_ref() {
                        if let Some(actor) = self
                            .actors3d
                            .iter_mut()
                            .find(|(i, _)| *i == id)
                            .map(|(_, a)| a)
                        {
                            actor.skip_to(&scene3d.geometry, &projection, x, y, false);
                        }
                    }
                }
                Return::null()
            }
            "SkipTo3D" => {
                let position = Vec3::new(
                    arg(0).get_float(self) as f32,
                    arg(1).get_float(self) as f32,
                    arg(2).get_float(self) as f32,
                );
                if let Some(actor) = self.actor3d_mut(id) {
                    actor.position = position;
                }
                Return::null()
            }
            "GetBonePosition2D" => {
                let bone = arg(0).get_string(self);
                let projection = self.projection_3d();
                let point = self
                    .actor3d(id)
                    .and_then(|a| a.bone_position(&bone, Vec3::ZERO))
                    .zip(projection)
                    .map(|(p, projection)| {
                        let (x, y) = projection.project(p);
                        (f64::from(x), f64::from(y), None)
                    });
                Return::value(point_object(point))
            }
            "GetBonePosition3D" => {
                let bone = arg(0).get_string(self);
                let point = self
                    .actor3d(id)
                    .and_then(|a| a.bone_position(&bone, Vec3::ZERO))
                    .map(|p| (f64::from(p.x), f64::from(p.y), Some(f64::from(p.z))));
                Return::value(point_object(point))
            }
            "AddIgnoredLight" => {
                let light = arg(0).get_string(self);
                if let Some(actor) = self.actor3d_mut(id) {
                    if !actor
                        .ignored_lights
                        .iter()
                        .any(|l| l.eq_ignore_ascii_case(&light))
                    {
                        actor.ignored_lights.push(light);
                    }
                }
                Return::value(true)
            }
            "RemoveIgnoredLight" => {
                let light = arg(0).get_string(self);
                let removed = self.actor3d_mut(id).is_some_and(|actor| {
                    let before = actor.ignored_lights.len();
                    actor
                        .ignored_lights
                        .retain(|l| !l.eq_ignore_ascii_case(&light));
                    actor.ignored_lights.len() != before
                });
                Return::value(removed)
            }
            "SetShadowImage" => {
                let image = optional_string(arg(0), self);
                let ok = image.as_deref().is_none_or(|p| self.vfs.exists(p));
                if let Some(actor) = self.actor3d_mut(id) {
                    actor.shadow_image = if ok { image } else { None };
                }
                Return::value(ok)
            }
            "GetShadowImage" => Return::value(
                self.actor3d(id)
                    .and_then(|a| a.shadow_image.clone())
                    .map_or(Value::Null, Value::from),
            ),
            "SetLightPosition" => {
                let position = Vec3::new(
                    arg(0).get_float(self) as f32,
                    arg(1).get_float(self) as f32,
                    arg(2).get_float(self) as f32,
                );
                if let Some(actor) = self.actor3d_mut(id) {
                    actor.light_position = position;
                }
                Return::null()
            }
            "Reset" => {
                if let Some(actor) = self.actor3d_mut(id) {
                    actor.reset();
                }
                ctx.engine.reset_object(id);
                Return::null()
            }
            "Talk" | "TalkAsync" => self.talk_call(id, name == "Talk", args),
            "StopTalk" | "StopTalking" => Return::value(self.stop_talk(id)),
            "IsTalking" => Return::value(self.is_talking(id)),
            "SetFont" => {
                let path = optional_string(arg(0), self);
                self.set_font(id, path.as_deref());
                Return::null()
            }
            "GetFont" => Return::value(
                self.actor3d(id)
                    .and_then(|a| a.font.clone())
                    .map_or(Value::Null, Value::from),
            ),
            "TakeItem" | "DropItem" | "GetItem" | "HasItem" => {
                return Some(self.inventory_call(id, name, args));
            }
            "CreateParticleEmitter" | "DeleteParticleEmitter" => {
                self.emitter_owner_call(id, name, args)
            }
            // Attachments and bone emitters (task 17), textures and
            // effects on materials (task 20, effects not planned): the
            // calls fail as the original's do when the file or the
            // material is missing.
            "AddAttachment"
            | "AddMesh"
            | "RemoveAttachment"
            | "RemoveMesh"
            | "SetTexture"
            | "SetTheoraTexture"
            | "SetEffect"
            | "RemoveEffect"
            | "SetEffectParam"
            | "SetEffectParamVector"
            | "SetEffectParamColor" => {
                self.stub(&format!("Actor3D.{name}"));
                Return::value(false)
            }
            "GetAttachment" | "CreateParticleEmitterBone" => {
                self.stub(&format!("Actor3D.{name}"));
                Return::null()
            }
            _ => return None,
        })
    }

    /// The 2D position of any object with one, for `TurnTo(object)`.
    pub(crate) fn object_position_2d(&self, id: NativeId) -> Option<(i32, i32)> {
        match &self.native(id)?.kind {
            Kind::Actor => self.actor(id).map(|a| (a.x, a.y)),
            Kind::Actor3D => self.actor3d(id).map(|a| (a.x, a.y)),
            Kind::Entity(r) => self.entity_def(*r).map(|e| (e.x, e.y)),
            Kind::Region { layer, index } => {
                let b = self.scene()?.region(*layer, *index)?.bounds()?;
                Some((b.left, b.top))
            }
            _ => None,
        }
    }
}
