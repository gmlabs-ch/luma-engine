//! What Windows does to a line of text before a TrueType font draws it,
//! and Luma Engine must do itself: put the characters of right-to-left
//! scripts in the order they are read, and give Arabic letters the forms
//! their neighbours call for (see `docs/formats/font.md`).
//!
//! The original engine hands each line to GDI's `ExtTextOut`, with
//! `ETO_RTLREADING` when `Game.TextRTL` is set (`CBFontTT::FormatText`
//! of Wintermute Engine 1.x, MIT License, Copyright (c) 2003-2013 Jan
//! Nedoma; see `NOTICE`). The reordering here is the Unicode
//! Bidirectional Algorithm; the Arabic forms are those of the Unicode
//! presentation blocks. Both stand for what GDI and the font's own
//! tables do there, and are Luma Engine's own.

use unicode_bidi::{BidiInfo, Level};

/// Whether a character can change the order of a line or its forms: one
/// of a right-to-left script, or a directional control. Everything
/// below U+0590 is left to right.
fn may_reorder(c: char) -> bool {
    matches!(u32::from(c),
        0x0590..=0x08FF | 0x200F | 0x202A..=0x202E | 0x2066..=0x2069
        | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x1_0800..=0x1_0FFF | 0x1_E800..=0x1_EFFF)
}

/// The character that mirrors another in right-to-left text, for the
/// pairs a line of a game holds.
fn mirrored(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        other => other,
    }
}

/// The characters of a line in the order they are drawn, from the left:
/// the Unicode Bidirectional Algorithm, with the line read from the
/// right when `rtl` (`ETO_RTLREADING`) and from the left otherwise.
/// Runs of right-to-left characters are reversed and their brackets
/// mirrored; a line without such characters and read from the left is
/// returned as it is.
pub(crate) fn visual_order(line: &str, rtl: bool) -> String {
    if !rtl && !line.chars().any(may_reorder) {
        return line.to_string();
    }
    let base = if rtl { Level::rtl() } else { Level::ltr() };
    let info = BidiInfo::new(line, Some(base));
    let mut out = String::with_capacity(line.len());
    for paragraph in &info.paragraphs {
        let (levels, runs) = info.visual_runs(paragraph, paragraph.range.clone());
        for run in runs {
            let text = &line[run.clone()];
            if levels[run.start].is_rtl() {
                out.extend(text.chars().rev().map(mirrored));
            } else {
                out.push_str(text);
            }
        }
    }
    out
}

/// How an Arabic letter joins its neighbours.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Joining {
    /// To both sides: four forms.
    Dual,
    /// To the letter before it only: two forms.
    Right,
    /// The tatweel: joins both sides and has one form.
    Causing,
    /// A mark: the letters around it join through it.
    Transparent,
    /// Not at all.
    None,
}

/// The isolated form of an Arabic letter in the presentation blocks,
/// and whether it joins to both sides: the final form follows the
/// isolated one, then the initial and the medial ones of a letter that
/// joins both ways.
fn forms(c: char) -> Option<(u32, bool)> {
    Some(match u32::from(c) {
        0x0622 => (0xFE81, false),
        0x0623 => (0xFE83, false),
        0x0624 => (0xFE85, false),
        0x0625 => (0xFE87, false),
        0x0626 => (0xFE89, true),
        0x0627 => (0xFE8D, false),
        0x0628 => (0xFE8F, true),
        0x0629 => (0xFE93, false),
        0x062A => (0xFE95, true),
        0x062B => (0xFE99, true),
        0x062C => (0xFE9D, true),
        0x062D => (0xFEA1, true),
        0x062E => (0xFEA5, true),
        0x062F => (0xFEA9, false),
        0x0630 => (0xFEAB, false),
        0x0631 => (0xFEAD, false),
        0x0632 => (0xFEAF, false),
        0x0633 => (0xFEB1, true),
        0x0634 => (0xFEB5, true),
        0x0635 => (0xFEB9, true),
        0x0636 => (0xFEBD, true),
        0x0637 => (0xFEC1, true),
        0x0638 => (0xFEC5, true),
        0x0639 => (0xFEC9, true),
        0x063A => (0xFECD, true),
        0x0641 => (0xFED1, true),
        0x0642 => (0xFED5, true),
        0x0643 => (0xFED9, true),
        0x0644 => (0xFEDD, true),
        0x0645 => (0xFEE1, true),
        0x0646 => (0xFEE5, true),
        0x0647 => (0xFEE9, true),
        0x0648 => (0xFEED, false),
        0x0649 => (0xFEEF, false),
        0x064A => (0xFEF1, true),
        // Persian letters.
        0x067E => (0xFB56, true),
        0x0686 => (0xFB7A, true),
        0x0698 => (0xFB8A, false),
        0x06A9 => (0xFB8E, true),
        0x06AF => (0xFB92, true),
        0x06CC => (0xFBFC, true),
        _ => return None,
    })
}

fn joining(c: char) -> Joining {
    match (forms(c), u32::from(c)) {
        (Some((_, true)), _) => Joining::Dual,
        (Some((_, false)), _) => Joining::Right,
        (None, 0x0640) => Joining::Causing,
        (None, 0x064B..=0x065F | 0x0670) => Joining::Transparent,
        (None, _) => Joining::None,
    }
}

/// The ligature of a lam followed by an alef, isolated; its final form
/// follows.
fn lam_alef(alef: char) -> Option<u32> {
    Some(match u32::from(alef) {
        0x0622 => 0xFEF5,
        0x0623 => 0xFEF7,
        0x0625 => 0xFEF9,
        0x0627 => 0xFEFB,
        _ => return None,
    })
}

