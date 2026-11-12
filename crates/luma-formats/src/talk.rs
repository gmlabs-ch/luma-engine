//! Talk definitions (`.talk`): the sprites an object shows while it
//! says one sentence, timed from the start of the sentence, for
//! lip-sync. The file sits next to the speech sound, with the same name.
//!
//! The format is documented in `docs/formats/talk.md` and was
//! established from the MIT-licensed Wintermute Engine 1.x sources
//! (`AdTalkDef.cpp`, `AdTalkNode.cpp`, `AdSentence.cpp`, Copyright (c)
//! 2011 Jan Nedoma). See `NOTICE` at the root of the repository.

use crate::actor::SpriteSet;
use crate::def::{Document, Node, keys};
use crate::error::{Error, Result};
use crate::scene::Direction;
use crate::template::Files;

/// How many `ACTION` nodes a talk definition may hold (with its
/// templates). The original has no limit; a file is untrusted input.
const MAX_NODES: usize = 4096;

/// Whether a sprite file loads (`CBSprite::LoadFile`).
pub type SpriteLoads<'a> = dyn FnMut(&str) -> bool + 'a;

/// The keys of `CAdTalkDef::LoadBuffer`, in the order of its table.
const TALK_KEYS: &[&str] = &[
    "TALK",
    "TEMPLATE",
    "ACTION",
    "DEFAULT_SPRITESET_FILE",
    "DEFAULT_SPRITESET",
    "DEFAULT_SPRITE",
    "EDITOR_PROPERTY",
];

/// The keys of `CAdTalkNode::LoadBuffer`, in the order of its table.
const ACTION_KEYS: &[&str] = &[
    "ACTION",
    "SPRITESET_FILE",
    "SPRITESET",
    "SPRITE",
    "START_TIME",
    "END_TIME",
    "COMMENT",
    "PRECACHE",
    "EDITOR_PROPERTY",
];

/// One timed action of a talk definition (`ACTION`, `CAdTalkNode`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TalkNode {
    /// `SPRITE`: the sprite file of the node; it wins over the set.
    pub sprite_file: Option<String>,
    /// Whether `sprite_file` loads. A sprite that does not load is tried
    /// again on every frame by the original, so the node never gets it.
    pub sprite_loaded: bool,
    /// `SPRITESET_FILE`: the file of the node's sprite set.
    pub set_file: Option<String>,
    /// The node's sprite set: the inline `SPRITESET` block, or the set of
    /// `set_file` when the original would have loaded it.
    pub set: Option<SpriteSet>,
    /// `START_TIME`: milliseconds from the start of the sentence.
    pub start_time: u32,
    /// `END_TIME`: the last millisecond of the node; 0 with
    /// `play_to_end`.
    pub end_time: u32,
    /// Whether the node lasts until its sprite has played to its end
    /// (no `END_TIME`, or 0).
    pub play_to_end: bool,
    /// `PRECACHE`: the sprite and the set file are loaded with the
    /// definition, and fail it when they do not load.
    pub precache: bool,
}

impl TalkNode {
    /// The sprite of the node for a direction (`CAdTalkNode::
    /// GetSprite`): its own sprite, else its set's.
    pub fn sprite(&self, direction: Direction) -> Option<&str> {
        match &self.sprite_file {
            Some(file) if self.sprite_loaded => Some(file),
            _ => self.set.as_ref().and_then(|set| set.sprite(direction)),
        }
    }

    /// Whether the node is active `time` milliseconds into the sentence
    /// (`CAdTalkNode::IsInTimeInterval`). A timed node is active from
    /// its start to its end, both included. A play-to-end node is
    /// active from its start while its sprite has not `finished`
    /// playing; one whose sprite or set file does not load stays active
    /// (and has no sprite), as the original keeps trying to load it.
    pub fn in_time_interval(
        &self,
        time: u32,
        direction: Direction,
        finished: &dyn Fn(&str) -> bool,
    ) -> bool {
        if time < self.start_time {
            return false;
        }
        if !self.play_to_end {
            return self.end_time >= time;
        }
        let own_pending = match &self.sprite_file {
            Some(file) => !self.sprite_loaded || !finished(file),
            None => false,
        };
        if own_pending {
            return true;
        }
        match &self.set {
            None => self.set_file.is_some(),
            Some(set) => set.sprite(direction).is_some_and(|s| !finished(s)),
        }
    }

