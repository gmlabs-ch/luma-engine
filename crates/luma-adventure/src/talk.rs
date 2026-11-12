//! Sentences: what an object says, for how long, with which talk
//! sprites, and where the text goes (see `docs/formats/font.md`,
//! "Speech").
//!
//! Written with reference to `AdSentence.cpp` and `AdObject.cpp` of WME
//! Lite, and `AdSentence.cpp`, `AdTalkDef.cpp` and `AdTalkNode.cpp` of
//! Wintermute Engine 1.x (MIT License, Copyright (c) 2011 Jan Nedoma);
//! see `NOTICE`.

use luma_formats::font::TextAlign;
use luma_formats::{Direction, TalkDef};

/// Which sprite of a talk definition: the original compares sprite
/// objects, and each node and the default have their own even when
/// they name the same file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TalkSource {
    /// The sprite of the node at this index.
    Node(usize),
    /// The default sprite of the definition.
    Default,
}

/// A talk definition being played with a sentence (`CAdSentence::
/// m_TalkDef`, `m_CurrentSprite`): the `.talk` file found next to the
/// speech sound, and where its sprites are.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TalkPlayback {
    /// The definition.
    pub def: TalkDef,
    /// The sprite the definition gives for now (`m_CurrentSprite`).
    pub current: Option<(TalkSource, String)>,
    /// The definition's sprites that played to their end and were not
    /// restarted since (`CBSprite::m_Finished` of those sprites).
    pub finished: Vec<(TalkSource, String)>,
}

/// What a talk definition gives for a frame (`CAdSentence::Update`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TalkFrame {
    /// The sprite to show; none leaves the talk stances in charge.
    pub sprite: Option<String>,
    /// Whether that sprite was restarted this frame.
    pub reset: bool,
    /// Whether that sprite has played to its end. The original then
    /// lets the talk stances take the frame (and restarts one on every
    /// frame for as long as the definition keeps giving this sprite).
    pub finished: bool,
}

impl TalkPlayback {
    /// A definition at its start.
    pub fn new(def: TalkDef) -> TalkPlayback {
        TalkPlayback {
            def,
            current: None,
            finished: Vec::new(),
        }
    }

    /// Notes that the sprite being shown, if it is the definition's
    /// current one, has played to its end.
    pub fn note_finished(&mut self, sprite: &str) {
        if let Some((source, path)) = &self.current {
            if same_file(path, sprite) && !self.is_finished(*source, path) {
                self.finished.push((*source, path.clone()));
            }
        }
    }

    fn is_finished(&self, source: TalkSource, sprite: &str) -> bool {
        self.finished
            .iter()
            .any(|(s, p)| *s == source && same_file(p, sprite))
    }

    /// Makes `new` the current sprite; a sprite that was not the
    /// current one starts again (`NewSprite->Reset()`). Returns whether
    /// it was restarted.
    fn show(&mut self, new: Option<(TalkSource, String)>) -> bool {
        let same = match (&self.current, &new) {
            (Some((a, p)), Some((b, q))) => a == b && same_file(p, q),
            (None, None) => true,
            _ => false,
        };
        let reset = !same && new.is_some();
        if let (true, Some((source, path))) = (reset, &new) {
            self.finished
                .retain(|(s, p)| !(s == source && same_file(p, path)));
        }
        self.current = new;
        reset
    }

    /// The sprite for `time` milliseconds into the sentence, facing
    /// `direction` (`CAdSentence::Update`): the sprite of each active
    /// node in file order, stopping at the first timed one (a
    /// play-to-end node lets the following ones override it); without
    /// an active node, the default sprite, else none.
    pub fn update(&mut self, time: u32, direction: Direction) -> TalkFrame {
        let mut found = false;
        let mut restarted: Option<(TalkSource, String)> = None;
        for index in 0..self.def.nodes.len() {
            let source = TalkSource::Node(index);
            let (active, sprite, play_to_end) = {
                let node = &self.def.nodes[index];
                let finished = |sprite: &str| self.is_finished(source, sprite);
                (
                    node.in_time_interval(time, direction, &finished),
                    node.sprite(direction).map(str::to_string),
                    node.play_to_end,
                )
            };
            if !active {
                continue;
            }
            found = true;
            let new = sprite.map(|path| (source, path));
            if self.show(new.clone()) {
                restarted = new;
            }
            if !play_to_end {
                break;
            }
        }
        if !found {
            let new = self
                .def
                .default_sprite(direction)
                .map(|path| (TalkSource::Default, path.to_string()));
            if self.show(new.clone()) {
                restarted = new;
            }
        }
        TalkFrame {
            sprite: self.current.as_ref().map(|(_, path)| path.clone()),
            reset: restarted.is_some() && restarted == self.current,
            finished: self
                .current
                .as_ref()
                .is_some_and(|(source, path)| self.is_finished(*source, path)),
        }
    }
}

