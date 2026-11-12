//! Actors (`.actor`): characters that walk, with a sprite per direction
//! for each of their states.
//!
//! The format is documented in `docs/formats/actor.md` and was
//! established from the MIT-licensed WME Lite sources (`AdActor.cpp`,
//! `AdSpriteSet.cpp`, Copyright (c) 2011 Jan Nedoma). See `NOTICE` at the
//! root of the repository.

use crate::def::{Document, Node, Nodes, document_keys, keys};
use crate::error::{Error, Result};
use crate::image::{BlendMode, Rgba};
use crate::scene::{Direction, Property, Region, WaypointGroup};
use crate::template::Files;

/// A sprite for each direction (`SPRITESET`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpriteSet {
    /// Name of the set, for talk stances and animations.
    pub name: String,
    /// A `.spriteset` file loaded first (`TEMPLATE`).
    pub template: Option<String>,
    /// The sprite of each direction, indexed by [`Direction::code`].
    pub sprites: [Option<String>; 8],
}

impl SpriteSet {
    /// The sprite of a direction, or of the nearest defined direction by
    /// index (`CAdSpriteSet::GetSprite`): the closer of the first
    /// defined index below and the first above, the lower one on a tie;
    /// no wrap-around.
    pub fn sprite(&self, direction: Direction) -> Option<&str> {
        let wanted = usize::from(direction.code());
        let below = (0..=wanted)
            .rev()
            .find_map(|i| self.sprites[i].as_deref().map(|s| (wanted - i, s)));
        let above = (wanted..8).find_map(|i| self.sprites[i].as_deref().map(|s| (i - wanted, s)));
        match (below, above) {
            (Some((db, b)), Some((da, a))) => Some(if db > da { a } else { b }),
            (Some((_, b)), None) => Some(b),
            (None, Some((_, a))) => Some(a),
            (None, None) => None,
        }
    }

    /// Whether any direction has a sprite.
    pub fn is_empty(&self) -> bool {
        self.sprites.iter().all(Option::is_none)
    }

    /// Every sprite path of the set, in direction order, without
    /// duplicates.
    pub fn paths(&self) -> Vec<&str> {
        let mut paths: Vec<&str> = Vec::new();
        for sprite in self.sprites.iter().flatten() {
            if !paths.iter().any(|p| p.eq_ignore_ascii_case(sprite)) {
                paths.push(sprite);
            }
        }
        paths
    }

    /// Parses a `.spriteset` file: a `SPRITESET` root block. No other
    /// file is read.
    pub fn parse(bytes: &[u8]) -> Result<SpriteSet> {
        SpriteSet::parse_with(bytes, &mut Files::none())
    }

    /// Parses a `.spriteset` file, reading the `TEMPLATE` files it names
    /// through `files`.
    pub fn parse_with(bytes: &[u8], files: &mut Files<'_>) -> Result<SpriteSet> {
        let doc = Document::parse(bytes)?;
        let mut set = SpriteSet::default();
        set.load_file(&doc, files)?;
        Ok(set)
    }

    /// `CAdSpriteSet::LoadBuffer(.., Complete = true)`: the first
    /// statement of `doc` must be the `SPRITESET` block.
    fn load_file(&mut self, doc: &Document, files: &mut Files<'_>) -> Result<()> {
        let root = doc.root("SPRITESET").ok_or_else(|| Error::Invalid {
            reason: "'SPRITESET' keyword expected".into(),
        })?;
        self.apply_with(root, files)
    }

    /// Reads the keys of a `SPRITESET` block into the set, overriding
    /// what is already there. No `TEMPLATE` file is read.
    pub fn apply(&mut self, node: &Node) -> Result<()> {
        self.apply_with(node, &mut Files::none())
    }

