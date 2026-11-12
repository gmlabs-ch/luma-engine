//! Run-time animation and skinning of a model: the ten animation
//! channels with their active and transition animations, the posed frame
//! matrices and the meshes skinned on the CPU every update, as
//! `CXModel::Update` does for a 3D actor.
//!
//! [`AnimatedModel`] wraps a [`Model`] and drives it with a millisecond
//! timer: [`AnimatedModel::play`] and [`AnimatedModel::stop`] act on a
//! channel, [`AnimatedModel::update`] advances the timer, poses the frames
//! and skins the meshes, returning the animation events crossed. The
//! skinned vertices of every mesh stay in a contiguous [`Vec<Vertex>`] per
//! mesh, indexed like the model's, so a render pass can upload them as they
//! are and draw them with the model's triangles and material groups.
//!
//! The behaviour is specified in `docs/3d-actors.md`, section 7.
//!
//! Written with reference to the MIT-licensed Wintermute Engine 1.x
//! sources (`XModel.cpp`, `XFrameNode.cpp`, `XMesh.cpp`,
//! `XAnimationChannel.cpp`, `XActiveAnimation.cpp`, `XAnimationSet.cpp`,
//! `AdActor3DX.cpp`, Copyright (c) 2003-2013 Jan Nedoma). See `NOTICE` at
//! the root of the repository.

use crate::math::{Mat4, Vec3, intersect_triangle};
use crate::model::{Mesh, Model, Transform, Vertex};

/// The number of animation channels of a model (`X_NUM_ANIMATION_CHANNELS`).
/// Channel 0 carries the standard animations the actor logic selects; the
/// others are for scripts.
pub const NUM_CHANNELS: usize = 10;

/// The default time of a transition between two animations, in
/// milliseconds (the actor's `AnimTransitionTime`).
pub const DEFAULT_TRANSITION_MS: u32 = 200;

/// The default hold time after an animation finishes or is stopped, in
/// milliseconds (the actor's `AnimStopTransitionTime`).
pub const DEFAULT_STOP_TRANSITION_MS: u32 = 200;

/// An axis-aligned box.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BoundingBox {
    /// The smallest corner.
    pub min: Vec3,
    /// The largest corner.
    pub max: Vec3,
}

impl BoundingBox {
    /// The box around points (`D3DXComputeBoundingBox`); both corners at
    /// the origin when there is no point.
    pub fn of_points(points: impl IntoIterator<Item = Vec3>) -> BoundingBox {
        let mut points = points.into_iter();
        let Some(first) = points.next() else {
            return BoundingBox::default();
        };
        let mut bounds = BoundingBox {
            min: first,
            max: first,
        };
        for point in points {
            bounds.min = bounds.min.min(point);
            bounds.max = bounds.max.max(point);
        }
        bounds
    }

    /// Grows the box to contain another one.
    pub fn include(&mut self, other: &BoundingBox) {
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
    }

    /// The eight corners, in the order the original projects them for the
    /// 2D bounding rectangle: the `x` of `min` or `max`, then `y`, then
    /// `z`, every combination.
    pub fn corners(&self) -> [Vec3; 8] {
        let (a, b) = (self.min, self.max);
        [
            Vec3::new(a.x, a.y, a.z),
            Vec3::new(b.x, a.y, a.z),
            Vec3::new(a.x, b.y, a.z),
            Vec3::new(b.x, b.y, a.z),
            Vec3::new(a.x, a.y, b.z),
            Vec3::new(b.x, a.y, b.z),
            Vec3::new(a.x, b.y, b.z),
            Vec3::new(b.x, b.y, b.z),
        ]
    }
}

/// A mesh posed by the last update: its vertices in model space.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinnedMesh {
    /// The posed vertices, indexed like the model mesh's `vertices`, so
    /// the model's `triangles` and `triangle_materials` apply to them.
    pub vertices: Vec<Vertex>,
    /// The box around the posed positions.
    pub bounds: BoundingBox,
}

/// An animation playing on a channel (`CXActiveAnimation`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveAnimation {
    /// The set, as an index into the model's `animation_sets`.
    set: usize,
    /// The timer value the animation started at.
    start_time: u32,
    /// Whether the animation wraps at its end.
    looping: bool,
    /// Whether a non-looping animation reached its end.
    finished: bool,
    /// The local time of the last update, held while transitioning.
    last_local_time: u32,
    /// The frame index of the last update, -1 before the first.
    current_frame: i32,
}

impl ActiveAnimation {
    fn start(set: usize, looping: bool, timer: u32) -> ActiveAnimation {
        ActiveAnimation {
            set,
            start_time: timer,
            looping,
            finished: false,
            last_local_time: 0,
            current_frame: -1,
        }
    }

    /// The animation set, as an index into the model's `animation_sets`.
    pub fn set(&self) -> usize {
        self.set
    }

    /// Whether the animation wraps at its end.
    pub fn is_looping(&self) -> bool {
        self.looping
    }

    /// Whether a non-looping animation has reached its end.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// The local time of the last update, in milliseconds.
    pub fn local_time(&self) -> u32 {
        self.last_local_time
    }

    /// The frame index of the last update (1-based), -1 before the first.
    pub fn current_frame(&self) -> i32 {
        self.current_frame
    }

    /// `CXActiveAnimation::Update`: computes the local time, raises the
    /// events of a frame change and writes the pose into a slot of the
    /// frames the set animates. With `frozen`, the local time is the one
    /// of the last update and no event fires (the pose is held while a
    /// transition blends it).
    fn update(&mut self, tick: &mut Tick<'_>, slot: usize, frozen: bool, lerp: f32) {
        let model = tick.model;
        let Some(set) = model.animation_sets.get(self.set) else {
            return;
        };
        let ticks_per_second = model.ticks_per_second;
        let total = set.total_time_ms(ticks_per_second);

        let mut local = self.last_local_time;
        if !frozen && !self.finished {
            local = tick.timer.wrapping_sub(self.start_time);
            if local > total {
                if self.looping {
                    local = if total == 0 { 0 } else { local % total };
                } else {
                    self.finished = true;
                }
            }
        }
        if self.finished {
            local = total.wrapping_sub(1);
        }
        self.last_local_time = local;

        let frame_time = set.frame_time_ms(ticks_per_second);
        let frame = local
            .checked_div(frame_time)
            .map_or(0, |frames| frames.wrapping_add(1) as i32);
        if frame != self.current_frame {
            if !frozen {
                tick.events.extend(
                    set.events_between(frame, self.current_frame)
                        .into_iter()
                        .map(str::to_string),
                );
            }
            self.current_frame = frame;
        }

        for (frame_index, transform) in set.sample(model.ms_to_ticks(local)) {
            if let Some(frame_slots) = tick.slots.get_mut(frame_index) {
                frame_slots.set(slot, transform, lerp);
            }
        }
    }
}

/// What one update passes down to the channels: the model, the frame
/// slots to write, the timer and the events raised so far.
struct Tick<'a> {
    model: &'a Model,
    slots: &'a mut [FrameSlots],
    timer: u32,
    events: &'a mut Vec<String>,
}

/// A channel (`CXAnimationChannel`): up to two active animations, the
/// current one in slot 0 and, while transitioning, the incoming one in
/// slot 1.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Channel {
    anims: [Option<ActiveAnimation>; 2],
    transitioning: bool,
    transition_start: u32,
    transition_time: u32,
    stop_transition_time: u32,
}

impl Channel {
    /// `CXAnimationChannel::PlayAnim`.
    fn play(
        &mut self,
        anim: ActiveAnimation,
        transition_time: u32,
        stop_transition_time: u32,
        timer: u32,
    ) {
        self.stop_transition_time = stop_transition_time;
        if transition_time == 0 {
            self.anims = [Some(anim), None];
            self.transitioning = false;
        } else if self.anims[0].is_some() {
            self.anims[1] = Some(anim);
            self.transitioning = true;
            self.transition_time = transition_time;
            self.transition_start = timer;
        } else {
            self.anims = [Some(anim), None];
            self.transitioning = false;
        }
    }

