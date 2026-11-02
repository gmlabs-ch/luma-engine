//! The particle emitter objects scripts hold (`CreateParticleEmitter`,
//! `ParticleEmitter` on an entity or an actor): their parameters,
//! sprites, forces and controls, as scripts read, write and call them,
//! and the three members of their owners that create, return and delete
//! them.
//!
//! Written with reference to the `ScGetProperty`, `ScSetProperty` and
//! `ScCallMethod` methods of `PartEmitter.cpp` and the
//! `CreateParticleEmitter`, `DeleteParticleEmitter` and
//! `ParticleEmitter` members of `AdObject.cpp` of Wintermute Engine 1.x
//! (MIT License, Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.

use luma_formats::BlendMode;
use luma_script::{NativeId, Return, Value};

use crate::objects::Kind;
use crate::particles::{Emitter, ForceKind, Rng};
use crate::world::World;

impl World {
    /// The emitter of a handle.
    pub fn emitter(&self, id: NativeId) -> Option<&Emitter> {
        self.emitters
            .iter()
            .find(|(eid, _)| *eid == id)
            .map(|(_, e)| e)
    }

    /// The emitter of a handle, mutable.
    pub fn emitter_mut(&mut self, id: NativeId) -> Option<&mut Emitter> {
        self.emitters
            .iter_mut()
            .find(|(eid, _)| *eid == id)
            .map(|(_, e)| e)
    }

    /// The handle of an object's emitter, if it has one
    /// (`ParticleEmitter`).
    pub fn emitter_of(&self, owner: NativeId) -> Option<NativeId> {
        self.objects.find(|k| *k == Kind::Emitter(owner))
    }

    /// Creates an object's emitter, or redefines how the existing one
    /// follows it (`CAdObject::CreateParticleEmitter`): `follow_parent`
    /// makes the emitter move with the owner, at `offset` scaled by the
    /// owner's scale. Returns the emitter's handle.
    pub fn create_particle_emitter(
        &mut self,
        owner: NativeId,
        follow_parent: bool,
        offset_x: i32,
        offset_y: i32,
    ) -> NativeId {
        let id = match self.emitter_of(owner) {
            Some(id) => id,
            None => {
                let id = self.objects.insert(Kind::Emitter(owner));
                self.emitters.push((id, Emitter::new(owner)));
                id
            }
        };
        if let Some(emitter) = self.emitter_mut(id) {
            emitter.follow_parent = follow_parent;
            emitter.offset_x = offset_x;
            emitter.offset_y = offset_y;
        }
        self.update_emitter(id);
        id
    }

    /// Deletes an object's emitter (`DeleteParticleEmitter`): its handle
    /// becomes invalid.
    pub fn delete_particle_emitter(&mut self, owner: NativeId) {
        if let Some(id) = self.emitter_of(owner) {
            self.remove_emitter(id);
        }
    }

    /// Removes an emitter and its handle.
    pub(crate) fn remove_emitter(&mut self, id: NativeId) {
        self.emitters.retain(|(eid, _)| *eid != id);
        self.objects.remove(id);
    }

    /// The position and the scale (in percent) of an emitter's owner,
    /// for an emitter that follows it (`CAdObject::UpdatePartEmitter`,
    /// with `GetScale`).
    fn owner_placement(&self, owner: NativeId) -> Option<(i32, i32, f32, f32)> {
        match &self.native(owner)?.kind {
            Kind::Actor => {
                let actor = self.actor(owner)?;
                let (scale_x, scale_y) = self.scene().map_or((100.0, 100.0), |view| {
                    let t = actor.transform(&view.scene);
                    (t.scale_x, t.scale_y)
                });
                Some((actor.x, actor.y, scale_x, scale_y))
            }
            // A 3D actor's emitter follows its projected position, unscaled
            // (`CAdActor3DX::UpdatePartEmitter` without a bone), or the
            // projected position of the bone `CreateParticleEmitterBone`
            // named; an unknown bone leaves it at the actor's position
            // (the original moves it to an uninitialised point).
            #[cfg(feature = "3d")]
            Kind::Actor3D => {
                let actor = self.actor3d(owner)?;
                let (x, y) = self
                    .actor3d_bone_emitter_position(owner)
                    .unwrap_or((actor.x, actor.y));
                Some((x, y, 100.0, 100.0))
            }
            Kind::Entity(r) => {
                let entity = self.entity_def(*r)?;
                let (scale_x, scale_y) = match r {
                    crate::scene_view::EntityRef::Ui(_) => {
                        let s = entity.scale.unwrap_or(100.0);
                        (s, s)
                    }
                    _ => self
                        .scene()
                        .map_or((100.0, 100.0), |view| view.scene.entity_scale_xy(entity)),
                };
                Some((entity.x, entity.y, scale_x, scale_y))
            }
            _ => None,
        }
    }