    /// `CAdSpriteSet::LoadBuffer`: reads the keys of a `SPRITESET` block
    /// into the set in order, overriding what is already there. A
    /// `TEMPLATE` loads the named `.spriteset` file into the same set
    /// where the key stands. A key that is not in the table is a syntax
    /// error.
    pub fn apply_with(&mut self, node: &Node, files: &mut Files<'_>) -> Result<()> {
        for item in keys(node, SPRITESET_KEYS, "SPRITESET") {
            let (key, child) = item?;
            let direction = match key {
                "NAME" => {
                    self.name = child.text();
                    continue;
                }
                "TEMPLATE" => {
                    let name = child.text();
                    let _ = files.template(&name, |doc, files| self.load_file(doc, files));
                    self.template = Some(name);
                    continue;
                }
                "UP" => Direction::Up,
                "UP_RIGHT" => Direction::UpRight,
                "RIGHT" => Direction::Right,
                "DOWN_RIGHT" => Direction::DownRight,
                "DOWN" => Direction::Down,
                "DOWN_LEFT" => Direction::DownLeft,
                "LEFT" => Direction::Left,
                "UP_LEFT" => Direction::UpLeft,
                _ => continue,
            };
            self.sprites[usize::from(direction.code())] = Some(child.text());
        }
        Ok(())
    }

    /// Builds a set from a block holding one `SPRITESET` block (`WALK`,
    /// `STAND`, `TALK`...): the text of the block is loaded as a
    /// `.spriteset` file is.
    fn from_wrapper(node: &Node, files: &mut Files<'_>) -> Result<SpriteSet> {
        let inner = node
            .nodes()
            .first()
            .filter(|first| first.key.eq_ignore_ascii_case("SPRITESET"))
            .ok_or_else(|| Error::Invalid {
                reason: format!(
                    "'SPRITESET' keyword expected in {} at line {}",
                    node.key, node.line
                ),
            })?;
        let mut set = SpriteSet::default();
        set.apply_with(inner, files)?;
        Ok(set)
    }
}

/// The keys of `CAdSpriteSet::LoadBuffer`, in the order of its table.
const SPRITESET_KEYS: &[&str] = &[
    "SPRITESET",
    "NAME",
    "UP_LEFT",
    "DOWN_LEFT",
    "LEFT",
    "UP_RIGHT",
    "DOWN_RIGHT",
    "RIGHT",
    "UP",
    "DOWN",
    "TEMPLATE",
    "EDITOR_PROPERTY",
];

/// The keys of `CAdActor::LoadBuffer`, in the order of its table.
const ACTOR_KEYS: &[&str] = &[
    "ACTOR",
    "X",
    "Y",
    "TEMPLATE",
    "NAME",
    "SCALABLE",
    "REGISTRABLE",
    "INTERACTIVE",
    "SHADOWABLE",
    "COLORABLE",
    "ACTIVE",
    "WALK",
    "STAND",
    "TALK_SPECIAL",
    "TALK",
    "TURN_LEFT",
    "TURN_RIGHT",
    "EVENTS",
    "FONT",
    "CURSOR",
    "SCRIPT",
    "SOUND_VOLUME",
    "SOUND_PANNING",
    "CAPTION",
    "PROPERTY",
    "BLOCKED_REGION",
    "WAYPOINTS",
    "IGNORE_ITEMS",
    "ROTABLE",
    "ROTATABLE",
    "ALPHA_COLOR",
    "SCALE",
    "RELATIVE_SCALE",
    "ALPHA",
    "EDITOR_PROPERTY",
    "ANIMATION",
];