    /// `CXAnimationChannel::StopAnim`.
    fn stop(&mut self, transition_time: u32, timer: u32) {
        if transition_time == 0 || self.anims[0].is_none() {
            self.transitioning = false;
            self.anims = [None, None];
        } else {
            self.anims[1] = None;
            if let Some(anim) = &mut self.anims[0] {
                anim.looping = false;
            }
            self.transitioning = true;
            self.transition_time = transition_time;
            self.transition_start = timer;
        }
    }

    /// `CXAnimationChannel::Update`.
    fn update(&mut self, tick: &mut Tick<'_>) {
        let timer = tick.timer;
        if self.transitioning {
            let elapsed = timer.wrapping_sub(self.transition_start);
            if elapsed >= self.transition_time {
                self.transitioning = false;
                self.anims[0] = self.anims[1].take();
                if let Some(anim) = &mut self.anims[0] {
                    // The time spent transitioning is not counted.
                    anim.start_time = timer;
                    anim.update(tick, 0, false, 0.0);
                }
            } else {
                let lerp = elapsed as f32 / self.transition_time as f32;
                if let Some(anim) = &mut self.anims[0] {
                    anim.update(tick, 0, true, lerp);
                }
                if let Some(anim) = &mut self.anims[1] {
                    anim.update(tick, 1, true, 0.0);
                }
            }
        } else if let Some(anim) = &mut self.anims[0] {
            anim.update(tick, 0, false, 0.0);
            if anim.finished {
                self.stop(self.stop_transition_time, timer);
            }
        }
    }

    /// `CXAnimationChannel::IsPlaying`: an unfinished animation in either
    /// slot.
    fn is_playing(&self) -> bool {
        self.anims.iter().flatten().any(|anim| !anim.finished)
    }

    /// The animation the channel is heading for: the incoming one during
    /// a transition, else the current one (`GetName`).
    fn leading(&self) -> Option<&ActiveAnimation> {
        self.anims[1].as_ref().or(self.anims[0].as_ref())
    }
}

/// The two transformation slots of a frame (`CXFrameNode::m_Trans*`):
/// slot 0 is the current pose, slot 1 the pose it blends toward.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct FrameSlots {
    used: [bool; 2],
    transforms: [Transform; 2],
    lerp: [f32; 2],
}

impl FrameSlots {
    fn set(&mut self, slot: usize, transform: Transform, lerp: f32) {
        self.used[slot] = true;
        self.transforms[slot] = transform;
        self.lerp[slot] = lerp;
    }

    /// `CXFrameNode::UpdateMatrices`: the local matrix the slots make,
    /// `None` when slot 0 is unused (the frame keeps its loaded matrix,
    /// whatever slot 1 holds). With both slots used the two transforms are
    /// blended by slot 0's factor.
    fn local_matrix(&self) -> Option<Mat4> {
        if !self.used[0] {
            return None;
        }
        let mut transform = self.transforms[0];
        if self.used[1] {
            transform = transform.blend(&self.transforms[1], self.lerp[0]);
        }
        Some(transform.to_matrix())
    }
}

/// A transition time set for a pair of animations
/// (`CBAnimTransitionTime`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct TransitionTime {
    from: String,
    to: String,
    time: u32,
}

/// An active animation as a saved game keeps it: the set by name, so
/// that the state survives a reload of the model (`CXActiveAnimation::
/// Persist` stores the set by pointer into the model's own list).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ActiveAnimationState {
    /// The animation set's name.
    pub set: String,
    /// The timer value the animation started at.
    pub start_time: u32,
    /// Whether it wraps at its end.
    pub looping: bool,
    /// Whether a non-looping animation has reached its end.
    pub finished: bool,
    /// The local time of the last update.
    pub last_local_time: u32,
    /// The frame index of the last update, -1 before the first.
    pub current_frame: i32,
}

/// A channel as a saved game keeps it (`CXAnimationChannel::Persist`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ChannelState {
    /// The current animation and, while transitioning, the incoming one.
    pub anims: [Option<ActiveAnimationState>; 2],
    /// Whether the channel blends the two.
    pub transitioning: bool,
    /// When the transition started, on the timer.
    pub transition_start: u32,
    /// How long it lasts.
    pub transition_time: u32,
    /// How long the last pose is held when the animation finishes.
    pub stop_transition_time: u32,
}

/// The animation state of a model, as a saved game keeps it: the timer,
/// the channels, the default transition times and the pair table
/// (`CXModel::Persist` and the actor's `Persist` together). Taken with
/// [`AnimatedModel::state`] and put back on a freshly loaded model with
/// [`AnimatedModel::restore`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AnimationState {
    /// The timer the deltas added up to.
    pub timer: u32,
    /// One entry per channel, 0 to 9.
    pub channels: Vec<ChannelState>,
    /// The default transition time, milliseconds.
    pub default_transition_ms: u32,
    /// The default stop transition time, milliseconds.
    pub default_stop_transition_ms: u32,
    /// The pair table: from, to, milliseconds.
    pub transition_times: Vec<(String, String, u32)>,
}

/// A model with its animation state: the channels, the posed frames and
/// the skinned meshes (`CXModel` at run time).
#[derive(Debug, Clone, PartialEq)]
pub struct AnimatedModel {
    model: Model,
    timer: u32,
    channels: Vec<Channel>,
    slots: Vec<FrameSlots>,
    locals: Vec<Mat4>,
    combined: Vec<Mat4>,
    meshes: Vec<SkinnedMesh>,
    bounds: BoundingBox,
    default_transition_ms: u32,
    default_stop_transition_ms: u32,
    transition_times: Vec<TransitionTime>,
}

impl AnimatedModel {
    /// Wraps a model, posed at rest: the timer is 0, no channel plays and
    /// the meshes are skinned once with the loaded frame matrices.
    pub fn new(model: Model) -> AnimatedModel {
        let frames = model.frames.len();
        let meshes = model
            .meshes
            .iter()
            .map(|mesh| SkinnedMesh {
                vertices: mesh.vertices.clone(),
                bounds: BoundingBox::default(),
            })
            .collect();
        let mut animated = AnimatedModel {
            model,
            timer: 0,
            channels: vec![Channel::default(); NUM_CHANNELS],
            slots: vec![FrameSlots::default(); frames],
            locals: vec![Mat4::IDENTITY; frames],
            combined: vec![Mat4::IDENTITY; frames],
            meshes,
            bounds: BoundingBox::default(),
            default_transition_ms: DEFAULT_TRANSITION_MS,
            default_stop_transition_ms: DEFAULT_STOP_TRANSITION_MS,
            transition_times: Vec::new(),
        };
        animated.pose();
        animated
    }

    /// The model.
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// The model, to merge animation sets into it or to set their looping
    /// flag and events. The frames and meshes must stay as they are, and a
    /// set a channel may be playing is removed with
    /// [`Self::remove_animation_set`], not through this.
    pub fn model_mut(&mut self) -> &mut Model {
        &mut self.model
    }

    /// Takes the model back.
    pub fn into_model(self) -> Model {
        self.model
    }

    /// The timer, in milliseconds: the sum of the deltas passed to
    /// [`Self::update`].
    pub fn timer(&self) -> u32 {
        self.timer
    }

    /// The default transition time (`AnimTransitionTime`), used by
    /// [`Self::play`] for a pair without its own.
    pub fn default_transition_ms(&self) -> u32 {
        self.default_transition_ms
    }

    /// Sets the default transition time.
    pub fn set_default_transition_ms(&mut self, ms: u32) {
        self.default_transition_ms = ms;
    }

    /// The default hold time after an animation finishes or is stopped
    /// (`AnimStopTransitionTime`), used by [`Self::play`].
    pub fn default_stop_transition_ms(&self) -> u32 {
        self.default_stop_transition_ms
    }

    /// Sets the default stop transition time.
    pub fn set_default_stop_transition_ms(&mut self, ms: u32) {
        self.default_stop_transition_ms = ms;
    }

    /// Sets the transition time from one animation to another, by name
    /// (`SetAnimTransitionTime`, names compared ignoring ASCII case);
    /// `None` removes the pair.
    pub fn set_transition_time(&mut self, from: &str, to: &str, ms: Option<u32>) {
        let found = self
            .transition_times
            .iter()
            .position(|t| t.from.eq_ignore_ascii_case(from) && t.to.eq_ignore_ascii_case(to));
        match (found, ms) {
            (Some(index), Some(time)) => self.transition_times[index].time = time,
            (Some(index), None) => {
                self.transition_times.remove(index);
            }
            (None, Some(time)) => self.transition_times.push(TransitionTime {
                from: from.to_string(),
                to: to.to_string(),
                time,
            }),
            (None, None) => {}
        }
    }

