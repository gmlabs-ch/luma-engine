//! Actors: characters that walk, turn and play animations, driven once
//! per frame (see `docs/formats/actor.md`).
//!
//! Written with reference to `AdActor.cpp`, `AdObject.cpp`,
//! `AdTalkHolder.cpp` and `BObject.cpp` of WME Lite (MIT License,
//! Copyright (c) 2011 Jan Nedoma); see `NOTICE`.

use std::collections::HashMap;

use luma_archive::Vfs;
use luma_formats::scene::{Direction, Entity, Point, SceneNode};
use luma_formats::{Rgba, Scene};

use crate::error::{Error, Result};
use crate::path::Floor;
use crate::scene_view::{AnimState, Drawable, SpriteBank, frame_height, key};

/// What an actor is doing (`TObjectState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorState {
    /// Standing; `Ready` for the scripts waiting for it.
    Ready,
    /// Playing a sprite file (`PlayAnim`).
    PlayingAnim,
    /// Playing a named animation set (`PlayAnim`).
    PlayingAnimSet,
    /// `GoTo` asked for a path.
    SearchingPath,
    /// The path is being computed.
    WaitingPath,
    /// Walking the path.
    FollowingPath,
    /// Turning counter-clockwise towards the target direction.
    TurningLeft,
    /// Turning clockwise.
    TurningRight,
}

/// An event an update raises, applied by the host after the update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorEvent {
    /// An event on the actor itself (`ActorIsBlocked`).
    OnActor(&'static str),
    /// An event on a region of the main layer (`ActorEntry`,
    /// `ActorLeave`).
    OnRegion {
        /// The main layer's index.
        layer: usize,
        /// The region node's index.
        index: usize,
        /// The event's name.
        event: &'static str,
    },
}

/// What an update needs from the world: the floor seen by this actor,
/// and the game timer.
pub struct WalkContext<'a> {
    /// The scene, the other objects' polygons and the waypoints.
    pub floor: Floor<'a>,
    /// The game timer, in milliseconds.
    pub timer: u64,
}

/// A loaded actor and its state.
#[derive(Debug)]
pub struct Actor {
    /// The definition, which scripts modify (name, caption, flags).
    pub def: luma_formats::Actor,
    /// Path the actor was loaded from.
    pub filename: String,
    /// Whether the actor goes with the scene (`Scene.LoadActor`).
    pub scene_bound: bool,
    /// Position of the hotspot.
    pub x: i32,
    /// See `x`.
    pub y: i32,
    /// The direction faced.
    pub dir: Direction,
    /// What the actor is doing.
    pub state: ActorState,
    /// The state to return to when the current one ends.
    pub next_state: ActorState,
    /// The actor's sprites and textures.
    pub bank: SpriteBank,
    /// Whether the last walk stopped on a block (`ActorIsBlocked`).
    pub actor_is_blocked: bool,
    /// Stop on a block instead of looking for another path.
    pub stop_on_blocked: bool,
    /// Name of the animation set used for talking.
    pub talk_anim: String,
    /// Name of the animation set used when standing without `STAND`.
    pub idle_anim: String,
    /// Name of the animation set used when walking without `WALK`.
    pub walk_anim: String,
    /// Name of the animation set used when turning left.
    pub turn_left_anim: String,
    /// Name of the animation set used when turning right.
    pub turn_right_anim: String,
    /// Whether no walk, turn or animation is in progress (`Ready`).
    pub ready: bool,
    anims: HashMap<String, AnimState>,
    /// The sprite set by `SetSprite`, shown instead of `STAND`.
    sprite_override: Option<String>,
    /// The sprite file played by `PlayAnim`.
    anim_sprite: Option<String>,
    /// The sprite of the animation set played by `PlayAnim`.
    anim_sprite2: Option<String>,
    /// The turn sprite being shown.
    temp_sprite2: Option<String>,
    /// The sprite shown this frame.
    current_sprite: Option<String>,
    target: Point,
    target_dir: Direction,
    after_walk_dir: Option<Direction>,
    path: Vec<Point>,
    path_index: usize,
    path_ready: bool,
    pf_x: f64,
    pf_y: f64,
    pf_step_x: f64,
    pf_step_y: f64,
    pf_count: i32,
    current_regions: Vec<(usize, usize)>,
}