/// An actor definition (`ACTOR`).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Actor {
    /// Name used by scripts.
    pub name: String,
    /// Caption shown when the mouse is over the actor.
    pub caption: String,
    /// The last `TEMPLATE` file named; it was applied where the key
    /// stands.
    pub template: Option<String>,
    /// Starting position.
    pub x: i32,
    /// See `x`.
    pub y: i32,
    /// Drawn, updated and clickable.
    pub active: bool,
    /// Follows the scene's scale.
    pub scalable: bool,
    /// Fixed scale in percent, overriding the scene's.
    pub scale: Option<f32>,
    /// Added to the scene's scale, in percent.
    pub relative_scale: f32,
    /// Horizontal scale in percent set by a script (`ScaleX`); with
    /// `scale_y`, it replaces `scale` and the scene's.
    pub scale_x: Option<f32>,
    /// See `scale_x`.
    pub scale_y: Option<f32>,
    /// Follows the scene's rotation levels.
    pub rotatable: bool,
    /// Fixed rotation in degrees set by a script (`Rotate`), overriding
    /// the scene's; only applies when rotatable.
    pub rotate: Option<f32>,
    /// Added to the scene's rotation, in degrees (`RelativeRotate`).
    pub relative_rotate: f32,
    /// How the sprites are blended (`BlendMode`).
    pub blend_mode: BlendMode,
    /// Reacts to the mouse.
    pub interactive: bool,
    /// Takes the tint of the region it stands in.
    pub colorable: bool,
    /// Fixed tint and opacity, overriding the scene's.
    pub tint: Option<Rgba>,
    /// Not clickable while an inventory item is selected.
    pub ignore_items: bool,
    /// Stop on a block instead of looking for another path. Not a key of
    /// the file: scripts set it.
    pub stop_on_blocked: bool,
    /// The walking sprites.
    pub walk: Option<SpriteSet>,
    /// The standing sprites.
    pub stand: Option<SpriteSet>,
    /// The sprites shown while turning left.
    pub turn_left: Option<SpriteSet>,
    /// The sprites shown while turning right.
    pub turn_right: Option<SpriteSet>,
    /// The talking sprite sets.
    pub talk: Vec<SpriteSet>,
    /// Talking sprite sets only used by name.
    pub talk_special: Vec<SpriteSet>,
    /// Named animations (`ANIMATION`).
    pub animations: Vec<SpriteSet>,
    /// Font of the actor's speech.
    pub font: Option<String>,
    /// Cursor sprite shown over the actor.
    pub cursor: Option<String>,
    /// Scripts attached to the actor.
    pub scripts: Vec<String>,
    /// Script properties.
    pub properties: Vec<Property>,
    /// Polygon, relative to the position, that blocks other actors.
    pub blocked_region: Option<Region>,
    /// Waypoints, relative to the position, for other actors.
    pub waypoints: Option<WaypointGroup>,
    /// Volume of the actor's sounds, in percent.
    pub sound_volume: i32,
    /// Pan the actor's sounds with its position on screen.
    pub sound_panning: bool,
}

impl Default for Actor {
    fn default() -> Self {
        Actor {
            name: crate::UNNAMED.to_string(),
            caption: String::new(),
            template: None,
            x: 0,
            y: 0,
            active: true,
            scalable: true,
            scale: None,
            relative_scale: 0.0,
            scale_x: None,
            scale_y: None,
            rotatable: false,
            rotate: None,
            relative_rotate: 0.0,
            blend_mode: BlendMode::Normal,
            interactive: true,
            colorable: true,
            tint: None,
            ignore_items: false,
            stop_on_blocked: false,
            walk: None,
            stand: None,
            turn_left: None,
            turn_right: None,
            talk: Vec::new(),
            talk_special: Vec::new(),
            animations: Vec::new(),
            font: None,
            cursor: None,
            scripts: Vec::new(),
            properties: Vec::new(),
            blocked_region: None,
            waypoints: None,
            sound_volume: 100,
            sound_panning: true,
        }
    }
}

impl Actor {
    /// Parses an `.actor` file: an `ACTOR` root block. No other file is
    /// read: a `TEMPLATE` key fails to load, which the original ignores.
    pub fn parse(bytes: &[u8]) -> Result<Actor> {
        Actor::parse_with(bytes, &mut Files::none())
    }

    /// Parses an `.actor` file, reading the `TEMPLATE` files it names
    /// through `files`.
    pub fn parse_with(bytes: &[u8], files: &mut Files<'_>) -> Result<Actor> {
        let mut actor = Actor::default();
        actor.merge_with(bytes, files)?;
        Ok(actor)
    }

    /// Reads an `.actor` file over the actor: its keys override what is
    /// already there. No `TEMPLATE` file is read.
    pub fn merge(&mut self, bytes: &[u8]) -> Result<()> {
        self.merge_with(bytes, &mut Files::none())
    }

    /// Reads an `.actor` file over the actor (`CAdActor::LoadBuffer` with
    /// `Complete`), reading the `TEMPLATE` files it names through `files`.
    pub fn merge_with(&mut self, bytes: &[u8], files: &mut Files<'_>) -> Result<()> {
        self.load_file(&Document::parse(bytes)?, files)
    }

