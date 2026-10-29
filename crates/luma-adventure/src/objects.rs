//! The native objects scripts can hold: the game, the scene and its
//! parts, sprites, and the inert stand-ins for what is not implemented
//! yet. Handles are generational, so that a handle to an object of a
//! previous scene is simply invalid.

use luma_script::{NativeId, Properties};

use crate::scene_view::EntityRef;

/// What a native object is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// The `Game` object.
    Game,
    /// `Game.Keyboard`.
    Keyboard,
    /// The current scene (`Scene`, `Game.Scene`); the same object across
    /// scene changes, as in the engine.
    Scene,
    /// A layer of the current scene.
    Layer(usize),
    /// An entity of the current scene.
    Entity(EntityRef),
    /// A region node of the current scene's layer.
    Region {
        /// The layer's index.
        layer: usize,
        /// The node's index in the layer.
        index: usize,
    },
    /// The sprite object of an entity (`GetSpriteObject`).
    Sprite(NativeId),
    /// An actor (`Game.LoadActor`, `Scene.LoadActor`); its state lives in
    /// the world's actor list.
    Actor,
    /// The screen fader scripts wait for (`Game.FadeOut`).
    Fader,
    /// A stand-in that keeps the properties scripts set and answers every
    /// method with `null`: windows, actors and other objects of later
    /// milestones.
    Inert(String),
}

/// A native object: its kind and the properties scripts set on it.
#[derive(Debug, Clone, PartialEq)]
pub struct Native {
    /// What it is.
    pub kind: Kind,
    /// Script properties (`ScProp`).
    pub props: Properties,
}

/// The table of native objects.
#[derive(Debug, Default)]
pub struct Objects {
    slots: Vec<(u32, Option<Native>)>,
    free: Vec<u32>,
}

fn encode(index: u32, generation: u32) -> NativeId {
    NativeId((u64::from(generation) << 32) | u64::from(index))
}

fn decode(id: NativeId) -> (u32, u32) {
    ((id.0 & 0xffff_ffff) as u32, (id.0 >> 32) as u32)
}

impl Objects {
    /// An empty table.
    pub fn new() -> Objects {
        Objects::default()
    }

    /// Adds an object and returns its handle.
    pub fn insert(&mut self, kind: Kind) -> NativeId {
        let native = Native {
            kind,
            props: Properties::new(),
        };
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.0 = slot.0.wrapping_add(1);
            slot.1 = Some(native);
            return encode(index, slot.0);
        }
        self.slots.push((1, Some(native)));
        encode(self.slots.len() as u32 - 1, 1)
    }

    /// The object of a handle, if it still exists.
    pub fn get(&self, id: NativeId) -> Option<&Native> {
        let (index, generation) = decode(id);
        let slot = self.slots.get(index as usize)?;
        if slot.0 != generation {
            return None;
        }
        slot.1.as_ref()
    }

    /// The object of a handle, mutable.
    pub fn get_mut(&mut self, id: NativeId) -> Option<&mut Native> {
        let (index, generation) = decode(id);
        let slot = self.slots.get_mut(index as usize)?;
        if slot.0 != generation {
            return None;
        }
        slot.1.as_mut()
    }

    /// Whether the handle names a live object.
    pub fn contains(&self, id: NativeId) -> bool {
        self.get(id).is_some()
    }

    /// Removes an object; its handle becomes invalid.
    pub fn remove(&mut self, id: NativeId) -> Option<Native> {
        let (index, generation) = decode(id);
        let slot = self.slots.get_mut(index as usize)?;
        if slot.0 != generation {
            return None;
        }
        let native = slot.1.take()?;
        self.free.push(index);
        Some(native)
    }

    /// Every live object with its handle.
    pub fn iter(&self) -> impl Iterator<Item = (NativeId, &Native)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, (generation, native))| {
                native.as_ref().map(|n| (encode(i as u32, *generation), n))
            })
    }

    /// The handle of the first object matching the predicate.
    pub fn find(&self, pred: impl Fn(&Kind) -> bool) -> Option<NativeId> {
        self.iter().find(|(_, n)| pred(&n.kind)).map(|(id, _)| id)
    }

    /// The number of live objects.
    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.1.is_some()).count()
    }

    /// Whether no object is live.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_are_generational() {
        let mut objects = Objects::new();
        let a = objects.insert(Kind::Game);
        let b = objects.insert(Kind::Inert("Window".to_string()));
        assert_ne!(a, b);
        assert_eq!(objects.get(a).unwrap().kind, Kind::Game);
        assert_eq!(objects.len(), 2);
        assert!(objects.remove(b).is_some());
        assert!(!objects.contains(b));
        assert!(objects.remove(b).is_none());
        let c = objects.insert(Kind::Scene);
        assert_ne!(b, c, "the slot is reused with a new generation");
        assert!(objects.get(b).is_none());
        assert_eq!(objects.get(c).unwrap().kind, Kind::Scene);
        assert_eq!(objects.find(|k| *k == Kind::Scene), Some(c));
        assert_eq!(objects.iter().count(), 2);
    }
}
