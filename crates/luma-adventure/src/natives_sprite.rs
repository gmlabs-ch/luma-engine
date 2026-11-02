//! The sprite, frame and sub-frame objects scripts hold: the sprite of an
//! entity (`GetSpriteObject`) or one of the images of a control
//! (`GetImageObject`, `GetHoverImageObject`...), its frames (`GetFrame`,
//! `AddFrame`) and their sub-frames (`GetSubframe`, `AddSubframe`).
//! Frames and sub-frames are handles by position into the sprite
//! definition held by the owner's sprite bank; what a script changes
//! through them changes what is drawn, and the edited sprite is kept in
//! saved games.
//!
//! Written with reference to the `ScGetProperty`, `ScSetProperty` and
//! `ScCallMethod` methods of `BSprite.cpp`, `BFrame.cpp` and
//! `BSubFrame.cpp` of Wintermute Engine 1.x (MIT License, Copyright (c)
//! 2003-2013 Jan Nedoma); see `NOTICE`.

use luma_formats::{Frame, Rgba, Sprite, SubFrame};
use luma_script::{Context, NativeId, Return, Value};

use crate::natives::{color_int, color_of};
use crate::objects::Kind;
use crate::scene_view::{AnimState, EntityRef, key};
use crate::ui::ImageSlot;
use crate::world::World;

/// Which sprite bank an owner's sprite lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Home {
    /// The scene's bank (scene entities).
    Scene,
    /// The windows' bank (the entities of entity containers, the items'
    /// images).
    Ui,
    /// The game's own view (game-level entities and attachments).
    Game,
    /// An actor's own bank.
    Actor(NativeId),
}

impl World {
    /// Logs a script runtime error with the calling script and line.
    pub(crate) fn runtime_error(&mut self, ctx: &Context<'_>, message: &str) {
        let script = ctx
            .engine
            .script(ctx.script)
            .map(|s| s.filename().to_string())
            .unwrap_or_default();
        self.log(format!(
            "Runtime error. Script '{script}', line {}: {message}",
            ctx.line
        ));
    }

    /// The owner of a sprite object, the bank its sprite is in and the
    /// sprite's path (`World::own_sprite_path`): an entity's sprite, the
    /// sprite `SetSprite` gave an actor, an item's image or hover image,
    /// or the image in a slot of a control (the items' and controls'
    /// images live in the windows' bank).
    fn sprite_home(&self, sprite: NativeId) -> Option<(NativeId, Home, String)> {
        match self.native(sprite)?.kind {
            Kind::Sprite(owner) => {
                let path = self.own_sprite_path(owner)?;
                let home = match self.native(owner)?.kind {
                    Kind::Entity(EntityRef::Ui(_)) | Kind::Item => Home::Ui,
                    Kind::Entity(EntityRef::Game(_)) => Home::Game,
                    Kind::Entity(_) => Home::Scene,
                    Kind::Actor => Home::Actor(owner),
                    _ => return None,
                };
                Some((owner, home, path))
            }
            Kind::ControlImage { control, slot } => {
                let path = self.control(control)?.image_in(slot)?.to_string();
                Some((control, Home::Ui, path))
            }
            Kind::HoverSprite(item) => {
                let path = self.item(item)?.def.sprite_hover.clone()?;
                Some((item, Home::Ui, path))
            }
            _ => None,
        }
    }

    /// The slot of a control's sprite object; `None` for an entity's.
    fn sprite_slot(&self, sprite: NativeId) -> Option<ImageSlot> {
        match self.native(sprite)?.kind {
            Kind::ControlImage { slot, .. } => Some(slot),
            _ => None,
        }
    }

    /// The definition of a sprite object's sprite.
    pub(crate) fn sprite_def(&self, sprite: NativeId) -> Option<&Sprite> {
        let (_, home, path) = self.sprite_home(sprite)?;
        let bank = match home {
            Home::Scene => &self.scene()?.bank,
            Home::Ui => &self.ui.bank,
            Home::Game => &self.game_view.bank,
            Home::Actor(actor) => &self.actor(actor)?.bank,
        };
        bank.sprite(&path).map(|l| &l.sprite)
    }