    /// Reads an `ACTION` block (`CAdTalkNode::LoadBuffer`). The node
    /// fails on a key that is not in the table and, with `PRECACHE`, on
    /// a sprite or a set file that does not load; an inline `SPRITESET`
    /// that fails only leaves the node without it.
    fn read(
        node: &Node,
        files: &mut Files<'_>,
        sprite_loads: &mut SpriteLoads<'_>,
    ) -> Result<TalkNode> {
        let mut action = TalkNode::default();
        let (mut start, mut end) = (0i32, 0i32);
        for item in keys(node, ACTION_KEYS, "ACTION") {
            let (key, child) = item?;
            match key {
                "SPRITE" => action.sprite_file = Some(child.text()),
                "SPRITESET_FILE" => action.set_file = Some(child.text()),
                "SPRITESET" => {
                    let mut set = SpriteSet::default();
                    action.set = set.apply_with(child, files).is_ok().then_some(set);
                }
                "START_TIME" => child.read_int(&mut start),
                "END_TIME" => child.read_int(&mut end),
                "PRECACHE" => child.read_bool(&mut action.precache),
                _ => {}
            }
        }
        // The times are unsigned in the original: a negative value read
        // by its `%d` is a huge one.
        action.start_time = start as u32;
        action.end_time = end as u32;
        action.play_to_end = action.end_time == 0;
        if action.precache {
            // Loaded with the file: a failure fails the node.
            if let Some(file) = &action.sprite_file {
                if !sprite_loads(file) {
                    return Err(load_error("ACTION", file));
                }
                action.sprite_loaded = true;
            }
            if let Some(file) = &action.set_file {
                action.set = Some(SpriteSet::load_named(file, files)?);
            }
        } else if let Some(file) = &action.sprite_file {
            // Loaded at first use (`LoadSprite`): the sprite first; the
            // set file only for a node without `SPRITE` and without an
            // inline set. A failure leaves the node without it.
            action.sprite_loaded = sprite_loads(file);
        } else if let (Some(file), None) = (&action.set_file, &action.set) {
            action.set = SpriteSet::load_named(file, files).ok();
        }
        Ok(action)
    }
}

/// A talk definition (`TALK`, `CAdTalkDef`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TalkDef {
    /// `DEFAULT_SPRITE`: the sprite shown when no node is active; it
    /// wins over the default set.
    pub default_sprite: Option<String>,
    /// `DEFAULT_SPRITESET_FILE`: the file of the default sprite set.
    pub default_set_file: Option<String>,
    /// The set of `default_set_file`.
    pub default_set: Option<SpriteSet>,
    /// The `ACTION` nodes, in file order.
    pub nodes: Vec<TalkNode>,
}

impl TalkDef {
    /// The name of the talk definition of a speech sound: the same
    /// path with the extension `.talk` (`CAdSentence::SetupTalkFile`).
    pub fn file_for_sound(sound: &str) -> String {
        let name_start = sound.rfind(['\\', '/', ':']).map_or(0, |i| i + 1);
        let stem_end = sound[name_start..]
            .rfind('.')
            .map_or(sound.len(), |dot| name_start + dot);
        format!("{}.talk", &sound[..stem_end])
    }

    /// Parses a `.talk` file as `CAdTalkDef::LoadFile` does, reading
    /// the files it names through `files`: a `TALK` root block, its keys
    /// in order. A `TEMPLATE` file is read into the same definition
    /// where the key stands. A template or an `ACTION` that fails is
    /// skipped (the original's error code is overwritten by the next
    /// statement); a key that is not in the table is a syntax error and
    /// fails the definition. At the end the default sprite and the
    /// default set file are loaded, and fail the definition when they do
    /// not load. The inline `DEFAULT_SPRITESET` block is dropped, as in
    /// the original.
    pub fn parse_with(
        bytes: &[u8],
        files: &mut Files<'_>,
        sprite_loads: &mut SpriteLoads<'_>,
    ) -> Result<TalkDef> {
        let mut def = TalkDef::default();
        def.read_file(&Document::parse(bytes)?, files, sprite_loads)?;
        Ok(def)
    }

    fn read_file(
        &mut self,
        doc: &Document,
        files: &mut Files<'_>,
        sprite_loads: &mut SpriteLoads<'_>,
    ) -> Result<()> {
        let root = doc.root("TALK").ok_or_else(|| Error::Invalid {
            reason: "'TALK' keyword expected".into(),
        })?;
        for item in keys(root, TALK_KEYS, "TALK") {
            let (key, child) = item?;
            match key {
                "TEMPLATE" => {
                    let name = child.text();
                    let _ = files
                        .template(&name, |doc, files| self.read_file(doc, files, sprite_loads));
                }
                "ACTION" => {
                    if let Ok(node) = TalkNode::read(child, files, sprite_loads) {
                        if self.nodes.len() < MAX_NODES {
                            self.nodes.push(node);
                        }
                    }
                }
                "DEFAULT_SPRITE" => self.default_sprite = Some(child.text()),
                "DEFAULT_SPRITESET_FILE" => self.default_set_file = Some(child.text()),
                // "DEFAULT_SPRITESET": deleted again below by the original.
                _ => {}
            }
        }
        self.default_set = None;
        if let Some(file) = &self.default_sprite {
            if !sprite_loads(file) {
                return Err(load_error("TALK", file));
            }
        }
        if let Some(file) = &self.default_set_file {
            self.default_set = Some(SpriteSet::load_named(file, files)?);
        }
        Ok(())
    }

