//! The state of a scene's objects between visits (`PERSISTENT_STATE`):
//! what the engine remembers of a scene's named nodes when the player
//! leaves it, and puts back when the scene loads again.
//!
//! Written with reference to `AdSceneState.cpp`, `AdNodeState.cpp`,
//! `AdScene.cpp` (`PersistState`) and `AdGame.cpp` (`GetSceneState`,
//! `ChangeScene`) of Wintermute Engine 1.x (MIT License, Copyright (c)
//! 2003-2013 Jan Nedoma); see `NOTICE`. The behaviour is documented in
//! `docs/game-loop.md`.

use std::collections::BTreeMap;

use luma_formats::{Entity, Rgba, SceneNode};

use crate::scene_view::EntityRef;
use crate::world::World;

/// What is remembered of one named node of a scene (`CAdNodeState`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NodeState {
    /// `Active`.
    pub active: bool,
    /// The caption, for an entity.
    pub caption: Option<String>,
    /// The sprite shown when the player left; `Some("")` when the scene
    /// keeps no sprites (`PERSISTENT_STATE_SPRITES=FALSE`); `None` for
    /// regions and for hotspot entities (a `REGION` without sprite).
    pub sprite: Option<String>,
    /// The cursor, when the entity has one.
    pub cursor: Option<String>,
    /// `AlphaColor`, `None` when never set.
    pub alpha: Option<Rgba>,
}

impl NodeState {
    /// Notes an entity's state (`CAdNodeState::TransferEntity`, saving).
    fn note(&mut self, e: &Entity, including_sprites: bool) {
        self.caption = Some(e.caption.clone());
        if e.region.is_none() {
            if let Some(sprite) = &e.sprite {
                self.sprite = Some(if including_sprites {
                    sprite.clone()
                } else {
                    String::new()
                });
            }
        }
        if let Some(cursor) = &e.cursor {
            self.cursor = Some(cursor.clone());
        }
        self.alpha = e.tint;
        self.active = e.active;
    }

    /// Puts the state back on an entity (`TransferEntity`, loading) and
    /// returns the sprite to switch to, when it differs from the
    /// entity's.
    fn apply(&self, e: &mut Entity, including_sprites: bool) -> Option<String> {
        if let Some(caption) = &self.caption {
            e.caption = caption.clone();
        }
        let sprite = match &self.sprite {
            Some(s)
                if including_sprites
                    && !s.is_empty()
                    && e.region.is_none()
                    && !e
                        .sprite
                        .as_deref()
                        .is_some_and(|current| current.eq_ignore_ascii_case(s)) =>
            {
                Some(s.clone())
            }
            _ => None,
        };
        if let Some(cursor) = &self.cursor {
            if !e
                .cursor
                .as_deref()
                .is_some_and(|current| current.eq_ignore_ascii_case(cursor))
            {
                e.cursor = Some(cursor.clone());
            }
        }
        e.active = self.active;
        e.tint = self.alpha;
        sprite
    }
}

/// The remembered states of a scene's nodes (`CAdSceneState`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SceneState {
    /// The nodes' states, keyed by their lowercased name (the engine
    /// looks nodes up case-insensitively).
    pub nodes: BTreeMap<String, NodeState>,
}

/// The key a scene file is remembered under: backslashes, as the engine
/// writes paths, and no case (`CAdGame::GetSceneState`).
pub fn state_key(filename: &str) -> String {
    filename.replace('/', "\\").to_ascii_lowercase()
}

