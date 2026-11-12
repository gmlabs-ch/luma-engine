//! Choosing the face a TrueType font is drawn with: the typeface a
//! `.font` file names with `FACE`, in the weight and slant its `BOLD` and
//! `ITALIC` ask for, among the faces of the font files the game ships and
//! those installed on the system, and the face that stands in for a
//! character the chosen one lacks (see `docs/formats/font.md`).
//!
//! Written with reference to `BFontTT.cpp` and `BFontStorage.cpp` of
//! Wintermute Engine 1.x (MIT License, Copyright (c) 2003-2013 Jan
//! Nedoma); see `NOTICE`. The original engine registers the game's font
//! file with Windows and asks GDI for a font by typeface name, weight and
//! italic flag (`CBFontTT::CreateWinFont`); the choice among the faces
//! found, which GDI's font mapper makes there, is Luma Engine's own.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use fontdb::{Database, ID, Source, Style};

/// The weight of a regular face.
pub const REGULAR: u16 = 400;
/// The weight of a bold face (`FW_BOLD`).
pub const BOLD: u16 = 700;
/// The weight from which a face counts as bold already.
pub(crate) const BOLD_ENOUGH: u16 = 600;

/// A face a font can be drawn with, as its file describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Face {
    /// The family names of the face (`Arial`), in the languages the file
    /// gives them.
    pub families: Vec<String>,
    /// The weight: 400 for regular, 700 for bold.
    pub weight: u16,
    /// Whether the face is italic or oblique.
    pub italic: bool,
}

impl Face {
    /// A face of a family.
    pub fn new(family: &str, weight: u16, italic: bool) -> Face {
        Face {
            families: vec![family.to_string()],
            weight,
            italic,
        }
    }

    fn is_of(&self, family: &str) -> bool {
        self.families.iter().any(|f| f.eq_ignore_ascii_case(family))
    }
}

/// The face chosen among candidates, and what is left to imitate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaceChoice {
    /// The index of the face among the candidates.
    pub index: usize,
    /// Bold was asked for and the face is not: its glyphs are thickened.
    pub embolden: bool,
    /// Italic was asked for and the face is upright: its glyphs are
    /// slanted.
    pub slant: bool,
}

/// The face of a family closest to a style: the nearest weight to the
/// one asked for (700 for bold, 400 otherwise), then the right slant,
/// then the first in the list. `None` when no face is of the family;
/// without a family every face is a candidate.
pub fn choose_face(
    faces: &[Face],
    family: Option<&str>,
    bold: bool,
    italic: bool,
) -> Option<FaceChoice> {
    let target = if bold { BOLD } else { REGULAR };
    let (index, face) = faces
        .iter()
        .enumerate()
        .filter(|(_, face)| family.is_none_or(|name| face.is_of(name)))
        .min_by_key(|(index, face)| {
            (face.weight.abs_diff(target), face.italic != italic, *index)
        })?;
    Some(FaceChoice {
        index,
        embolden: bold && face.weight < BOLD_ENOUGH,
        slant: italic && !face.italic,
    })
}

/// The families that stand for one a system lacks: the faces drawn with
/// the same metrics, then the nearest common ones. The family itself
/// comes first.
pub fn family_and_substitutes(family: &str) -> Vec<&str> {
    let substitutes: &[&str] = match family.to_ascii_lowercase().as_str() {
        "arial" | "helvetica" | "ms sans serif" | "microsoft sans serif" | "tahoma" | "verdana" => {
            &[
                "Arial",
                "Liberation Sans",
                "Arimo",
                "Helvetica",
                "Helvetica Neue",
                "Nimbus Sans",
                "DejaVu Sans",
            ]
        }
        "times new roman" | "times" => &[
            "Times New Roman",
            "Liberation Serif",
            "Tinos",
            "Times",
            "Nimbus Roman",
            "DejaVu Serif",
        ],
        "courier new" | "courier" => &[
            "Courier New",
            "Liberation Mono",
            "Cousine",
            "Courier",
            "Nimbus Mono PS",
            "DejaVu Sans Mono",
        ],
        _ => &[],
    };
    let mut names = vec![family];
    names.extend(
        substitutes
            .iter()
            .copied()
            .filter(|name| !name.eq_ignore_ascii_case(family)),
    );
    names
}