    fn load_file(&mut self, doc: &Document, files: &mut Files<'_>) -> Result<()> {
        let root = doc.root("ACTOR").ok_or_else(|| Error::Invalid {
            reason: "'ACTOR' keyword expected".into(),
        })?;
        self.apply(root, files)
    }

    /// Adds the `ANIMATION` blocks of a file (`CAdActor::MergeAnims`):
    /// the statements are read up to the first one that is not an
    /// `ANIMATION`. A block that cannot be read is left out, the others
    /// are added, and the error is returned.
    pub fn merge_animations(&mut self, bytes: &[u8]) -> Result<usize> {
        let doc = Document::parse(bytes)?;
        let mut count = 0;
        let mut failed = None;
        for item in document_keys(&doc, &["ANIMATION"], "ANIMATION") {
            let Ok((_, node)) = item else { break };
            let mut set = SpriteSet::default();
            match set.apply(node) {
                Ok(()) => {
                    self.animations.push(set);
                    count += 1;
                }
                Err(err) => failed = Some(err),
            }
        }
        match failed {
            Some(err) => Err(err),
            None => Ok(count),
        }
    }

    /// The animation with this name, ignoring case.
    pub fn animation(&self, name: &str) -> Option<&SpriteSet> {
        self.animations
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
    }

    /// Every sprite file the actor refers to, without duplicates.
    pub fn sprite_paths(&self) -> Vec<&str> {
        let mut paths: Vec<&str> = Vec::new();
        let sets = self
            .walk
            .iter()
            .chain(&self.stand)
            .chain(&self.turn_left)
            .chain(&self.turn_right)
            .chain(&self.talk)
            .chain(&self.talk_special)
            .chain(&self.animations);
        for path in sets
            .flat_map(SpriteSet::paths)
            .chain(self.cursor.as_deref())
        {
            if !paths.iter().any(|p| p.eq_ignore_ascii_case(path)) {
                paths.push(path);
            }
        }
        paths
    }

