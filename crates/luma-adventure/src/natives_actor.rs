//! The properties and methods of actors, as scripts call them.
//!
//! Written with reference to the `ScGetProperty`, `ScSetProperty` and
//! `ScCallMethod` methods of `AdActor.cpp`, `AdTalkHolder.cpp`,
//! `AdObject.cpp` and `BObject.cpp` of WME Lite (MIT License, Copyright
//! (c) 2011 Jan Nedoma); see `NOTICE`.

use luma_formats::scene::Direction;
use luma_script::{Context, Effect, NativeId, Return, Value, rgba};

use crate::objects::Kind;
use crate::world::World;

/// Methods of later milestones that actors accept and log once.
const ACTOR_STUBS: &[&str] = &[
    "ForceTalkAnim",
    "AddTalkSprite",
    "RemoveTalkSprite",
    "SetTalkSprite",
    "SetCursor",
    "RemoveCursor",
    "StickToRegion",
    "CreateParticleEmitter",
    "DeleteParticleEmitter",
    "AddAttachment",
    "RemoveAttachment",
    "SetCaption",
];

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

impl World {
    pub(crate) fn actor_get(&self, id: NativeId, name: &str) -> Option<Value> {
        let actor = self.actor(id)?;
        let scene = self.scene().map(|v| &v.scene);
        Some(match name {
            "Type" => Value::from("actor"),
            "Name" => Value::from(actor.def.name.clone()),
            "Caption" => Value::from(actor.def.caption.clone()),
            "Filename" => Value::from(actor.filename.clone()),
            "X" => Value::Int(actor.x),
            "Y" => Value::Int(actor.y),
            "Height" => Value::Int(scene.map_or(0, |s| actor.height(s))),
            "Ready" => Value::Bool(actor.ready),
            "Direction" => Value::Int(i32::from(actor.dir.code())),
            "ActorIsBlocked" => Value::Bool(actor.actor_is_blocked),
            "TalkAnimName" => Value::from(actor.talk_anim.clone()),
            "IdleAnimName" => Value::from(actor.idle_anim.clone()),
            "WalkAnimName" => Value::from(actor.walk_anim.clone()),
            "TurnLeftAnimName" => Value::from(actor.turn_left_anim.clone()),
            "TurnRightAnimName" => Value::from(actor.turn_right_anim.clone()),
            "Active" => Value::Bool(actor.def.active),
            "Movable" => Value::Bool(true),
            "Registrable" | "Interactive" => Value::Bool(actor.def.interactive),
            "Zoomable" | "Scalable" => Value::Bool(actor.def.scalable),
            "Rotatable" => Value::Bool(actor.def.rotatable),
            "AlphaColor" => Value::Int(actor.def.tint.map_or(0, color_int)),
            "BlendMode" => Value::Int(0),
            "Scale" => actor
                .def
                .scale
                .map_or(Value::Null, |s| Value::Float(f64::from(s))),
            "ScaleX" | "ScaleY" | "Rotate" => Value::Null,
            "RelativeScale" => Value::Float(f64::from(actor.def.relative_scale)),
            "RelativeRotate" => Value::Float(0.0),
            "Colorable" => Value::Bool(actor.def.colorable),
            "SoundPanning" => Value::Bool(actor.def.sound_panning),
            "SaveState" => Value::Bool(true),
            "NonIntMouseEvents" => Value::Bool(false),
            "AccCaption" => Value::Null,
            "IgnoreItems" => Value::Bool(actor.def.ignore_items),
            "SceneIndependent" => Value::Bool(false),
            "SubtitlesWidth" => Value::Int(actor.subtitles.width),
            "SubtitlesPosX" => Value::Int(actor.subtitles.x),
            "SubtitlesPosY" => Value::Int(actor.subtitles.y),
            "SubtitlesPosRelative" => Value::Bool(actor.subtitles.relative),
            "SubtitlesPosXCenter" => Value::Bool(actor.subtitles.x_center),
            "NumItems" => Value::Int(self.inventory.taken.len() as i32),
            "NumAttachments" => Value::Int(0),
            "ParticleEmitter" | "Item" => Value::Null,
            _ => return None,
        })
    }