/// Where a font's face was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceOrigin {
    /// In the font file of the game (`FILENAME`).
    Game,
    /// Among the fonts of the system.
    System,
}

/// The face a font is drawn with (`CBFontTT::CreateWinFont`): the
/// family `face` in the game's own font file when the file is of that
/// family, else among the system's fonts, where a family the system
/// lacks is replaced by its substitutes in turn. `None` when neither has
/// the family: the caller falls back to whatever it can draw with.
pub fn resolve_face(
    game: &[Face],
    system: &[Face],
    face: &str,
    bold: bool,
    italic: bool,
) -> Option<(FaceOrigin, FaceChoice)> {
    if let Some(choice) = choose_face(game, Some(face), bold, italic) {
        return Some((FaceOrigin::Game, choice));
    }
    family_and_substitutes(face)
        .into_iter()
        .find_map(|family| choose_face(system, Some(family), bold, italic))
        .map(|choice| (FaceOrigin::System, choice))
}

/// The faces of a set of font files, with their data.
pub struct FaceSet {
    db: Database,
    ids: Vec<ID>,
    faces: Vec<Face>,
}

impl std::fmt::Debug for FaceSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FaceSet")
            .field("faces", &self.faces.len())
            .finish()
    }
}

impl FaceSet {
    fn of(db: Database) -> FaceSet {
        let (ids, faces) = db
            .faces()
            .map(|info| {
                let face = Face {
                    families: info.families.iter().map(|(name, _)| name.clone()).collect(),
                    weight: info.weight.0,
                    italic: info.style != Style::Normal,
                };
                (info.id, face)
            })
            .unzip();
        FaceSet { db, ids, faces }
    }

    /// The faces of one font file (several for a collection); none when
    /// the bytes are not a font.
    pub fn from_bytes(bytes: Vec<u8>) -> FaceSet {
        let mut db = Database::new();
        db.load_font_data(bytes);
        FaceSet::of(db)
    }