    /// The definition of a sprite object's sprite, for a script to change
    /// it; the sprite is then kept in saved games.
    fn sprite_def_mut(&mut self, sprite: NativeId) -> Option<&mut Sprite> {
        let (_, home, path) = self.sprite_home(sprite)?;
        match home {
            Home::Scene => self.scene.as_mut()?.bank.edit_sprite(&path),
            Home::Ui => self.ui.bank.edit_sprite(&path),
            Home::Game => self.game_view.bank.edit_sprite(&path),
            Home::Actor(actor) => self.actor_mut(actor)?.bank.edit_sprite(&path),
        }
    }

    /// The animation state of a sprite object's sprite, if it has been
    /// shown.
    fn sprite_anim(&self, sprite: NativeId) -> Option<&AnimState> {
        let (owner, _, path) = self.sprite_home(sprite)?;
        match self.native(owner)?.kind {
            Kind::Entity(EntityRef::Ui(i)) => self.ui_entities.get(i)?.anim.as_ref(),
            Kind::Entity(r) => self.view_of(r)?.anim(r),
            Kind::Actor => self.actor(owner)?.anim_state(&path),
            Kind::Item => self
                .item(owner)?
                .anim
                .as_ref()
                .filter(|a| a.sprite == key(&path)),
            Kind::Control => self.control(owner)?.anim_in(self.sprite_slot(sprite)?),
            _ => None,
        }
    }

    /// The animation state of a sprite object's sprite, created for it.
    fn sprite_anim_mut(&mut self, sprite: NativeId) -> Option<&mut AnimState> {
        let (owner, _, path) = self.sprite_home(sprite)?;
        let slot = self.sprite_slot(sprite);
        let anim = match self.native(owner)?.kind {
            Kind::Entity(EntityRef::Ui(i)) => &mut self.ui_entities.get_mut(i)?.anim,
            Kind::Entity(r) => return self.view_of_mut(r)?.anim_mut(r),
            Kind::Actor => return Some(self.actor_mut(owner)?.anim(&path)),
            Kind::Item => &mut self.item_mut(owner)?.anim,
            Kind::Control => self.control_mut(owner)?.anim_slot_mut(slot?)?,
            _ => return None,
        };
        let anim = anim.get_or_insert_with(|| AnimState::new(&path));
        if anim.sprite != key(&path) {
            *anim = AnimState::new(&path);
        }
        Some(anim)
    }

    /// Restarts a sprite object's animation (`Reset`, `CBSprite::Reset`):
    /// an entity's `Reset` ends a `PlayAnim` too; a control image's
    /// starts over with the sprite's frame sounds stopped; the others go
    /// back to their first frame.
    fn reset_sprite_anim(&mut self, sprite: NativeId) {
        let Some((owner, _, _)) = self.sprite_home(sprite) else {
            return;
        };
        match self.native(owner).map(|n| n.kind.clone()) {
            Some(Kind::Entity(r)) => self.reset_entity_anim(r),
            Some(Kind::Control) => {
                if let Some(anim) = self
                    .sprite_slot(sprite)
                    .and_then(|slot| self.control_mut(owner)?.anim_slot_mut(slot))
                {
                    *anim = None;
                }
                self.kill_frame_sounds(owner);
            }
            _ => {
                if let Some(anim) = self.sprite_anim_mut(sprite) {
                    anim.reset();
                }
            }
        }
    }

    /// The handle of a frame of a sprite object, created on first request.
    fn frame_handle(&mut self, sprite: NativeId, index: usize) -> NativeId {
        let kind = Kind::Frame { sprite, index };
        if let Some(id) = self.objects.find(|k| *k == kind) {
            return id;
        }
        self.objects.insert(kind)
    }