const TEMPLATE_DEPTH: usize = 8;

fn load_definition(
    def: &mut luma_formats::Actor,
    vfs: &Vfs,
    path: &str,
    depth: usize,
) -> Result<()> {
    let bytes = vfs.read(path)?;
    let format = |source| Error::Format {
        path: path.to_string(),
        source,
    };
    let mut probe = luma_formats::Actor::default();
    probe.merge(&bytes).map_err(format)?;
    if let Some(template) = probe.template {
        if depth < TEMPLATE_DEPTH && !template.eq_ignore_ascii_case(path) {
            load_definition(def, vfs, &template, depth + 1)?;
        }
    }
    def.merge(&bytes).map_err(format)
}

impl Actor {
    /// Loads an actor file (with its `TEMPLATE` chain) and the sprites
    /// it names. Missing sprites are recorded in the bank, not errors.
    pub fn load(vfs: &Vfs, path: &str, scene_bound: bool) -> Result<Actor> {
        let mut def = luma_formats::Actor::default();
        load_definition(&mut def, vfs, path, 0)?;
        let mut bank = SpriteBank::new();
        for sprite in def.sprite_paths() {
            bank.add_sprite(vfs, sprite);
        }
        Ok(Actor::from_definition(def, path, scene_bound, bank))
    }

    /// An actor from a parsed definition and its sprites.
    pub fn from_definition(
        def: luma_formats::Actor,
        path: &str,
        scene_bound: bool,
        bank: SpriteBank,
    ) -> Actor {
        Actor {
            x: def.x,
            y: def.y,
            stop_on_blocked: def.stop_on_blocked,
            def,
            filename: path.to_string(),
            scene_bound,
            dir: Direction::Left,
            state: ActorState::Ready,
            next_state: ActorState::Ready,
            bank,
            actor_is_blocked: false,
            talk_anim: "talk".to_string(),
            idle_anim: "idle".to_string(),
            walk_anim: "walk".to_string(),
            turn_left_anim: "turnleft".to_string(),
            turn_right_anim: "turnright".to_string(),
            ready: true,
            anims: HashMap::new(),
            sprite_override: None,
            anim_sprite: None,
            anim_sprite2: None,
            temp_sprite2: None,
            current_sprite: None,
            target: Point::default(),
            target_dir: Direction::Left,
            after_walk_dir: None,
            path: Vec::new(),
            path_index: 0,
            path_ready: false,
            pf_x: 0.0,
            pf_y: 0.0,
            pf_step_x: 0.0,
            pf_step_y: 0.0,
            pf_count: 0,
            current_regions: Vec::new(),
        }
    }

    /// The actor's position.
    pub fn position(&self) -> Point {
        Point {
            x: self.x,
            y: self.y,
        }
    }

    /// Whether the actor is following a path (`IsWalking`).
    pub fn is_walking(&self) -> bool {
        self.state == ActorState::FollowingPath
    }

    /// The sprite shown this frame, if any.
    pub fn current_sprite(&self) -> Option<&str> {
        self.current_sprite.as_deref()
    }

    /// The sprite set by `SetSprite`.
    pub fn sprite_override(&self) -> Option<&str> {
        self.sprite_override.as_deref()
    }

    /// Replaces the standing sprite (`SetSprite`); `None` restores the
    /// `STAND` set. Returns whether the sprite loaded.
    pub fn set_sprite(&mut self, vfs: &Vfs, path: Option<&str>) -> bool {
        let shown = self.current_sprite.is_some() && self.current_sprite == self.sprite_override;
        match path {
            None => {
                self.sprite_override = None;
                if shown {
                    self.current_sprite = None;
                }
                true
            }
            Some(path) => {
                if !self.bank.add_sprite(vfs, path) {
                    return false;
                }
                self.anims.insert(key(path), AnimState::new(path));
                self.sprite_override = Some(path.to_string());
                if shown {
                    self.current_sprite = Some(path.to_string());
                }
                true
            }
        }
    }

