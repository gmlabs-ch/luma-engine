//! The game's persistent private settings (`Game.RegReadNumber`,
//! `RegReadString`, `RegWriteNumber`, `RegWriteString`): values a game
//! keeps between runs, such as its options, and the settings the
//! engine keeps for itself. The original engine writes them to the
//! Windows registry, and reads them from the `wme.ini` beside the game
//! first, then from the registry. Luma keeps them in `settings.ini` in
//! the save directory, under the registry's section names, and reads a
//! game's own `wme.ini` for the values never written, so that a value
//! written is always read back. See `docs/saving.md`.
//!
//! Written with reference to `BRegistry.cpp` and `BGame.cpp`
//! (`RegReadNumber`, `RegReadString`, `RegWriteNumber`,
//! `RegWriteString`) of Wintermute Engine 1.x (MIT License, Copyright
//! (c) 2003-2013 Jan Nedoma); see `NOTICE`.

use std::path::PathBuf;

use crate::world::World;

/// The file holding the settings, in the save directory.
pub const SETTINGS_INI: &str = "settings.ini";

/// The section the script-visible settings live in.
pub const PRIVATE_SECTION: &str = "PrivateSettings";

/// The settings file a game may ship in its directory, which presets
/// the values the player's own file lacks (`CBRegistry::m_IniName`).
pub const GAME_INI: &str = "wme.ini";

/// The largest `wme.ini` read; a settings file is a few lines.
const GAME_INI_MAX: u64 = 1 << 20;

/// A small INI file: sections of `key=value` lines. Names compare
/// without case, as Windows reads them; the file keeps its order and
/// the spelling it was written with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ini {
    sections: Vec<(String, Vec<(String, String)>)>,
}

impl Ini {
    /// Parses the text of an INI file. Lines starting with `;` or `#`
    /// are comments; lines before the first section go to the unnamed
    /// section; a line without `=` is kept as a key with an empty value.
    pub fn parse(text: &str) -> Ini {
        let mut ini = Ini::default();
        let mut current = String::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                current = name.trim().to_string();
                ini.section_mut(&current);
                continue;
            }
            let (key, value) = match line.split_once('=') {
                Some((k, v)) => (k.trim(), v.trim()),
                None => (line, ""),
            };
            ini.section_mut(&current)
                .push((key.to_string(), value.to_string()));
        }
        ini
    }

    fn section_mut(&mut self, name: &str) -> &mut Vec<(String, String)> {
        let index = match self
            .sections
            .iter()
            .position(|(n, _)| n.eq_ignore_ascii_case(name))
        {
            Some(i) => i,
            None => {
                self.sections.push((name.to_string(), Vec::new()));
                self.sections.len() - 1
            }
        };
        &mut self.sections[index].1
    }

    /// A value, if the section has the key.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(section))
            .and_then(|(_, entries)| {
                entries
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(key))
                    .map(|(_, v)| v.as_str())
            })
    }

    /// Sets a value, replacing the key's or adding it at the end of its
    /// section.
    pub fn set(&mut self, section: &str, key: &str, value: &str) {
        let entries = self.section_mut(section);
        match entries
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
        {
            Some((_, v)) => *v = value.to_string(),
            None => entries.push((key.to_string(), value.to_string())),
        }
    }

    /// The file's text.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for (name, entries) in &self.sections {
            if !name.is_empty() {
                out.push('[');
                out.push_str(name);
                out.push_str("]\n");
            }
            for (key, value) in entries {
                out.push_str(key);
                out.push('=');
                out.push_str(value);
                out.push('\n');
            }
        }
        out
    }
}

impl World {
    /// The settings file (`settings.ini` in the save directory).
    pub fn settings_ini_path(&mut self) -> PathBuf {
        self.checked_save_directory().join(SETTINGS_INI)
    }