    /// The handle of a sub-frame of a sprite object, created on first
    /// request.
    fn subframe_handle(&mut self, sprite: NativeId, frame: usize, index: usize) -> NativeId {
        let kind = Kind::SubFrame {
            sprite,
            frame,
            index,
        };
        if let Some(id) = self.objects.find(|k| *k == kind) {
            return id;
        }
        self.objects.insert(kind)
    }

    /// Keeps the frame and sub-frame handles of a sprite object pointing
    /// at their frames after a frame was inserted at `at` or deleted from
    /// there: the handles of the deleted frame become invalid, as the
    /// engine's pointers to a deleted frame do.
    fn shift_frame_handles(&mut self, sprite: NativeId, at: usize, inserted: bool) {
        let ids: Vec<NativeId> = self
            .objects
            .iter()
            .filter(|(_, n)| {
                matches!(
                    n.kind,
                    Kind::Frame { sprite: s, .. } | Kind::SubFrame { sprite: s, .. } if s == sprite
                )
            })
            .map(|(id, _)| id)
            .collect();
        let mut gone = Vec::new();
        for id in ids {
            let Some(native) = self.objects.get_mut(id) else {
                continue;
            };
            let index = match &mut native.kind {
                Kind::Frame { index, .. } => index,
                Kind::SubFrame { frame, .. } => frame,
                _ => continue,
            };
            if inserted {
                if *index >= at {
                    *index += 1;
                }
            } else if *index == at {
                gone.push(id);
            } else if *index > at {
                *index -= 1;
            }
        }
        for id in gone {
            self.objects.remove(id);
        }
    }

    /// Like [`World::shift_frame_handles`], for the sub-frames of a frame.
    fn shift_subframe_handles(
        &mut self,
        sprite: NativeId,
        frame: usize,
        at: usize,
        inserted: bool,
    ) {
        let ids: Vec<NativeId> = self
            .objects
            .iter()
            .filter(|(_, n)| {
                matches!(
                    n.kind,
                    Kind::SubFrame { sprite: s, frame: f, .. } if s == sprite && f == frame
                )
            })
            .map(|(id, _)| id)
            .collect();
        let mut gone = Vec::new();
        for id in ids {
            let Some(Kind::SubFrame { index, .. }) = self.objects.get_mut(id).map(|n| &mut n.kind)
            else {
                continue;
            };
            if inserted {
                if *index >= at {
                    *index += 1;
                }
            } else if *index == at {
                gone.push(id);
            } else if *index > at {
                *index -= 1;
            }
        }
        for id in gone {
            self.objects.remove(id);
        }
    }

    /// A new frame showing an image file, as `AddFrame` and `InsertFrame`
    /// build it (`CBSubFrame::SetSurface`): without a file, or with one
    /// that cannot be found, the frame has no sub-frame.
    fn frame_from_image(&mut self, file: Option<&str>) -> Frame {
        let mut frame = Frame::default();
        if let Some(file) = file {
            if self.vfs.exists(file) {
                frame.subframes.push(SubFrame::with_image(file));
            } else {
                self.log(format!("Error loading image '{file}'"));
            }
        }
        frame
    }

    // ----------------------------------------------------------------
    // Sprite
    // ----------------------------------------------------------------

    pub(crate) fn sprite_get(&self, id: NativeId, owner: NativeId, name: &str) -> Option<Value> {
        let path = self.sprite_home(id).map(|(_, _, path)| path);
        let def = self.sprite_def(id);
        let frames = def.map_or(0, |s| s.frames.len());
        let anim = self.sprite_anim(id);
        Some(match name {
            "Type" => Value::from("sprite"),
            "Filename" => path.map_or(Value::Null, Value::from),
            "NumFrames" => Value::Int(frames as i32),
            "CurrentFrame" => Value::Int(if frames == 0 {
                -1
            } else {
                anim.map_or(0, |a| a.current_frame.min(frames - 1) as i32)
            }),
            "PixelPerfect" => Value::Bool(def.is_some_and(|s| s.precise)),
            "Looping" => Value::Bool(
                anim.and_then(|a| a.looping)
                    .unwrap_or_else(|| def.is_some_and(|s| s.looping)),
            ),
            "Owner" => Value::Native(owner),
            "Finished" => Value::Bool(anim.is_some_and(|a| a.finished)),
            "Paused" => Value::Bool(anim.is_some_and(|a| a.paused)),
            _ => return None,
        })
    }

