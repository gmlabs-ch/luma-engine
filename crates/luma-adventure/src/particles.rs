//! Particle emitters: a rectangle attached to an entity or an actor that
//! spawns sprites with a random position, speed, direction, scale,
//! rotation, opacity and lifetime, moves them under global and point
//! forces, fades them in and out and kills them at their border or at
//! the end of their life (`CreateParticleEmitter` on an object). The
//! update runs on the game timer, in batches at a generation interval,
//! and the particles are drawn right after their owner. An emitter and
//! its live particles are kept in saved games, as in the original
//! engine.
//!
//! Written with reference to `PartEmitter.cpp`, `PartParticle.cpp`,
//! `PartForce.cpp` and `utils.cpp` (`RandomInt`, `RandomFloat`,
//! `RandomAngle`, `NormalizeAngle`) of Wintermute Engine 1.x (MIT
//! License, Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.

use luma_archive::Vfs;
use luma_formats::scene::Region;
use luma_formats::{BlendMode, Rgba};
use luma_render::{Attributes, Quad};
use luma_script::NativeId;

use crate::scene_view::{
    AnimState, SpriteBank, Transform, drawn_subframes, key, place_transformed,
};

/// The game's random generator, for the scripts' `Random` and the
/// particle emitters: xorshift64*. A run is deterministic under a seed
/// ([`World::seed_random`](crate::World::seed_random)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng(u64);

impl Rng {
    /// A generator at a state; `0` is replaced by a fixed seed, since
    /// xorshift never leaves zero.
    pub fn new(state: u64) -> Rng {
        Rng(if state == 0 {
            0x2545_F491_4F6C_DD1D
        } else {
            state
        })
    }

    /// The state, to put back where it came from.
    pub fn state(self) -> u64 {
        self.0
    }

    /// The next 32 random bits.
    pub fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as u32
    }

    /// An integer in `[from, to]`, in either order (`CBUtils::
    /// RandomInt`), over the whole span however wide. (The original
    /// draws from the C library's `rand()`, 15 bits wide, so it never
    /// goes beyond `from + 32767`; Luma does not keep that limit.)
    pub fn int(&mut self, from: i32, to: i32) -> i32 {
        let (from, to) = if to < from { (to, from) } else { (from, to) };
        let span = (i64::from(to) - i64::from(from) + 1) as u64;
        // Two draws of 31 bits: enough for any span of integers.
        let draw = (u64::from(self.next_u32()) << 31) | u64::from(self.next_u32());
        (i64::from(from) + (draw % span) as i64) as i32
    }

    /// A float in `[from, to]` (`CBUtils::RandomFloat`): `from` plus a
    /// fraction of the distance to `to`, so `to < from` works too; both
    /// ends can be drawn.
    pub fn float(&mut self, from: f32, to: f32) -> f32 {
        // `next_u32` gives 31 bits.
        let unit = (f64::from(self.next_u32()) / f64::from(0x7FFF_FFFFu32)) as f32;
        from + (to - from) * unit
    }

    /// An angle in degrees between `from` and `to` going clockwise
    /// (`CBUtils::RandomAngle`): `to` is raised by whole turns until it
    /// is not below `from`, and the result is normalised to `[0, 360]`.
    pub fn angle(&mut self, from: f32, to: f32) -> f32 {
        let mut to = to;
        while to < from {
            to += 360.0;
        }
        normalize_angle(self.float(from, to))
    }
}

/// An angle brought into `[0, 360]` by whole turns
/// (`CBUtils::NormalizeAngle`; both bounds are kept, as in the
/// original).
pub fn normalize_angle(mut angle: f32) -> f32 {
    while angle > 360.0 {
        angle -= 360.0;
    }
    while angle < 0.0 {
        angle += 360.0;
    }
    angle
}

/// A rectangle as the engine keeps its borders: edges, right and bottom
/// exclusive; empty when the right edge is not past the left one or the
/// bottom not past the top (`IsRectEmpty`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Border {
    /// Left edge.
    pub left: i32,
    /// Top edge.
    pub top: i32,
    /// Right edge, exclusive.
    pub right: i32,
    /// Bottom edge, exclusive.
    pub bottom: i32,
}

impl Border {
    /// Whether the rectangle has no area (`IsRectEmpty`).
    pub fn is_empty(self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }

    /// Whether a point is inside (`PtInRect`: the right and bottom edges
    /// are out).
    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// What a force does to the particles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ForceKind {
    /// A push in a fixed direction whose strength falls with the
    /// distance to a point (`FORCE_POINT`). The original engine's script
    /// interface never creates one: its `AddPointForce` makes a global
    /// force (see [`Emitter::add_force`]).
    Point,
    /// A push in a fixed direction everywhere (`FORCE_GLOBAL`): wind,
    /// gravity.
    Global,
}

/// A named force acting on every particle of an emitter.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Force {
    /// The name scripts remove it by, compared without case.
    pub name: String,
    /// The point of a point force.
    pub x: f32,
    /// See `x`.
    pub y: f32,
    /// The push, in pixels per second per second, as a vector.
    pub dir_x: f32,
    /// See `dir_x`.
    pub dir_y: f32,
    /// What it does.
    pub kind: ForceKind,
}

/// Where a particle is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ParticleState {
    /// Living: moving, turning, growing, its opacity following its age.
    Normal,
    /// Appearing: its opacity rises to `alpha1` over the fade-in time.
    FadeIn,
    /// Disappearing: its opacity falls to zero over the fade-out time,
    /// then it is dead.
    FadeOut,
}