    /// The animation state of a sprite path, created at its first frame.
    pub fn anim(&mut self, path: &str) -> &mut AnimState {
        self.anims
            .entry(key(path))
            .or_insert_with(|| AnimState::new(path))
    }

    /// The frame a sprite path is at.
    pub fn frame_of(&self, path: &str) -> usize {
        self.anims.get(&key(path)).map_or(0, |a| a.current_frame)
    }

    fn is_finished(&self, path: &str) -> bool {
        self.anims.get(&key(path)).is_some_and(|a| a.finished)
    }

    /// Steps a sprite's animation to `timer`; `None` when the sprite is
    /// not loaded. Returns whether the frame changed, and the `MOVE` of
    /// the new frame.
    fn step(&mut self, path: &str, timer: u64) -> Option<(bool, i32, i32)> {
        let k = key(path);
        let loaded = self.bank.sprite(&k)?;
        let state = self.anims.entry(k).or_insert_with(|| AnimState::new(path));
        let changed = state.step(&loaded.sprite, timer);
        Some((changed, state.move_x, state.move_y))
    }

    fn set_sprite_of(set: Option<&luma_formats::SpriteSet>, dir: Direction) -> Option<String> {
        set.and_then(|s| s.sprite(dir)).map(str::to_string)
    }

    /// The sprite shown when standing: the `SetSprite` override, the
    /// `STAND` set, else the idle animation.
    fn stand_sprite(&self) -> Option<String> {
        if let Some(path) = &self.sprite_override {
            return Some(path.clone());
        }
        match &self.def.stand {
            Some(set) => Self::set_sprite_of(Some(set), self.dir),
            None => Self::set_sprite_of(self.def.animation(&self.idle_anim), self.dir),
        }
    }

    fn walk_sprite(&self) -> Option<String> {
        match &self.def.walk {
            Some(set) => Self::set_sprite_of(Some(set), self.dir),
            None => Self::set_sprite_of(self.def.animation(&self.walk_anim), self.dir),
        }
    }

    fn turn_sprite(&self, left: bool) -> Option<String> {
        let (set, anim) = if left {
            (&self.def.turn_left, &self.turn_left_anim)
        } else {
            (&self.def.turn_right, &self.turn_right_anim)
        };
        match set {
            Some(set) => Self::set_sprite_of(Some(set), self.dir),
            None => Self::set_sprite_of(self.def.animation(anim), self.dir),
        }
    }

    /// Starts a walk to a point (`CAdActor::GoTo`): the target is
    /// corrected to a walkable point and the path is searched on the
    /// next update. `after` is the direction to face on arrival.
    pub fn go_to(&mut self, x: i32, y: i32, after: Option<Direction>, floor: &Floor<'_>) {
        self.after_walk_dir = after;
        self.actor_is_blocked = false;
        if x == self.target.x && y == self.target.y && self.state == ActorState::FollowingPath {
            return;
        }
        self.path.clear();
        self.path_index = 0;
        self.path_ready = false;
        self.target = floor.correct_target(self.position(), Point { x, y });
        self.state = ActorState::SearchingPath;
    }

    /// Walks to an entity (`GoToObject`): to its `WALK_TO` point and
    /// direction, or to its position when the point is `0,0`.
    pub fn go_to_entity(&mut self, entity: &Entity, floor: &Floor<'_>) {
        if entity.walk_to_x == 0 && entity.walk_to_y == 0 {
            self.go_to(entity.x, entity.y, None, floor);
        } else {
            self.go_to(
                entity.walk_to_x,
                entity.walk_to_y,
                entity.walk_to_dir,
                floor,
            );
        }
    }

    /// Turns towards a direction by the shorter way (`CAdActor::TurnTo`).
    /// A difference of one step is not a turn.
    pub fn turn_to(&mut self, dir: Direction) {
        let d1 = i32::from(dir.code()) - i32::from(self.dir.code());
        let d2 = d1 + 8;
        let d3 = d1 - 8;
        let d1 = if d1.abs() <= d2.abs() { d1 } else { d2 };
        let delta = if d1.abs() <= d3.abs() { d1 } else { d3 };
        if delta.abs() < 2 {
            self.dir = dir;
            self.target_dir = dir;
            self.state = self.next_state;
            self.next_state = ActorState::Ready;
            self.temp_sprite2 = None;
            return;
        }
        self.target_dir = dir;
        self.state = if delta < 0 {
            ActorState::TurningLeft
        } else {
            ActorState::TurningRight
        };
        self.temp_sprite2 = None;
    }

