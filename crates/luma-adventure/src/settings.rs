//! The game's settings file (`startup.settings`).

use luma_archive::Vfs;
use luma_formats::Document;
use luma_formats::def::keys;

use crate::error::{Error, Result};

/// Name of the settings file at the game root.
pub const SETTINGS_FILE: &str = "startup.settings";

/// The keys of `CBGame::LoadSettings`, in the order of its table.
const SETTINGS_KEYS: &[&str] = &[
    "SETTINGS",
    "GAME",
    "STRING_TABLE",
    "RESOLUTION",
    "REQUIRE_3D_ACCELERATION",
    "REQUIRE_SOUND",
    "HWTL_MODE",
    "ALLOW_WINDOWED_MODE",
    "ALLOW_ACCESSIBILITY_TAB",
    "ALLOW_ABOUT_TAB",
    "ALLOW_ADVANCED",
    "ALLOW_DESKTOP_RES",
    "REGISTRY_PATH",
    "RICH_SAVED_GAMES",
    "SAVED_GAME_EXT",
    "GUID",
];

/// What `startup.settings` says about the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSettings {
    /// The game definition file, `default.game` by default.
    pub game_file: String,
    /// Width of the game's resolution in pixels.
    pub width: u32,
    /// Height of the game's resolution in pixels.
    pub height: u32,
    /// `STRING_TABLE`: the string tables loaded when the game starts, in
    /// the order of the file (each is added to the ones before it).
    pub string_tables: Vec<String>,
    /// `REQUIRE_SOUND`: the game refuses to start without a sound
    /// device; false by default.
    pub require_sound: bool,
    /// `ALLOW_WINDOWED_MODE`: the player may run the game in a window;
    /// true by default.
    pub allow_windowed: bool,
    /// `ALLOW_ACCESSIBILITY_TAB`: the player may turn the accessibility
    /// features on (text-to-speech, keyboard play); true by default.
    pub allow_accessibility_tab: bool,
    /// `ALLOW_DESKTOP_RES`: the player may have the game stretched to
    /// the desktop's resolution; false by default.
    pub allow_desktop_res: bool,
    /// `SAVED_GAME_EXT`: the extension of the saved game files, `dsv`
    /// when absent.
    pub saved_game_ext: Option<String>,
}

impl Default for GameSettings {
    /// The engine's defaults: `default.game` at 800x600.
    fn default() -> Self {
        GameSettings {
            game_file: "default.game".into(),
            width: 800,
            height: 600,
            string_tables: Vec::new(),
            require_sound: false,
            allow_windowed: true,
            allow_accessibility_tab: true,
            allow_desktop_res: false,
            saved_game_ext: None,
        }
    }
}

impl GameSettings {
    /// Reads the settings of a game, or the defaults when the file is
    /// absent.
    pub fn load(vfs: &Vfs) -> Result<GameSettings> {
        if !vfs.exists(SETTINGS_FILE) {
            return Ok(GameSettings::default());
        }
        let bytes = vfs.read(SETTINGS_FILE)?;
        GameSettings::parse(&bytes).map_err(|source| Error::Format {
            path: SETTINGS_FILE.into(),
            source,
        })
    }

    /// Parses the content of a settings file (`CBGame::LoadSettings`):
    /// the first statement must be the `SETTINGS` block, and a key that
    /// is not in the engine's table is a syntax error.
    pub fn parse(bytes: &[u8]) -> luma_formats::Result<GameSettings> {
        let doc = Document::parse(bytes)?;
        let root = doc
            .root("SETTINGS")
            .ok_or_else(|| luma_formats::Error::Invalid {
                reason: "'SETTINGS' keyword expected in game settings file".into(),
            })?;
        let mut settings = GameSettings::default();
        let (mut width, mut height) = (800, 600);
        for item in keys(root, SETTINGS_KEYS, "SETTINGS") {
            let (key, node) = item?;
            match key {
                "GAME" => settings.game_file = node.text(),
                "STRING_TABLE" => settings.string_tables.push(node.text()),
                "RESOLUTION" => node.read_ints(&mut [&mut width, &mut height]),
                "REQUIRE_SOUND" => node.read_bool(&mut settings.require_sound),
                "ALLOW_WINDOWED_MODE" => node.read_bool(&mut settings.allow_windowed),
                "ALLOW_ACCESSIBILITY_TAB" => {
                    node.read_bool(&mut settings.allow_accessibility_tab);
                }
                "ALLOW_DESKTOP_RES" => node.read_bool(&mut settings.allow_desktop_res),
                "SAVED_GAME_EXT" => settings.saved_game_ext = Some(node.text()),
                // The other keys configure the Windows settings dialog,
                // Direct3D and the registry.
                _ => {}
            }
        }
        // A resolution that is not positive cannot be shown: the default
        // stays (the original would fail to create its device).
        if width > 0 && height > 0 {
            settings.width = width as u32;
            settings.height = height as u32;
        }
        Ok(settings)
    }