    /// The transition time from one animation to another
    /// (`GetAnimTransitionTime`): the pair's, else the default.
    pub fn transition_time(&self, from: &str, to: &str) -> u32 {
        self.transition_times
            .iter()
            .find(|t| t.from.eq_ignore_ascii_case(from) && t.to.eq_ignore_ascii_case(to))
            .map_or(self.default_transition_ms, |t| t.time)
    }

    /// The transition times set for pairs, as `(from, to, ms)`.
    pub fn transition_times(&self) -> impl Iterator<Item = (&str, &str, u32)> {
        self.transition_times
            .iter()
            .map(|t| (t.from.as_str(), t.to.as_str(), t.time))
    }

    /// Plays an animation set on a channel as the actor requests it
    /// (`CXModel::PlayAnim` with an owner): the transition time is the
    /// one of the pair (current animation, `name`) or the default, the
    /// stop transition time is the default. See [`Self::play_with`].
    pub fn play(&mut self, channel: usize, name: &str, force_restart: bool) -> bool {
        let transition = match self.current_animation(channel) {
            Some(current) => self.transition_time(current, name),
            None => self.default_transition_ms,
        };
        self.play_with(
            channel,
            name,
            transition,
            self.default_stop_transition_ms,
            force_restart,
        )
    }

    /// Plays an animation set on a channel with explicit times
    /// (`CXModel::PlayAnim` without an owner). The set is found by name
    /// ignoring ASCII case; `false` when the channel or the set does not
    /// exist. Asking for the animation the channel already heads for does
    /// nothing unless `force_restart`. With a transition time of 0, or on
    /// an empty channel, the new animation replaces the current one at
    /// once; otherwise both poses are held and blended for `transition_ms`
    /// before the new one starts. `stop_transition_ms` is how long the
    /// last pose is held when the animation finishes.
    pub fn play_with(
        &mut self,
        channel: usize,
        name: &str,
        transition_ms: u32,
        stop_transition_ms: u32,
        force_restart: bool,
    ) -> bool {
        if channel >= self.channels.len() {
            return false;
        }
        if !force_restart
            && self
                .current_animation(channel)
                .is_some_and(|current| current.eq_ignore_ascii_case(name))
        {
            return true;
        }
        let Some(set) = self
            .model
            .animation_sets
            .iter()
            .position(|set| set.name.eq_ignore_ascii_case(name))
        else {
            return false;
        };
        let looping = self.model.animation_sets[set].looping;
        let anim = ActiveAnimation::start(set, looping, self.timer);
        self.channels[channel].play(anim, transition_ms, stop_transition_ms, self.timer);
        true
    }

    /// Stops the animation of a channel (`CXModel::StopAnim`): at once
    /// with a transition time of 0, else the current pose is held for
    /// that time (the animation is made non-looping, no event fires) and
    /// the channel then empties, the frames falling back to their loaded
    /// matrices. `false` when the channel does not exist.
    pub fn stop(&mut self, channel: usize, transition_ms: u32) -> bool {
        match self.channels.get_mut(channel) {
            Some(ch) => {
                ch.stop(transition_ms, self.timer);
                true
            }
            None => false,
        }
    }

    /// Stops every channel.
    pub fn stop_all(&mut self, transition_ms: u32) {
        for channel in &mut self.channels {
            channel.stop(transition_ms, self.timer);
        }
    }

    /// `IsAnimPending(channel, name)`: whether the channel has an
    /// unfinished animation and, when a name is given, whether the
    /// animation it heads for has that name (ignoring ASCII case). A held
    /// pose after a stop still counts as playing; a finished animation
    /// held after its end does not.
    pub fn is_playing(&self, channel: usize, name: Option<&str>) -> bool {
        let Some(ch) = self.channels.get(channel) else {
            return false;
        };
        ch.is_playing()
            && match name {
                Some(name) => self
                    .current_animation(channel)
                    .is_some_and(|current| current.eq_ignore_ascii_case(name)),
                None => true,
            }
    }

    /// Whether any channel plays (`IsAnimPending(name)`).
    pub fn is_playing_on_any_channel(&self, name: Option<&str>) -> bool {
        (0..self.channels.len()).any(|channel| self.is_playing(channel, name))
    }

    /// The name of the animation a channel heads for: the incoming one
    /// during a transition, else the current one; `None` on an empty
    /// channel.
    pub fn current_animation(&self, channel: usize) -> Option<&str> {
        let anim = self.channels.get(channel)?.leading()?;
        self.model
            .animation_sets
            .get(anim.set)
            .map(|set| set.name.as_str())
    }

    /// The active animations of a channel: the current one and, during a
    /// transition, the incoming one.
    pub fn active_animations(&self, channel: usize) -> impl Iterator<Item = &ActiveAnimation> {
        self.channels
            .get(channel)
            .into_iter()
            .flat_map(|ch| ch.anims.iter().flatten())
    }

    /// Whether a channel is blending between two poses.
    pub fn is_transitioning(&self, channel: usize) -> bool {
        self.channels
            .get(channel)
            .is_some_and(|ch| ch.transitioning)
    }

    /// Removes the animation sets of a name (`CXModel::UnloadAnimation`):
    /// the sets [`Model::remove_animation_set`] removes, after the
    /// channels playing them have dropped them. `false` when the model
    /// has no such set.
    pub fn remove_animation_set(&mut self, name: &str) -> bool {
        let removed = self.model.animation_sets_to_unload(name);
        if removed.is_empty() {
            return false;
        }
        for channel in &mut self.channels {
            for slot in &mut channel.anims {
                let Some(anim) = slot else { continue };
                if removed.contains(&anim.set) {
                    *slot = None;
                } else {
                    anim.set -= removed.iter().filter(|&&index| index < anim.set).count();
                }
            }
        }
        self.model.remove_animation_set(name)
    }

    /// The animation state for a saved game: the timer, every channel's
    /// active animations by set name, the transition in progress and the
    /// transition times. See [`AnimationState`].
    pub fn state(&self) -> AnimationState {
        let name_of = |anim: &ActiveAnimation| ActiveAnimationState {
            set: self
                .model
                .animation_sets
                .get(anim.set)
                .map(|set| set.name.clone())
                .unwrap_or_default(),
            start_time: anim.start_time,
            looping: anim.looping,
            finished: anim.finished,
            last_local_time: anim.last_local_time,
            current_frame: anim.current_frame,
        };
        AnimationState {
            timer: self.timer,
            channels: self
                .channels
                .iter()
                .map(|channel| ChannelState {
                    anims: [
                        channel.anims[0].as_ref().map(name_of),
                        channel.anims[1].as_ref().map(name_of),
                    ],
                    transitioning: channel.transitioning,
                    transition_start: channel.transition_start,
                    transition_time: channel.transition_time,
                    stop_transition_time: channel.stop_transition_time,
                })
                .collect(),
            default_transition_ms: self.default_transition_ms,
            default_stop_transition_ms: self.default_stop_transition_ms,
            transition_times: self
                .transition_times
                .iter()
                .map(|t| (t.from.clone(), t.to.clone(), t.time))
                .collect(),
        }
    }

    /// Puts a saved animation state back on a model loaded again from
    /// its files: an active animation whose set the model no longer has
    /// is dropped (the original keeps a dangling pointer there), a
    /// transition left without an incoming animation ends, and the model
    /// is posed again at the saved timer, without firing events.
    pub fn restore(&mut self, state: &AnimationState) {
        self.timer = state.timer;
        self.default_transition_ms = state.default_transition_ms;
        self.default_stop_transition_ms = state.default_stop_transition_ms;
        self.transition_times = state
            .transition_times
            .iter()
            .map(|(from, to, time)| TransitionTime {
                from: from.clone(),
                to: to.clone(),
                time: *time,
            })
            .collect();
        let sets = &self.model.animation_sets;
        let revive = |saved: &Option<ActiveAnimationState>| -> Option<ActiveAnimation> {
            let saved = saved.as_ref()?;
            let set = sets
                .iter()
                .position(|set| set.name.eq_ignore_ascii_case(&saved.set))?;
            Some(ActiveAnimation {
                set,
                start_time: saved.start_time,
                looping: saved.looping,
                finished: saved.finished,
                last_local_time: saved.last_local_time,
                current_frame: saved.current_frame,
            })
        };
        for (index, channel) in self.channels.iter_mut().enumerate() {
            let Some(saved) = state.channels.get(index) else {
                *channel = Channel::default();
                continue;
            };
            channel.anims = [revive(&saved.anims[0]), revive(&saved.anims[1])];
            channel.transitioning = saved.transitioning && channel.anims[1].is_some();
            channel.transition_start = saved.transition_start;
            channel.transition_time = saved.transition_time;
            channel.stop_transition_time = saved.stop_transition_time;
        }
        // Pose at the saved timer: with no time elapsed the local times
        // and frame indices are what they were, so nothing fires.
        self.update(0);
    }