    /// Turns towards a point (`TurnTo(object)`).
    pub fn turn_to_point(&mut self, x: i32, y: i32) {
        self.turn_to(Self::direction_to(self.x, self.y, x, y));
    }

    /// The direction from one point to another, as the engine computes
    /// it: `atan2` in degrees with its rounded π, truncated.
    #[allow(clippy::approx_constant, reason = "the engine's own constant")]
    fn direction_to(from_x: i32, from_y: i32, to_x: i32, to_y: i32) -> Direction {
        let angle = f64::from(to_y - from_y).atan2(f64::from(to_x - from_x)) * (180.0 / 3.14);
        Direction::from_angle(angle as i32)
    }

    /// Jumps to a point (`SkipTo`).
    pub fn skip_to(&mut self, x: i32, y: i32, scene: &Scene, events: &mut Vec<ActorEvent>) {
        self.x = x;
        self.y = y;
        self.after_move(scene, events);
    }

    /// Ends the current walk, turn or animation (`CAdObject::Reset`).
    /// The caller also ends the scripts waiting for the actor.
    pub fn reset(&mut self) {
        if self.state == ActorState::PlayingAnim {
            self.anim_sprite = None;
        }
        self.state = ActorState::Ready;
        self.next_state = ActorState::Ready;
    }

    /// Plays an animation (`PlayAnim`): the named set in the current
    /// direction, else the sprite file of that name. Returns whether it
    /// started.
    pub fn play_anim(&mut self, vfs: &Vfs, name: &str) -> bool {
        if let Some(path) = Self::set_sprite_of(self.def.animation(name), self.dir) {
            self.anim(&path).reset();
            self.anim_sprite2 = Some(path);
            self.state = ActorState::PlayingAnimSet;
            return true;
        }
        if !self.bank.add_sprite(vfs, name) {
            return false;
        }
        self.anims.insert(key(name), AnimState::new(name));
        self.anim_sprite = Some(name.to_string());
        self.state = ActorState::PlayingAnim;
        true
    }

    /// Adds the animations of a file (`MergeAnims`).
    pub fn merge_anims(&mut self, vfs: &Vfs, path: &str) -> bool {
        let Ok(bytes) = vfs.read(path) else {
            return false;
        };
        let before = self.def.animations.len();
        if self.def.merge_animations(&bytes).is_err() {
            return false;
        }
        let paths: Vec<String> = self.def.animations[before..]
            .iter()
            .flat_map(|a| a.paths().into_iter().map(str::to_string))
            .collect();
        for sprite in paths {
            self.bank.add_sprite(vfs, &sprite);
        }
        true
    }

    /// Removes the animations of a name (`UnloadAnim`); returns whether
    /// any was found.
    pub fn unload_anim(&mut self, name: &str) -> bool {
        let gone: Vec<luma_formats::SpriteSet> = self
            .def
            .animations
            .iter()
            .filter(|a| a.name.eq_ignore_ascii_case(name))
            .cloned()
            .collect();
        if gone.is_empty() {
            return false;
        }
        let in_gone = |path: &Option<String>| {
            path.as_deref().is_some_and(|p| {
                gone.iter()
                    .any(|a| a.paths().iter().any(|q| key(q) == key(p)))
            })
        };
        if in_gone(&self.temp_sprite2) {
            self.temp_sprite2 = None;
        }
        if in_gone(&self.current_sprite) {
            self.current_sprite = None;
        }
        if in_gone(&self.anim_sprite2) {
            self.anim_sprite2 = None;
        }
        self.def
            .animations
            .retain(|a| !a.name.eq_ignore_ascii_case(name));
        true
    }

    /// Forgets the regions the actor stood in (a scene change).
    pub fn invalidate_regions(&mut self) {
        self.current_regions.clear();
    }