    /// The extension of the saved game files (`SAVED_GAME_EXT`, `dsv` by
    /// default), without a dot. A value that is not a plain file
    /// extension is not used: it would let a game file choose a path.
    pub fn save_extension(&self) -> &str {
        match self.saved_game_ext.as_deref() {
            Some(ext)
                if !ext.is_empty()
                    && ext.len() <= 16
                    && ext.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') =>
            {
                ext
            }
            _ => crate::save::SAVE_EXTENSION,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_resolution_and_game_file() {
        let s = GameSettings::parse(
            b"; generated by ProjectMan\nSETTINGS {\n  GAME=\"default.game\"\n  RESOLUTION { 1024,768 }\n  REQUIRE_3D_ACCELERATION=TRUE\n}\n",
        )
        .unwrap();
        assert_eq!(s.width, 1024);
        assert_eq!(s.height, 768);
        assert!(s.string_tables.is_empty());
        assert!(s.allow_windowed, "allowed unless the game says otherwise");
        let s = GameSettings::parse(b"SETTINGS { ALLOW_WINDOWED_MODE=FALSE }").unwrap();
        assert!(!s.allow_windowed);
        let s = GameSettings::parse(
            b"SETTINGS {\n  GAME=\"default.game\"\n  STRING_TABLE=\"string.tab\"\n  STRING_TABLE=\"extra.tab\"\n  RESOLUTION { 320,200 }\n}\n",
        )
        .unwrap();
        assert_eq!(s.string_tables, ["string.tab", "extra.tab"]);
        assert_eq!(s.game_file, "default.game");

        let s = GameSettings::parse(b"SETTINGS { GAME=\"wc.game\" }").unwrap();
        assert_eq!((s.width, s.height), (800, 600));
        assert_eq!(s.game_file, "wc.game");
    }

    /// `CBGame::LoadSettings`: the keys of the table, their defaults,
    /// and the errors.
    #[test]
    fn reads_the_other_keys_and_refuses_unknown_ones() {
        let s = GameSettings::parse(b"SETTINGS { }").unwrap();
        assert!(!s.require_sound && s.allow_windowed && s.allow_accessibility_tab);
        assert!(!s.allow_desktop_res);
        assert_eq!(s.save_extension(), "dsv");

        let s = GameSettings::parse(
            b"SETTINGS {\n REQUIRE_SOUND=TRUE\n ALLOW_WINDOWED_MODE=FALSE\n ALLOW_ACCESSIBILITY_TAB=FALSE\n ALLOW_DESKTOP_RES=TRUE\n SAVED_GAME_EXT=\"sav\"\n HWTL_MODE=1\n ALLOW_ABOUT_TAB=TRUE\n ALLOW_ADVANCED=TRUE\n REGISTRY_PATH=\"Company\\Game\"\n RICH_SAVED_GAMES=TRUE\n GUID=\"{665ADE9A-1727}\"\n}",
        )
        .unwrap();
        assert!(s.require_sound && !s.allow_windowed && !s.allow_accessibility_tab);
        assert!(s.allow_desktop_res);
        assert_eq!(s.save_extension(), "sav");

        // An extension that is not a plain one is not used.
        let s = GameSettings::parse(b"SETTINGS { SAVED_GAME_EXT=\"../x\" }").unwrap();
        assert_eq!(s.save_extension(), "dsv");

        assert!(GameSettings::parse(b"").is_err());
        assert!(GameSettings::parse(b"GAME { }").is_err());
        assert!(GameSettings::parse(b"SETTINGS { BOGUS=1 }").is_err());
        // `ALLOW_ACCESSIBILITY_TAB` is read here, not in the game file.
        assert!(luma_formats::GameFile::parse(b"GAME { ALLOW_ACCESSIBILITY_TAB=FALSE }").is_err());
    }
}