    /// The sprite shown when no node is active (`CAdTalkDef::
    /// GetDefaultSprite`): the default sprite, else the default set's
    /// sprite for the direction.
    pub fn default_sprite(&self, direction: Direction) -> Option<&str> {
        match &self.default_sprite {
            Some(file) => Some(file),
            None => self
                .default_set
                .as_ref()
                .and_then(|set| set.sprite(direction)),
        }
    }

    /// Every sprite file the definition can show, without duplicates.
    pub fn sprite_paths(&self) -> Vec<&str> {
        let mut paths: Vec<&str> = Vec::new();
        let own = self.default_sprite.as_deref().into_iter().chain(
            self.nodes
                .iter()
                .filter(|n| n.sprite_loaded)
                .filter_map(|n| n.sprite_file.as_deref()),
        );
        let sets = self
            .default_set
            .iter()
            .chain(self.nodes.iter().filter_map(|n| n.set.as_ref()));
        for path in own.chain(sets.flat_map(SpriteSet::paths)) {
            if !paths.iter().any(|p| p.eq_ignore_ascii_case(path)) {
                paths.push(path);
            }
        }
        paths
    }
}

fn load_error(block: &str, file: &str) -> Error {
    Error::Invalid {
        reason: format!("Error loading {block} definition: '{file}' does not load"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<TalkDef> {
        let mut read = |file: &str| {
            match file {
            "mouth.spriteset" => {
                Some(b"SPRITESET { LEFT=\"l.sprite\" RIGHT=\"r.sprite\" }".to_vec())
            }
            "base.talk" => Some(
                b"TALK { DEFAULT_SPRITE=\"base.sprite\" ACTION { START_TIME=5 END_TIME=6 SPRITE=\"b.sprite\" } }"
                    .to_vec(),
            ),
            "bad.talk" => Some(
                b"TALK { ACTION { START_TIME=7 SPRITE=\"c.sprite\" } DEFAULT_SPRITE=\"gone.sprite\" }"
                    .to_vec(),
            ),
            _ => None,
        }
        };
        let mut sprite_loads = |file: &str| !file.starts_with("gone");
        TalkDef::parse_with(
            text.as_bytes(),
            &mut Files::new(&mut read),
            &mut sprite_loads,
        )
    }

    #[test]
    fn names_the_talk_file_of_a_sound() {
        assert_eq!(
            TalkDef::file_for_sound("speech\\en\\line01.ogg"),
            "speech\\en\\line01.talk"
        );
        assert_eq!(TalkDef::file_for_sound("line"), "line.talk");
        assert_eq!(TalkDef::file_for_sound("a.b/line"), "a.b/line.talk");
        assert_eq!(TalkDef::file_for_sound("été.wav"), "été.talk");
    }

    #[test]
    fn parses_a_talk_definition() {
        let def = parse(
            "TALK {
               DEFAULT_SPRITESET_FILE=\"mouth.spriteset\"
               DEFAULT_SPRITESET { DOWN=\"dropped.sprite\" }
               ACTION { COMMENT=\"a\" START_TIME=100 END_TIME=400 SPRITE=\"a.sprite\" }
               ACTION { START_TIME=500 SPRITESET_FILE=\"mouth.spriteset\" }
               ACTION { START_TIME=900 END_TIME=0 SPRITESET { UP=\"u.sprite\" } PRECACHE=TRUE }
             }",
        )
        .unwrap();
        assert_eq!(def.nodes.len(), 3);
        // The inline default set is dropped; the file's is the default.
        assert_eq!(def.default_sprite(Direction::Left), Some("l.sprite"));
        // Down has no sprite: the nearer of its neighbours, the lower on a tie.
        assert_eq!(def.default_sprite(Direction::Down), Some("r.sprite"));
        let a = &def.nodes[0];
        assert!(!a.play_to_end && a.sprite_loaded);
        assert_eq!(a.sprite(Direction::Up), Some("a.sprite"));
        let never = |_: &str| false;
        let always = |_: &str| true;
        assert!(!a.in_time_interval(99, Direction::Down, &never));
        assert!(a.in_time_interval(100, Direction::Down, &always));
        assert!(a.in_time_interval(400, Direction::Down, &always));
        assert!(!a.in_time_interval(401, Direction::Down, &never));
        // No end time: until the sprite of the direction has played.
        let b = &def.nodes[1];
        assert!(b.play_to_end);
        assert_eq!(b.sprite(Direction::Right), Some("r.sprite"));
        assert!(!b.in_time_interval(499, Direction::Right, &never));
        assert!(b.in_time_interval(100_000, Direction::Right, &never));
        assert!(!b.in_time_interval(500, Direction::Right, &|s| s == "r.sprite"));
        assert!(b.in_time_interval(500, Direction::Left, &|s| s == "r.sprite"));
        assert!(def.nodes[2].play_to_end && def.nodes[2].precache);
        assert_eq!(
            def.sprite_paths(),
            ["a.sprite", "r.sprite", "l.sprite", "u.sprite"]
        );
    }

    #[test]
    fn a_template_is_read_in_place_and_the_default_sprite_wins() {
        let def = parse(
            "TALK { ACTION { START_TIME=1 SPRITE=\"a.sprite\" } TEMPLATE=\"base.talk\" DEFAULT_SPRITESET_FILE=\"mouth.spriteset\" }",
        )
        .unwrap();
        assert_eq!(def.nodes.len(), 2);
        assert_eq!(def.nodes[1].sprite_file.as_deref(), Some("b.sprite"));
        assert_eq!(def.default_sprite(Direction::Left), Some("base.sprite"));
        assert!(def.default_set.is_some());
    }

    #[test]
    fn what_fails_a_definition_and_what_is_only_skipped() {
        // The root block, a key the block does not know, and the
        // defaults, which are loaded with the definition.
        assert!(parse("ACTION { }").is_err());
        assert!(parse("TALK { COLOR=1 }").is_err());
        assert!(parse("TALK { DEFAULT_SPRITE=\"gone.sprite\" }").is_err());
        assert!(parse("TALK { DEFAULT_SPRITESET_FILE=\"none.spriteset\" }").is_err());
        // A template that cannot be read is skipped. One that fails at
        // its end has been applied by then: its nodes and the default it
        // named stay, and that default fails this file at its own end.
        let def = parse("TALK { TEMPLATE=\"none.talk\" ACTION { SPRITE=\"a.sprite\" } }").unwrap();
        assert_eq!(def.nodes.len(), 1);
        assert!(parse("TALK { TEMPLATE=\"bad.talk\" }").is_err());
        let def = parse("TALK { TEMPLATE=\"bad.talk\" DEFAULT_SPRITE=\"ok.sprite\" }").unwrap();
        assert_eq!(def.nodes.len(), 1);
        assert_eq!(def.default_sprite(Direction::Up), Some("ok.sprite"));
        // A node that fails is left out and the reading goes on: a key
        // it does not know, or with `PRECACHE` a file that does not load.
        let def = parse(
            "TALK {
               ACTION { LOOP=TRUE }
               ACTION { PRECACHE=TRUE SPRITE=\"gone.sprite\" }
               ACTION { PRECACHE=TRUE SPRITESET_FILE=\"none.spriteset\" }
               ACTION { START_TIME=3 SPRITESET { SPEED=1 } }
               DEFAULT_SPRITESET { SPEED=1 }
             }",
        )
        .unwrap();
        assert_eq!(def.nodes.len(), 1);
        assert_eq!(def.nodes[0].start_time, 3);
        assert_eq!(def.nodes[0].set, None, "a bad inline set is dropped");
        // Without `PRECACHE` a sprite or a set that does not load only
        // leaves the node without it: it stays active and shows nothing.
        let def = parse(
            "TALK { ACTION { SPRITE=\"gone.sprite\" SPRITESET_FILE=\"mouth.spriteset\" } ACTION { SPRITESET_FILE=\"none.spriteset\" } }",
        )
        .unwrap();
        let never = |_: &str| false;
        let always = |_: &str| true;
        let a = &def.nodes[0];
        assert!(!a.sprite_loaded);
        assert_eq!(a.set, None, "the set file is not tried after a sprite");
        assert_eq!(a.sprite(Direction::Left), None);
        assert!(a.in_time_interval(0, Direction::Left, &always));
        let b = &def.nodes[1];
        assert_eq!(b.sprite(Direction::Left), None);
        assert!(b.in_time_interval(0, Direction::Left, &never));
    }
}