    pub(crate) fn actor_set(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
        let (text, int, flag, float) = (
            value.get_string(self),
            value.get_int(self),
            value.get_bool(self),
            value.get_float(self),
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
        let Some(actor) = self.actor_mut(id) else {
            return false;
        };
        match name {
            "Name" => actor.def.name = text,
            "Caption" => actor.def.caption = caption,
            "X" => actor.x = int,
            "Y" => actor.y = int,
            "Direction" => {
                if let Some(dir) = Direction::from_index(i64::from(int)) {
                    actor.dir = dir;
                }
            }
            "TalkAnimName" => actor.talk_anim = anim_name("talk"),
            "IdleAnimName" => actor.idle_anim = anim_name("idle"),
            "WalkAnimName" => actor.walk_anim = anim_name("walk"),
            "TurnLeftAnimName" => actor.turn_left_anim = anim_name("turnleft"),
            "TurnRightAnimName" => actor.turn_right_anim = anim_name("turnright"),
            "Active" => actor.def.active = flag,
            "Registrable" | "Interactive" => actor.def.interactive = flag,
            "Zoomable" | "Scalable" => actor.def.scalable = flag,
            "Rotatable" => actor.def.rotatable = flag,
            "AlphaColor" => actor.def.tint = Some(color_of(int)),
            "Scale" => actor.def.scale = if is_null { None } else { Some(float as f32) },
            "RelativeScale" => actor.def.relative_scale = float as f32,
            "Colorable" => actor.def.colorable = flag,
            "SoundPanning" => actor.def.sound_panning = flag,
            "IgnoreItems" => actor.def.ignore_items = flag,
            "SubtitlesWidth" => actor.subtitles.width = int,
            "SubtitlesPosX" => actor.subtitles.x = int,
            "SubtitlesPosY" => actor.subtitles.y = int,
            "SubtitlesPosRelative" => actor.subtitles.relative = flag,
            "SubtitlesPosXCenter" => actor.subtitles.x_center = flag,
            "Movable" | "BlendMode" | "ScaleX" | "ScaleY" | "Rotate" | "RelativeRotate"
            | "SaveState" | "NonIntMouseEvents" | "AccCaption" | "SceneIndependent" => {}
            _ => return false,
        }
        true
    }

    pub(crate) fn actor_call(
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
        Some(match name {
            "GoTo" | "GoToAsync" => {
                let (x, y) = (arg(0).get_int(self), arg(1).get_int(self));
                self.with_floor(id, |actor, floor| actor.go_to(x, y, None, floor));
                wait(name == "GoTo")
            }
            "GoToObject" | "GoToObjectAsync" => {
                let entity = arg(0).as_native().and_then(|target| {
                    match self.native(target).map(|n| n.kind.clone()) {
                        Some(Kind::Entity(r)) => self.scene()?.entity(r).cloned(),
                        _ => None,
                    }
                });
                let Some(entity) = entity else {
                    self.log(format!(
                        "Runtime error. Script '{}', line {}: actor.{name} method accepts an entity refrence only",
                        self.script_name(&ctx),
                        ctx.line
                    ));
                    return Some(Return::null());
                };
                self.with_floor(id, |actor, floor| actor.go_to_entity(&entity, floor));
                wait(name == "GoToObject")
            }
            "TurnTo" | "TurnToAsync" => {
                let target = arg(0);
                let dir = match target.as_native().filter(|t| self.objects.contains(*t)) {
                    Some(object) => {
                        let position = self.object_position(object);
                        if let (Some((x, y)), Some(actor)) = (position, self.actor_mut(id)) {
                            actor.turn_to_point(x, y);
                            return Some(wait(name == "TurnTo"));
                        }
                        None
                    }
                    None => Direction::from_index(i64::from(target.get_int(self))),
                };
                match (dir, self.actor_mut(id)) {
                    (Some(dir), Some(actor)) => {
                        actor.turn_to(dir);
                        wait(name == "TurnTo")
                    }
                    _ => Return::null(),
                }
            }
            "IsWalking" => Return::value(self.actor(id).is_some_and(|a| a.is_walking())),
            "SkipTo" => {
                let (x, y) = (arg(0).get_int(self), arg(1).get_int(self));
                let mut events = Vec::new();
                let scene = self.scene().map(|v| v.scene.clone());
                if let (Some(scene), Some(actor)) = (scene, self.actor_mut(id)) {
                    actor.skip_to(x, y, &scene, &mut events);
                } else if let Some(actor) = self.actor_mut(id) {
                    actor.x = x;
                    actor.y = y;
                }
                self.queue_events(id, events);
                Return::null()
            }
            "Reset" => {
                if let Some(actor) = self.actor_mut(id) {
                    actor.reset();
                }
                ctx.engine.reset_object(id);
                Return::null()
            }
            "PlayAnim" | "PlayAnimAsync" => {
                let anim = arg(0).get_string(self);
                let started = self
                    .actor_and_vfs(id)
                    .is_some_and(|(vfs, actor)| actor.play_anim(vfs, &anim));
                if !started {
                    let who = self.describe(id);
                    self.log(format!(
                        "CAdObject::PlayAnim: error loading temp sprite (object:\"{who}\" sprite:\"{anim}\")"
                    ));
                    return Some(Return::value(false));
                }
                if name == "PlayAnim" {
                    Return {
                        value: Value::Bool(true),
                        effect: Effect::WaitFor(id),
                    }
                } else {
                    Return::value(true)
                }
            }
            "SetSprite" => {
                let path = match arg(0) {
                    Value::Null => None,
                    v => Some(v.get_string(self)),
                };
                let ok = self
                    .actor_and_vfs(id)
                    .is_some_and(|(vfs, actor)| actor.set_sprite(vfs, path.as_deref()));
                if !ok {
                    let path = path.unwrap_or_default();
                    self.log(format!("SetSprite method failed for file '{path}'"));
                }
                Return::value(ok)
            }
            "GetSprite" => Return::value(
                self.actor(id)
                    .and_then(|a| a.sprite_override())
                    .map_or(Value::Null, Value::from),
            ),
            "GetSpriteObject" => {
                self.stub("Actor.GetSpriteObject");
                Return::null()
            }
            "Caption" => Return::value(
                self.actor(id)
                    .map(|a| a.def.caption.clone())
                    .unwrap_or_default(),
            ),
            "MergeAnims" => {
                let path = arg(0).get_string(self);
                let ok = self
                    .actor_and_vfs(id)
                    .is_some_and(|(vfs, actor)| actor.merge_anims(vfs, &path));
                if !ok {
                    self.log(format!("CAdActor::MergeAnims failed for file '{path}'"));
                }
                Return::value(ok)
            }
            "UnloadAnim" => {
                let anim = arg(0).get_string(self);
                Return::value(
                    self.actor_mut(id)
                        .is_some_and(|actor| actor.unload_anim(&anim)),
                )
            }
            "HasAnim" => {
                let anim = arg(0).get_string(self);
                Return::value(
                    self.actor(id)
                        .is_some_and(|actor| actor.def.animation(&anim).is_some()),
                )
            }
            "SetStopOnBlocked" => {
                let flag = arg(0).get_bool(self);
                if let Some(actor) = self.actor_mut(id) {
                    actor.stop_on_blocked = flag;
                }
                Return::null()
            }
            "GetStopOnBlocked" => Return::value(self.actor(id).is_some_and(|a| a.stop_on_blocked)),
            "Talk" | "TalkAsync" => self.talk_call(id, name == "Talk", args),
            "StopTalk" | "StopTalking" => Return::value(self.stop_talk(id)),
            "IsTalking" => Return::value(self.is_talking(id)),
            "SetFont" => {
                let path = match arg(0) {
                    Value::Null => None,
                    v => Some(v.get_string(self)),
                };
                self.set_font(id, path.as_deref());
                Return::null()
            }
            "GetFont" => Return::value(
                self.actor(id)
                    .and_then(|a| a.def.font.clone())
                    .map_or(Value::Null, Value::from),
            ),
            "TakeItem" | "DropItem" | "GetItem" | "HasItem" => {
                return Some(self.inventory_call(name, args));
            }
            "HasCursor" => Return::value(false),
            "GetCursor" | "GetCursorObject" | "GetAttachment" => Return::null(),
            _ if ACTOR_STUBS.contains(&name) => {
                self.stub(&format!("Actor.{name}"));
                Return::null()
            }
            _ => return None,
        })
    }

    /// The position of any object with one, for `TurnTo(object)`.
    fn object_position(&self, id: NativeId) -> Option<(i32, i32)> {
        match &self.native(id)?.kind {
            Kind::Actor => self.actor(id).map(|a| (a.x, a.y)),
            Kind::Entity(r) => self.scene()?.entity(*r).map(|e| (e.x, e.y)),
            Kind::Region { layer, index } => {
                let b = self.scene()?.region(*layer, *index)?.bounds()?;
                Some((b.left, b.top))
            }
            _ => None,
        }
    }

    /// The file of the script making a call, for messages.
    fn script_name(&self, ctx: &Context<'_>) -> String {
        ctx.engine
            .script(ctx.script)
            .map(|s| s.filename().to_string())
            .unwrap_or_default()
    }
}
