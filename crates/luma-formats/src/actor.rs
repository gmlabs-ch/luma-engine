//! Actors (`.actor`): characters that walk, with a sprite per direction
//! for each of their states.
//!
//! The format is documented in `docs/formats/actor.md` and was
//! established from the MIT-licensed WME Lite sources (`AdActor.cpp`,
//! `AdSpriteSet.cpp`, Copyright (c) 2011 Jan Nedoma). See `NOTICE` at the
//! root of the repository.

use crate::def::{Document, Node, Nodes};
use crate::error::{Error, Result};
use crate::image::Rgba;
use crate::scene::{Direction, Property, Region, WaypointGroup};

/// A sprite for each direction (`SPRITESET`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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

    /// Parses a `.spriteset` file: a `SPRITESET` root block.
    pub fn parse(bytes: &[u8]) -> Result<SpriteSet> {
        let doc = Document::parse(bytes)?;
        let root = doc.get("SPRITESET").ok_or_else(|| Error::Invalid {
            reason: "'SPRITESET' keyword expected".into(),
        })?;
        let mut set = SpriteSet::default();
        set.apply(root);
        Ok(set)
    }

    /// Reads the keys of a `SPRITESET` block into the set, overriding
    /// what is already there.
    pub fn apply(&mut self, node: &Node) {
        for child in node.nodes() {
            let direction = match child.key.to_ascii_uppercase().as_str() {
                "NAME" => {
                    self.name = text(child);
                    continue;
                }
                "TEMPLATE" => {
                    self.template = child.as_str().map(str::to_string);
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
            self.sprites[usize::from(direction.code())] = child.as_str().map(str::to_string);
        }
    }

    /// Builds a set from a block holding one `SPRITESET` block (`WALK`,
    /// `STAND`, `TALK`...).
    fn from_wrapper(node: &Node) -> Result<SpriteSet> {
        let inner = node.get("SPRITESET").ok_or_else(|| Error::Invalid {
            reason: format!(
                "'SPRITESET' keyword expected in {} at line {}",
                node.key, node.line
            ),
        })?;
        let mut set = SpriteSet::default();
        set.apply(inner);
        Ok(set)
    }
}

/// An actor definition (`ACTOR`).
#[derive(Debug, Clone, PartialEq)]
pub struct Actor {
    /// Name used by scripts.
    pub name: String,
    /// Caption shown when the mouse is over the actor.
    pub caption: String,
    /// Actor file loaded first (`TEMPLATE`), to be merged by the caller.
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
    /// Follows the scene's rotation levels.
    pub rotatable: bool,
    /// Reacts to the mouse.
    pub interactive: bool,
    /// Takes the tint of the region it stands in.
    pub colorable: bool,
    /// Fixed tint and opacity, overriding the scene's.
    pub tint: Option<Rgba>,
    /// Not clickable while an inventory item is selected.
    pub ignore_items: bool,
    /// Stop on a block instead of looking for another path.
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
            name: String::new(),
            caption: String::new(),
            template: None,
            x: 0,
            y: 0,
            active: true,
            scalable: true,
            scale: None,
            relative_scale: 0.0,
            rotatable: false,
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
    /// Parses an `.actor` file: an `ACTOR` root block. A `TEMPLATE` is
    /// recorded, not loaded.
    pub fn parse(bytes: &[u8]) -> Result<Actor> {
        let mut actor = Actor::default();
        actor.merge(bytes)?;
        Ok(actor)
    }

    /// Reads an `.actor` file over the actor: its keys override what is
    /// already there, as when a file names another by `TEMPLATE`.
    pub fn merge(&mut self, bytes: &[u8]) -> Result<()> {
        let doc = Document::parse(bytes)?;
        let root = doc.get("ACTOR").ok_or_else(|| Error::Invalid {
            reason: "'ACTOR' keyword expected".into(),
        })?;
        self.apply(root)
    }

    /// Adds the `ANIMATION` blocks of a file (`MergeAnims`).
    pub fn merge_animations(&mut self, bytes: &[u8]) -> Result<usize> {
        let doc = Document::parse(bytes)?;
        let mut count = 0;
        for node in doc.get_all("ANIMATION") {
            let mut set = SpriteSet::default();
            set.apply(node);
            self.animations.push(set);
            count += 1;
        }
        Ok(count)
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

    fn apply(&mut self, root: &Node) -> Result<()> {
        let (mut r, mut g, mut b, mut alpha) = (0i64, 0i64, 0i64, 0i64);
        for child in root.nodes() {
            match child.key.to_ascii_uppercase().as_str() {
                "TEMPLATE" => self.template = child.as_str().map(str::to_string),
                "NAME" => self.name = text(child),
                "CAPTION" => self.caption = text(child),
                "X" => self.x = integer(child),
                "Y" => self.y = integer(child),
                "ACTIVE" => self.active = boolean(child),
                "SCALABLE" => self.scalable = boolean(child),
                "SCALE" => self.scale = Some(integer(child) as f32),
                "RELATIVE_SCALE" => self.relative_scale = integer(child) as f32,
                "ROTATABLE" | "ROTABLE" => self.rotatable = boolean(child),
                "INTERACTIVE" | "REGISTRABLE" => self.interactive = boolean(child),
                "COLORABLE" | "SHADOWABLE" => self.colorable = boolean(child),
                "ALPHA_COLOR" => {
                    let v = child.as_ints().unwrap_or_default();
                    r = v.first().copied().unwrap_or(0);
                    g = v.get(1).copied().unwrap_or(0);
                    b = v.get(2).copied().unwrap_or(0);
                }
                "ALPHA" => alpha = child.as_int().unwrap_or(0),
                "IGNORE_ITEMS" => self.ignore_items = boolean(child),
                "STOP_ON_BLOCKED" => self.stop_on_blocked = boolean(child),
                "WALK" => self.walk = Some(SpriteSet::from_wrapper(child)?),
                "STAND" => self.stand = Some(SpriteSet::from_wrapper(child)?),
                "TURN_LEFT" => self.turn_left = Some(SpriteSet::from_wrapper(child)?),
                "TURN_RIGHT" => self.turn_right = Some(SpriteSet::from_wrapper(child)?),
                "TALK" => self.talk.push(SpriteSet::from_wrapper(child)?),
                "TALK_SPECIAL" => self.talk_special.push(SpriteSet::from_wrapper(child)?),
                "ANIMATION" => {
                    let mut set = SpriteSet::default();
                    set.apply(child);
                    self.animations.push(set);
                }
                "FONT" => self.font = child.as_str().map(str::to_string),
                "CURSOR" => self.cursor = child.as_str().map(str::to_string),
                "SCRIPT" => self.scripts.extend(child.as_str().map(str::to_string)),
                "PROPERTY" => self.properties.extend(Property::from_node(child)?),
                "BLOCKED_REGION" => self.blocked_region = Some(Region::from_node(child, false)?),
                "WAYPOINTS" => self.waypoints = Some(WaypointGroup::from_node(child)?),
                "SOUND_VOLUME" => self.sound_volume = integer(child),
                "SOUND_PANNING" => self.sound_panning = boolean(child),
                _ => {}
            }
        }
        if alpha != 0 && r == 0 && g == 0 && b == 0 {
            (r, g, b) = (255, 255, 255);
        }
        if (r, g, b, alpha) != (0, 0, 0, 0) {
            self.tint = Some(Rgba {
                r: clamp_channel(r),
                g: clamp_channel(g),
                b: clamp_channel(b),
                a: clamp_channel(alpha),
            });
        }
        Ok(())
    }
}

fn text(node: &Node) -> String {
    node.as_str().unwrap_or_default().to_string()
}

fn boolean(node: &Node) -> bool {
    node.as_bool().unwrap_or(false)
}

fn integer(node: &Node) -> i32 {
    node.as_int().unwrap_or(0) as i32
}

fn clamp_channel(v: i64) -> u8 {
    v.clamp(0, 255) as u8
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
  STOP_ON_BLOCKED = TRUE
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
        assert!(actor.stop_on_blocked);
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

    #[test]
    fn rejects_bad_files() {
        assert!(Actor::parse(b"ENTITY { }").is_err());
        assert!(Actor::parse(b"ACTOR { WALK { LEFT=\"x\" } }").is_err());
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