/// Whether two paths name the same file (case and slash apart).
fn same_file(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes().zip(b.bytes()).all(|(x, y)| {
            let norm = |c: u8| {
                if c == b'/' {
                    b'\\'
                } else {
                    c.to_ascii_lowercase()
                }
            };
            norm(x) == norm(y)
        })
}

/// A sentence being said.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Sentence {
    /// The text, expanded through the string table.
    pub text: String,
    /// The talk sprite names, comma-separated, one per stance.
    pub stances: Option<String>,
    /// The stance being played; `-1` before the first.
    pub current_stance: i32,
    /// How long the sentence lasts, in milliseconds.
    pub duration: u64,
    /// When it started, on the game timer.
    pub start_time: u64,
    /// The font the text is drawn with.
    pub font: Option<String>,
    /// Where the text box is, in scene coordinates unless `fixed_pos`.
    pub x: i32,
    /// See `x`.
    pub y: i32,
    /// The width of the text box.
    pub width: i32,
    /// How the text is aligned in the box.
    pub align: TextAlign,
    /// Whether the box stays put while the scene scrolls.
    pub fixed_pos: bool,
    /// The speech sound playing with the sentence, if any; not kept by
    /// a saved game.
    #[serde(skip)]
    pub sound: Option<luma_audio::SoundId>,
    /// The file of the speech sound, and where the sound was at the last
    /// frame, in milliseconds: a saved game starts it again from there.
    pub sound_file: Option<String>,
    /// See `sound_file`.
    pub sound_position: u64,
    /// Whether the sentence is hidden while the game is frozen: the
    /// `Freezable` of the object saying it (`m_Freezable`).
    pub freezable: bool,
    /// The talk definition found next to the speech sound, if any.
    pub talk: Option<TalkPlayback>,
}

impl Sentence {
    /// Whether the sentence's time is up at `timer` (`TimeIsUp` in
    /// `CAdActor::Update` and `CAdEntity::Update`). With a speech sound
    /// the duration does not count: the sentence lasts until the sound
    /// stops playing, which `World::update_speech` sees (it then
    /// finishes the sentence and takes the sound away).
    pub fn time_is_up(&self, timer: u64) -> bool {
        if self.sound.is_some() && self.duration != 0 {
            return false;
        }
        self.duration <= timer.saturating_sub(self.start_time)
    }

    /// Whether a click may end the sentence: not in its first 300 ms
    /// (`CAdSentence::CanSkip`).
    pub fn can_skip(&self, timer: u64) -> bool {
        timer.saturating_sub(self.start_time) > 300
    }

    /// Ends the sentence at once.
    pub fn finish(&mut self) {
        self.duration = 0;
    }

    /// The sprite the sentence's talk definition gives at `timer`
    /// (`CAdSentence::Update`); `None` without a definition.
    pub fn update_talk(&mut self, timer: u64, direction: Direction) -> Option<TalkFrame> {
        let elapsed = timer.saturating_sub(self.start_time);
        let time = u32::try_from(elapsed).unwrap_or(u32::MAX);
        self.talk.as_mut().map(|talk| talk.update(time, direction))
    }

    /// Notes that `sprite`, being shown for the sentence, has played to
    /// its end.
    pub fn note_sprite_finished(&mut self, sprite: &str) {
        if let Some(talk) = &mut self.talk {
            talk.note_finished(sprite);
        }
    }

    /// The name of stance `index`, trimmed; `None` past the last
    /// (`CAdSentence::GetStance`).
    pub fn stance(&self, index: i32) -> Option<String> {
        let stances = self.stances.as_deref()?;
        let index = usize::try_from(index).ok()?;
        stances
            .split(',')
            .nth(index)
            .map(|s| s.trim_matches(' ').to_string())
    }

    /// Moves to the next stance and returns its name.
    pub fn next_stance(&mut self) -> Option<String> {
        self.current_stance += 1;
        self.stance(self.current_stance)
    }
}

/// A sentence as drawn this frame, in screen coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct SentenceDraw {
    /// The font.
    pub font: String,
    /// The text.
    pub text: String,
    /// The left of the text box on screen.
    pub x: i32,
    /// The top of the text box on screen.
    pub y: i32,
    /// The width of the box.
    pub width: i32,
    /// The alignment.
    pub align: TextAlign,
    /// Whether the sentence is hidden while the game is frozen.
    pub freezable: bool,
}