/// One particle of an emitter.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Particle {
    /// Growth of the scale, in percent points per second, or in percent
    /// of the scale per second when `exponential_growth`.
    pub growth_rate: f32,
    /// See `growth_rate`.
    pub exponential_growth: bool,
    /// Rotation in degrees.
    pub rotation: f32,
    /// Angular velocity in degrees per second.
    pub ang_velocity: f32,
    /// Opacity at birth (`alpha1`) and at death (`alpha2`), the same
    /// unless the emitter's alpha is time based.
    pub alpha1: i32,
    /// See `alpha1`.
    pub alpha2: i32,
    /// The rectangle the particle fades out on leaving; empty for none.
    pub border: Border,
    /// Position, in scene pixels.
    pub x: f32,
    /// See `x`.
    pub y: f32,
    /// Depth, `0` to `100`, drawn back to front when the emitter's
    /// ranges depend on it.
    pub z: f32,
    /// Velocity in pixels per second.
    pub vx: f32,
    /// See `vx`.
    pub vy: f32,
    /// Scale in percent.
    pub scale: f32,
    /// The sprite shown, by path, and its animation.
    pub sprite: Option<String>,
    /// The animation of `sprite`.
    pub anim: AnimState,
    /// When it was born, on the game timer.
    pub creation_time: u64,
    /// How long it lives, in milliseconds; `0` for ever.
    pub life_time: i32,
    /// Whether it is dead (its slot is reused by the next one born).
    pub is_dead: bool,
    /// Where it is in its life.
    pub state: ParticleState,
    /// When the current fade started.
    pub fade_start: u64,
    /// How long the current fade lasts.
    pub fade_time: i32,
    /// The opacity drawn, `0` to `255`.
    pub current_alpha: i32,
    /// The opacity the fade-out started from.
    pub fade_start_alpha: i32,
}

impl Default for Particle {
    fn default() -> Particle {
        Particle {
            growth_rate: 0.0,
            exponential_growth: false,
            rotation: 0.0,
            ang_velocity: 0.0,
            alpha1: 255,
            alpha2: 255,
            border: Border::default(),
            x: 0.0,
            y: 0.0,
            z: 0.0,
            vx: 0.0,
            vy: 0.0,
            scale: 100.0,
            sprite: None,
            anim: AnimState::new(""),
            creation_time: 0,
            life_time: 0,
            is_dead: true,
            state: ParticleState::Normal,
            fade_start: 0,
            fade_time: 0,
            current_alpha: 255,
            fade_start_alpha: 0,
        }
    }
}

/// The difference of two timer values as the original engine computes
/// it on unsigned 32-bit values compared with a signed one: a negative
/// difference wraps to a large number.
fn elapsed(now: u64, since: u64) -> u64 {
    now.wrapping_sub(since)
}

/// A signed duration as the unsigned number the original engine
/// compares it with.
fn as_unsigned(duration: i32) -> u64 {
    i64::from(duration) as u64
}

impl Particle {
    /// Starts the fade-in (`CPartParticle::FadeIn`).
    pub fn fade_in(&mut self, now: u64, fade_time: i32) {
        self.current_alpha = 0;
        self.fade_start = now;
        self.fade_time = fade_time;
        self.state = ParticleState::FadeIn;
    }

    /// Starts the fade-out from the current opacity
    /// (`CPartParticle::FadeOut`).
    pub fn fade_out(&mut self, now: u64, fade_time: i32) {
        self.fade_start_alpha = self.current_alpha;
        self.fade_start = now;
        self.fade_time = fade_time;
        self.state = ParticleState::FadeOut;
    }

    /// One step of the particle's life (`CPartParticle::Update`): the
    /// fades, the end of life and the border, then the opacity by age,
    /// the forces, the motion, the spin and the growth; a particle shrunk
    /// to nothing dies. `delta` is the frame's duration on the game
    /// timer, in milliseconds.
    pub fn update(&mut self, fade_out_time: i32, forces: &[Force], now: u64, delta: u64) {
        match self.state {
            ParticleState::FadeIn => {
                if elapsed(now, self.fade_start) >= as_unsigned(self.fade_time) {
                    self.state = ParticleState::Normal;
                    self.current_alpha = self.alpha1;
                } else {
                    self.current_alpha = ((now as f32 - self.fade_start as f32)
                        / self.fade_time as f32
                        * self.alpha1 as f32) as i32;
                }
                return;
            }
            ParticleState::FadeOut => {
                if elapsed(now, self.fade_start) >= as_unsigned(self.fade_time) {
                    self.is_dead = true;
                } else {
                    self.current_alpha = (self.fade_start_alpha as f32
                        - (now as f32 - self.fade_start as f32) / self.fade_time as f32
                            * self.fade_start_alpha as f32)
                        as i32;
                }
                return;
            }
            ParticleState::Normal => {}
        }
        // Time is up.
        if self.life_time > 0 && elapsed(now, self.creation_time) >= as_unsigned(self.life_time) {
            if fade_out_time > 0 {
                self.fade_out(now, fade_out_time);
            } else {
                self.is_dead = true;
            }
        }
        // The particle left its border.
        if !self.is_dead
            && !self.border.is_empty()
            && !self.border.contains(self.x as i32, self.y as i32)
        {
            self.fade_out(now, fade_out_time);
        }
        if self.state != ParticleState::Normal {
            return;
        }
        if self.life_time > 0 {
            let age = elapsed(now, self.creation_time) as i32;
            let alpha_delta = self.alpha2 - self.alpha1;
            self.current_alpha =
                self.alpha1 + (alpha_delta as f32 / self.life_time as f32 * age as f32) as i32;
        }
        let seconds = delta as f32 / 1000.0;
        for force in forces {
            match force.kind {
                ForceKind::Global => {
                    self.vx += force.dir_x * seconds;
                    self.vy += force.dir_y * seconds;
                }
                ForceKind::Point => {
                    let (dx, dy) = (force.x - self.x, force.y - self.y);
                    let dist = 100.0 / (dx * dx + dy * dy).sqrt().abs();
                    self.vx += force.dir_x * dist * seconds;
                    self.vy += force.dir_y * dist * seconds;
                }
            }
        }
        self.x += self.vx * seconds;
        self.y += self.vy * seconds;
        self.rotation = normalize_angle(self.rotation + self.ang_velocity * seconds);
        if self.exponential_growth {
            self.scale += self.scale / 100.0 * self.growth_rate * seconds;
        } else {
            self.scale += self.growth_rate * seconds;
        }
        if self.scale <= 0.0 {
            self.is_dead = true;
        }
    }
}

