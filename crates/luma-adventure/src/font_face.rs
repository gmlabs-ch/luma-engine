//! Choosing the face a TrueType font is drawn with: the typeface a
//! `.font` file names with `FACE`, in the weight and slant its `BOLD` and
//! `ITALIC` ask for, among the faces of the font file the game ships and
//! those installed on the system (see `docs/formats/font.md`).
//!
//! Written with reference to `BFontTT.cpp` and `BFontStorage.cpp` of
//! Wintermute Engine 1.x (MIT License, Copyright (c) 2003-2013 Jan
//! Nedoma); see `NOTICE`. The original engine registers the game's font
//! file with Windows and asks GDI for a font by typeface name, weight and
//! italic flag (`CBFontTT::CreateWinFont`); the choice among the faces
//! found, which GDI's font mapper makes there, is Luma Engine's own.

use std::sync::OnceLock;

use fontdb::{Database, ID, Source, Style};

/// The weight of a regular face.
pub const REGULAR: u16 = 400;
/// The weight of a bold face (`FW_BOLD`).
pub const BOLD: u16 = 700;
/// The weight from which a face counts as bold already.
const BOLD_ENOUGH: u16 = 600;

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

    /// The file a face comes from, when it comes from one.
    pub fn path(&self, index: usize) -> Option<String> {
        let info = self.db.face(*self.ids.get(index)?)?;
        match &info.source {
            Source::File(path) | Source::SharedFile(path, _) => Some(path.display().to_string()),
            Source::Binary(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