    /// The fonts installed on the system, listed once per process: the
    /// font directories of the platform are scanned for their faces'
    /// names (the files are read only when a face is used).
    pub fn system() -> &'static FaceSet {
        static SYSTEM: OnceLock<FaceSet> = OnceLock::new();
        SYSTEM.get_or_init(|| {
            let mut db = Database::new();
            db.load_system_fonts();
            FaceSet::of(db)
        })
    }

    /// The faces, in the order [`FaceChoice::index`] counts them.
    pub fn faces(&self) -> &[Face] {
        &self.faces
    }

    /// Gives `f` the bytes of the file holding a face and the face's
    /// index in it. `None` when the file can no longer be read.
    pub fn with_data<T>(&self, index: usize, f: impl FnOnce(&[u8], u32) -> T) -> Option<T> {
        self.db.with_face_data(*self.ids.get(index)?, f)
    }

    /// Whether a face has a glyph for a character.
    pub fn has_glyph(&self, index: usize, ch: char) -> bool {
        self.with_data(index, |bytes, face| {
            ttf_parser::Face::parse(bytes, face).is_ok_and(|face| face.glyph_index(ch).is_some())
        })
        .unwrap_or(false)
    }

    /// The face to draw a character with when the font asked for lacks
    /// it: the first with a glyph for it among the faces of the
    /// `preferred` families, in their order, then among all the others;
    /// in a family the regular upright face is looked at first.
    pub fn face_with_glyph(&self, ch: char, preferred: &[&str]) -> Option<usize> {
        let plainest = |faces: &mut Vec<usize>| {
            faces.sort_by_key(|&i| {
                let face = &self.faces[i];
                (face.italic, face.weight.abs_diff(REGULAR), i)
            });
        };
        let mut order: Vec<usize> = Vec::with_capacity(self.faces.len());
        for family in preferred {
            let mut faces: Vec<usize> = (0..self.faces.len())
                .filter(|&i| self.faces[i].is_of(family) && !order.contains(&i))
                .collect();
            plainest(&mut faces);
            order.extend(faces);
        }
        let mut rest: Vec<usize> = (0..self.faces.len())
            .filter(|i| !order.contains(i))
            .collect();
        plainest(&mut rest);
        order.extend(rest);
        order.into_iter().find(|&i| self.has_glyph(i, ch))
    }

    /// The face of the family of `index` closest to a style that still
    /// has the character, with what is left to imitate; `index` itself
    /// when no other face of its family will do.
    pub fn styled(&self, index: usize, ch: char, bold: bool, italic: bool) -> FaceChoice {
        let own = || {
            let face = self.faces.get(index);
            FaceChoice {
                index,
                embolden: bold && face.is_some_and(|f| f.weight < BOLD_ENOUGH),
                slant: italic && face.is_some_and(|f| !f.italic),
            }
        };
        let Some(family) = self.faces.get(index).and_then(|f| f.families.first()) else {
            return own();
        };
        match choose_face(&self.faces, Some(family), bold, italic) {
            Some(choice) if choice.index == index || self.has_glyph(choice.index, ch) => choice,
            _ => own(),
        }
    }

    /// The file a face comes from, when it comes from one.
    pub fn path(&self, index: usize) -> Option<String> {
        let info = self.db.face(*self.ids.get(index)?)?;
        match &info.source {
            Source::File(path) | Source::SharedFile(path, _) => Some(path.display().to_string()),
            Source::Binary(_) => None,
        }
    }
}

/// The families looked at first for a character the font of a text
/// lacks: the usual text faces of the platforms, then those that cover
/// the most scripts.
const FALLBACK_FAMILIES: &[&str] = &[
    "Arial",
    "Liberation Sans",
    "Helvetica",
    "DejaVu Sans",
    "Noto Sans",
    "Segoe UI",
    "Tahoma",
    "Microsoft Sans Serif",
    "Lucida Grande",
    "Arial Unicode MS",
    "Noto Sans CJK SC",
    "Noto Sans CJK JP",
    "Noto Sans CJK KR",
    "PingFang SC",
    "Hiragino Sans",
    "Microsoft YaHei",
    "Yu Gothic",
    "MS Gothic",
    "Malgun Gothic",
    "Apple SD Gothic Neo",
];

/// The system's face to draw a character with when no font of the game
/// has it (see [`FaceSet::face_with_glyph`]); `None` when no installed
/// font has it. The answer is kept: the fonts are searched once per
/// character.
pub fn system_face_with_glyph(ch: char) -> Option<usize> {
    static FOUND: OnceLock<Mutex<HashMap<char, Option<usize>>>> = OnceLock::new();
    let found = FOUND.get_or_init(Mutex::default);
    if let Some(known) = found.lock().ok().and_then(|f| f.get(&ch).copied()) {
        return known;
    }
    // Characters no font draws (private use, noncharacters) are not
    // looked for, and a text made of characters the machine has no font
    // for must not make the engine read every installed font once per
    // character: after some misses the search is given up for the run.
    let code = u32::from(ch);
    let unassigned = matches!(code, 0xE000..=0xF8FF | 0xF_0000..=0x10_FFFF)
        || code & 0xFFFE == 0xFFFE
        || (0xFDD0..=0xFDEF).contains(&code);
    let misses = found
        .lock()
        .map_or(0, |f| f.values().filter(|face| face.is_none()).count());
    let face = if unassigned || misses >= MAX_FALLBACK_MISSES {
        None
    } else {
        FaceSet::system().face_with_glyph(ch, FALLBACK_FAMILIES)
    };
    if let Ok(mut found) = found.lock() {
        // The answers are kept up to a bound; past it they are computed
        // again, which costs nothing once the search is given up.
        if found.len() < 65_536 {
            found.insert(ch, face);
        }
    }
    face
}