/// A vector of a length pointing the way the engine turns an angle into
/// a direction: `(0, strength)` rotated by `angle - 180` degrees, so
/// that `0` points up the screen, `90` right, `180` down and `270` left.
fn direction(angle: f32, strength: f32) -> (f32, f32) {
    let radians = normalize_angle(angle - 180.0).to_radians();
    let (sin, cos) = radians.sin_cos();
    // Rotation of (0, strength): x' = -strength * sin, y' = strength * cos.
    (-strength * sin, strength * cos)
}

/// A particle emitter (`CPartEmitter`): its parameters as scripts set
/// them, its sprites, its forces and its particles.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Emitter {
    /// The object the emitter belongs to; the emit event is raised on
    /// it and it decides where the particles are drawn.
    pub owner: NativeId,
    /// Whether the emitter moves with its owner.
    pub follow_parent: bool,
    /// Offset from the owner's position when following, scaled by the
    /// owner's scale.
    pub offset_x: i32,
    /// See `offset_x`.
    pub offset_y: i32,
    /// Position of the emission rectangle (`X`, `Y`).
    pub x: i32,
    /// See `x`.
    pub y: i32,
    /// Size of the emission rectangle.
    pub width: i32,
    /// See `width`.
    pub height: i32,
    /// Direction range in degrees (`Angle1`, `Angle2`).
    pub angle1: i32,
    /// See `angle1`.
    pub angle2: i32,
    /// Initial rotation range in degrees.
    pub rotation1: f32,
    /// See `rotation1`.
    pub rotation2: f32,
    /// Angular velocity range in degrees per second.
    pub ang_velocity1: f32,
    /// See `ang_velocity1`.
    pub ang_velocity2: f32,
    /// Growth range, see [`Particle::growth_rate`].
    pub growth_rate1: f32,
    /// See `growth_rate1`.
    pub growth_rate2: f32,
    /// Whether the growth is a percentage of the scale.
    pub exponential_growth: bool,
    /// Speed range in pixels per second.
    pub velocity1: f32,
    /// See `velocity1`.
    pub velocity2: f32,
    /// Whether the speed follows the depth instead of being drawn at
    /// random.
    pub velocity_z_based: bool,
    /// Scale range in percent.
    pub scale1: f32,
    /// See `scale1`.
    pub scale2: f32,
    /// Whether the scale follows the depth.
    pub scale_z_based: bool,
    /// How many particles may live at once.
    pub max_particles: i32,
    /// Lifetime range in milliseconds.
    pub life_time1: i32,
    /// See `life_time1`.
    pub life_time2: i32,
    /// Whether the lifetime follows the depth (deeper particles live
    /// longer).
    pub life_time_z_based: bool,
    /// Milliseconds between two batches.
    pub gen_interval: i32,
    /// Particles born per batch.
    pub gen_amount: i32,
    /// Whether the emitter emits and its particles move.
    pub running: bool,
    /// The milliseconds `Start` was asked to run ahead; `0` once done.
    pub overhead_time: i32,
    /// Batches after which emission stops; `0` for no limit.
    pub max_batches: i32,
    /// Batches emitted since the last `Start`.
    pub batches_generated: i32,
    /// The rectangle particles fade out on leaving (`SetBorder`).
    pub border: Border,
    /// How much the border shrinks for the nearest particles
    /// (`SetBorderThickness`), per edge.
    pub border_thickness_left: i32,
    /// See `border_thickness_left`.
    pub border_thickness_right: i32,
    /// See `border_thickness_left`.
    pub border_thickness_top: i32,
    /// See `border_thickness_left`.
    pub border_thickness_bottom: i32,
    /// Fade-in duration of every particle.
    pub fade_in_time: i32,
    /// Fade-out duration at the end of life or at the border.
    pub fade_out_time: i32,
    /// Opacity range, `0` to `255`.
    pub alpha1: i32,
    /// See `alpha1`.
    pub alpha2: i32,
    /// Whether a particle's opacity goes from `alpha1` to `alpha2` over
    /// its life, instead of one random value.
    pub alpha_time_based: bool,
    /// Whether only the particles inside the owner's region are drawn.
    pub use_region: bool,
    /// How the particles are blended: the emitter's own `BlendMode`, a
    /// `CBObject` property of the emitter object (`CPartParticle::Display`
    /// passes `Emitter->m_BlendMode`).
    pub blend_mode: BlendMode,
    /// The event raised on the owner when a batch is born.
    pub emit_event: Option<String>,
    /// When the last batch was born.
    pub last_gen_time: u64,
    /// The particles, dead ones included.
    pub particles: Vec<Particle>,
    /// The sprite files particles are drawn with, one picked at random
    /// per particle.
    pub sprites: Vec<String>,
    /// The forces.
    pub forces: Vec<Force>,
    /// The sprites and their textures.
    pub bank: SpriteBank,
}

impl Emitter {
    /// An emitter with the original engine's defaults: a point, no
    /// speed, scale 100, a hundred particles living a second, one per
    /// batch, batches every frame, opaque, not running.
    pub fn new(owner: NativeId) -> Emitter {
        Emitter {
            owner,
            follow_parent: false,
            offset_x: 0,
            offset_y: 0,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            angle1: 0,
            angle2: 0,
            rotation1: 0.0,
            rotation2: 0.0,
            ang_velocity1: 0.0,
            ang_velocity2: 0.0,
            growth_rate1: 0.0,
            growth_rate2: 0.0,
            exponential_growth: false,
            velocity1: 0.0,
            velocity2: 0.0,
            velocity_z_based: false,
            scale1: 100.0,
            scale2: 100.0,
            scale_z_based: false,
            max_particles: 100,
            life_time1: 1000,
            life_time2: 1000,
            life_time_z_based: false,
            gen_interval: 0,
            gen_amount: 1,
            running: false,
            overhead_time: 0,
            max_batches: 0,
            batches_generated: 0,
            border: Border::default(),
            border_thickness_left: 0,
            border_thickness_right: 0,
            border_thickness_top: 0,
            border_thickness_bottom: 0,
            fade_in_time: 0,
            fade_out_time: 0,
            alpha1: 255,
            alpha2: 255,
            alpha_time_based: false,
            use_region: false,
            blend_mode: BlendMode::Normal,
            emit_event: None,
            last_gen_time: 0,
            particles: Vec::new(),
            sprites: Vec::new(),
            forces: Vec::new(),
            bank: SpriteBank::new(),
        }
    }