impl SentenceDraw {
    /// Where a sentence is drawn on a screen of `screen_width`, with the
    /// scene scrolled by `(offset_x, offset_y)`: the scene's offset less
    /// the origin of its viewport, `CAdScene::GetOffsetLeft` and
    /// `GetOffsetTop` (`CAdSentence::Display`).
    pub fn of(
        sentence: &Sentence,
        offset_x: i32,
        offset_y: i32,
        screen_width: i32,
    ) -> Option<SentenceDraw> {
        let font = sentence.font.clone()?;
        let (mut x, mut y) = (sentence.x, sentence.y);
        if !sentence.fixed_pos {
            x -= offset_x;
            y -= offset_y;
        }
        x = x.max(0).min(screen_width - sentence.width);
        y = y.max(0);
        Some(SentenceDraw {
            font,
            text: sentence.text.clone(),
            x,
            y,
            width: sentence.width,
            align: sentence.align,
            freezable: sentence.freezable,
        })
    }
}

/// Which mouse button ends a sentence (`TALK_SKIP_BUTTON`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum TalkSkipButton {
    /// Code 0, the default.
    #[default]
    Left,
    /// Code 1.
    Right,
    /// Code 2.
    Both,
    /// Code 3.
    None,
}

impl TalkSkipButton {
    /// The setting of a `default.game` value.
    pub fn parse(text: &str) -> TalkSkipButton {
        if text.eq_ignore_ascii_case("right") {
            TalkSkipButton::Right
        } else if text.eq_ignore_ascii_case("both") {
            TalkSkipButton::Both
        } else if text.eq_ignore_ascii_case("none") {
            TalkSkipButton::None
        } else {
            TalkSkipButton::Left
        }
    }

    /// The script code.
    pub fn code(self) -> i32 {
        self as i32
    }

    /// The setting of a script code.
    pub fn from_code(code: i32) -> TalkSkipButton {
        match code {
            1 => TalkSkipButton::Right,
            2 => TalkSkipButton::Both,
            3 => TalkSkipButton::None,
            _ => TalkSkipButton::Left,
        }
    }

    /// Whether a left click skips.
    pub fn skips_on_left(self) -> bool {
        matches!(self, TalkSkipButton::Left | TalkSkipButton::Both)
    }

    /// Whether a right click skips.
    pub fn skips_on_right(self) -> bool {
        matches!(self, TalkSkipButton::Right | TalkSkipButton::Both)
    }
}

/// The subtitle placement an object carries (`SubtitlesWidth`,
/// `SubtitlesPosX`...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SubtitleMods {
    /// A fixed width; `0` lets the engine choose.
    pub width: i32,
    /// Horizontal mod.
    pub x: i32,
    /// Vertical mod.
    pub y: i32,
    /// Whether the mods move the box (else they place it).
    pub relative: bool,
    /// Whether the box is centred on the object.
    pub x_center: bool,
}