    /// Raises `ActorEntry` and `ActorLeave` for the regions of the main
    /// layer the actor entered and left (`CAdObject::AfterMove`).
    fn after_move(&mut self, scene: &Scene, events: &mut Vec<ActorEvent>) {
        let new = crate::scene_view::regions_at(scene, self.x, self.y);
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

    /// One frame (`CAdActor::Update`): animations advance, turns and
    /// walks progress, and the sprite to show is chosen.
    pub fn update(&mut self, ctx: &WalkContext<'_>, events: &mut Vec<ActorEvent>) {
        self.current_sprite = None;
        if self.state == ActorState::Ready {
            self.anim_sprite = None;
            self.anim_sprite2 = None;
        }
        if self.state == ActorState::PlayingAnim
            && self
                .anim_sprite
                .as_deref()
                .is_some_and(|p| self.is_finished(p))
        {
            self.state = self.next_state;
            self.next_state = ActorState::Ready;
            self.current_sprite = self.anim_sprite.clone();
        }
        if self.state == ActorState::PlayingAnimSet
            && self
                .anim_sprite2
                .as_deref()
                .is_some_and(|p| self.is_finished(p))
        {
            self.state = self.next_state;
            self.next_state = ActorState::Ready;
            self.current_sprite = self.anim_sprite2.clone();
        }
        if self.current_sprite.is_none() {
            self.current_sprite = self.stand_sprite();
        }
        let mut already_moved = false;
        match self.state {
            ActorState::PlayingAnim => self.current_sprite = self.anim_sprite.clone(),
            ActorState::PlayingAnimSet => self.current_sprite = self.anim_sprite2.clone(),
            ActorState::TurningLeft => self.turn_step(true),
            ActorState::TurningRight => self.turn_step(false),
            ActorState::SearchingPath => {
                self.path = ctx.floor.find_path(self.position(), self.target);
                self.path_ready = true;
                self.state = ActorState::WaitingPath;
            }
            ActorState::WaitingPath => {
                if self.path_ready {
                    self.follow_path();
                }
            }
            ActorState::FollowingPath => {
                self.next_step(ctx, events);
                already_moved = true;
            }
            ActorState::Ready => {
                if self.anim_sprite.is_none() && self.anim_sprite2.is_none() {
                    self.current_sprite = self.stand_sprite();
                }
            }
        }
        if !already_moved {
            if let Some(path) = self.current_sprite.clone() {
                if let Some((true, move_x, move_y)) = self.step(&path, ctx.timer) {
                    self.x += move_x;
                    self.y += move_y;
                    self.after_move(ctx.floor.scene, events);
                }
            }
        }
        self.ready = self.state == ActorState::Ready;
    }

    fn turn_step(&mut self, left: bool) {
        let done = self
            .temp_sprite2
            .as_deref()
            .is_none_or(|p| self.is_finished(p));
        if !done {
            self.current_sprite = self.temp_sprite2.clone();
            return;
        }
        self.dir = if left {
            self.dir.left()
        } else {
            self.dir.right()
        };
        if self.dir == self.target_dir {
            self.temp_sprite2 = None;
            self.state = self.next_state;
            self.next_state = ActorState::Ready;
            return;
        }
        self.temp_sprite2 = self.turn_sprite(left);
        if let Some(path) = self.temp_sprite2.clone() {
            let state = self.anim(&path);
            state.reset();
            state.looping = Some(false);
        }
        self.current_sprite = self.temp_sprite2.clone();
    }

    /// Starts walking the path found (`CAdActor::FollowPath`).
    fn follow_path(&mut self) {
        self.path_index = 0;
        while self
            .path
            .get(self.path_index)
            .is_some_and(|p| *p == self.position())
        {
            self.path_index += 1;
        }
        match self.path.get(self.path_index).copied() {
            Some(next) => {
                self.state = ActorState::FollowingPath;
                self.init_line(self.position(), next);
            }
            None => match self.after_walk_dir {
                Some(dir) => self.turn_to(dir),
                None => self.state = ActorState::Ready,
            },
        }
    }

    /// Sets up the walk along one segment (`CAdActor::InitLine`) and
    /// turns towards it.
    fn init_line(&mut self, start: Point, end: Point) {
        self.pf_count = (end.x - start.x).abs().max((end.y - start.y).abs());
        if self.pf_count > 0 {
            self.pf_step_x = f64::from(end.x - start.x) / f64::from(self.pf_count);
            self.pf_step_y = f64::from(end.y - start.y) / f64::from(self.pf_count);
        } else {
            self.pf_step_x = 0.0;
            self.pf_step_y = 0.0;
        }
        self.pf_x = f64::from(start.x);
        self.pf_y = f64::from(start.y);
        self.next_state = ActorState::FollowingPath;
        self.turn_to(Self::direction_to(start.x, start.y, end.x, end.y));
    }

    /// One frame of walking (`CAdActor::GetNextStep`).
    fn next_step(&mut self, ctx: &WalkContext<'_>, events: &mut Vec<ActorEvent>) {
        self.current_sprite = self.walk_sprite();
        let Some(path) = self.current_sprite.clone() else {
            return;
        };
        let Some((true, move_x, move_y)) = self.step(&path, ctx.timer) else {
            return;
        };
        let mut max_step = move_x.abs().max(move_y.abs()).max(1);
        while self.pf_count > 0 && max_step >= 0 {
            self.pf_x += self.pf_step_x;
            self.pf_y += self.pf_step_y;
            self.pf_count -= 1;
            max_step -= 1;
        }
        if ctx
            .floor
            .is_blocked_at(self.pf_x as i32, self.pf_y as i32, false)
        {
            // At the final position already: not a block.
            if self.pf_count == 0 {
                self.state = self.next_state;
                self.next_state = ActorState::Ready;
                return;
            }
            events.push(ActorEvent::OnActor("ActorIsBlocked"));
            if self.stop_on_blocked {
                self.actor_is_blocked = true;
                self.state = self.next_state;
                self.next_state = ActorState::Ready;
                return;
            }
            let target = self.target;
            self.go_to(target.x, target.y, None, &ctx.floor);
            return;
        }
        self.x = self.pf_x as i32;
        self.y = self.pf_y as i32;
        self.after_move(ctx.floor.scene, events);
        if self.pf_count == 0 {
            self.path_index += 1;
            match self.path.get(self.path_index).copied() {
                None => {
                    self.x = self.target.x;
                    self.y = self.target.y;
                    self.path.clear();
                    self.path_index = 0;
                    match self.after_walk_dir {
                        Some(dir) => self.turn_to(dir),
                        None => {
                            self.state = self.next_state;
                            self.next_state = ActorState::Ready;
                        }
                    }
                }
                Some(next) => self.init_line(self.position(), next),
            }
        }
    }

    /// Scale in percent at the actor's position (`CAdObject::GetScale`).
    pub fn scale(&self, scene: &Scene) -> f32 {
        if !self.def.scalable {
            return 100.0;
        }
        self.def
            .scale
            .unwrap_or_else(|| scene.zoom_at(self.x, self.y) + self.def.relative_scale)
    }

    /// Tint at the actor's position.
    pub fn tint(&self, scene: &Scene) -> Rgba {
        match self.def.tint {
            Some(tint) => tint,
            None if self.def.colorable => scene.tint_at(self.x, self.y),
            None => Rgba::OPAQUE_WHITE,
        }
    }

    /// The actor's height (`CAdActor::GetHeight`): that of the current
    /// frame, or of the standing sprite before the first update.
    pub fn height(&self, scene: &Scene) -> i32 {
        let Some(path) = self.current_sprite.clone().or_else(|| self.stand_sprite()) else {
            return 0;
        };
        let Some(loaded) = self.bank.sprite(&path) else {
            return 0;
        };
        let zoom = if self.def.scalable {
            scene.zoom_at(self.x, self.y)
        } else {
            100.0
        };
        frame_height(&loaded.sprite, self.frame_of(&path), zoom)
    }

    /// The actor as drawn this frame, if it shows a sprite.
    pub fn drawable<'a>(&'a self, scene: &Scene) -> Option<Drawable<'a>> {
        if !self.def.active {
            return None;
        }
        let path = self.current_sprite.as_deref()?;
        let loaded = self.bank.sprite(path)?;
        Some(Drawable {
            x: self.x,
            y: self.y,
            sprite: Some(loaded),
            bank: &self.bank,
            frame: self.frame_of(path),
            scale: self.scale(scene),
            tint: self.tint(scene),
        })
    }

    /// The polygon the actor blocks for the others, in scene
    /// coordinates (`UpdateBlockRegion`): the file's polygon scaled by
    /// the scene's scale level at the actor's `y` and moved to its
    /// position.
    pub fn block_polygon(&self, scene: &Scene) -> Option<Vec<Point>> {
        let region = self.def.blocked_region.as_ref()?;
        Some(mimic(
            &region.points,
            self.mimic_scale(scene),
            self.x,
            self.y,
        ))
    }

    /// The actor's waypoints for the others, in scene coordinates.
    pub fn waypoints(&self, scene: &Scene) -> Vec<Point> {
        match &self.def.waypoints {
            Some(group) => mimic(&group.points, self.mimic_scale(scene), self.x, self.y),
            None => Vec::new(),
        }
    }

    fn mimic_scale(&self, scene: &Scene) -> f32 {
        if self.def.scalable {
            scene.scale_at(self.y)
        } else {
            100.0
        }
    }
}