    /// Adds a sprite file (`AddSprite`); a file already added is kept
    /// once. Returns whether the file exists; a file that exists but
    /// does not load leaves its particles dead at birth, as in the
    /// original engine.
    pub fn add_sprite(&mut self, vfs: &Vfs, path: &str) -> bool {
        if self.sprites.iter().any(|s| s.eq_ignore_ascii_case(path)) {
            return true;
        }
        if !vfs.exists(path) {
            return false;
        }
        self.bank.add_sprite(vfs, path);
        self.sprites.push(path.to_string());
        true
    }

    /// Removes a sprite file (`RemoveSprite`); particles already born
    /// with it keep it. Returns whether it was there.
    pub fn remove_sprite(&mut self, path: &str) -> bool {
        match self
            .sprites
            .iter()
            .position(|s| s.eq_ignore_ascii_case(path))
        {
            Some(i) => {
                self.sprites.remove(i);
                true
            }
            None => false,
        }
    }

    /// Loads again the sprites the emitter and its particles name, after
    /// a saved game.
    pub fn reload_sprites(&mut self, vfs: &Vfs) {
        let mut paths = self.sprites.clone();
        paths.extend(self.particles.iter().filter_map(|p| p.sprite.clone()));
        for path in paths {
            self.bank.add_sprite(vfs, &path);
        }
    }

    /// Sets the border (`SetBorder`), from a position and a size.
    pub fn set_border(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.border = Border {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        };
    }

    /// Sets how much the border shrinks for the nearest particles
    /// (`SetBorderThickness`).
    pub fn set_border_thickness(&mut self, left: i32, right: i32, top: i32, bottom: i32) {
        self.border_thickness_left = left;
        self.border_thickness_right = right;
        self.border_thickness_top = top;
        self.border_thickness_bottom = bottom;
    }

    /// Adds a force, or redefines the one with this name (`AddForce`):
    /// `angle` in degrees (`0` up, `90` right), `strength` in pixels per
    /// second per second. The scripts' `AddGlobalForce` passes
    /// [`ForceKind::Global`]; so does their `AddPointForce` in the
    /// original engine 1.x (`CPartEmitter::ScCallMethod` passes
    /// `FORCE_GLOBAL` for both), which Luma reproduces, so that games
    /// tuned against it look the same: the point is kept but unused.
    pub fn add_force(
        &mut self,
        name: &str,
        kind: ForceKind,
        x: i32,
        y: i32,
        angle: f32,
        strength: f32,
    ) {
        let (dir_x, dir_y) = direction(angle, strength);
        let force = Force {
            name: name.to_string(),
            x: x as f32,
            y: y as f32,
            dir_x,
            dir_y,
            kind,
        };
        match self
            .forces
            .iter_mut()
            .find(|f| f.name.eq_ignore_ascii_case(name))
        {
            Some(existing) => *existing = force,
            None => self.forces.push(force),
        }
    }

    /// Removes a force by name (`RemoveForce`). Returns whether it was
    /// there.
    pub fn remove_force(&mut self, name: &str) -> bool {
        match self
            .forces
            .iter()
            .position(|f| f.name.eq_ignore_ascii_case(name))
        {
            Some(i) => {
                self.forces.remove(i);
                true
            }
            None => false,
        }
    }

    /// Particles alive (`NumLiveParticles`).
    pub fn num_live(&self) -> usize {
        self.particles.iter().filter(|p| !p.is_dead).count()
    }

    /// Starts emitting (`Start`): every particle dies, the batch count
    /// restarts, and when `overhead_time` is set the emitter runs that
    /// many milliseconds ahead in steps of half a second, so that a
    /// scene opens with its snow already falling; the emit event is not
    /// raised for those steps.
    pub fn start(&mut self, timer: u64, rng: &mut Rng) {
        for particle in &mut self.particles {
            particle.is_dead = true;
        }
        self.running = true;
        self.batches_generated = 0;
        if self.overhead_time > 0 {
            let delta = 500u64;
            let steps = self.overhead_time as u64 / delta;
            let mut now = timer.wrapping_sub(self.overhead_time as u64);
            for _ in 0..steps {
                self.update_internal(now, delta, rng);
                now = now.wrapping_add(delta);
            }
            self.overhead_time = 0;
        }
    }

    /// Stops emitting and removes every particle (`Stop`).
    pub fn stop(&mut self) {
        self.particles.clear();
        self.running = false;
    }

    /// Holds the emitter where it is (`Pause`).
    pub fn pause(&mut self) {
        self.running = false;
    }

    /// Goes on after a pause (`Resume`).
    pub fn resume(&mut self) {
        self.running = true;
    }

    /// One frame of the emitter (`CPartEmitter::Update`), when running:
    /// returns whether a batch was born and the emit event is due.
    pub fn update(&mut self, timer: u64, delta: u64, rng: &mut Rng) -> bool {
        if !self.running {
            return false;
        }
        self.update_internal(timer, delta, rng)
    }