    /// Advances the timer by `delta_ms` and updates the model
    /// (`CXModel::Update`): the frames start from their loaded matrices,
    /// the channels write their poses in order 0 to 9 (a later channel
    /// overriding an earlier one on the frames both animate), the
    /// matrices are combined from the root and the meshes are skinned.
    /// Returns the names of the animation events crossed, in the order
    /// they fire.
    pub fn update(&mut self, delta_ms: u32) -> Vec<String> {
        self.timer = self.timer.wrapping_add(delta_ms);
        let mut events = Vec::new();
        let mut tick = Tick {
            model: &self.model,
            slots: &mut self.slots,
            timer: self.timer,
            events: &mut events,
        };
        for channel in &mut self.channels {
            channel.update(&mut tick);
        }
        self.pose();
        events
    }

    /// Builds the local and combined matrices from the slots the channels
    /// wrote, clears the slots, skins the meshes and updates the boxes.
    fn pose(&mut self) {
        for (index, frame) in self.model.frames.iter().enumerate() {
            let slots = &mut self.slots[index];
            self.locals[index] = slots.local_matrix().unwrap_or(frame.transform);
            *slots = FrameSlots::default();
            self.combined[index] = match frame.parent {
                Some(parent) => self.locals[index] * self.combined[parent],
                None => self.locals[index],
            };
        }
        for (mesh, skinned) in self.model.meshes.iter().zip(&mut self.meshes) {
            skin_mesh(mesh, &self.combined, &self.combined, skinned);
        }
        // The model's box starts at the origin and grows around the
        // meshes, so it always contains the origin (`UpdateBoundingRect`).
        let mut bounds = BoundingBox::default();
        for skinned in &self.meshes {
            bounds.include(&skinned.bounds);
        }
        self.bounds = bounds;
    }

    /// The local matrix of every frame after the last update.
    pub fn local_matrices(&self) -> &[Mat4] {
        &self.locals
    }

    /// The combined (model-space) matrix of every frame after the last
    /// update, indexed like the model's `frames`.
    pub fn combined_matrices(&self) -> &[Mat4] {
        &self.combined
    }

    /// The combined matrix of a frame by name (`GetBoneMatrix`), matched
    /// case-sensitively like `FindFrame`.
    pub fn bone_matrix(&self, name: &str) -> Option<&Mat4> {
        self.model
            .find_frame(name)
            .and_then(|frame| self.combined.get(frame))
    }

    /// The posed meshes after the last update, indexed like the model's
    /// `meshes`.
    pub fn meshes(&self) -> &[SkinnedMesh] {
        &self.meshes
    }

    /// The box around the posed meshes after the last update, in model
    /// space. As in the original it also contains the origin.
    pub fn bounding_box(&self) -> BoundingBox {
        self.bounds
    }

    /// The first posed triangle a line meets (`PickPoly`), with the
    /// engine's single-sided test ([`intersect_triangle`]); the ray is in
    /// model space. Triangles whose first vertex is not a number are
    /// skipped, as in the original.
    pub fn pick(&self, origin: Vec3, direction: Vec3) -> Option<Vec3> {
        for (mesh, skinned) in self.model.meshes.iter().zip(&self.meshes) {
            for triangle in &mesh.triangles {
                let [a, b, c] = triangle.map(|i| skinned.vertices.get(i as usize));
                let (Some(a), Some(b), Some(c)) = (a, b, c) else {
                    continue;
                };
                if a.position.x.is_nan() {
                    continue;
                }
                if let Some(hit) =
                    intersect_triangle(origin, direction, a.position, b.position, c.position)
                {
                    return Some(hit);
                }
            }
        }
        None
    }
}

/// A model loaded against a parent model, posed with the parent's bones
/// (`CXModel` with a `ParentModel`): the low-polygon `SHADOW_MODEL` of an
/// actor, which casts the shadows in place of the model drawn.
///
/// Its skinned meshes follow the frames of the **parent**: their bones
/// were looked up by name in the parent's skeleton
/// ([`Model::parse_with_parent`]). Its meshes without bones stay where
/// its own frames put them at rest, since nothing animates this model.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowModel {
    model: Model,
    /// The combined matrices of the model's own frames at rest.
    combined: Vec<Mat4>,
    meshes: Vec<SkinnedMesh>,
    bounds: BoundingBox,
}

impl ShadowModel {
    /// Parses a `.X` file as the shadow model of `parent`
    /// (`CXModel::LoadFromFile(file, ParentModel)`). The meshes are as
    /// loaded until the first [`Self::update`].
    pub fn parse(bytes: &[u8], file_name: &str, parent: &Model) -> crate::model::Result<Self> {
        Model::parse_with_parent(bytes, file_name, parent).map(ShadowModel::new)
    }

    /// Wraps a model loaded with [`Model::parse_with_parent`]. The meshes
    /// are as loaded until the first [`Self::update`].
    pub fn new(model: Model) -> ShadowModel {
        let combined = model.combine(&model.rest_pose());
        let meshes: Vec<SkinnedMesh> = model
            .meshes
            .iter()
            .map(|mesh| SkinnedMesh {
                vertices: mesh.vertices.clone(),
                bounds: BoundingBox::of_points(mesh.vertices.iter().map(|v| v.position)),
            })
            .collect();
        ShadowModel {
            model,
            combined,
            meshes,
            bounds: BoundingBox::default(),
        }
    }

    /// The model: the triangles of [`Self::meshes`] are its meshes'.
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// Poses the meshes after the parent's update (`CXModel::Update` of
    /// the shadow model, which the actor calls right after its model's):
    /// a skinned mesh with `offset * the parent's combined matrix` of each
    /// bone (identity for a bone the parent lacks, where the original
    /// reads an unset pointer), a mesh without bones with the combined
    /// matrix of its own frame at rest.
    pub fn update(&mut self, parent: &AnimatedModel) {
        for (mesh, skinned) in self.model.meshes.iter().zip(&mut self.meshes) {
            skin_mesh(mesh, parent.combined_matrices(), &self.combined, skinned);
        }
        let mut bounds = BoundingBox::default();
        for skinned in &self.meshes {
            bounds.include(&skinned.bounds);
        }
        self.bounds = bounds;
    }

    /// The posed meshes after the last update, indexed like the model's
    /// `meshes`.
    pub fn meshes(&self) -> &[SkinnedMesh] {
        &self.meshes
    }

    /// The box around the posed meshes and the origin after the last
    /// update, in model space.
    pub fn bounding_box(&self) -> BoundingBox {
        self.bounds
    }
}