/// Points relative to an object, scaled and moved to its position
/// (`CAdWaypointGroup::Mimic`, `CBRegion::Mimic`).
pub fn mimic(points: &[Point], scale: f32, x: i32, y: i32) -> Vec<Point> {
    points
        .iter()
        .map(|p| Point {
            x: (p.x as f32 * scale / 100.0) as i32 + x,
            y: (p.y as f32 * scale / 100.0) as i32 + y,
        })
        .collect()
}

/// The blocking polygon of an entity, in scene coordinates, when it is
/// active and has one.
pub fn entity_block_polygon(scene: &Scene, entity: &Entity) -> Option<Vec<Point>> {
    if !entity.active {
        return None;
    }
    let region = entity.blocked_region.as_ref()?;
    Some(mimic(
        &region.points,
        entity_mimic_scale(scene, entity),
        entity.x,
        entity.y,
    ))
}

/// The waypoints of an entity, in scene coordinates, when it is active
/// and has some.
pub fn entity_waypoints(scene: &Scene, entity: &Entity) -> Vec<Point> {
    match (&entity.waypoints, entity.active) {
        (Some(group), true) => mimic(
            &group.points,
            entity_mimic_scale(scene, entity),
            entity.x,
            entity.y,
        ),
        _ => Vec::new(),
    }
}