    /// The frame itself (`UpdateInternal`): every particle advances;
    /// when fewer than `max_particles` live and the interval has passed,
    /// a batch of `gen_amount` is born in the dead slots (or new ones),
    /// up to the cap, unless the batch limit is reached; the particles
    /// are then sorted by depth when a range depends on it. Returns
    /// whether a batch was born outside a run ahead.
    fn update_internal(&mut self, now: u64, delta: u64, rng: &mut Rng) -> bool {
        let mut num_live = 0;
        let fade_out = self.fade_out_time;
        let forces = std::mem::take(&mut self.forces);
        for particle in &mut self.particles {
            particle.update(fade_out, &forces, now, delta);
            if !particle.is_dead {
                num_live += 1;
            }
        }
        self.forces = forces;
        let timer_now = now;
        for particle in self.particles.iter_mut().filter(|p| !p.is_dead) {
            if let Some(sprite) = particle
                .sprite
                .as_ref()
                .and_then(|path| self.bank.sprite(path))
            {
                particle.anim.step(&sprite.sprite, timer_now);
            }
        }
        if num_live >= self.max_particles {
            return false;
        }
        let mut generated = false;
        if elapsed(now, self.last_gen_time) > as_unsigned(self.gen_interval) {
            self.last_gen_time = now;
            self.batches_generated += 1;
            if self.max_batches > 0 && self.batches_generated > self.max_batches {
                return false;
            }
            let mut to_gen = self.gen_amount.min(self.max_particles - num_live);
            while to_gen > 0 {
                let index = match self.particles.iter().position(|p| p.is_dead) {
                    Some(i) => i,
                    None => {
                        self.particles.push(Particle::default());
                        self.particles.len() - 1
                    }
                };
                self.init_particle(index, now, rng);
                generated = true;
                to_gen -= 1;
            }
        }
        if generated && (self.scale_z_based || self.velocity_z_based || self.life_time_z_based) {
            self.particles
                .sort_by(|a, b| a.z.partial_cmp(&b.z).unwrap_or(std::cmp::Ordering::Equal));
        }
        generated && self.overhead_time == 0
    }

    /// Gives a particle its birth values (`InitParticle`), in the order
    /// the original engine draws them, so that a seed gives the same
    /// particles. A particle whose sprite did not load is born dead.
    fn init_particle(&mut self, index: usize, now: u64, rng: &mut Rng) {
        if self.sprites.is_empty() {
            return;
        }
        let pos_x = rng.int(self.x, self.x + self.width);
        let pos_y = rng.int(self.y, self.y + self.height);
        let pos_z = rng.float(0.0, 100.0);
        let velocity = if self.velocity_z_based {
            self.velocity1 + pos_z * (self.velocity2 - self.velocity1) / 100.0
        } else {
            rng.float(self.velocity1, self.velocity2)
        };
        let scale = if self.scale_z_based {
            self.scale1 + pos_z * (self.scale2 - self.scale1) / 100.0
        } else {
            rng.float(self.scale1, self.scale2)
        };
        let life_time = if self.life_time_z_based {
            (self.life_time2 as f32 - pos_z * (self.life_time2 - self.life_time1) as f32 / 100.0)
                as i32
        } else {
            rng.int(self.life_time1, self.life_time2)
        };
        let angle = rng.angle(self.angle1 as f32, self.angle2 as f32);
        let sprite_index = rng.int(0, self.sprites.len() as i32 - 1) as usize;
        let rotation = rng.angle(self.rotation1, self.rotation2);
        let ang_velocity = rng.float(self.ang_velocity1, self.ang_velocity2);
        let growth_rate = rng.float(self.growth_rate1, self.growth_rate2);
        let border = if self.border.is_empty() {
            Border::default()
        } else {
            // `int Thickness = T - (float)T * PosZ / 100.0f`: the whole
            // difference is truncated.
            let shrink =
                |thickness: i32| (thickness as f32 - thickness as f32 * pos_z / 100.0) as i32;
            Border {
                left: self.border.left + shrink(self.border_thickness_left),
                right: self.border.right - shrink(self.border_thickness_right),
                top: self.border.top + shrink(self.border_thickness_top),
                bottom: self.border.bottom - shrink(self.border_thickness_bottom),
            }
        };
        let (vx, vy) = direction(angle, velocity);
        let (alpha1, alpha2) = if self.alpha_time_based {
            (self.alpha1, self.alpha2)
        } else {
            let alpha = rng.int(self.alpha1, self.alpha2);
            (alpha, alpha)
        };
        let path = self.sprites[sprite_index].clone();
        let loaded = self.bank.sprite(&path).is_some();
        let particle = &mut self.particles[index];
        particle.border = border;
        particle.alpha1 = alpha1;
        particle.alpha2 = alpha2;
        particle.creation_time = now;
        particle.x = pos_x as f32;
        particle.y = pos_y as f32;
        particle.z = pos_z;
        particle.vx = vx;
        particle.vy = vy;
        particle.scale = scale;
        particle.life_time = life_time;
        particle.rotation = rotation;
        particle.ang_velocity = ang_velocity;
        particle.growth_rate = growth_rate;
        particle.exponential_growth = self.exponential_growth;
        particle.state = ParticleState::Normal;
        // The same sprite is kept and rewound (`CBSprite::Reset`), another
        // one is loaded afresh.
        if particle.sprite.as_deref() == Some(path.as_str()) {
            particle.anim.reset();
        } else {
            particle.anim = AnimState::new(&path);
            particle.sprite = Some(path);
        }
        particle.is_dead = !loaded;
        particle.fade_in(now, self.fade_in_time);
    }