    /// One frame of an emitter (`UpdatePartEmitter`): it follows its
    /// owner when asked, then updates when running; a batch born raises
    /// the emit event on the owner. An emitter whose owner is gone goes
    /// too.
    fn update_emitter(&mut self, id: NativeId) {
        let Some(owner) = self.emitter(id).map(|e| e.owner) else {
            return;
        };
        let Some((x, y, scale_x, scale_y)) = self.owner_placement(owner) else {
            self.remove_emitter(id);
            return;
        };
        let (timer, delta) = (self.timer, self.timer_delta);
        let mut rng = Rng::new(self.rng);
        let mut event = None;
        if let Some(emitter) = self.emitter_mut(id) {
            if emitter.follow_parent {
                emitter.x = x + (scale_x / 100.0 * emitter.offset_x as f32) as i32;
                emitter.y = y + (scale_y / 100.0 * emitter.offset_y as f32) as i32;
            }
            if emitter.update(timer, delta, &mut rng) {
                event = emitter.emit_event.clone();
            }
        }
        self.rng = rng.state();
        if let Some(event) = event {
            self.pending_events.push((owner, event));
        }
    }

    /// One frame of every emitter, after the objects moved; nothing
    /// while the game is frozen, as the scene does not update then.
    pub fn update_emitters(&mut self) {
        if self.ui.frozen {
            return;
        }
        let ids: Vec<NativeId> = self.emitters.iter().map(|(id, _)| *id).collect();
        for id in ids {
            self.update_emitter(id);
        }
    }

    /// `CreateParticleEmitter(followParent, offsetX, offsetY)` and
    /// `DeleteParticleEmitter()` on an entity or an actor.
    pub(crate) fn emitter_owner_call(
        &mut self,
        owner: NativeId,
        name: &str,
        args: &[Value],
    ) -> Return {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        match name {
            "CreateParticleEmitter" => {
                let follow = arg(0).get_bool(self);
                let offset_x = arg(1).get_int(self);
                let offset_y = arg(2).get_int(self);
                let id = self.create_particle_emitter(owner, follow, offset_x, offset_y);
                Return::value(Value::Native(id))
            }
            _ => {
                self.delete_particle_emitter(owner);
                Return::null()
            }
        }
    }

