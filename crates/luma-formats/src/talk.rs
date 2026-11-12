//! Talk definitions (`.talk`): the sprites an object shows while it
//! says one sentence, timed from the start of the sentence, for
//! lip-sync. The file sits next to the speech sound, with the same name.
//!
//! The format is documented in `docs/formats/talk.md` and was
//! established from the MIT-licensed Wintermute Engine 1.x sources
//! (`AdTalkDef.cpp`, `AdTalkNode.cpp`, `AdSentence.cpp`, Copyright (c)
//! 2011 Jan Nedoma). See `NOTICE` at the root of the repository.

use crate::actor::{SetFiles, SpriteSet};
use crate::def::{Document, Node, Nodes};
use crate::error::{Error, Result};
use crate::scene::Direction;

/// How many `TEMPLATE` files a talk definition may chain.
const TEMPLATE_DEPTH: u32 = 8;

/// How many `ACTION` nodes a talk definition may hold (with its
/// templates). The original has no limit; a file is untrusted input.
const MAX_NODES: usize = 4096;

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

    /// Reads an `ACTION` block (`CAdTalkNode::LoadBuffer`).
    fn read(node: &Node, files: &mut SetFiles<'_>) -> Result<TalkNode> {
        let mut action = TalkNode::default();
        for child in node.nodes() {
            match child.key.to_ascii_uppercase().as_str() {
                "SPRITE" => action.sprite_file = child.as_str().map(str::to_string),
                "SPRITESET_FILE" => action.set_file = child.as_str().map(str::to_string),
                "SPRITESET" => {
                    let mut set = SpriteSet::default();
                    set.read(child, files)?;
                    action.set = Some(set);
                }
                "START_TIME" => action.start_time = time_of(child),
                "END_TIME" => action.end_time = time_of(child),
                "PRECACHE" => action.precache = child.as_bool().unwrap_or(false),
                "COMMENT" | "EDITOR_PROPERTY" => {}
                other => {
                    return Err(Error::Invalid {
                        reason: format!(
                            "Syntax error in ACTION definition: unknown key {other} at line {}",
                            child.line
                        ),
                    });
                }
            }
        }
        action.play_to_end = action.end_time == 0;
        if action.precache {
            // Loaded with the file: a failure fails the definition.
            if let Some(file) = &action.sprite_file {
                if !(files.sprite_loads)(file) {
                    return Err(load_error("ACTION", file));
                }
                action.sprite_loaded = true;
            }
            if let Some(file) = &action.set_file {
                action.set = Some(load_set(file, files).ok_or_else(|| load_error("ACTION", file))?);
            }
        } else if let Some(file) = &action.sprite_file {
            // Loaded at first use (`LoadSprite`): the sprite first; the
            // set file only for a node without `SPRITE` and without an
            // inline set. A failure leaves the node without it.
            action.sprite_loaded = (files.sprite_loads)(file);
        } else if let (Some(file), None) = (&action.set_file, &action.set) {
            action.set = load_set(file, files);
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

    /// Parses a `.talk` file with the game's files, as `CAdTalkDef::
    /// LoadFile` does: a `TALK` root block; a `TEMPLATE` file is read
    /// where the key stands; a node, a set or a template that fails,
    /// and a key the block does not know, fail the definition; then the
    /// default sprite and the default set file are loaded, and fail it
    /// when they do not load. The inline `DEFAULT_SPRITESET` block is
    /// read (a bad one fails the file) and dropped, as in the original.
    pub fn parse_with(bytes: &[u8], files: &mut SetFiles<'_>) -> Result<TalkDef> {
        let mut def = TalkDef::default();
        def.read_file(bytes, files, 0)?;
        Ok(def)
    }

    fn read_file(&mut self, bytes: &[u8], files: &mut SetFiles<'_>, depth: u32) -> Result<()> {
        let doc = Document::parse(bytes)?;
        let root = doc.get("TALK").ok_or_else(|| Error::Invalid {
            reason: "'TALK' keyword expected".into(),
        })?;
        for child in root.nodes() {
            match child.key.to_ascii_uppercase().as_str() {
                "TEMPLATE" => {
                    let file = child.as_str().unwrap_or_default().to_string();
                    let bytes = if depth < TEMPLATE_DEPTH {
                        (files.load)(&file)
                    } else {
                        None
                    };
                    let bytes = bytes.ok_or_else(|| Error::Invalid {
                        reason: format!("CAdTalkDef::LoadFile failed for file '{file}'"),
                    })?;
                    self.read_file(&bytes, files, depth + 1)?;
                }
                "ACTION" => {
                    if self.nodes.len() >= MAX_NODES {
                        return Err(Error::Invalid {
                            reason: format!("more than {MAX_NODES} ACTION nodes in a TALK file"),
                        });
                    }
                    self.nodes.push(TalkNode::read(child, files)?);
                }
                "DEFAULT_SPRITE" => self.default_sprite = child.as_str().map(str::to_string),
                "DEFAULT_SPRITESET_FILE" => {
                    self.default_set_file = child.as_str().map(str::to_string);
                }
                "DEFAULT_SPRITESET" => {
                    SpriteSet::default().read(child, files)?;
                }
                "EDITOR_PROPERTY" => {}
                other => {
                    return Err(Error::Invalid {
                        reason: format!(
                            "Syntax error in TALK definition: unknown key {other} at line {}",
                            child.line
                        ),
                    });
                }
            }
        }
        self.default_set = None;
        if let Some(file) = &self.default_sprite {
            if !(files.sprite_loads)(file) {
                return Err(load_error("TALK", file));
            }
        }
        if let Some(file) = &self.default_set_file {
            self.default_set = Some(load_set(file, files).ok_or_else(|| load_error("TALK", file))?);
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

/// `START_TIME` and `END_TIME`: an unsigned number of milliseconds (a
/// negative value read by the original's `%d` into a `DWORD` is a huge
/// one).
fn time_of(node: &Node) -> u32 {
    node.as_int().unwrap_or(0) as i32 as u32
}

fn load_set(file: &str, files: &mut SetFiles<'_>) -> Option<SpriteSet> {
    let bytes = (files.load)(file)?;
    SpriteSet::parse_with(&bytes, files).ok()
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
        let mut load = |file: &str| {
            match file {
            "mouth.spriteset" => {
                Some(b"SPRITESET { LEFT=\"l.sprite\" RIGHT=\"r.sprite\" }".to_vec())
            }
            "base.talk" => Some(
                b"TALK { DEFAULT_SPRITE=\"base.sprite\" ACTION { START_TIME=5 END_TIME=6 SPRITE=\"b.sprite\" } }"
                    .to_vec(),
            ),
            _ => None,
        }
        };
        let mut sprite_loads = |file: &str| !file.starts_with("gone");
        TalkDef::parse_with(
            text.as_bytes(),
            &mut SetFiles {
                load: &mut load,
                sprite_loads: &mut sprite_loads,
                follow_templates: true,
            },
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
    fn what_fails_a_definition_and_what_does_not() {
        assert!(parse("ACTION { }").is_err());
        assert!(parse("TALK { COLOR=1 }").is_err());
        assert!(parse("TALK { ACTION { LOOP=TRUE } }").is_err());
        assert!(parse("TALK { TEMPLATE=\"none.talk\" }").is_err());
        assert!(parse("TALK { DEFAULT_SPRITE=\"gone.sprite\" }").is_err());
        assert!(parse("TALK { DEFAULT_SPRITESET_FILE=\"none.spriteset\" }").is_err());
        assert!(parse("TALK { DEFAULT_SPRITESET { UP=\"gone.sprite\" } }").is_err());
        assert!(parse("TALK { ACTION { SPRITESET { UP=\"gone.sprite\" } } }").is_err());
        assert!(parse("TALK { ACTION { PRECACHE=TRUE SPRITE=\"gone.sprite\" } }").is_err());
        assert!(
            parse("TALK { ACTION { PRECACHE=TRUE SPRITESET_FILE=\"none.spriteset\" } }").is_err()
        );
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