/// Arabic letters in the forms their neighbours call for: isolated,
/// final, initial or medial, and the ligature of a lam and an alef. The
/// text stays in reading order. `drawn(form, letter)` says whether the
/// font can draw a form in place of a letter; when it cannot the letter
/// is left as it is. A text without Arabic letters is returned as it
/// is.
pub(crate) fn join_arabic(text: &str, drawn: impl Fn(char, char) -> bool) -> String {
    if !text.chars().any(|c| forms(c).is_some()) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    // The letter a letter can join on each side: marks are looked
    // through.
    let neighbour = |from: usize, forward: bool| -> Joining {
        let mut at = from;
        loop {
            at = if forward {
                at + 1
            } else {
                match at.checked_sub(1) {
                    Some(before) => before,
                    None => return Joining::None,
                }
            };
            match chars.get(at).map(|c| joining(*c)) {
                Some(Joining::Transparent) => {}
                Some(other) => return other,
                None => return Joining::None,
            }
        }
    };
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let Some((isolated, dual)) = forms(c) else {
            out.push(c);
            i += 1;
            continue;
        };
        // The letter before joins this one when it joins to its left.
        let after_joining = matches!(neighbour(i, false), Joining::Dual | Joining::Causing);
        // A lam and the alef right after it are one sign.
        if c == '\u{644}' {
            if let Some(ligature) = chars.get(i + 1).and_then(|next| lam_alef(*next)) {
                let form = char::from_u32(ligature + u32::from(after_joining));
                if let Some(form) = form.filter(|form| drawn(*form, c)) {
                    out.push(form);
                    i += 2;
                    continue;
                }
            }
        }
        let before_joining = dual
            && matches!(
                neighbour(i, true),
                Joining::Dual | Joining::Right | Joining::Causing
            );
        let offset = match (after_joining, before_joining) {
            (false, false) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (true, true) => 3,
        };
        match char::from_u32(isolated + offset).filter(|form| drawn(*form, c)) {
            Some(form) => out.push(form),
            None => out.push(c),
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_to_right_text_keeps_its_order() {
        assert_eq!(visual_order("Hello, world!", false), "Hello, world!");
        assert_eq!(visual_order("", true), "");
        // Read from the right, Latin words keep their letters; the
        // punctuation that ends the line goes to its left end.
        assert_eq!(visual_order("Hello!", true), "!Hello");
        assert_eq!(visual_order("ab cd", true), "ab cd");
    }

    #[test]
    fn right_to_left_runs_are_reversed() {
        // Hebrew: alef bet gimel.
        assert_eq!(visual_order("אבג", true), "גבא");
        assert_eq!(visual_order("אבג", false), "גבא", "whatever the line");
        // A Latin word in a Hebrew line read from the right: the Hebrew
        // comes first, at the right.
        assert_eq!(visual_order("אב ab", true), "ab בא");
        assert_eq!(visual_order("אב ab", false), "בא ab");
        // Numbers keep their digits in order.
        assert_eq!(visual_order("אב 123", true), "123 בא");
        // Brackets are mirrored with the run they are in.
        assert_eq!(visual_order("(אב)", true), "(בא)");
        // A space at the end of the line goes to its left end.
        assert_eq!(visual_order("אב ", true), " בא");
        assert_eq!(visual_order("אב ", false), "בא ");
    }

    #[test]
    fn arabic_letters_take_the_forms_of_their_place() {
        let all = |_: char, _: char| true;
        // Kaf, teh, beh: initial, medial, final.
        assert_eq!(join_arabic("كتب", all), "\u{fedb}\u{fe98}\u{fe90}");
        // Seen, lam, alef, meem: the lam and the alef are one sign,
        // joined to the seen; the meem after an alef stands alone.
        assert_eq!(join_arabic("سلام", all), "\u{feb3}\u{fefc}\u{fee1}");
        // A lam and an alef at the start of a word: the isolated sign.
        assert_eq!(join_arabic("لا", all), "\u{fefb}");
        // A letter alone, a letter that only joins the one before it.
        assert_eq!(join_arabic("ب", all), "\u{fe8f}");
        assert_eq!(join_arabic("دب", all), "\u{fea9}\u{fe8f}");
        assert_eq!(join_arabic("بد", all), "\u{fe91}\u{feaa}");
        // A mark does not break the joining; a space does.
        assert_eq!(join_arabic("بَب", all), "\u{fe91}\u{64e}\u{fe90}");
        assert_eq!(join_arabic("ب ب", all), "\u{fe8f} \u{fe8f}");
        // The tatweel joins both ways.
        assert_eq!(join_arabic("بـب", all), "\u{fe91}\u{640}\u{fe90}");
        // Persian: peh, gaf.
        assert_eq!(join_arabic("پگ", all), "\u{fb58}\u{fb93}");
        // The hamza joins nothing.
        assert_eq!(join_arabic("بء", all), "\u{fe8f}\u{621}");
        // A font without the forms keeps the letters.
        assert_eq!(join_arabic("كتب", |_, _| false), "كتب");
        assert_eq!(
            join_arabic("سلام", |form, _| form == '\u{feb3}'),
            "\u{feb3}لام"
        );
        // Other text is untouched.
        assert_eq!(join_arabic("Hello אב", all), "Hello אב");
        // Joined and put in drawing order: the last letter at the left.
        assert_eq!(
            visual_order(&join_arabic("كتب", all), true),
            "\u{fe90}\u{fe98}\u{fedb}"
        );
    }
}