    /// The quads drawing the live particles (`CPartEmitter::Display`,
    /// `CPartParticle::Display`), in the particles' order: each one its
    /// sprite's current frame at its position, scaled, turned by its
    /// rotation around its hotspot, with its opacity and the emitter's
    /// blend mode; only those inside `region` when the emitter uses the
    /// owner's region.
    pub fn quads<'a>(&'a self, region: Option<&Region>) -> Vec<(Quad<'a>, Attributes)> {
        let mut quads = Vec::new();
        for particle in self.particles.iter().filter(|p| !p.is_dead) {
            if self.use_region
                && let Some(region) = region
                && !region.contains(particle.x as i32, particle.y as i32)
            {
                continue;
            }
            let Some(loaded) = particle
                .sprite
                .as_ref()
                .and_then(|path| self.bank.sprite(path))
            else {
                continue;
            };
            if loaded.sprite.frames.is_empty() {
                continue;
            }
            let frame = particle
                .anim
                .current_frame
                .min(loaded.sprite.frames.len() - 1);
            let tint = Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: particle.current_alpha.clamp(0, 255) as u8,
            };
            for sub in drawn_subframes(&loaded.sprite.frames[frame]) {
                let Some(image) = &sub.image else { continue };
                let Some(texture) = self.bank.texture(image, sub.color_key) else {
                    continue;
                };
                let (quad, attributes) = place_transformed(
                    sub,
                    texture,
                    particle.x as i32,
                    particle.y as i32,
                    Transform {
                        scale_x: particle.scale,
                        scale_y: particle.scale,
                        rotation: particle.rotation,
                    },
                    tint,
                );
                quads.push((
                    quad,
                    Attributes {
                        blend: self.blend_mode,
                        ..attributes
                    },
                ));
            }
        }
        quads
    }

    /// Whether a sprite path is one of the emitter's, by key.
    pub fn has_sprite(&self, path: &str) -> bool {
        let k = key(path);
        self.sprites.iter().any(|s| key(s) == k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_cover_their_whole_span() {
        let mut rng = Rng::new(1);
        let (mut low, mut high) = (i32::MAX, i32::MIN);
        let (mut least, mut most) = (f32::MAX, f32::MIN);
        let (mut negative, mut positive) = (false, false);
        for _ in 0..20_000 {
            let value = rng.int(-3, 3);
            assert!((-3..=3).contains(&value));
            // A span wider than 32768 values is covered whole.
            let value = rng.int(0, 1_000_000);
            assert!((0..=1_000_000).contains(&value));
            low = low.min(value);
            high = high.max(value);
            let value = rng.float(2.0, -2.0);
            assert!((-2.0..=2.0).contains(&value));
            least = least.min(value);
            most = most.max(value);
            // The widest span there is does not overflow.
            let value = rng.int(i32::MAX, i32::MIN);
            negative |= value < 0;
            positive |= value > 0;
        }
        assert!(low < 1000 && high > 999_000, "{low}..{high}");
        assert!(least < -1.99 && most > 1.99, "{least}..{most}");
        assert!(negative && positive);
        assert_eq!(rng.int(5, 5), 5);
    }
    use luma_formats::Sprite;

    const OWNER: NativeId = NativeId(1);

    fn emitter_with_sprite() -> Emitter {
        let mut emitter = Emitter::new(OWNER);
        emitter
            .bank
            .insert("art\\dot.png", Sprite::from_image("art\\dot.png"));
        emitter.sprites.push("art\\dot.png".to_string());
        emitter
    }

    #[test]
    fn random_helpers_follow_the_engine() {
        let mut rng = Rng::new(42);
        for _ in 0..200 {
            let v = rng.int(5, 2);
            assert!((2..=5).contains(&v), "{v}");
            let f = rng.float(1.0, 3.0);
            assert!((1.0..=3.0).contains(&f), "{f}");
            let a = rng.angle(350.0, 10.0);
            assert!(a >= 350.0 || a <= 10.0, "{a}");
        }
        assert_eq!(normalize_angle(-90.0), 270.0);
        assert_eq!(normalize_angle(450.0), 90.0);
        assert_eq!(normalize_angle(360.0), 360.0);
        let (x, y) = direction(0.0, 10.0);
        assert!(
            x.abs() < 1e-4 && (y + 10.0).abs() < 1e-4,
            "0 is up: {x} {y}"
        );
        let (x, y) = direction(90.0, 10.0);
        assert!(
            (x - 10.0).abs() < 1e-4 && y.abs() < 1e-4,
            "90 is right: {x} {y}"
        );
        let (x, y) = direction(180.0, 10.0);
        assert!(
            x.abs() < 1e-4 && (y - 10.0).abs() < 1e-4,
            "180 is down: {x} {y}"
        );
        assert_eq!(Rng::new(0), Rng::new(0), "zero gets a fixed seed");
        assert_ne!(Rng::new(0).state(), 0);
    }

    #[test]
    fn spawns_within_its_ranges_and_respects_max_particles() {
        let mut emitter = emitter_with_sprite();
        emitter.x = 10;
        emitter.y = 20;
        emitter.width = 30;
        emitter.height = 40;
        emitter.scale1 = 50.0;
        emitter.scale2 = 150.0;
        emitter.velocity1 = 10.0;
        emitter.velocity2 = 20.0;
        emitter.angle1 = 170;
        emitter.angle2 = 190;
        emitter.life_time1 = 500;
        emitter.life_time2 = 1500;
        emitter.alpha1 = 100;
        emitter.alpha2 = 200;
        emitter.max_particles = 7;
        emitter.gen_amount = 3;
        let mut rng = Rng::new(7);
        emitter.start(1000, &mut rng);
        assert!(emitter.running);
        assert!(emitter.update(1000, 16, &mut rng), "a batch is born");
        assert_eq!(emitter.num_live(), 3);
        for p in emitter.particles.iter().filter(|p| !p.is_dead) {
            assert!((10.0..=40.0).contains(&p.x), "{}", p.x);
            assert!((20.0..=60.0).contains(&p.y), "{}", p.y);
            assert!((50.0..=150.0).contains(&p.scale), "{}", p.scale);
            let speed = (p.vx * p.vx + p.vy * p.vy).sqrt();
            assert!((9.99..=20.01).contains(&speed), "{speed}");
            assert!(p.vy > 0.0, "angles around 180 point down");
            assert!((500..=1500).contains(&p.life_time), "{}", p.life_time);
            assert!((100..=200).contains(&p.alpha1), "{}", p.alpha1);
            assert_eq!(p.alpha1, p.alpha2, "not time based: one value");
            assert_eq!(p.creation_time, 1000);
            assert_eq!(p.state, ParticleState::FadeIn, "a fade-in of 0 ms");
        }
        emitter.update(1016, 16, &mut rng);
        emitter.update(1032, 16, &mut rng);
        emitter.update(1048, 16, &mut rng);
        assert_eq!(emitter.num_live(), 7, "capped at MaxParticles");
        assert_eq!(emitter.particles.len(), 7, "no slot beyond the cap");
        assert!(
            emitter
                .particles
                .iter()
                .all(|p| p.state == ParticleState::Normal && p.current_alpha == p.alpha1),
            "the fade-in ended at once"
        );
    }

    #[test]
    fn particles_die_at_their_lifetime_and_slots_are_reused() {
        let mut emitter = emitter_with_sprite();
        emitter.life_time1 = 100;
        emitter.life_time2 = 100;
        emitter.max_particles = 2;
        emitter.gen_amount = 2;
        emitter.gen_interval = 1000;
        let mut rng = Rng::new(3);
        emitter.start(0, &mut rng);
        emitter.update(2000, 16, &mut rng);
        assert_eq!(emitter.num_live(), 2);
        emitter.update(2050, 50, &mut rng);
        assert_eq!(emitter.num_live(), 2, "still young");
        emitter.update(2100, 50, &mut rng);
        assert_eq!(emitter.num_live(), 0, "dead at 100 ms, no new batch yet");
        assert_eq!(emitter.particles.len(), 2);
        emitter.update(3100, 16, &mut rng);
        assert_eq!(emitter.num_live(), 2, "the next batch reuses the slots");
        assert_eq!(emitter.particles.len(), 2);
        assert_eq!(emitter.batches_generated, 2);
        // With a fade-out the particle lingers, fading. The frame that
        // ends the fade-in does nothing else, as in the original engine.
        emitter.fade_out_time = 100;
        emitter.update(3200, 100, &mut rng);
        assert!(
            emitter
                .particles
                .iter()
                .all(|p| p.state == ParticleState::Normal && p.current_alpha == 255)
        );
        emitter.update(3201, 1, &mut rng);
        assert_eq!(emitter.num_live(), 2);
        assert!(
            emitter
                .particles
                .iter()
                .all(|p| p.state == ParticleState::FadeOut)
        );
        emitter.update(3251, 50, &mut rng);
        assert!(
            emitter
                .particles
                .iter()
                .all(|p| !p.is_dead && p.current_alpha < 255 && p.current_alpha > 0),
            "{:?}",
            emitter
                .particles
                .iter()
                .map(|p| p.current_alpha)
                .collect::<Vec<_>>()
        );
        emitter.update(3301, 50, &mut rng);
        assert_eq!(emitter.num_live(), 0);
    }

    #[test]
    fn forces_move_the_particles() {
        let mut emitter = emitter_with_sprite();
        emitter.max_particles = 1;
        emitter.life_time1 = 0;
        emitter.life_time2 = 0;
        emitter.add_force("wind", ForceKind::Global, 0, 0, 90.0, 100.0);
        let mut rng = Rng::new(11);
        emitter.start(0, &mut rng);
        emitter.update(10, 16, &mut rng);
        let p = &emitter.particles[0];
        assert!(!p.is_dead && p.vx == 0.0 && p.vy == 0.0, "born still");
        emitter.update(11, 1, &mut rng);
        assert_eq!(emitter.particles[0].state, ParticleState::Normal);
        emitter.update(1011, 1000, &mut rng);
        let p = &emitter.particles[0];
        assert!((p.vx - 100.0).abs() < 1e-3, "a second of wind: {}", p.vx);
        assert!(p.vy.abs() < 1e-3);
        assert!((p.x - 100.0).abs() < 1e-3, "and moved by it: {}", p.x);
        assert!(emitter.remove_force("WIND"), "names ignore case");
        assert!(!emitter.remove_force("wind"));
        emitter.update(2011, 1000, &mut rng);
        assert!(
            (emitter.particles[0].x - 200.0).abs() < 1e-3,
            "it keeps its speed"
        );
        // A point force pushes in its direction, harder when near.
        let mut p = Particle {
            is_dead: false,
            x: 0.0,
            y: 0.0,
            ..Particle::default()
        };
        let pull = Force {
            name: "p".into(),
            x: 10.0,
            y: 0.0,
            dir_x: 0.0,
            dir_y: 1.0,
            kind: ForceKind::Point,
        };
        p.update(0, &[pull], 1000, 1000);
        assert!((p.vy - 10.0).abs() < 1e-3, "{}", p.vy);
        // Redefining a force keeps one entry.
        emitter.add_force("g", ForceKind::Global, 0, 0, 180.0, 10.0);
        emitter.add_force("G", ForceKind::Global, 0, 0, 180.0, 20.0);
        assert_eq!(emitter.forces.len(), 1);
        assert!((emitter.forces[0].dir_y - 20.0).abs() < 1e-3);
    }

    #[test]
    fn stop_start_pause_and_batches() {
        let mut emitter = emitter_with_sprite();
        emitter.max_particles = 5;
        emitter.gen_amount = 1;
        emitter.gen_interval = 100;
        emitter.max_batches = 2;
        emitter.life_time1 = 0;
        emitter.life_time2 = 0;
        let mut rng = Rng::new(5);
        assert!(!emitter.update(10, 16, &mut rng), "not started");
        emitter.start(0, &mut rng);
        assert!(emitter.update(200, 16, &mut rng));
        assert!(emitter.update(400, 16, &mut rng));
        assert!(!emitter.update(600, 16, &mut rng), "MaxBatches reached");
        assert_eq!(emitter.num_live(), 2);
        emitter.pause();
        assert!(!emitter.update(800, 16, &mut rng));
        emitter.resume();
        assert!(emitter.running);
        emitter.stop();
        assert!(!emitter.running);
        assert!(emitter.particles.is_empty());
        emitter.max_batches = 0;
        emitter.start(1000, &mut rng);
        assert_eq!(emitter.batches_generated, 0);
        assert!(emitter.update(2000, 16, &mut rng));
        assert_eq!(emitter.num_live(), 1);
        // Start marks the old particles dead and runs ahead when asked.
        emitter.overhead_time = 2000;
        emitter.start(10_000, &mut rng);
        assert_eq!(emitter.overhead_time, 0);
        assert_eq!(emitter.num_live(), 4, "four steps of 500 ms, one each");
        assert_eq!(emitter.particles[0].creation_time, 8000);
    }

    #[test]
    fn depth_based_ranges_and_the_border() {
        let mut emitter = emitter_with_sprite();
        emitter.x = 50;
        emitter.y = 50;
        emitter.max_particles = 50;
        emitter.gen_amount = 50;
        emitter.scale_z_based = true;
        emitter.scale1 = 10.0;
        emitter.scale2 = 110.0;
        emitter.velocity_z_based = true;
        emitter.velocity1 = 0.0;
        emitter.velocity2 = 100.0;
        emitter.life_time_z_based = true;
        emitter.life_time1 = 1000;
        emitter.life_time2 = 2000;
        emitter.set_border(0, 0, 100, 100);
        emitter.set_border_thickness(10, 10, 10, 10);
        emitter.alpha_time_based = true;
        emitter.alpha1 = 255;
        emitter.alpha2 = 55;
        let mut rng = Rng::new(9);
        emitter.start(0, &mut rng);
        emitter.update(100, 16, &mut rng);
        assert_eq!(emitter.num_live(), 50);
        let zs: Vec<f32> = emitter.particles.iter().map(|p| p.z).collect();
        assert!(zs.windows(2).all(|w| w[0] <= w[1]), "sorted by depth");
        for p in &emitter.particles {
            assert!((p.scale - (10.0 + p.z)).abs() < 1e-3);
            let speed = (p.vx * p.vx + p.vy * p.vy).sqrt();
            assert!((speed - p.z).abs() < 1e-2, "{speed} vs {}", p.z);
            assert_eq!(p.life_time, (2000.0 - p.z * 10.0) as i32);
            assert_eq!(p.alpha1, 255);
            assert_eq!(p.alpha2, 55);
            let shrink = (10.0 - 10.0 * p.z / 100.0) as i32;
            assert_eq!(p.border.left, shrink);
            assert_eq!(p.border.right, 100 - shrink);
        }
        // Opacity follows the age when time based (after the frame that
        // ends the fade-in).
        emitter.update(101, 1, &mut rng);
        emitter.update(600, 499, &mut rng);
        for p in emitter.particles.iter().filter(|p| !p.is_dead) {
            let expected = 255 + ((55.0 - 255.0) / p.life_time as f32 * 500.0) as i32;
            assert_eq!(p.current_alpha, expected);
        }
        // A particle outside its border fades out (and dies at once
        // without a fade-out time).
        let mut p = Particle {
            is_dead: false,
            x: 500.0,
            y: 0.0,
            border: Border {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            },
            ..Particle::default()
        };
        p.update(0, &[], 10, 10);
        assert_eq!(p.state, ParticleState::FadeOut);
        p.update(0, &[], 20, 10);
        assert!(p.is_dead);
    }

    #[test]
    fn growth_and_shrinking_to_nothing() {
        let mut p = Particle {
            is_dead: false,
            scale: 100.0,
            growth_rate: 50.0,
            ..Particle::default()
        };
        p.update(0, &[], 1000, 1000);
        assert!((p.scale - 150.0).abs() < 1e-3);
        p.exponential_growth = true;
        p.update(0, &[], 2000, 1000);
        assert!((p.scale - 225.0).abs() < 1e-3, "{}", p.scale);
        p.growth_rate = -1000.0;
        p.exponential_growth = false;
        p.update(0, &[], 3000, 1000);
        assert!(p.is_dead, "shrunk to nothing");
        let mut spin = Particle {
            is_dead: false,
            rotation: 350.0,
            ang_velocity: 20.0,
            ..Particle::default()
        };
        spin.update(0, &[], 1000, 1000);
        assert!((spin.rotation - 10.0).abs() < 1e-3, "{}", spin.rotation);
    }

    #[test]
    fn sprites_and_determinism() {
        let mut a = emitter_with_sprite();
        let mut b = emitter_with_sprite();
        a.bank
            .insert("art\\star.png", Sprite::from_image("art\\star.png"));
        a.sprites.push("art\\star.png".to_string());
        assert!(a.has_sprite("ART/STAR.PNG"));
        assert!(a.remove_sprite("art\\STAR.png"));
        assert!(!a.remove_sprite("art\\star.png"));
        a.max_particles = 20;
        a.gen_amount = 20;
        b.max_particles = 20;
        b.gen_amount = 20;
        a.start(0, &mut Rng::new(123));
        b.start(0, &mut Rng::new(123));
        a.update(50, 16, &mut Rng::new(123));
        b.update(50, 16, &mut Rng::new(123));
        assert_eq!(
            a.particles, b.particles,
            "the same seed gives the same particles"
        );
        let mut c = emitter_with_sprite();
        c.max_particles = 20;
        c.gen_amount = 20;
        c.start(0, &mut Rng::new(124));
        c.update(50, 16, &mut Rng::new(124));
        assert_ne!(a.particles, c.particles);
        // Without a sprite the slot is made but stays dead (the original
        // engine still counts the batch and raises the emit event); a
        // sprite that did not load gives dead particles too.
        let mut none = Emitter::new(OWNER);
        none.start(0, &mut Rng::new(1));
        assert!(none.update(100, 16, &mut Rng::new(1)));
        assert_eq!(none.particles.len(), 1);
        assert_eq!(none.num_live(), 0);
        let mut broken = Emitter::new(OWNER);
        broken.sprites.push("missing.sprite".to_string());
        broken.start(0, &mut Rng::new(1));
        broken.update(100, 16, &mut Rng::new(1));
        assert_eq!(broken.particles.len(), 1);
        assert!(broken.particles[0].is_dead);
        assert_eq!(broken.num_live(), 0);
        assert!(broken.quads(None).is_empty());
    }
}
