//! The game's persistent private settings (`Game.RegReadNumber`,
//! `RegReadString`, `RegWriteNumber`, `RegWriteString`): values a game
//! keeps between runs, such as its options. The original engine writes
//! them to the `PrivateSettings` section of its `wme.ini` or to the
//! Windows registry; Luma keeps them in `settings.ini` in the save
//! directory, in the same section, so that a game's own values can be
//! carried over by hand. See `docs/saving.md`.
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
        let mut ini = self.read_ini();
        ini.set(PRIVATE_SECTION, key, value);
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