/// Poses one mesh (`CXMesh::Update`). A skinned mesh takes, for every
/// bone, `palette = offset * bones[bone frame]` (identity for a bone
/// whose frame the model lacks, where the original reads an unset
/// pointer) and accumulates `weight * (vertex * palette)` for the
/// positions and `weight * (normal * palette)` for the normals of the
/// vertices the bone weights, the normals then normalised; a vertex no
/// bone weights keeps its loaded position (what D3DX does with it is not
/// known). A rigid mesh moves its positions by `frames[its frame]` and
/// keeps its normals as loaded, as the original does: a rigid mesh in a
/// turned frame is lit as if it had not turned. `bones` and `frames` are
/// the same matrices but for a model with a parent, whose bones are the
/// parent's.
fn skin_mesh(mesh: &Mesh, bones: &[Mat4], frames: &[Mat4], out: &mut SkinnedMesh) {
    if out.vertices.len() != mesh.vertices.len() {
        out.vertices = mesh.vertices.clone();
    } else {
        out.vertices.copy_from_slice(&mesh.vertices);
    }
    if mesh.is_skinned() {
        for bone in &mesh.bones {
            for weight in &bone.weights {
                if let Some(vertex) = out.vertices.get_mut(weight.vertex as usize) {
                    vertex.position = Vec3::ZERO;
                    vertex.normal = Vec3::ZERO;
                }
            }
        }
        for bone in &mesh.bones {
            let world = bone
                .frame
                .and_then(|frame| bones.get(frame))
                .copied()
                .unwrap_or(Mat4::IDENTITY);
            let palette = bone.offset * world;
            for weight in &bone.weights {
                let index = weight.vertex as usize;
                let Some(source) = mesh.vertices.get(index) else {
                    continue;
                };
                let [x, y, z, _] = palette.transform(source.position);
                let target = &mut out.vertices[index];
                target.position += Vec3::new(x, y, z) * weight.weight;
                target.normal += palette.transform_normal(source.normal) * weight.weight;
            }
        }
        for vertex in &mut out.vertices {
            vertex.normal = vertex.normal.normalized();
        }
    } else {
        let world = frames.get(mesh.frame).copied().unwrap_or(Mat4::IDENTITY);
        for (target, source) in out.vertices.iter_mut().zip(&mesh.vertices) {
            let [x, y, z, _] = world.transform(source.position);
            target.position = Vec3::new(x, y, z);
        }
    }
    out.bounds = BoundingBox::of_points(out.vertices.iter().map(|v| v.position));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::PI;

    /// A quad in the XZ plane skinned to two bones, `Root` (left edge,
    /// x = 0) and its child `Tip` (right edge, x = 2, placed 2 units along
    /// +X), plus a bone `Ghost` that names no frame and holds the fourth
    /// vertex. A rigid triangle `blade` hangs from `Tip`. Four animation
    /// sets, all over 4800 ticks (999 ms):
    ///
    /// - `lift` (looping): `Root` rises by 10, keys every 1200 ticks, so
    ///   the frame time is 249 ms (1200 / 4.8 in single precision,
    ///   truncated) and the set has 5 frames; `Tip` keeps its place;
    /// - `sink` (not looping): `Root` sinks by 10;
    /// - `tilt` (not looping): `Tip` turns by 90 degrees about Y (the file
    ///   quaternion is +90, applied conjugated);
    /// - `raise_tip`: `Tip` alone sits 3 units higher than at rest.
    const RIG: &str = r#"xof 0303txt 0032

AnimTicksPerSecond { 4800; }

Frame Root {
 FrameTransformMatrix { 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1;; }
 Frame Tip {
  FrameTransformMatrix { 1,0,0,0, 0,1,0,0, 0,0,1,0, 2,0,0,1;; }
  Mesh blade {
   3;
   0.0;0.0;0.0;,
   1.0;0.0;0.0;,
   0.0;0.0;1.0;;
   1;
   3;0,1,2;;
   MeshNormals { 1; 0.0;1.0;0.0;; 1; 3;0,0,0;; }
  }
 }
 Mesh quad {
  4;
  0.0;0.0;0.0;,
  2.0;0.0;0.0;,
  2.0;0.0;1.0;,
  0.0;0.0;1.0;;
  1;
  4;0,1,2,3;;
  MeshNormals { 1; 0.0;1.0;0.0;; 1; 4;0,0,0,0;; }
  MeshTextureCoords { 4; 0.0;0.0;, 1.0;0.0;, 1.0;1.0;, 0.0;1.0;; }
  XSkinMeshHeader { 1; 2; 3; }
  SkinWeights {
   "Root";
   1;
   0;
   1.000000;
   1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1;;
  }
  SkinWeights {
   "Tip";
   2;
   1,
   2;
   1.000000,
   1.000000;
   1,0,0,0, 0,1,0,0, 0,0,1,0, -2,0,0,1;;
  }
  SkinWeights {
   "Ghost";
   1;
   3;
   1.000000;
   1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1;;
  }
 }
}

AnimationSet lift {
 Animation {
  AnimationKey {
   2;
   5;
   0;3;0.0,0.0,0.0;;,
   1200;3;0.0,2.5,0.0;;,
   2400;3;0.0,5.0,0.0;;,
   3600;3;0.0,7.5,0.0;;,
   4800;3;0.0,10.0,0.0;;;
  }
  AnimationOptions { 1; 0; }
  { Root }
 }
 Animation {
  AnimationKey {
   2;
   2;
   0;3;2.0,0.0,0.0;;,
   4800;3;2.0,0.0,0.0;;;
  }
  { Tip }
 }
}

AnimationSet sink {
 Animation {
  AnimationKey {
   2;
   2;
   0;3;0.0,0.0,0.0;;,
   4800;3;0.0,-10.0,0.0;;;
  }
  { Root }
 }
 Animation {
  AnimationKey {
   2;
   2;
   0;3;2.0,0.0,0.0;;,
   4800;3;2.0,0.0,0.0;;;
  }
  { Tip }
 }
}

AnimationSet tilt {
 Animation {
  AnimationKey {
   0;
   2;
   0;4;1.0,0.0,0.0,0.0;;,
   4800;4;0.70710678,0.0,0.70710678,0.0;;;
  }
  AnimationKey {
   2;
   2;
   0;3;2.0,0.0,0.0;;,
   4800;3;2.0,0.0,0.0;;;
  }
  { Tip }
 }
}

AnimationSet raise_tip {
 Animation {
  AnimationKey {
   2;
   2;
   0;3;2.0,3.0,0.0;;,
   4800;3;2.0,3.0,0.0;;;
  }
  AnimationOptions { 1; 0; }
  { Tip }
 }
}
"#;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn assert_vec(v: Vec3, x: f32, y: f32, z: f32) {
        assert!(
            close(v.x, x) && close(v.y, y) && close(v.z, z),
            "expected ({x}, {y}, {z}), got {v:?}"
        );
    }

    fn new_rig() -> AnimatedModel {
        let mut model = Model::parse(RIG.as_bytes(), "rig.x").unwrap();
        let lift = model.animation_set_mut("lift").unwrap();
        for frame in 1..=4 {
            lift.add_event(&format!("f{frame}"), frame, 4800);
        }
        AnimatedModel::new(model)
    }

    /// `Root`'s own mesh comes before the mesh of its child frame `Tip`,
    /// although the file writes it after.
    fn quad_of(rig: &AnimatedModel) -> &[Vertex] {
        &rig.meshes()[0].vertices
    }

    fn blade_of(rig: &AnimatedModel) -> &[Vertex] {
        &rig.meshes()[1].vertices
    }

    #[test]
    fn the_fixture_has_the_expected_timing() {
        let rig = new_rig();
        let lift = rig.model().animation_set("lift").unwrap();
        assert_eq!(lift.total_time_ms(4800), 999);
        assert_eq!(lift.frame_time_ms(4800), 249);
        assert_eq!(lift.total_frames(4800), 5);
        assert!(lift.looping);
        assert!(!rig.model().animation_set("sink").unwrap().looping);
        assert_eq!(rig.model().meshes[1].name, "blade");
        assert!(!rig.model().meshes[1].is_skinned());
        assert_eq!(rig.model().meshes[0].name, "quad");
        assert!(
            rig.model().meshes[0].bones[2].frame.is_none(),
            "Ghost dangles"
        );
    }

    #[test]
    fn the_state_restores_on_a_fresh_model_without_firing_events() {
        let mut rig = new_rig();
        rig.set_default_transition_ms(50);
        rig.set_transition_time("lift", "sink", Some(120));
        assert!(rig.play(0, "lift", true));
        rig.update(300);
        // A transition into `sink` on channel 0 (its 120 ms elapse in the
        // long update, so `sink` has just started), `tilt` finished and
        // stopped on channel 1.
        assert!(rig.play(0, "sink", true));
        rig.update(40);
        assert!(rig.play_with(1, "tilt", 0, 0, true));
        rig.update(2000);
        let state = rig.state();
        assert_eq!(state.timer, 2340);
        assert_eq!(state.channels.len(), NUM_CHANNELS);
        assert_eq!(state.channels[0].anims[0].as_ref().unwrap().set, "sink");
        assert!(state.channels[0].anims[1].is_none(), "the transition ended");
        assert!(state.channels[1].anims[0].is_none(), "tilt finished");
        assert_eq!(
            state.transition_times,
            vec![("lift".into(), "sink".into(), 120)]
        );

        // The update where `tilt` finished still showed its last pose;
        // the next one shows the channels as saved.
        rig.update(0);
        let mut back = new_rig();
        back.restore(&state);
        assert_eq!(back.timer(), rig.timer());
        assert_eq!(back.current_animation(0), rig.current_animation(0));
        assert_eq!(back.is_playing(0, None), rig.is_playing(0, None));
        assert_eq!(back.is_playing(1, None), rig.is_playing(1, None));
        assert_eq!(back.transition_time("lift", "sink"), 120);
        assert_eq!(back.default_transition_ms(), 50);
        assert_eq!(quad_of(&back), quad_of(&rig), "the same pose");
        // The two go on in step, events included.
        assert_eq!(back.update(100), rig.update(100));
        assert_eq!(quad_of(&back), quad_of(&rig));

        // A set the model lost is dropped from its channel.
        let mut saved = rig.state();
        saved.channels[0].anims[0].as_mut().unwrap().set = "gone".into();
        let mut other = new_rig();
        other.restore(&saved);
        assert!(!other.is_playing(0, None));
        assert_eq!(other.state().channels[0].anims[0], None);
    }

    #[test]
    fn a_new_rig_is_skinned_at_rest() {
        let rig = new_rig();
        let quad = quad_of(&rig);
        assert_vec(quad[0].position, 0.0, 0.0, 0.0);
        assert_vec(quad[1].position, 2.0, 0.0, 0.0);
        assert_vec(quad[2].position, 2.0, 0.0, 1.0);
        assert_vec(quad[3].position, 0.0, 0.0, 1.0);
        assert_vec(quad[2].normal, 0.0, 1.0, 0.0);
        assert_eq!(quad[2].uv, [1.0, 1.0]);
        // The rigid blade sits where Tip is.
        assert_vec(blade_of(&rig)[0].position, 2.0, 0.0, 0.0);
        assert_vec(blade_of(&rig)[1].position, 3.0, 0.0, 0.0);
        let bounds = rig.bounding_box();
        assert_vec(bounds.min, 0.0, 0.0, 0.0);
        assert_vec(bounds.max, 3.0, 0.0, 1.0);
        assert_eq!(rig.combined_matrices().len(), 3);
        assert_vec(
            rig.bone_matrix("Tip").unwrap().translation_part(),
            2.0,
            0.0,
            0.0,
        );
        assert!(rig.bone_matrix("tip").is_none(), "case-sensitive");
        assert!(!rig.is_playing(0, None));
        assert_eq!(rig.current_animation(0), None);
    }

    #[test]
    fn plays_an_animation_over_time_with_its_events() {
        let mut rig = new_rig();
        assert!(rig.play_with(0, "LIFT", 0, 0, false), "case-insensitive");
        assert!(rig.is_playing(0, None));
        assert!(rig.is_playing(0, Some("lift")));
        assert!(!rig.is_playing(0, Some("sink")));
        assert_eq!(rig.current_animation(0), Some("lift"));

        // t = 0: the first pose, and the events of frame 1.
        assert_eq!(rig.update(0), vec!["f1"]);
        assert_vec(quad_of(&rig)[0].position, 0.0, 0.0, 0.0);
        assert_vec(quad_of(&rig)[1].position, 2.0, 0.0, 0.0);

        // Halfway: Root has risen by 5 and Tip with it; frames 2 and 3
        // were crossed; the Ghost-weighted vertex stays where it was.
        assert_eq!(rig.update(500), vec!["f2", "f3"]);
        let quad = quad_of(&rig);
        assert_vec(quad[0].position, 0.0, 5.0, 0.0);
        assert_vec(quad[1].position, 2.0, 5.0, 0.0);
        assert_vec(quad[2].position, 2.0, 5.0, 1.0);
        assert_vec(quad[3].position, 0.0, 0.0, 1.0);
        assert_vec(quad[0].normal, 0.0, 1.0, 0.0);
        // The rigid blade follows Tip.
        assert_vec(blade_of(&rig)[0].position, 2.0, 5.0, 0.0);
        assert_vec(blade_of(&rig)[2].position, 2.0, 5.0, 1.0);
        assert_vec(blade_of(&rig)[0].normal, 0.0, 1.0, 0.0);
        // The box contains the posed meshes and the origin.
        let bounds = rig.bounding_box();
        assert_vec(bounds.min, 0.0, 0.0, 0.0);
        assert_vec(bounds.max, 3.0, 5.0, 1.0);

        // The end: 999 ms is the last millisecond of the set.
        assert_eq!(rig.update(499), vec!["f4"]);
        let y = 10.0 * 4795.0 / 4800.0;
        assert_vec(quad_of(&rig)[0].position, 0.0, y, 0.0);
        assert_eq!(rig.timer(), 999);
    }

    #[test]
    fn a_looping_animation_wraps() {
        let mut rig = new_rig();
        rig.play_with(0, "lift", 0, 0, false);
        rig.update(999);
        // 1000 > 999 wraps to local time 1 (4 ticks) and frame 1: the
        // events above frame 4 (none), then those up to frame 1.
        assert_eq!(rig.update(1), vec!["f1"]);
        assert_vec(quad_of(&rig)[0].position, 0.0, 10.0 * 4.0 / 4800.0, 0.0);
        assert!(rig.is_playing(0, None));
        assert_eq!(rig.active_animations(0).next().unwrap().local_time(), 1);
        // A long jump crosses several frames and wraps once.
        let events = rig.update(999 * 3 + 600);
        assert_eq!(events, vec!["f2", "f3"]);
        // 601 ms is 2884 ticks (truncated), between the keys at 2400 and
        // 3600.
        assert_vec(quad_of(&rig)[0].position, 0.0, 10.0 * 2884.0 / 4800.0, 0.0);
    }

    #[test]
    fn a_finished_animation_holds_its_last_pose_then_stops() {
        // Stop transition 0: the channel empties on the frame it finishes;
        // that frame still shows the last pose.
        let mut rig = new_rig();
        rig.play_with(0, "sink", 0, 0, false);
        rig.update(500);
        assert_vec(quad_of(&rig)[0].position, 0.0, -5.0, 0.0);
        rig.update(1500);
        // The last millisecond, 998 ms, is 4790 ticks (truncated).
        let y = -10.0 * 4790.0 / 4800.0;
        assert_vec(quad_of(&rig)[0].position, 0.0, y, 0.0);
        assert!(!rig.is_playing(0, None));
        assert_eq!(rig.current_animation(0), None);
        rig.update(16);
        assert_vec(quad_of(&rig)[0].position, 0.0, 0.0, 0.0);

        // Stop transition 300: the last pose is held for 300 ms, the
        // animation counting as finished, then the rest pose returns.
        let mut rig = new_rig();
        rig.play_with(0, "sink", 0, 300, false);
        rig.update(2000);
        assert!(rig.is_transitioning(0));
        assert!(!rig.is_playing(0, None));
        assert_eq!(rig.current_animation(0), Some("sink"));
        rig.update(299);
        assert_vec(quad_of(&rig)[0].position, 0.0, y, 0.0);
        rig.update(1);
        assert_vec(quad_of(&rig)[0].position, 0.0, 0.0, 0.0);
        assert_eq!(rig.current_animation(0), None);
    }

    #[test]
    fn stopping_holds_the_pose_without_events() {
        let mut rig = new_rig();
        rig.play_with(0, "lift", 0, 0, false);
        rig.update(500);
        assert!(rig.stop(0, 200));
        // Held: no movement, no event, still "playing" since it did not
        // finish on its own.
        assert_eq!(rig.update(100), Vec::<String>::new());
        assert_vec(quad_of(&rig)[0].position, 0.0, 5.0, 0.0);
        assert!(rig.is_playing(0, None));
        assert_eq!(rig.update(100), Vec::<String>::new());
        assert_vec(quad_of(&rig)[0].position, 0.0, 0.0, 0.0);
        assert!(!rig.is_playing(0, None));

        rig.play_with(0, "lift", 0, 0, false);
        rig.update(500);
        assert!(rig.stop(0, 0));
        rig.update(0);
        assert_vec(quad_of(&rig)[0].position, 0.0, 0.0, 0.0);
        assert!(!rig.stop(NUM_CHANNELS, 0));
    }

    #[test]
    fn a_transition_blends_two_frozen_poses() {
        let mut rig = new_rig();
        rig.play_with(0, "lift", 0, 0, false);
        rig.update(500);
        assert!(rig.play_with(0, "sink", 200, 0, false));
        assert!(rig.is_transitioning(0));
        assert_eq!(rig.current_animation(0), Some("sink"));
        assert_eq!(rig.active_animations(0).count(), 2);

        // Halfway through: lift frozen at y = 5, sink at its first pose
        // (y = 0), blended to 2.5; no event (lift would have crossed a
        // frame otherwise).
        assert_eq!(rig.update(100), Vec::<String>::new());
        assert_vec(quad_of(&rig)[0].position, 0.0, 2.5, 0.0);
        assert_vec(quad_of(&rig)[1].position, 2.0, 2.5, 0.0);
        assert!(rig.is_playing(0, Some("sink")));

        // Done: sink starts from its beginning, the transition time not
        // counted.
        rig.update(100);
        assert!(!rig.is_transitioning(0));
        assert_vec(quad_of(&rig)[0].position, 0.0, 0.0, 0.0);
        assert_eq!(rig.active_animations(0).count(), 1);
        rig.update(250);
        assert_vec(quad_of(&rig)[0].position, 0.0, -2.5, 0.0);
    }

    #[test]
    fn a_transition_only_blends_the_frames_both_animations_have() {
        // lift animates Root and Tip, raise_tip only Tip: during the
        // transition Root keeps lift's frozen pose unblended and Tip
        // blends toward its raised place.
        let mut rig = new_rig();
        rig.play_with(0, "lift", 0, 0, false);
        rig.update(500);
        rig.play_with(0, "raise_tip", 200, 0, false);
        rig.update(100);
        assert_vec(quad_of(&rig)[0].position, 0.0, 5.0, 0.0);
        assert_vec(quad_of(&rig)[1].position, 2.0, 6.5, 0.0);
        // The other way round: a frame only the incoming animation has
        // stays at rest until the transition ends.
        let mut rig = new_rig();
        rig.play_with(0, "raise_tip", 0, 0, false);
        rig.update(100);
        rig.play_with(0, "lift", 200, 0, false);
        rig.update(100);
        assert_vec(quad_of(&rig)[0].position, 0.0, 0.0, 0.0);
        assert_vec(quad_of(&rig)[1].position, 2.0, 1.5, 0.0);
        rig.update(100);
        assert_vec(quad_of(&rig)[1].position, 2.0, 0.0, 0.0);
    }

    #[test]
    fn a_later_channel_overrides_the_frames_it_animates() {
        let mut rig = new_rig();
        rig.play_with(0, "lift", 0, 0, false);
        rig.play_with(1, "raise_tip", 0, 0, false);
        rig.update(500);
        let quad = quad_of(&rig);
        assert_vec(quad[0].position, 0.0, 5.0, 0.0);
        assert_vec(quad[1].position, 2.0, 8.0, 0.0);
        assert!(rig.is_playing(1, Some("raise_tip")));
        assert!(rig.is_playing_on_any_channel(Some("raise_tip")));
        assert!(!rig.is_playing_on_any_channel(Some("tilt")));
        rig.stop(1, 0);
        rig.update(0);
        assert_vec(quad_of(&rig)[1].position, 2.0, 5.0, 0.0);
    }

    #[test]
    fn a_rotation_turns_the_skinned_vertices_about_the_bone() {
        let mut rig = new_rig();
        rig.play_with(0, "tilt", 0, 0, false);
        // Halfway: Tip has turned by -45 degrees about Y, so its local +Z
        // vertex swings toward -X.
        rig.update(500);
        let s = (PI / 4.0).sin();
        assert_vec(quad_of(&rig)[2].position, 2.0 - s, 0.0, s);
        assert_vec(quad_of(&rig)[1].position, 2.0, 0.0, 0.0);
        // The normal turns with the bone and stays unit length.
        let n = quad_of(&rig)[2].normal;
        assert_vec(n, 0.0, 1.0, 0.0);
        // The rigid blade's +X vertex turns the same way.
        assert_vec(blade_of(&rig)[1].position, 2.0 + s, 0.0, s);
    }

    #[test]
    fn the_same_animation_is_not_restarted_unless_forced() {
        let mut rig = new_rig();
        rig.play_with(0, "lift", 0, 0, false);
        rig.update(500);
        assert!(rig.play_with(0, "Lift", 0, 0, false));
        rig.update(0);
        assert_vec(quad_of(&rig)[0].position, 0.0, 5.0, 0.0);
        assert!(rig.play_with(0, "lift", 0, 0, true));
        rig.update(0);
        assert_vec(quad_of(&rig)[0].position, 0.0, 0.0, 0.0);
        assert!(!rig.play_with(0, "missing", 0, 0, false));
        assert!(!rig.play_with(NUM_CHANNELS, "lift", 0, 0, false));
    }

    #[test]
    fn play_uses_the_transition_table_and_the_defaults() {
        let mut rig = new_rig();
        assert_eq!(rig.default_transition_ms(), 200);
        assert_eq!(rig.default_stop_transition_ms(), 200);
        assert_eq!(rig.transition_time("lift", "sink"), 200);
        rig.set_transition_time("LIFT", "Sink", Some(400));
        assert_eq!(rig.transition_time("lift", "sink"), 400);
        assert_eq!(rig.transition_time("sink", "lift"), 200);
        assert_eq!(rig.transition_times().count(), 1);

        assert!(rig.play(0, "lift", false));
        rig.update(500);
        assert!(rig.play(0, "sink", false));
        // 400 ms transition: halfway at 200.
        rig.update(200);
        assert_vec(quad_of(&rig)[0].position, 0.0, 2.5, 0.0);
        rig.update(200);
        assert!(!rig.is_transitioning(0));

        rig.set_transition_time("lift", "sink", None);
        assert_eq!(rig.transition_times().count(), 0);
        rig.set_transition_time("lift", "sink", None);
        rig.set_default_transition_ms(0);
        rig.play(0, "lift", false);
        assert!(!rig.is_transitioning(0), "0 replaces at once");

        // The default stop transition holds a finished animation.
        rig.set_default_stop_transition_ms(100);
        rig.play(0, "sink", true);
        rig.update(1500);
        assert!(rig.is_transitioning(0));
        rig.update(100);
        assert_eq!(rig.current_animation(0), None);
    }

    #[test]
    fn removing_a_set_drops_it_from_the_channels() {
        let mut rig = new_rig();
        rig.play_with(0, "lift", 0, 0, false);
        rig.play_with(1, "tilt", 0, 0, false);
        assert!(rig.remove_animation_set("LIFT"));
        assert!(!rig.remove_animation_set("lift"));
        assert_eq!(rig.current_animation(0), None);
        assert_eq!(rig.current_animation(1), Some("tilt"));
        rig.update(500);
        assert_vec(quad_of(&rig)[0].position, 0.0, 0.0, 0.0);
        let s = (PI / 4.0).sin();
        assert_vec(quad_of(&rig)[2].position, 2.0 - s, 0.0, s);
    }

    /// `CXModel::UnloadAnimation` steps over the two sets that follow a
    /// removed one.
    #[test]
    fn removing_a_set_skips_the_two_sets_after_it() {
        let mut model = Model::parse(RIG.as_bytes(), "rig.x").unwrap();
        let lift = model.animation_set("lift").unwrap().clone();
        let named = |name: &str| {
            let mut set = lift.clone();
            set.name = name.to_string();
            set
        };
        model.animation_sets = ["a", "x", "X", "x", "x", "b", "x"]
            .iter()
            .map(|name| named(name))
            .collect();
        assert_eq!(model.animation_sets_to_unload("X"), vec![1, 4]);
        let mut rig = AnimatedModel::new(model);
        rig.play_with(0, "x", 0, 0, false);
        rig.play_with(1, "b", 0, 0, false);
        assert!(rig.remove_animation_set("X"));
        let names: Vec<&str> = rig
            .model()
            .animation_sets
            .iter()
            .map(|set| set.name.as_str())
            .collect();
        assert_eq!(names, ["a", "X", "x", "b", "x"]);
        assert_eq!(rig.current_animation(0), None);
        assert_eq!(rig.current_animation(1), Some("b"));
        // The next call removes the first of the two left side by side,
        // and the one after `b`.
        assert!(rig.remove_animation_set("x"));
        assert_eq!(rig.model().animation_sets.len(), 3);
        assert_eq!(rig.current_animation(1), Some("b"));
        assert!(rig.remove_animation_set("x"));
        assert!(!rig.remove_animation_set("x"));
        assert_eq!(rig.model().animation_sets.len(), 2);
    }

    /// `CXMesh::Update` moves the positions of a mesh without bones and
    /// leaves its normals as loaded.
    #[test]
    fn a_rigid_mesh_keeps_its_loaded_normals() {
        // The frame turns +X into +Y.
        let text = r#"xof 0303txt 0032
Frame Turned {
 FrameTransformMatrix { 0,1,0,0, -1,0,0,0, 0,0,1,0, 0,0,0,1;; }
 Mesh plate {
  3;
  1.0;0.0;0.0;,
  2.0;0.0;0.0;,
  1.0;0.0;1.0;;
  1;
  3;0,1,2;;
  MeshNormals { 1; 1.0;0.0;0.0;; 1; 3;0,0,0;; }
 }
}
"#;
        let rig = AnimatedModel::new(Model::parse(text.as_bytes(), "plate.x").unwrap());
        let vertices = &rig.meshes()[0].vertices;
        assert_vec(vertices[0].position, 0.0, 1.0, 0.0);
        assert_vec(vertices[1].position, 0.0, 2.0, 0.0);
        assert_vec(vertices[0].normal, 1.0, 0.0, 0.0);
    }

    /// `CXMesh::Update` of a model with a `ParentModel`: the skinned
    /// meshes follow the parent's frames, the others their own at rest.
    #[test]
    fn a_shadow_model_follows_the_bones_of_its_parent() {
        // A triangle skinned to the parent's `Tip` (vertices 0 and 1) and
        // to a bone the parent lacks (vertex 2), and a rigid triangle in a
        // frame of its own, 5 units up. The file's own `Tip` frame sits
        // elsewhere and plays no role.
        let text = r#"xof 0303txt 0032
Frame Tip {
 FrameTransformMatrix { 1,0,0,0, 0,1,0,0, 0,0,1,0, 50,0,0,1;; }
}
Frame Low {
 FrameTransformMatrix { 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,5,0,1;; }
 Mesh rigid {
  3;
  0.0;0.0;0.0;, 1.0;0.0;0.0;, 0.0;0.0;1.0;;
  1;
  3;0,1,2;;
 }
 Mesh skin {
  3;
  2.0;0.0;0.0;, 3.0;0.0;0.0;, 2.0;0.0;1.0;;
  1;
  3;0,1,2;;
  SkinWeights { "Tip"; 2; 0,1; 1.0,1.0; 1,0,0,0, 0,1,0,0, 0,0,1,0, -2,0,0,1;; }
  SkinWeights { "Nowhere"; 1; 2; 1.0; 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1;; }
 }
}
"#;
        let mut rig = new_rig();
        let mut shadow = ShadowModel::parse(text.as_bytes(), "low.x", rig.model()).unwrap();
        assert_eq!(shadow.model().meshes.len(), 2);
        // As loaded until the first update.
        assert_vec(shadow.meshes()[1].vertices[0].position, 2.0, 0.0, 0.0);

        rig.play_with(0, "raise_tip", 0, 0, false);
        rig.update(100);
        shadow.update(&rig);
        // The parent's Tip is at (2, 3, 0): offset (-2) then the bone.
        let skin = &shadow.meshes()[1].vertices;
        assert_vec(skin[0].position, 2.0, 3.0, 0.0);
        assert_vec(skin[1].position, 3.0, 3.0, 0.0);
        // The bone the parent lacks: identity.
        assert_vec(skin[2].position, 2.0, 0.0, 1.0);
        // The rigid mesh: its own frame at rest.
        let rigid = &shadow.meshes()[0].vertices;
        assert_vec(rigid[0].position, 0.0, 5.0, 0.0);
        assert_vec(rigid[1].position, 1.0, 5.0, 0.0);
        let bounds = shadow.bounding_box();
        assert_vec(bounds.min, 0.0, 0.0, 0.0);
        assert_vec(bounds.max, 3.0, 5.0, 1.0);

        // The parent moves on, the shadow model follows at its update.
        rig.stop(0, 0);
        rig.play_with(0, "lift", 0, 0, false);
        rig.update(999);
        shadow.update(&rig);
        let lifted = shadow.meshes()[1].vertices[0].position;
        assert!(close(lifted.x, 2.0) && lifted.y > 9.9, "{lifted:?}");
        assert_vec(shadow.meshes()[0].vertices[0].position, 0.0, 5.0, 0.0);
    }

    #[test]
    fn picks_the_posed_triangles() {
        // The quad's front faces -Y (its winding), and the engine's test
        // is single-sided, so the line must come from below.
        let mut rig = new_rig();
        let up = Vec3::new(0.0, 1.0, 0.0);
        let hit = rig.pick(Vec3::new(1.5, -5.0, 0.25), up).unwrap();
        assert_vec(hit, 1.5, 0.0, 0.25);
        assert!(rig.pick(Vec3::new(5.0, -5.0, 0.25), up).is_none());
        assert!(
            rig.pick(Vec3::new(1.5, 5.0, 0.25), Vec3::new(0.0, -1.0, 0.0))
                .is_none(),
            "a back face is not hit"
        );
        rig.play_with(0, "lift", 0, 0, false);
        rig.update(500);
        // The quad's first triangle (vertices 0, 1, 2) is at y = 5.
        let hit = rig.pick(Vec3::new(1.5, -10.0, 0.25), up).unwrap();
        assert_vec(hit, 1.5, 5.0, 0.25);
    }

    #[test]
    fn bounding_boxes_have_corners() {
        let bounds = BoundingBox::of_points([Vec3::new(1.0, 2.0, 3.0), Vec3::new(-1.0, 5.0, 0.0)]);
        assert_vec(bounds.min, -1.0, 2.0, 0.0);
        assert_vec(bounds.max, 1.0, 5.0, 3.0);
        let corners = bounds.corners();
        assert_vec(corners[0], -1.0, 2.0, 0.0);
        assert_vec(corners[7], 1.0, 5.0, 3.0);
        assert_eq!(BoundingBox::of_points([]), BoundingBox::default());
    }

    /// Plays every animation set of the demo's Trinity model to its end
    /// when the model is present under `testdata/local/wme_demo_3d/`.
    #[test]
    fn animates_the_demo_model_when_present() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/local/wme_demo_3d");
        let path = dir.join("trinity.x");
        if !path.is_file() {
            eprintln!("skipped: {} is absent", path.display());
            return;
        }
        let model = Model::parse(&std::fs::read(&path).unwrap(), "trinity.x").unwrap();
        let names: Vec<String> = model
            .animation_sets
            .iter()
            .map(|s| s.name.clone())
            .collect();
        let mut rig = AnimatedModel::new(model);
        let rest = rig.bounding_box();
        assert!(rest.max.y > rest.min.y, "the rest pose has a height");
        for name in &names {
            let set = rig.model().animation_set(name).unwrap();
            let (total, looping) = (set.total_time_ms(4800), set.looping);
            assert!(rig.play_with(0, name, 0, 0, true), "{name}");
            let mut moved = false;
            let mut elapsed = 0;
            while elapsed <= total + 100 {
                rig.update(16);
                elapsed += 16;
                let bounds = rig.bounding_box();
                assert!(
                    !bounds.min.x.is_nan() && !bounds.max.x.is_nan(),
                    "{name}: NaN in the box at {elapsed} ms"
                );
                if bounds != rest {
                    moved = true;
                }
            }
            assert!(moved, "{name}: the bounding box never moved");
            if looping {
                assert!(rig.is_playing(0, Some(name)), "{name}: loops");
                rig.stop(0, 0);
            } else {
                assert!(!rig.is_playing(0, None), "{name}: finished");
            }
            rig.update(16);
        }
    }
}