    /// A method of a particle emitter.
    pub(crate) fn emitter_call(
        &mut self,
        id: NativeId,
        name: &str,
        args: &[Value],
    ) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        let ints: Vec<i32> = (0..5).map(|n| arg(n).get_int(self)).collect();
        let floats: Vec<f64> = (0..5).map(|n| arg(n).get_float(self)).collect();
        let text = arg(0).get_string(self);
        let timer = self.timer;
        let mut rng = Rng::new(self.rng);
        let vfs = &self.vfs;
        let emitter = self
            .emitters
            .iter_mut()
            .find(|(eid, _)| *eid == id)
            .map(|(_, e)| e)?;
        let result = match name {
            "SetBorder" => {
                emitter.set_border(ints[0], ints[1], ints[2], ints[3]);
                true
            }
            "SetBorderThickness" => {
                emitter.set_border_thickness(ints[0], ints[1], ints[2], ints[3]);
                true
            }
            "AddSprite" => emitter.add_sprite(vfs, &text),
            "RemoveSprite" => emitter.remove_sprite(&text),
            "Start" => {
                emitter.overhead_time = ints[0];
                emitter.start(timer, &mut rng);
                true
            }
            "Stop" => {
                emitter.stop();
                true
            }
            "Pause" => {
                emitter.pause();
                true
            }
            "Resume" => {
                emitter.resume();
                true
            }
            "AddGlobalForce" => {
                emitter.add_force(
                    &text,
                    ForceKind::Global,
                    0,
                    0,
                    floats[1] as f32,
                    floats[2] as f32,
                );
                true
            }
            // The original engine 1.x makes a global force here too (see
            // `Emitter::add_force`).
            "AddPointForce" => {
                emitter.add_force(
                    &text,
                    ForceKind::Global,
                    ints[1],
                    ints[2],
                    floats[3] as f32,
                    floats[4] as f32,
                );
                true
            }
            "RemoveForce" => emitter.remove_force(&text),
            _ => return None,
        };
        self.rng = rng.state();
        Some(Return::value(result))
    }

    /// A property of a particle emitter.
    pub(crate) fn emitter_get(&self, id: NativeId, name: &str) -> Option<Value> {
        let e = self.emitter(id)?;
        let int = |v: i32| Some(Value::Int(v));
        let float = |v: f32| Some(Value::Float(f64::from(v)));
        let flag = |v: bool| Some(Value::Bool(v));
        match name {
            "Type" => Some(Value::from("particle-emitter")),
            "X" => int(e.x),
            "Y" => int(e.y),
            "Width" => int(e.width),
            "Height" => int(e.height),
            "Scale1" => float(e.scale1),
            "Scale2" => float(e.scale2),
            "ScaleZBased" => flag(e.scale_z_based),
            "Velocity1" => float(e.velocity1),
            "Velocity2" => float(e.velocity2),
            "VelocityZBased" => flag(e.velocity_z_based),
            "LifeTime1" => int(e.life_time1),
            "LifeTime2" => int(e.life_time2),
            "LifeTimeZBased" => flag(e.life_time_z_based),
            "Angle1" => int(e.angle1),
            "Angle2" => int(e.angle2),
            "AngVelocity1" => float(e.ang_velocity1),
            "AngVelocity2" => float(e.ang_velocity2),
            "Rotation1" => float(e.rotation1),
            "Rotation2" => float(e.rotation2),
            "Alpha1" => int(e.alpha1),
            "Alpha2" => int(e.alpha2),
            "AlphaTimeBased" => flag(e.alpha_time_based),
            "MaxParticles" => int(e.max_particles),
            "NumLiveParticles" => int(e.num_live() as i32),
            "GenerationInterval" => int(e.gen_interval),
            "GenerationAmount" => int(e.gen_amount),
            "MaxBatches" => int(e.max_batches),
            "FadeInTime" => int(e.fade_in_time),
            "FadeOutTime" => int(e.fade_out_time),
            "GrowthRate1" => float(e.growth_rate1),
            "GrowthRate2" => float(e.growth_rate2),
            "ExponentialGrowth" => flag(e.exponential_growth),
            "UseRegion" => flag(e.use_region),
            "BlendMode" => int(e.blend_mode.to_int()),
            "EmitEvent" => Some(e.emit_event.clone().map_or(Value::Null, Value::from)),
            _ => None,
        }
    }

    /// Sets a property of a particle emitter; `false` when it has no
    /// such property.
    pub(crate) fn emitter_set(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
        let (int, flag, float) = (
            value.get_int(self),
            value.get_bool(self),
            value.get_float(self) as f32,
        );
        let text = if value.is_null() {
            None
        } else {
            Some(value.get_string(self))
        };
        let Some(e) = self.emitter_mut(id) else {
            return false;
        };
        match name {
            "X" => e.x = int,
            "Y" => e.y = int,
            "Width" => e.width = int,
            "Height" => e.height = int,
            "Scale1" => e.scale1 = float,
            "Scale2" => e.scale2 = float,
            "ScaleZBased" => e.scale_z_based = flag,
            "Velocity1" => e.velocity1 = float,
            "Velocity2" => e.velocity2 = float,
            "VelocityZBased" => e.velocity_z_based = flag,
            "LifeTime1" => e.life_time1 = int,
            "LifeTime2" => e.life_time2 = int,
            "LifeTimeZBased" => e.life_time_z_based = flag,
            "Angle1" => e.angle1 = int,
            "Angle2" => e.angle2 = int,
            "AngVelocity1" => e.ang_velocity1 = float,
            "AngVelocity2" => e.ang_velocity2 = float,
            "Rotation1" => e.rotation1 = float,
            "Rotation2" => e.rotation2 = float,
            "Alpha1" => e.alpha1 = int.clamp(0, 255),
            "Alpha2" => e.alpha2 = int.clamp(0, 255),
            "AlphaTimeBased" => e.alpha_time_based = flag,
            "MaxParticles" => e.max_particles = int,
            "GenerationInterval" => e.gen_interval = int,
            "GenerationAmount" => e.gen_amount = int,
            "MaxBatches" => e.max_batches = int,
            "FadeInTime" => e.fade_in_time = int,
            "FadeOutTime" => e.fade_out_time = int,
            "GrowthRate1" => e.growth_rate1 = float,
            "GrowthRate2" => e.growth_rate2 = float,
            "ExponentialGrowth" => e.exponential_growth = flag,
            "UseRegion" => e.use_region = flag,
            "BlendMode" => e.blend_mode = BlendMode::from_int(int),
            "EmitEvent" => e.emit_event = text,
            _ => return false,
        }
        true
    }
}