fn entity_mimic_scale(scene: &Scene, entity: &Entity) -> f32 {
    if entity.scalable {
        scene.scale_at(entity.y)
    } else {
        100.0
    }
}

/// Every entity of a scene with its polygon and waypoints, for the
/// floor the actors walk on.
pub fn scene_obstacles(scene: &Scene) -> (Vec<Vec<Point>>, Vec<Point>) {
    let mut blockers = Vec::new();
    let mut waypoints: Vec<Point> = scene
        .waypoint_groups
        .iter()
        .flat_map(|g| g.points.iter().copied())
        .collect();
    let entities = scene
        .layers
        .iter()
        .flat_map(|l| {
            l.nodes.iter().filter_map(|n| match n {
                SceneNode::Entity(e) => Some(&**e),
                SceneNode::Region(_) => None,
            })
        })
        .chain(scene.free_entities.iter());
    for entity in entities {
        if let Some(polygon) = entity_block_polygon(scene, entity) {
            blockers.push(polygon);
        }
        waypoints.extend(entity_waypoints(scene, entity));
    }
    (blockers, waypoints)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> Scene {
        Scene::parse(
            b"SCENE {
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=200 HEIGHT=100
    REGION { NAME=\"floor\" POINT {0,0} POINT {200,0} POINT {200,100} POINT {0,100} }
  }
}",
        )
        .unwrap()
    }

    fn actor() -> Actor {
        let def = luma_formats::Actor::parse(
            b"ACTOR { NAME=\"a\" X=10 Y=50 WALK { SPRITESET { LEFT=\"w.sprite\" } } }",
        )
        .unwrap();
        let mut bank = SpriteBank::new();
        // A one-frame looping sprite: a new frame every update.
        let sprite = luma_formats::Sprite::parse(
            b"SPRITE { LOOPING=TRUE FRAME { DELAY=10 IMAGE=\"w.png\" } }",
        )
        .unwrap();
        bank.insert("w.sprite", sprite);
        Actor::from_definition(def, "a.actor", false, bank)
    }

    #[test]
    fn walks_a_straight_line_and_turns_first() {
        let scene = room();
        let mut actor = actor();
        let floor = Floor {
            scene: &scene,
            blockers: &[],
            waypoints: &[],
        };
        actor.go_to(50, 50, Some(Direction::Up), &floor);
        assert_eq!(actor.state, ActorState::SearchingPath);
        let mut events = Vec::new();
        let mut timer = 1;
        let mut update = |actor: &mut Actor, events: &mut Vec<ActorEvent>| {
            let ctx = WalkContext {
                floor: Floor {
                    scene: &scene,
                    blockers: &[],
                    waypoints: &[],
                },
                timer,
            };
            timer += 20;
            actor.update(&ctx, events);
        };
        update(&mut actor, &mut events);
        assert_eq!(actor.state, ActorState::WaitingPath);
        update(&mut actor, &mut events);
        // Facing left, the walk goes right: a turn of four steps, one
        // per frame without turn sprites.
        assert_eq!(actor.state, ActorState::TurningLeft);
        for _ in 0..4 {
            update(&mut actor, &mut events);
        }
        assert_eq!(actor.dir, Direction::Right);
        assert_eq!(actor.state, ActorState::FollowingPath);
        assert!(actor.is_walking());
        assert!(!actor.ready);
        let mut steps = 0;
        while actor.is_walking() && steps < 100 {
            update(&mut actor, &mut events);
            steps += 1;
        }
        // 40 pixels, two per frame.
        assert_eq!((actor.x, actor.y), (50, 50));
        assert_eq!(steps, 20);
        // Then the turn to the after-walk direction.
        assert_eq!(actor.state, ActorState::TurningLeft);
        for _ in 0..2 {
            update(&mut actor, &mut events);
        }
        assert_eq!(actor.dir, Direction::Up);
        assert_eq!(actor.state, ActorState::Ready);
        assert!(actor.ready);
        assert_eq!(
            events,
            vec![ActorEvent::OnRegion {
                layer: 0,
                index: 0,
                event: "ActorEntry"
            }]
        );
    }

    #[test]
    fn turning_takes_the_short_way() {
        let mut actor = actor();
        actor.dir = Direction::Up;
        actor.turn_to(Direction::UpRight);
        assert_eq!(actor.dir, Direction::UpRight, "one step is immediate");
        assert_eq!(actor.state, ActorState::Ready);
        actor.turn_to(Direction::Left);
        assert_eq!(actor.state, ActorState::TurningLeft);
        actor.dir = Direction::Up;
        actor.turn_to(Direction::DownRight);
        assert_eq!(actor.state, ActorState::TurningRight);
        actor.dir = Direction::Up;
        actor.turn_to(Direction::Down);
        assert_eq!(
            actor.state,
            ActorState::TurningRight,
            "a half turn goes right"
        );
    }

    #[test]
    fn obstacles_follow_their_owner() {
        let scene = room();
        let mut actor = actor();
        actor.def.blocked_region = Some(luma_formats::Region {
            points: vec![
                Point { x: -10, y: -5 },
                Point { x: 10, y: -5 },
                Point { x: 10, y: 5 },
                Point { x: -10, y: 5 },
            ],
            ..luma_formats::Region::default()
        });
        actor.def.waypoints = Some(luma_formats::WaypointGroup {
            points: vec![Point { x: 0, y: -20 }],
            ..luma_formats::WaypointGroup::default()
        });
        let polygon = actor.block_polygon(&scene).unwrap();
        assert_eq!(polygon[0], Point { x: 0, y: 45 });
        assert_eq!(actor.waypoints(&scene), vec![Point { x: 10, y: 30 }]);
        actor.def.scalable = false;
        assert_eq!(
            actor.block_polygon(&scene).unwrap()[2],
            Point { x: 20, y: 55 }
        );
    }
}