impl World {
    /// Notes the current scene's state before it unloads
    /// (`CAdScene::SaveState`), when the scene is persistent: the
    /// entities marked `SAVE_STATE` (the default) and the regions, of
    /// the layers and among the free entities.
    pub(crate) fn save_scene_state(&mut self) {
        let Some(view) = &self.scene else {
            return;
        };
        let scene = &view.scene;
        if !scene.persistent_state {
            return;
        }
        let including = scene.persistent_state_sprites;
        let state = self.scene_states.entry(state_key(&view.path)).or_default();
        let nodes = scene.layers.iter().flat_map(|l| l.nodes.iter());
        for node in nodes {
            match node {
                SceneNode::Entity(e) if e.save_state => {
                    state
                        .nodes
                        .entry(e.name.to_ascii_lowercase())
                        .or_default()
                        .note(e, including);
                }
                SceneNode::Region(r) => {
                    state
                        .nodes
                        .entry(r.name.to_ascii_lowercase())
                        .or_default()
                        .active = r.active;
                }
                SceneNode::Entity(_) => {}
            }
        }
        for e in scene.free_entities.iter().filter(|e| e.save_state) {
            state
                .nodes
                .entry(e.name.to_ascii_lowercase())
                .or_default()
                .note(e, including);
        }
    }

    /// Puts the remembered state back on the scene just loaded
    /// (`CAdScene::LoadState`), before its scripts run. A sprite that
    /// cannot be loaded is logged and the entity keeps the file's.
    pub(crate) fn restore_scene_state(&mut self) {
        let Some(view) = self.scene.as_mut() else {
            return;
        };
        if !view.scene.persistent_state {
            return;
        }
        let Some(state) = self.scene_states.get(&state_key(&view.path)) else {
            return;
        };
        let including = view.scene.persistent_state_sprites;
        let mut switches: Vec<(EntityRef, String)> = Vec::new();
        for r in view.entity_refs() {
            let Some(e) = view.entity_mut(r) else {
                continue;
            };
            if !e.save_state {
                continue;
            }
            if let Some(node) = state.nodes.get(&e.name.to_ascii_lowercase()) {
                if let Some(sprite) = node.apply(e, including) {
                    switches.push((r, sprite));
                }
            }
        }
        for layer in &mut view.scene.layers {
            for node in &mut layer.nodes {
                if let SceneNode::Region(r) = node {
                    if let Some(node) = state.nodes.get(&r.name.to_ascii_lowercase()) {
                        r.active = node.active;
                    }
                }
            }
        }
        let mut failed = Vec::new();
        for (r, sprite) in switches {
            if view.add_sprite(&self.vfs, &sprite) {
                if let Some(e) = view.entity_mut(r) {
                    e.sprite = Some(sprite);
                }
                view.reset_anim(r);
            } else {
                failed.push(sprite);
            }
        }
        for sprite in failed {
            self.log(format!("SetSprite method failed for file '{sprite}'"));
        }
    }

    /// The states remembered so far, by scene file key.
    pub fn scene_states(&self) -> &BTreeMap<String, SceneState> {
        &self.scene_states
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_fold_case_and_slashes() {
        assert_eq!(
            state_key("scenes/Hall/Hall.scene"),
            "scenes\\hall\\hall.scene"
        );
    }

    #[test]
    fn a_state_moves_between_entities() {
        let mut from = Entity {
            name: "door".into(),
            caption: "Door".into(),
            sprite: Some("art\\open.sprite".into()),
            active: false,
            ..Default::default()
        };
        from.tint = Some(Rgba {
            r: 1,
            g: 2,
            b: 3,
            a: 4,
        });
        let mut state = NodeState::default();
        state.note(&from, true);
        let mut to = Entity {
            name: "door".into(),
            sprite: Some("art\\closed.sprite".into()),
            active: true,
            ..Default::default()
        };
        assert_eq!(
            state.apply(&mut to, true),
            Some("art\\open.sprite".to_string())
        );
        assert!(!to.active);
        assert_eq!(to.caption, "Door");
        assert_eq!(to.tint, from.tint);
        // Without sprites, the sprite is noted empty and never applied.
        let mut bare = NodeState::default();
        bare.note(&from, false);
        assert_eq!(bare.sprite.as_deref(), Some(""));
        assert_eq!(bare.apply(&mut to, false), None);
    }
}