    fn read_ini(&mut self) -> Ini {
        let path = self.settings_ini_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => Ini::parse(&text),
            Err(_) => Ini::default(),
        }
    }

    /// The `wme.ini` in the game's directory, empty when the game has
    /// none. Its bytes are the game's ANSI text, one character each.
    fn read_game_ini(&self) -> Ini {
        let path = self.game_directory().join(GAME_INI);
        let small = std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= GAME_INI_MAX);
        if !small {
            return Ini::default();
        }
        match std::fs::read(&path) {
            Ok(bytes) => Ini::parse(&bytes.iter().map(|b| char::from(*b)).collect::<String>()),
            Err(_) => Ini::default(),
        }
    }

    /// A text setting (`CBRegistry::ReadString`): the value the
    /// player's `settings.ini` holds, else the game's `wme.ini` preset,
    /// else `init`. (The original looks in `wme.ini` first and never
    /// writes there, so a preset hides whatever the game writes
    /// afterwards; in Luma a value written is read back.)
    pub fn registry_string(&mut self, section: &str, key: &str, init: &str) -> String {
        self.read_engine_setting(section, key)
            .unwrap_or_else(|| init.to_string())
    }

    /// A number setting (`CBRegistry::ReadInt`): the leading integer of
    /// the value, as `GetPrivateProfileInt` reads it, from the player's
    /// `settings.ini`, else from the game's `wme.ini`, else `init`.
    pub fn registry_int(&mut self, section: &str, key: &str, init: i32) -> i32 {
        self.read_engine_int(section, key).unwrap_or(init)
    }

    /// A private setting as stored (`RegReadString` without its
    /// default), `None` when it was never written.
    pub fn read_setting(&mut self, key: &str) -> Option<String> {
        self.read_ini()
            .get(PRIVATE_SECTION, key)
            .map(str::to_string)
    }

    /// Stores a private setting (`RegWriteString`, `RegWriteNumber`);
    /// whether the file could be written.
    pub fn write_setting(&mut self, key: &str, value: &str) -> bool {
        self.write_engine_setting(PRIVATE_SECTION, key, value)
    }

    /// A value the engine itself keeps between runs (`CBRegistry::
    /// ReadString` for a section of its own, such as `Video`): the
    /// player's `settings.ini` first, then the game's `wme.ini`; `None`
    /// when neither has the key.
    pub fn read_engine_setting(&mut self, section: &str, key: &str) -> Option<String> {
        if let Some(value) = self.read_ini().get(section, key) {
            return Some(value.to_string());
        }
        self.read_game_ini().get(section, key).map(str::to_string)
    }

    /// A number the engine keeps between runs (`CBRegistry::ReadInt`):
    /// the leading integer of the value, as `GetPrivateProfileInt`
    /// reads it; `None` when the key is absent.
    pub fn read_engine_int(&mut self, section: &str, key: &str) -> Option<i32> {
        self.read_engine_setting(section, key)
            .map(|value| luma_script::atoi(&value))
    }

    /// Stores a value the engine keeps between runs (`CBRegistry::
    /// WriteString`, `WriteInt`); whether the file could be written.
    pub fn write_engine_setting(&mut self, section: &str, key: &str, value: &str) -> bool {
        let mut ini = self.read_ini();
        ini.set(section, key, value);
        let path = self.settings_ini_path();
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, ini.to_text()));
        match written {
            Ok(()) => true,
            Err(err) => {
                self.log(format!("cannot write '{}': {err}", path.display()));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_round_trips_and_compares_names_without_case() {
        let mut ini = Ini::parse(
            "; a comment\nloose=1\n[PrivateSettings]\nVolume = 80\nName=Bob=Smith\n\n[System]\nMostRecentSaveSlot=3\n",
        );
        assert_eq!(ini.get("", "loose"), Some("1"));
        assert_eq!(ini.get("privatesettings", "VOLUME"), Some("80"));
        assert_eq!(ini.get("PrivateSettings", "Name"), Some("Bob=Smith"));
        assert_eq!(ini.get("PrivateSettings", "Missing"), None);
        assert_eq!(ini.get("Other", "Volume"), None);
        ini.set("privatesettings", "volume", "50");
        ini.set("PrivateSettings", "Lang", "fr");
        ini.set("New", "k", "v");
        assert_eq!(
            ini.to_text(),
            "loose=1\n[PrivateSettings]\nVolume=50\nName=Bob=Smith\nLang=fr\n[System]\nMostRecentSaveSlot=3\n[New]\nk=v\n"
        );
        assert_eq!(Ini::parse(&ini.to_text()), ini);
        assert_eq!(Ini::parse("").to_text(), "");
    }
}
