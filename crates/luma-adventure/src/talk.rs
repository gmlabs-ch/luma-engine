//! Sentences: what an object says, for how long, with which talk
//! sprites, and where the text goes (see `docs/formats/font.md`,
//! "Speech").
//!
//! Written with reference to `AdSentence.cpp` and `AdObject.cpp` of WME
//! Lite (MIT License, Copyright (c) 2011 Jan Nedoma); see `NOTICE`.

use luma_formats::font::TextAlign;

/// A sentence being said.
#[derive(Debug, Clone, PartialEq)]
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
    /// The speech sound playing with the sentence, if any.
    pub sound: Option<luma_audio::SoundId>,
}

impl Sentence {
    /// Whether the sentence's time is up at `timer`.
    pub fn time_is_up(&self, timer: u64) -> bool {
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
}

impl SentenceDraw {
    /// Where a sentence is drawn on a screen of `screen_width`, with the
    /// scene scrolled by `(offset_x, offset_y)` (`CAdSentence::Display`).
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
        })
    }
}

/// Which mouse button ends a sentence (`TALK_SKIP_BUTTON`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        }
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