    /// `CAdActor::LoadBuffer`: the keys of the block in order, over what
    /// the actor holds. A key that is not in the table is a syntax error
    /// that fails the load. A sprite set, region or waypoint group that
    /// cannot be read is left out alone, and so is a `TEMPLATE` that
    /// cannot be loaded: the original sets an error code that the next
    /// statement overwrites.
    fn apply(&mut self, root: &Node, files: &mut Files<'_>) -> Result<()> {
        let (mut r, mut g, mut b, mut alpha) = (0, 0, 0, 0);
        // Whether this block names a tint of its own.
        let mut tinted = false;
        for item in keys(root, ACTOR_KEYS, "ACTOR") {
            let (key, child) = item?;
            match key {
                "TEMPLATE" => {
                    // `LoadFile` on the actor itself. (The original then
                    // answers the template's name as the actor's file;
                    // the actor keeps its own here.)
                    let name = child.text();
                    let _ = files.template(&name, |doc, files| self.load_file(doc, files));
                    self.template = Some(name);
                }
                "NAME" => self.name = child.text(),
                "CAPTION" => self.caption = child.text(),
                "X" => child.read_int(&mut self.x),
                "Y" => child.read_int(&mut self.y),
                "ACTIVE" => child.read_bool(&mut self.active),
                "SCALABLE" => child.read_bool(&mut self.scalable),
                "SCALE" => {
                    let mut scale = 0;
                    child.read_int(&mut scale);
                    self.scale = Some(scale as f32);
                }
                "RELATIVE_SCALE" => {
                    let mut scale = 0;
                    child.read_int(&mut scale);
                    self.relative_scale = scale as f32;
                }
                "ROTATABLE" | "ROTABLE" => child.read_bool(&mut self.rotatable),
                "INTERACTIVE" | "REGISTRABLE" => child.read_bool(&mut self.interactive),
                "COLORABLE" | "SHADOWABLE" => child.read_bool(&mut self.colorable),
                "ALPHA_COLOR" => {
                    child.read_ints(&mut [&mut r, &mut g, &mut b]);
                    tinted = true;
                }
                "ALPHA" => {
                    child.read_int(&mut alpha);
                    tinted = true;
                }
                "IGNORE_ITEMS" => child.read_bool(&mut self.ignore_items),
                "WALK" => self.walk = SpriteSet::from_wrapper(child, files).ok(),
                "STAND" => self.stand = SpriteSet::from_wrapper(child, files).ok(),
                "TURN_LEFT" => self.turn_left = SpriteSet::from_wrapper(child, files).ok(),
                "TURN_RIGHT" => self.turn_right = SpriteSet::from_wrapper(child, files).ok(),
                "TALK" => self.talk.extend(SpriteSet::from_wrapper(child, files).ok()),
                "TALK_SPECIAL" => self
                    .talk_special
                    .extend(SpriteSet::from_wrapper(child, files).ok()),
                "ANIMATION" => {
                    let mut set = SpriteSet::default();
                    if set.apply_with(child, files).is_ok() {
                        self.animations.push(set);
                    }
                }
                "FONT" => self.font = Some(child.text()),
                "CURSOR" => self.cursor = Some(child.text()),
                "SCRIPT" => self.scripts.push(child.text()),
                "PROPERTY" => self.properties.extend(Property::from_node(child)),
                "BLOCKED_REGION" => self.blocked_region = Region::from_node(child, false).ok(),
                "WAYPOINTS" => {
                    let mut group = WaypointGroup::default();
                    self.waypoints = group.load(child, files).is_ok().then_some(group);
                }
                "SOUND_VOLUME" => child.read_int(&mut self.sound_volume),
                "SOUND_PANNING" => child.read_bool(&mut self.sound_panning),
                _ => {}
            }
        }
        // A file that names no tint keeps the one the actor has: a
        // template's. (The original computes it again from this file
        // alone and loses the template's.)
        if tinted {
            if alpha != 0 && r == 0 && g == 0 && b == 0 {
                (r, g, b) = (255, 255, 255);
            }
            let tint = Rgba::from_ints(r, g, b, alpha);
            self.tint = (tint != Rgba::from_ints(0, 0, 0, 0)).then_some(tint);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIRL: &[u8] = br#"; actor
ACTOR {
  NAME = "theGirl"
  CAPTION=""
  SCALABLE = TRUE
  INTERACTIVE = FALSE
  X = 400
  Y = 460
  SCRIPT="actors\thegirl\theGirl.script"
  FONT = "fonts\outline_red.font"
  ALPHA = 200
  WALK {
    SPRITESET {
      LEFT       = "actors\thegirl\ll\walk.sprite"
      RIGHT      = "actors\thegirl\rr\walk.sprite"
      UP         = "actors\thegirl\uu\walk.sprite"
      DOWN       = "actors\thegirl\dd\walk.sprite"
      UP_LEFT    = "actors\thegirl\ll\walk.sprite"
      UP_RIGHT   = "actors\thegirl\rr\walk.sprite"
      DOWN_LEFT  = "actors\thegirl\ll\walk.sprite"
      DOWN_RIGHT = "actors\thegirl\rr\walk.sprite"
    }
  }
  STAND {
    SPRITESET {
      LEFT       = "actors\thegirl\ll\stand.sprite"
      RIGHT      = "actors\thegirl\rr\stand.sprite"
    }
  }
  TALK {
    SPRITESET {
      NAME = "talk1"
      LEFT       = "actors\thegirl\ll\talk1.sprite"
    }
  }
  TALK {
    SPRITESET {
      NAME = "talk2"
      LEFT       = "actors\thegirl\ll\talk2.sprite"
    }
  }
  ANIMATION {
    NAME = "wave"
    LEFT = "actors\thegirl\ll\wave.sprite"
  }
  BLOCKED_REGION {
    POINT {-10,-5}
    POINT {10,-5}
    POINT {10,5}
    POINT {-10,5}
  }
  WAYPOINTS {
    POINT {0,-20}
  }
  PROPERTY { NAME="Mood" VALUE="calm" }
}
"#;

    #[test]
    fn parses_an_actor() {
        let actor = Actor::parse(GIRL).unwrap();
        assert_eq!(actor.name, "theGirl");
        assert_eq!((actor.x, actor.y), (400, 460));
        assert!(actor.scalable && !actor.interactive && actor.active);
        assert!(!actor.stop_on_blocked);
        assert_eq!(actor.scripts, ["actors\\thegirl\\theGirl.script"]);
        assert_eq!(actor.font.as_deref(), Some("fonts\\outline_red.font"));
        assert_eq!(
            actor.tint,
            Some(Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 200
            })
        );
        let walk = actor.walk.as_ref().unwrap();
        assert_eq!(
            walk.sprite(Direction::DownLeft),
            Some("actors\\thegirl\\ll\\walk.sprite")
        );
        assert_eq!(walk.paths().len(), 4);
        let stand = actor.stand.as_ref().unwrap();
        assert_eq!(
            stand.sprite(Direction::Left),
            Some("actors\\thegirl\\ll\\stand.sprite")
        );
        assert!(actor.turn_left.is_none());
        assert_eq!(actor.talk.len(), 2);
        assert_eq!(actor.talk[1].name, "talk2");
        assert_eq!(actor.animation("WAVE").unwrap().name, "wave");
        assert!(actor.animation("nope").is_none());
        assert_eq!(actor.blocked_region.as_ref().unwrap().points.len(), 4);
        assert_eq!(actor.waypoints.as_ref().unwrap().points.len(), 1);
        assert_eq!(actor.properties[0].name, "Mood");
        assert_eq!(actor.sprite_paths().len(), 4 + 2 + 2 + 1);
    }

    #[test]
    fn nearest_direction_rule() {
        let mut set = SpriteSet::default();
        set.sprites[usize::from(Direction::Up.code())] = Some("up".into());
        set.sprites[usize::from(Direction::Left.code())] = Some("left".into());
        // 7 (up-left): 6 is one step below, nothing above: left.
        assert_eq!(set.sprite(Direction::UpLeft), Some("left"));
        // 3 (down-right): 0 is three below, 6 three above: the lower wins.
        assert_eq!(set.sprite(Direction::DownRight), Some("up"));
        // 4 (down): 0 is four below, 6 two above: left.
        assert_eq!(set.sprite(Direction::Down), Some("left"));
        assert_eq!(set.sprite(Direction::Up), Some("up"));
        assert!(SpriteSet::default().sprite(Direction::Up).is_none());
        assert!(SpriteSet::default().is_empty());
    }

    #[test]
    fn templates_merge_and_animations_load() {
        let mut actor = Actor::parse(GIRL).unwrap();
        actor
            .merge(b"ACTOR { TEMPLATE=\"base.actor\" NAME=\"other\" X=1 }")
            .unwrap();
        assert_eq!(actor.name, "other");
        assert_eq!((actor.x, actor.y), (1, 460));
        assert_eq!(actor.template.as_deref(), Some("base.actor"));
        assert!(actor.walk.is_some(), "the template's sets stay");
        let added = actor
            .merge_animations(
                b"ANIMATION { NAME=\"jump\" UP=\"j.sprite\" } ANIMATION { NAME=\"sit\" }",
            )
            .unwrap();
        assert_eq!(added, 2);
        assert_eq!(actor.animations.len(), 3);
        assert_eq!(
            actor.animation("jump").unwrap().sprite(Direction::Down),
            Some("j.sprite")
        );
        let set = SpriteSet::parse(b"SPRITESET { NAME=\"s\" DOWN=\"d.sprite\" }").unwrap();
        assert_eq!(set.name, "s");
        assert_eq!(set.sprite(Direction::Up), Some("d.sprite"));
    }

    /// `CAdActor::LoadBuffer TEMPLATE`: the template is loaded into the
    /// actor where the key stands; its tint stays when the outer file
    /// names none.
    #[test]
    fn a_template_is_loaded_in_place() {
        let base = br#"ACTOR { NAME="base" X=10 Y=20 ALPHA=100 SCALABLE=FALSE
  WALK { SPRITESET { LEFT="base_walk.sprite" } }
  TALK { SPRITESET { NAME="t" LEFT="base_talk.sprite" } }
}"#;
        let mut read = |name: &str| (name == "base.actor").then(|| base.to_vec());
        let actor = Actor::parse_with(
            br#"ACTOR { X=1 CAPTION="Mine" TEMPLATE="base.actor" Y=99
  STAND { SPRITESET { TEMPLATE="base.spriteset" RIGHT="r.sprite" } }
}"#,
            &mut Files::new(&mut read),
        )
        .unwrap();
        assert_eq!(actor.name, "base");
        assert_eq!((actor.x, actor.y), (10, 99));
        assert_eq!(actor.caption, "Mine");
        assert!(!actor.scalable);
        // The file names no tint: the template's stays.
        assert_eq!(actor.tint, Some(Rgba::from_ints(255, 255, 255, 100)));
        assert_eq!(actor.talk.len(), 1);
        assert_eq!(
            actor.walk.as_ref().unwrap().sprite(Direction::Left),
            Some("base_walk.sprite")
        );
        // The set's template is missing: skipped, the set goes on.
        assert_eq!(
            actor.stand.as_ref().unwrap().sprite(Direction::Right),
            Some("r.sprite")
        );
    }

    /// `CAdSpriteSet::LoadBuffer TEMPLATE`, and `MergeAnims` stopping at
    /// the first statement that is not an `ANIMATION`.
    #[test]
    fn sprite_set_templates_and_merged_animations() {
        let mut read = |name: &str| {
            (name == "base.spriteset")
                .then(|| br#"SPRITESET { NAME="base" LEFT="l.sprite" UP="u.sprite" }"#.to_vec())
        };
        let set = SpriteSet::parse_with(
            br#"SPRITESET { LEFT="mine.sprite" TEMPLATE="base.spriteset" UP="mine_up.sprite" }"#,
            &mut Files::new(&mut read),
        )
        .unwrap();
        assert_eq!(set.name, "base");
        assert_eq!(set.sprite(Direction::Left), Some("l.sprite"));
        assert_eq!(set.sprite(Direction::Up), Some("mine_up.sprite"));
        assert!(SpriteSet::parse(b"SPRITESET { BOGUS=1 }").is_err());

        let mut actor = Actor::default();
        let added = actor
            .merge_animations(b"ANIMATION { NAME=\"a\" } OTHER { } ANIMATION { NAME=\"b\" }")
            .unwrap();
        assert_eq!(added, 1);
        // A block that cannot be read is an error, the others are added.
        assert!(
            actor
                .merge_animations(b"ANIMATION { BOGUS=1 } ANIMATION { NAME=\"c\" }")
                .is_err()
        );
        assert!(actor.animation("c").is_some());
    }

    #[test]
    fn rejects_bad_files() {
        assert!(Actor::parse(b"ENTITY { }").is_err());
        // A set that cannot be read (no `SPRITESET` block) is left out.
        let actor = Actor::parse(b"ACTOR { WALK { LEFT=\"x\" } NAME=\"n\" }").unwrap();
        assert!(actor.walk.is_none());
        assert_eq!(actor.name, "n");
        // `STOP_ON_BLOCKED` is not a key of the original.
        assert!(Actor::parse(b"ACTOR { STOP_ON_BLOCKED=TRUE }").is_err());
        assert!(Actor::parse(b"NAME=\"x\"\nACTOR { }").is_err());
        assert!(SpriteSet::parse(b"ACTOR { }").is_err());
    }

    /// Every `.actor` file of the local test game parses.
    #[test]
    #[ignore = "needs the local test game in testdata/local"]
    fn parses_every_local_actor() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/local/white-chamber/unpacked");
        if !root.is_dir() {
            eprintln!("skipped: {} is absent", root.display());
            return;
        }
        let mut count = 0;
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("actor"))
                {
                    let actor = Actor::parse(&std::fs::read(&path).unwrap())
                        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                    assert!(actor.walk.is_some(), "{}", path.display());
                    assert!(actor.stand.is_some(), "{}", path.display());
                    count += 1;
                }
            }
        }
        eprintln!("{count} actor files parsed");
        assert!(count > 0);
    }
}
