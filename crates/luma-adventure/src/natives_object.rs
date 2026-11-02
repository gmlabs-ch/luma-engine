//! The members every object shares (`CBObject`, `CAdObject`): its own
//! cursor, its caption slots, and the flags scripts set on it
//! (`Movable`, `SceneIndependent`, `NonIntMouseEvents`).
//!
//! Written with reference to `BObject.cpp` and `AdObject.cpp` of
//! Wintermute Engine 1.x (MIT License, Copyright (c) 2003-2013 Jan
//! Nedoma); see `NOTICE`.

use luma_script::{Engine, NativeId, Return, Value};

use crate::objects::{Kind, ObjectState};
use crate::world::World;

impl World {
    /// The runtime state of an object.
    pub fn object_state(&self, id: NativeId) -> Option<&ObjectState> {
        self.objects.get(id).map(|n| &n.state)
    }

    /// The runtime state of an object, mutable.
    pub(crate) fn object_state_mut(&mut self, id: NativeId) -> Option<&mut ObjectState> {
        self.objects.get_mut(id).map(|n| &mut n.state)
    }

    /// The object's own cursor (`CURSOR` of entities, actors and
    /// controls, `SetCursor`).
    pub fn object_cursor(&self, id: NativeId) -> Option<String> {
        match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Actor) => self.actor(id).and_then(|a| a.def.cursor.clone()),
            Some(Kind::Entity(r)) => self.entity_def(*r).and_then(|e| e.cursor.clone()),
            Some(Kind::Control) => self.control(id).and_then(|c| c.cursor.clone()),
            _ => None,
        }
    }

    /// Sets the object's own cursor (`SetCursor`, `RemoveCursor` with
    /// `None`); `false` when the object has no cursor or the sprite does
    /// not load.
    pub fn set_object_cursor(&mut self, id: NativeId, path: Option<&str>) -> bool {
        if let Some(path) = path {
            if !self.load_cursor(path) {
                return false;
            }
        }
        let value = path.map(str::to_string);
        match self.objects.get(id).map(|n| n.kind.clone()) {
            Some(Kind::Actor) => match self.actor_mut(id) {
                Some(actor) => actor.def.cursor = value,
                None => return false,
            },
            Some(Kind::Entity(r)) => match self.entity_def_mut(r) {
                Some(entity) => entity.cursor = value,
                None => return false,
            },
            Some(Kind::Control) => match self.control_mut(id) {
                Some(control) => control.cursor = value,
                None => return false,
            },
            _ => return false,
        }
        true
    }

    /// The cursor of an object as a sprite object (`GetCursorObject`),
    /// created on first request; `None` without a cursor.
    pub(crate) fn cursor_sprite_object(&mut self, owner: NativeId) -> Option<NativeId> {
        self.object_cursor(owner)?;
        if let Some(id) = self.objects.find(|k| *k == Kind::CursorSprite(owner)) {
            return Some(id);
        }
        Some(self.objects.insert(Kind::CursorSprite(owner)))
    }

    /// Removes the sprite objects that stand for parts of an object
    /// (`GetSpriteObject`, `GetCursorObject`), once the object is gone.
    pub(crate) fn forget_owned(&mut self, owner: NativeId) {
        let owned: Vec<NativeId> = self
            .objects
            .iter()
            .filter(|(_, n)| {
                matches!(
                    n.kind,
                    Kind::Sprite(o) | Kind::CursorSprite(o) | Kind::EntityRegion(o) if o == owner
                )
            })
            .map(|(id, _)| id)
            .collect();
        for id in owned {
            self.objects.remove(id);
        }
    }

    /// A caption slot of an object (`CBObject::GetCaption`): slot 1 (or
    /// 0) is the caption, slots 2 to 7 are the extra ones `SetCaption`
    /// fills; anything else, or an empty slot, is `""`.
    pub fn caption_of(&self, id: NativeId, slot: i32) -> String {
        let slot = if slot == 0 { 1 } else { slot };
        match slot {
            1 => self.main_caption(id).unwrap_or_default(),
            2..=7 => self
                .object_state(id)
                .and_then(|s| s.captions.get(&(slot as u8)).cloned())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// The object's caption (slot 1), when its kind has one.
    fn main_caption(&self, id: NativeId) -> Option<String> {
        match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Game) => Some(self.caption.clone()),
            Some(Kind::Actor) => self.actor(id).map(|a| a.def.caption.clone()),
            Some(Kind::Entity(r)) => self.entity_def(*r).map(|e| e.caption.clone()),
            Some(Kind::Region { layer, index }) => self
                .scene()
                .and_then(|v| v.region(*layer, *index))
                .map(|r| r.caption.clone()),
            Some(Kind::Layer(i)) => self
                .scene()
                .and_then(|v| v.scene.layers.get(*i))
                .map(|l| l.caption.clone()),
            Some(Kind::Control) => self.control(id).map(|c| c.caption.clone()),
            Some(Kind::Item) => self.item(id).map(|i| i.def.caption.clone()),
            _ => None,
        }
    }

    /// Sets a caption slot (`CBObject::SetCaption`), expanding the text
    /// through the string table; slots outside 1 to 7 are ignored.
    pub fn set_caption(&mut self, id: NativeId, text: &str, slot: i32) {
        let slot = if slot == 0 { 1 } else { slot };
        let text = self.expand(text);
        match slot {
            1 => match self.objects.get(id).map(|n| n.kind.clone()) {
                Some(Kind::Game) => self.caption = text,
                Some(Kind::Actor) => {
                    if let Some(actor) = self.actor_mut(id) {
                        actor.def.caption = text;
                    }
                }
                Some(Kind::Entity(r)) => {
                    if let Some(entity) = self.entity_def_mut(r) {
                        entity.caption = text;
                    }
                }
                Some(Kind::Region { layer, index }) => {
                    if let Some(region) = self.scene_mut().and_then(|v| v.region_mut(layer, index))
                    {
                        region.caption = text;
                    }
                }
                Some(Kind::Layer(i)) => {
                    if let Some(layer) = self.scene_mut().and_then(|v| v.scene.layers.get_mut(i)) {
                        layer.caption = text;
                    }
                }
                Some(Kind::Control) => {
                    if let Some(control) = self.control_mut(id) {
                        control.caption = text;
                    }
                }
                Some(Kind::Item) => {
                    if let Some(item) = self.item_mut(id) {
                        item.def.caption = text;
                    }
                }
                _ => {}
            },
            2..=7 => {
                if let Some(state) = self.object_state_mut(id) {
                    state.captions.insert(slot as u8, text);
                }
            }
            _ => {}
        }
    }

    /// The flag properties every object has.
    pub(crate) fn object_get(&self, id: NativeId, name: &str) -> Option<Value> {
        let state = self.object_state(id)?;
        Some(match name {
            "Movable" => Value::Bool(state.movable),
            "SceneIndependent" => Value::Bool(state.scene_independent),
            "NonIntMouseEvents" => Value::Bool(state.non_int_mouse_events),
            "NumAttachments"
                if matches!(
                    self.objects.get(id).map(|n| &n.kind),
                    Some(Kind::Actor | Kind::Entity(_))
                ) =>
            {
                Value::Int(state.attachments.len() as i32)
            }
            _ => return None,
        })
    }

    /// Sets a flag property; `false` when the name is not one.
    pub(crate) fn object_set(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
        let flag = value.get_bool(self);
        let Some(state) = self.object_state_mut(id) else {
            return false;
        };
        match name {
            "Movable" => state.movable = flag,
            "SceneIndependent" => state.scene_independent = flag,
            "NonIntMouseEvents" => state.non_int_mouse_events = flag,
            _ => return false,
        }
        true
    }

    /// The methods every object has: its cursor and its captions
    /// (`CBObject::ScCallMethod`), and those of every adventure object
    /// (`CAdObject`): the forced talk animation, the region it sticks
    /// to, its attachments.
    pub(crate) fn object_call(
        &mut self,
        id: NativeId,
        name: &str,
        args: &[Value],
        engine: &mut Engine,
    ) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        Some(match name {
            "SetCursor" => {
                let path = arg(0).get_string(self);
                Return::value(self.set_object_cursor(id, Some(&path)))
            }
            "RemoveCursor" => {
                self.set_object_cursor(id, None);
                Return::null()
            }
            "GetCursor" => Return::value(self.object_cursor(id).map_or(Value::Null, Value::from)),
            "GetCursorObject" => Return::value(
                self.cursor_sprite_object(id)
                    .map_or(Value::Null, Value::Native),
            ),
            "HasCursor" => Return::value(self.object_cursor(id).is_some()),
            "SetCaption" => {
                let text = arg(0).get_string(self);
                let slot = arg(1).get_int(self);
                self.set_caption(id, &text, slot);
                Return::null()
            }
            "Caption" => Return::value(self.caption_of(id, arg(0).get_int(self))),
            "StickToRegion"
                if matches!(
                    self.objects.get(id).map(|n| &n.kind),
                    Some(Kind::Actor | Kind::Entity(_))
                ) =>
            {
                Return::value(self.stick_to_region(id, &arg(0)))
            }
            "AddAttachment" | "RemoveAttachment" | "GetAttachment"
                if matches!(
                    self.objects.get(id).map(|n| &n.kind),
                    Some(Kind::Actor | Kind::Entity(_))
                ) =>
            {
                match name {
                    "AddAttachment" => {
                        let path = arg(0).get_string(self);
                        let pre = match arg(1) {
                            Value::Null => true,
                            v => v.get_bool(self),
                        };
                        let (x, y) = (arg(2).get_int(self), arg(3).get_int(self));
                        Return::value(self.add_attachment(engine, id, &path, pre, x, y))
                    }
                    "RemoveAttachment" => {
                        Return::value(self.remove_attachment(engine, id, &arg(0)))
                    }
                    _ => Return::value(
                        self.attachment(id, &arg(0))
                            .map_or(Value::Null, Value::Native),
                    ),
                }
            }
            "ForceTalkAnim" => {
                // The sprite plays as the first stance of the next sentence
                // (`CAdObject::ScCallMethod`).
                let path = arg(0).get_string(self);
                match self.object_state_mut(id) {
                    Some(state) => {
                        state.forced_talk_anim = Some(path);
                        state.forced_talk_used = false;
                        Return::value(true)
                    }
                    None => Return::value(false),
                }
            }
            _ => return None,
        })
    }
}