    pub(crate) fn sprite_set(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
        let (int, flag) = (value.get_int(self), value.get_bool(self));
        match name {
            "CurrentFrame" => {
                // Out of range is "no frame" in the engine (`-1`); Luma
                // clamps when it draws, so the frame count is checked here.
                let frames = self.sprite_def(id).map_or(0, |s| s.frames.len());
                let Some(anim) = self.sprite_anim_mut(id) else {
                    return true;
                };
                anim.current_frame = usize::try_from(int)
                    .ok()
                    .filter(|i| *i < frames)
                    .unwrap_or(0);
                anim.last_frame_time = 0;
                anim.finished = false;
            }
            "Looping" => {
                if let Some(anim) = self.sprite_anim_mut(id) {
                    anim.looping = Some(flag);
                }
            }
            "PixelPerfect" => {
                if let Some(sprite) = self.sprite_def_mut(id) {
                    sprite.precise = flag;
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn sprite_call(
        &mut self,
        id: NativeId,
        name: &str,
        args: &[Value],
        ctx: &Context<'_>,
    ) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        self.sprite_home(id)?;
        Some(match name {
            "Reset" => {
                self.reset_sprite_anim(id);
                Return::null()
            }
            "Pause" | "Play" => {
                let paused = name == "Pause";
                if let Some(anim) = self.sprite_anim_mut(id) {
                    anim.paused = paused;
                }
                Return::null()
            }
            "GetFrame" => {
                let index = arg(0).get_int(self);
                let frames = self.sprite_def(id).map_or(0, |s| s.frames.len());
                match usize::try_from(index).ok().filter(|i| *i < frames) {
                    Some(index) => Return::value(self.frame_handle(id, index)),
                    None => {
                        self.runtime_error(
                            ctx,
                            &format!("Sprite.GetFrame: Frame index {index} is out of range."),
                        );
                        Return::null()
                    }
                }
            }
            "DeleteFrame" => {
                let frames = self.sprite_def(id).map_or(0, |s| s.frames.len());
                let index = match arg(0) {
                    Value::Native(frame) => match self.native(frame).map(|n| n.kind.clone()) {
                        Some(Kind::Frame { sprite, index }) if sprite == id => Some(index),
                        _ => None,
                    },
                    v => {
                        let index = v.get_int(self);
                        let valid = usize::try_from(index).ok().filter(|i| *i < frames);
                        if valid.is_none() {
                            self.runtime_error(
                                ctx,
                                &format!(
                                    "Sprite.DeleteFrame: Frame index {index} is out of range."
                                ),
                            );
                        }
                        valid
                    }
                };
                if let Some(index) = index.filter(|i| *i < frames) {
                    let current = self.sprite_anim(id).map(|a| a.current_frame);
                    if let Some(sprite) = self.sprite_def_mut(id) {
                        sprite.frames.remove(index);
                    }
                    if current == Some(index) {
                        if let Some(anim) = self.sprite_anim_mut(id) {
                            anim.last_frame_time = 0;
                        }
                    }
                    self.shift_frame_handles(id, index, false);
                }
                Return::null()
            }
            "AddFrame" | "InsertFrame" => {
                let (at, file) = if name == "AddFrame" {
                    (None, arg(0))
                } else {
                    (Some(arg(0).get_int(self).max(0) as usize), arg(1))
                };
                let file = match file {
                    Value::Null => None,
                    v => Some(v.get_string(self)),
                };
                let frame = self.frame_from_image(file.as_deref());
                let Some(sprite) = self.sprite_def_mut(id) else {
                    return Some(Return::null());
                };
                let index = at.map_or(sprite.frames.len(), |at| at.min(sprite.frames.len()));
                sprite.frames.insert(index, frame);
                if index < sprite.frames.len() - 1 {
                    self.shift_frame_handles(id, index, true);
                }
                Return::value(self.frame_handle(id, index))
            }
            _ => return None,
        })
    }

    // ----------------------------------------------------------------
    // Frame
    // ----------------------------------------------------------------

    /// The sprite object and position of a frame handle.
    fn frame_at(&self, id: NativeId) -> Option<(NativeId, usize)> {
        match self.native(id)?.kind {
            Kind::Frame { sprite, index } => Some((sprite, index)),
            _ => None,
        }
    }

    /// The definition of a frame handle.
    fn frame_def(&self, id: NativeId) -> Option<&Frame> {
        let (sprite, index) = self.frame_at(id)?;
        self.sprite_def(sprite)?.frames.get(index)
    }

    /// The definition of a frame handle, for a script to change it.
    fn frame_def_mut(&mut self, id: NativeId) -> Option<&mut Frame> {
        let (sprite, index) = self.frame_at(id)?;
        self.sprite_def_mut(sprite)?.frames.get_mut(index)
    }

    pub(crate) fn frame_get(&self, id: NativeId, name: &str) -> Option<Value> {
        let frame = self.frame_def(id)?;
        Some(match name {
            "Type" => Value::from("frame"),
            "Delay" => Value::Int(frame.delay_ms.min(i32::MAX as u32) as i32),
            "Keyframe" => Value::Bool(frame.keyframe),
            "KillSounds" => Value::Bool(frame.kill_sound),
            "MoveX" => Value::Int(frame.move_x),
            "MoveY" => Value::Int(frame.move_y),
            "NumSubframes" => Value::Int(frame.subframes.len() as i32),
            "NumEvents" => Value::Int(frame.apply_events.len() as i32),
            // A frame with a single sub-frame answers for it.
            _ => match frame.subframes.as_slice() {
                [sub] => return subframe_value(sub, name),
                _ => return None,
            },
        })
    }

    pub(crate) fn frame_set(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
        let (int, flag) = (value.get_int(self), value.get_bool(self));
        let Some(frame) = self.frame_def_mut(id) else {
            return false;
        };
        match name {
            "Delay" => frame.delay_ms = int.max(0) as u32,
            "Keyframe" => frame.keyframe = flag,
            "KillSounds" => frame.kill_sound = flag,
            "MoveX" => frame.move_x = int,
            "MoveY" => frame.move_y = int,
            _ => {
                return match frame.subframes.as_mut_slice() {
                    [sub] => subframe_assign(sub, name, int, flag),
                    _ => false,
                };
            }
        }
        true
    }

    pub(crate) fn frame_call(
        &mut self,
        id: NativeId,
        name: &str,
        args: &[Value],
        ctx: &Context<'_>,
    ) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        let (sprite, at) = self.frame_at(id)?;
        let subframes = self.frame_def(id)?.subframes.len();
        Some(match name {
            "GetSound" => Return::value(
                self.frame_def(id)
                    .and_then(|f| f.sound.clone())
                    .map_or(Value::Null, Value::from),
            ),
            "SetSound" => {
                let file = match arg(0) {
                    Value::Null => None,
                    v => Some(v.get_string(self)),
                };
                // The engine drops the frame's sound first, then loads
                // the new one; a file that cannot be loaded leaves none.
                let loaded = match &file {
                    Some(file) => self.load_clip(file).is_some(),
                    None => true,
                };
                if let Some(frame) = self.frame_def_mut(id) {
                    frame.sound = if loaded { file } else { None };
                }
                Return::value(loaded)
            }
            "GetSubframe" => {
                let index = arg(0).get_int(self);
                match usize::try_from(index).ok().filter(|i| *i < subframes) {
                    Some(index) => Return::value(self.subframe_handle(sprite, at, index)),
                    None => {
                        self.runtime_error(
                            ctx,
                            &format!("Frame.GetSubframe: Subframe index {index} is out of range."),
                        );
                        Return::null()
                    }
                }
            }
            "DeleteSubframe" => {
                let index = match arg(0) {
                    Value::Native(sub) => match self.native(sub).map(|n| n.kind.clone()) {
                        Some(Kind::SubFrame {
                            sprite: s,
                            frame,
                            index,
                        }) if s == sprite && frame == at => Some(index),
                        _ => None,
                    },
                    v => {
                        let index = v.get_int(self);
                        let valid = usize::try_from(index).ok().filter(|i| *i < subframes);
                        if valid.is_none() {
                            self.runtime_error(
                                ctx,
                                &format!(
                                    "Frame.DeleteSubframe: Subframe index {index} is out of range."
                                ),
                            );
                        }
                        valid
                    }
                };
                if let Some(index) = index.filter(|i| *i < subframes) {
                    if let Some(frame) = self.frame_def_mut(id) {
                        frame.subframes.remove(index);
                    }
                    self.shift_subframe_handles(sprite, at, index, false);
                }
                Return::null()
            }
            "AddSubframe" | "InsertSubframe" => {
                let (position, file) = if name == "AddSubframe" {
                    (None, arg(0))
                } else {
                    (Some(arg(0).get_int(self).max(0) as usize), arg(1))
                };
                let file = match file {
                    Value::Null => None,
                    v => Some(v.get_string(self)),
                };
                // The engine adds the sub-frame whether or not its image
                // loads.
                let sub = match file {
                    Some(file) => {
                        if !self.vfs.exists(&file) {
                            self.log(format!("Error loading image '{file}'"));
                        }
                        SubFrame::with_image(file)
                    }
                    None => SubFrame::default(),
                };
                let Some(frame) = self.frame_def_mut(id) else {
                    return Some(Return::null());
                };
                let index =
                    position.map_or(frame.subframes.len(), |p| p.min(frame.subframes.len()));
                frame.subframes.insert(index, sub);
                if index < frame.subframes.len() - 1 {
                    self.shift_subframe_handles(sprite, at, index, true);
                }
                Return::value(self.subframe_handle(sprite, at, index))
            }
            "AddEvent" => {
                let event = arg(0).get_string(self);
                if let Some(frame) = self.frame_def_mut(id) {
                    if !frame
                        .apply_events
                        .iter()
                        .any(|e| e.eq_ignore_ascii_case(&event))
                    {
                        frame.apply_events.push(event);
                    }
                }
                Return::null()
            }
            "DeleteEvent" => {
                let event = arg(0).get_string(self);
                if let Some(frame) = self.frame_def_mut(id) {
                    if let Some(i) = frame
                        .apply_events
                        .iter()
                        .position(|e| e.eq_ignore_ascii_case(&event))
                    {
                        frame.apply_events.remove(i);
                    }
                }
                Return::null()
            }
            // A frame with a single sub-frame answers for it.
            _ if subframes == 1 => return self.subframe_call_at(sprite, at, 0, name, args),
            _ => return None,
        })
    }

    // ----------------------------------------------------------------
    // SubFrame
    // ----------------------------------------------------------------

    /// The sprite object and positions of a sub-frame handle.
    fn subframe_at(&self, id: NativeId) -> Option<(NativeId, usize, usize)> {
        match self.native(id)?.kind {
            Kind::SubFrame {
                sprite,
                frame,
                index,
            } => Some((sprite, frame, index)),
            _ => None,
        }
    }

    pub(crate) fn subframe_get(&self, id: NativeId, name: &str) -> Option<Value> {
        let (sprite, frame, index) = self.subframe_at(id)?;
        let sub = self
            .sprite_def(sprite)?
            .frames
            .get(frame)?
            .subframes
            .get(index)?;
        subframe_value(sub, name)
    }

    pub(crate) fn subframe_set(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
        let (int, flag) = (value.get_int(self), value.get_bool(self));
        let Some((sprite, frame, index)) = self.subframe_at(id) else {
            return false;
        };
        let Some(sub) = self
            .sprite_def_mut(sprite)
            .and_then(|s| s.frames.get_mut(frame))
            .and_then(|f| f.subframes.get_mut(index))
        else {
            return false;
        };
        subframe_assign(sub, name, int, flag)
    }

    pub(crate) fn subframe_call(
        &mut self,
        id: NativeId,
        name: &str,
        args: &[Value],
    ) -> Option<Return> {
        let (sprite, frame, index) = self.subframe_at(id)?;
        self.subframe_call_at(sprite, frame, index, name, args)
    }

    /// A method of the sub-frame at a position of a sprite object.
    fn subframe_call_at(
        &mut self,
        sprite: NativeId,
        frame: usize,
        index: usize,
        name: &str,
        args: &[Value],
    ) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        Some(match name {
            "GetImage" => Return::value(
                self.sprite_def(sprite)
                    .and_then(|s| s.frames.get(frame))
                    .and_then(|f| f.subframes.get(index))
                    .and_then(|sub| sub.image.clone())
                    .map_or(Value::Null, Value::from),
            ),
            "SetImage" => {
                let file = match arg(0) {
                    Value::Null => None,
                    v => Some(v.get_string(self)),
                };
                let ok = file.as_deref().is_none_or(|f| self.vfs.exists(f));
                if !ok {
                    let file = file.clone().unwrap_or_default();
                    self.log(format!("Error loading image '{file}'"));
                }
                let sub = self
                    .sprite_def_mut(sprite)
                    .and_then(|s| s.frames.get_mut(frame))
                    .and_then(|f| f.subframes.get_mut(index));
                if let Some(sub) = sub {
                    if ok {
                        // A new image shows whole (`SetDefaultRect`);
                        // `null` removes the image.
                        sub.image = file;
                        sub.rect = None;
                    }
                }
                Return::value(ok)
            }
            _ => return None,
        })
    }
}

/// A property of a sub-frame (`CBSubFrame::ScGetProperty`).
fn subframe_value(sub: &SubFrame, name: &str) -> Option<Value> {
    Some(match name {
        "Type" => Value::from("subframe"),
        "AlphaColor" => Value::Int(color_int(sub.tint)),
        "TransparentColor" => Value::Int(color_int(Rgba {
            r: sub.color_key.r,
            g: sub.color_key.g,
            b: sub.color_key.b,
            a: 255,
        })),
        "Is2DOnly" => Value::Bool(sub.only_2d),
        "Is3DOnly" => Value::Bool(sub.only_3d),
        "MirrorX" => Value::Bool(sub.mirror_x),
        "MirrorY" => Value::Bool(sub.mirror_y),
        "Decoration" => Value::Bool(sub.decoration),
        "HotspotX" => Value::Int(sub.hotspot_x),
        "HotspotY" => Value::Int(sub.hotspot_y),
        _ => return None,
    })
}

/// Sets a property of a sub-frame (`CBSubFrame::ScSetProperty`); `false`
/// when the sub-frame has no such property.
fn subframe_assign(sub: &mut SubFrame, name: &str, int: i32, flag: bool) -> bool {
    match name {
        "AlphaColor" => sub.tint = color_of(int),
        "Is2DOnly" => sub.only_2d = flag,
        "Is3DOnly" => sub.only_3d = flag,
        "MirrorX" => sub.mirror_x = flag,
        "MirrorY" => sub.mirror_y = flag,
        "Decoration" => sub.decoration = flag,
        "HotspotX" => sub.hotspot_x = int,
        "HotspotY" => sub.hotspot_y = int,
        _ => return false,
    }
    true
}