/// How many characters may be searched in vain in the system's fonts
/// before the search is given up.
const MAX_FALLBACK_MISSES: usize = 64;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_font::TestFont;

    #[test]
    fn a_missing_character_is_looked_up_in_the_other_faces() {
        let mut db = Database::new();
        for (family, chars, weight) in [
            ("Plain", "ab", 400),
            ("Wide", "abж", 700),
            ("Wide", "abж", 400),
            ("Other", "abжé", 400),
        ] {
            let font = TestFont {
                family,
                chars,
                weight,
                italic: false,
            };
            db.load_font_data(font.build());
        }
        let set = FaceSet::of(db);
        let family = |i: usize| set.faces()[i].families[0].clone();
        assert!(set.has_glyph(0, 'a') && !set.has_glyph(0, 'ж'));
        assert!(!set.has_glyph(9, 'a'));
        // The first face that has it; the regular one of its family.
        let found = set.face_with_glyph('ж', &[]).unwrap();
        assert_eq!(
            (family(found).as_str(), set.faces()[found].weight),
            ("Wide", 400)
        );
        // A preferred family comes first.
        let found = set.face_with_glyph('ж', &["Missing", "other"]).unwrap();
        assert_eq!(family(found), "Other");
        let found = set.face_with_glyph('é', &["Wide"]).unwrap();
        assert_eq!(family(found), "Other");
        assert_eq!(set.face_with_glyph('Ω', &["Wide"]), None);
        // The style asked for, in the family found.
        let regular = set.face_with_glyph('ж', &[]).unwrap();
        let bold = set.styled(regular, 'ж', true, false);
        assert_eq!(set.faces()[bold.index].weight, 700);
        assert!(!bold.embolden && !bold.slant);
        let italic = set.styled(regular, 'ж', false, true);
        assert_eq!((italic.index, italic.slant), (regular, true));
        let other = set.face_with_glyph('é', &[]).unwrap();
        let imitated = set.styled(other, 'é', true, true);
        assert_eq!(
            (imitated.index, imitated.embolden, imitated.slant),
            (other, true, true)
        );
    }

    fn arial_family() -> Vec<Face> {
        vec![
            Face::new("Arial", 400, false),
            Face::new("Arial", 700, false),
            Face::new("Arial", 400, true),
            Face::new("Arial", 700, true),
            Face::new("Arial Narrow", 700, false),
            Face::new("Times New Roman", 400, false),
        ]
    }

    fn pick(faces: &[Face], family: &str, bold: bool, italic: bool) -> (usize, bool, bool) {
        let choice = choose_face(faces, Some(family), bold, italic).unwrap();
        (choice.index, choice.embolden, choice.slant)
    }

    #[test]
    fn the_exact_style_of_the_family_is_chosen() {
        let faces = arial_family();
        assert_eq!(pick(&faces, "Arial", false, false), (0, false, false));
        assert_eq!(pick(&faces, "arial", true, false), (1, false, false));
        assert_eq!(pick(&faces, "ARIAL", false, true), (2, false, false));
        assert_eq!(pick(&faces, "Arial", true, true), (3, false, false));
        assert_eq!(
            pick(&faces, "Arial Narrow", false, false),
            (4, false, false)
        );
        assert_eq!(choose_face(&faces, Some("Verdana"), true, false), None);
        assert_eq!(choose_face(&[], Some("Arial"), false, false), None);
    }

    #[test]
    fn the_closest_style_is_chosen_and_the_rest_imitated() {
        // One regular face: bold and italic are imitated.
        let one = [Face::new("Cyber", 400, false)];
        assert_eq!(pick(&one, "Cyber", true, true), (0, true, true));
        assert_eq!(pick(&one, "Cyber", true, false), (0, true, false));
        assert_eq!(pick(&one, "Cyber", false, true), (0, false, true));
        assert_eq!(pick(&one, "Cyber", false, false), (0, false, false));
        // The weight comes first: bold italic without a bold italic face
        // is the bold face, slanted.
        let three = [
            Face::new("A", 400, false),
            Face::new("A", 400, true),
            Face::new("A", 700, false),
        ];
        assert_eq!(pick(&three, "A", true, true), (2, false, true));
        assert_eq!(pick(&three, "A", false, true), (1, false, false));
        // Nothing thins a bold face or straightens an italic one.
        let heavy = [Face::new("B", 900, true)];
        assert_eq!(pick(&heavy, "B", false, false), (0, false, false));
        // A semi-bold face is bold enough; a medium one is not.
        assert_eq!(
            pick(&[Face::new("C", 600, false)], "C", true, false),
            (0, false, false)
        );
        assert_eq!(
            pick(&[Face::new("C", 500, false)], "C", true, false),
            (0, true, false)
        );
        // Without a family, every face is a candidate.
        let choice = choose_face(&three, None, true, false).unwrap();
        assert_eq!(choice.index, 2);
        // A family name in another language of the file counts.
        let named = [Face {
            families: vec!["Gothic".into(), "ゴシック".into()],
            weight: 400,
            italic: false,
        }];
        assert_eq!(pick(&named, "ゴシック", false, false), (0, false, false));
    }

    #[test]
    fn the_games_file_comes_first_then_the_system_then_substitutes() {
        let game = [Face::new("Bitstream Cyberbit", 400, false)];
        let system = arial_family();
        // The game's file is not Arial: the system's Arial Bold.
        assert_eq!(
            resolve_face(&game, &system, "Arial", true, false),
            Some((
                FaceOrigin::System,
                FaceChoice {
                    index: 1,
                    embolden: false,
                    slant: false
                }
            ))
        );
        // The file is the face asked for: it is used, bold imitated,
        // even though the system has the family too.
        let system_too = [Face::new("Bitstream Cyberbit", 700, false)];
        assert_eq!(
            resolve_face(&game, &system_too, "bitstream cyberbit", true, false),
            Some((
                FaceOrigin::Game,
                FaceChoice {
                    index: 0,
                    embolden: true,
                    slant: false
                }
            ))
        );
        // A system without Arial draws with its metric twin.
        let linux = [
            Face::new("DejaVu Sans", 400, false),
            Face::new("Liberation Sans", 400, false),
            Face::new("Liberation Sans", 700, true),
        ];
        assert_eq!(
            resolve_face(&game, &linux, "Arial", true, true),
            Some((
                FaceOrigin::System,
                FaceChoice {
                    index: 2,
                    embolden: false,
                    slant: false
                }
            ))
        );
        // No such family anywhere, and no substitute: the caller's
        // fallback.
        assert_eq!(resolve_face(&game, &linux, "Papyrus", false, false), None);
        assert_eq!(resolve_face(&[], &[], "Arial", false, false), None);
    }

    #[test]
    fn substitutes_start_with_the_family_itself() {
        assert_eq!(family_and_substitutes("Papyrus"), ["Papyrus"]);
        let arial = family_and_substitutes("arial");
        assert_eq!(arial[0], "arial");
        assert_eq!(arial[1], "Liberation Sans");
        assert!(!arial[1..].iter().any(|n| n.eq_ignore_ascii_case("arial")));
        assert_eq!(family_and_substitutes("Courier New")[1], "Liberation Mono");
    }

    #[test]
    fn bytes_that_are_not_a_font_have_no_face() {
        let set = FaceSet::from_bytes(b"not a font".to_vec());
        assert!(set.faces().is_empty());
        assert_eq!(set.with_data(0, |_, _| ()), None);
        assert_eq!(set.path(0), None);
    }
}