impl Default for SubtitleMods {
    fn default() -> Self {
        SubtitleMods {
            width: 0,
            x: 0,
            y: 0,
            relative: true,
            x_center: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sentence() -> Sentence {
        Sentence {
            text: "hi".into(),
            stances: Some(" talk1, special ,talk2".into()),
            current_stance: -1,
            duration: 1000,
            start_time: 100,
            font: Some("f.font".into()),
            x: 50,
            y: 40,
            width: 100,
            align: TextAlign::Center,
            fixed_pos: false,
            sound: None,
            sound_file: None,
            sound_position: 0,
            freezable: true,
            talk: None,
        }
    }

    fn talk_def(text: &str) -> TalkDef {
        let mut load = |_: &str| None;
        let mut sprite_loads = |file: &str| !file.starts_with("gone");
        TalkDef::parse_with(
            text.as_bytes(),
            &mut luma_formats::SetFiles {
                load: &mut load,
                sprite_loads: &mut sprite_loads,
                follow_templates: true,
            },
        )
        .unwrap()
    }

    fn frame(sprite: Option<&str>, reset: bool) -> Option<TalkFrame> {
        Some(TalkFrame {
            sprite: sprite.map(str::to_string),
            reset,
            finished: false,
        })
    }

    #[test]
    fn a_talk_definition_picks_the_sprite_of_the_time() {
        let mut s = sentence();
        assert_eq!(s.update_talk(150, Direction::Down), None);
        s.talk = Some(TalkPlayback::new(talk_def(
            "TALK {
               DEFAULT_SPRITE=\"rest.sprite\"
               ACTION { START_TIME=100 END_TIME=200 SPRITE=\"a.sprite\" }
               ACTION { START_TIME=150 END_TIME=300 SPRITE=\"b.sprite\" }
               ACTION { START_TIME=400 SPRITE=\"once.sprite\" }
             }",
        )));
        // The sentence started at 100 on the timer. Before any node:
        // the default sprite, restarted when it comes in.
        assert_eq!(
            s.update_talk(150, Direction::Down),
            frame(Some("rest.sprite"), true)
        );
        assert_eq!(
            s.update_talk(160, Direction::Down),
            frame(Some("rest.sprite"), false)
        );
        // The first active timed node wins over a later one.
        assert_eq!(
            s.update_talk(200, Direction::Down),
            frame(Some("a.sprite"), true)
        );
        assert_eq!(
            s.update_talk(260, Direction::Down),
            frame(Some("a.sprite"), false)
        );
        assert_eq!(
            s.update_talk(300, Direction::Down),
            frame(Some("a.sprite"), false)
        );
        // A timed node keeps giving a sprite that has played to its
        // end, marked so.
        s.note_sprite_finished("a.sprite");
        let again = s.update_talk(300, Direction::Down).unwrap();
        assert!(again.finished && !again.reset);
        assert_eq!(
            s.update_talk(301, Direction::Down),
            frame(Some("b.sprite"), true)
        );
        assert_eq!(
            s.update_talk(401, Direction::Down),
            frame(Some("rest.sprite"), true)
        );
        // A node without an end lasts until its sprite has played.
        assert_eq!(
            s.update_talk(500, Direction::Down),
            frame(Some("once.sprite"), true)
        );
        assert_eq!(
            s.update_talk(9000, Direction::Down),
            frame(Some("once.sprite"), false)
        );
        s.note_sprite_finished("rest.sprite");
        assert_eq!(
            s.update_talk(9001, Direction::Down),
            frame(Some("once.sprite"), false)
        );
        s.note_sprite_finished("ONCE.sprite");
        assert_eq!(
            s.update_talk(9002, Direction::Down),
            frame(Some("rest.sprite"), true)
        );
        assert_eq!(
            s.update_talk(9003, Direction::Down),
            frame(Some("rest.sprite"), false)
        );
    }

    #[test]
    fn talk_nodes_without_a_sprite_and_overlapping_ones() {
        // No default: outside the nodes the definition gives nothing.
        // A node whose sprite does not load is active and shows nothing.
        let mut s = sentence();
        s.talk = Some(TalkPlayback::new(talk_def(
            "TALK {
               ACTION { START_TIME=0 END_TIME=50 SPRITE=\"a.sprite\" }
               ACTION { START_TIME=100 END_TIME=150 SPRITE=\"gone.sprite\" }
             }",
        )));
        assert_eq!(
            s.update_talk(120, Direction::Down),
            frame(Some("a.sprite"), true)
        );
        assert_eq!(s.update_talk(160, Direction::Down), frame(None, false));
        assert_eq!(s.update_talk(210, Direction::Down), frame(None, false));
        // Two play-to-end nodes active together: the later one is shown
        // and, as in the original, restarted on every frame, since the
        // earlier one takes the place for a moment each time.
        let mut s = sentence();
        s.talk = Some(TalkPlayback::new(talk_def(
            "TALK {
               ACTION { START_TIME=0 SPRITE=\"a.sprite\" }
               ACTION { START_TIME=10 SPRITE=\"b.sprite\" }
             }",
        )));
        assert_eq!(
            s.update_talk(105, Direction::Down),
            frame(Some("a.sprite"), true)
        );
        assert_eq!(
            s.update_talk(120, Direction::Down),
            frame(Some("b.sprite"), true)
        );
        assert_eq!(
            s.update_talk(130, Direction::Down),
            frame(Some("b.sprite"), true)
        );
    }

    #[test]
    fn stances_and_timing() {
        let mut s = sentence();
        assert_eq!(s.next_stance().as_deref(), Some("talk1"));
        assert_eq!(s.next_stance().as_deref(), Some("special"));
        assert_eq!(s.next_stance().as_deref(), Some("talk2"));
        assert_eq!(s.next_stance(), None);
        assert!(!s.time_is_up(1099));
        assert!(s.time_is_up(1100));
        assert!(!s.can_skip(400));
        assert!(s.can_skip(401));
        s.finish();
        assert!(s.time_is_up(100));
        let mut none = Sentence {
            stances: None,
            ..sentence()
        };
        assert_eq!(none.next_stance(), None);
    }

    #[test]
    fn draws_on_screen() {
        let s = sentence();
        let d = SentenceDraw::of(&s, 30, 60, 320).unwrap();
        assert_eq!((d.x, d.y), (20, 0));
        let fixed = Sentence {
            fixed_pos: true,
            x: 300,
            ..sentence()
        };
        let d = SentenceDraw::of(&fixed, 30, 60, 320).unwrap();
        assert_eq!((d.x, d.y), (220, 40), "clamped to the screen");
        assert_eq!(TalkSkipButton::parse("Both"), TalkSkipButton::Both);
        assert!(TalkSkipButton::parse("x").skips_on_left());
        assert_eq!(TalkSkipButton::from_code(1).code(), 1);
    }
}
