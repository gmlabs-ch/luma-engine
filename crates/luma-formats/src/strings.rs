//! String tables (`string.tab`): the game's localised text, looked up by
//! references of the form `/ID/fallback text`.
//!
//! Written with reference to `BStringTable.cpp` of WME Lite (MIT License,
//! Copyright (c) 2011 Jan Nedoma); see `NOTICE`.
//!
//! The file has one entry per line: an identifier, a tab (or a space) and
//! the text, in which `|` stands for a line break. Lines starting with `;`
//! are comments. A UTF-8 byte order mark switches the game to UTF-8 text.
//! Identifiers are case-insensitive.

use std::collections::HashMap;

/// A loaded string table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StringTable {
    strings: HashMap<String, String>,
    /// Whether the last file loaded started with a UTF-8 byte order mark.
    pub utf8: bool,
    /// Whether the table declared `@right-to-left`.
    pub right_to_left: bool,
}

impl StringTable {
    /// An empty table.
    pub fn new() -> StringTable {
        StringTable::default()
    }

    /// A table loaded from one file.
    pub fn parse(bytes: &[u8]) -> StringTable {
        let mut table = StringTable::new();
        table.load(bytes, true);
        table
    }

    /// Loads a file into the table, replacing (`clear_old`) or extending
    /// the current entries. Returns how many entries the file defined.
    pub fn load(&mut self, bytes: &[u8], clear_old: bool) -> usize {
        if clear_old {
            self.strings.clear();
        }
        let bytes = match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
            Some(rest) => {
                self.utf8 = true;
                rest
            }
            None => {
                self.utf8 = false;
                bytes
            }
        };
        let mut count = 0;
        for raw in bytes.split(|&b| b == b'\n' || b == 0) {
            let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
            let line: String = raw.iter().map(|&b| char::from(b)).collect();
            if line.starts_with(';') || line.is_empty() {
                continue;
            }
            let (key, value) = match line.find('\t').or_else(|| line.find(' ')) {
                Some(at) => (&line[..at], line[at + 1..].replace('|', "\n")),
                None => (line.as_str(), String::new()),
            };
            self.add(key, &value);
            count += 1;
        }
        count
    }

    /// Adds or replaces an entry.
    pub fn add(&mut self, key: &str, value: &str) {
        if key.eq_ignore_ascii_case("@right-to-left") {
            self.right_to_left = true;
            return;
        }
        self.strings
            .insert(key.to_ascii_lowercase(), value.to_string());
    }

    /// The text of an identifier.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.strings
            .get(&key.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Whether the table has no entry.
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// The identifier of a `/ID/fallback` reference, lower-cased
    /// (`CBStringTable::GetKey`); when the table's text for it is itself
    /// a reference, that one's identifier. `None` for plain text.
    pub fn key_of(&self, text: &str) -> Option<String> {
        let mut text = text.to_string();
        for _ in 0..16 {
            let rest = text.strip_prefix('/')?;
            let at = rest.find('/')?;
            let key = rest[..at].to_ascii_lowercase();
            match self.get(&key) {
                Some(value) if value.starts_with('/') && value[1..].contains('/') => {
                    text = value.to_string();
                }
                _ => return Some(key),
            }
        }
        None
    }

    /// Resolves a `/ID/fallback` reference: the table's text for `ID`, or
    /// the fallback when the table has none. Text not starting with `/`
    /// is returned as is, and a resolved text that is itself a reference
    /// is resolved again.
    pub fn expand(&self, text: &str) -> String {
        let mut text = text.to_string();
        for _ in 0..16 {
            let Some(rest) = text.strip_prefix('/') else {
                break;
            };
            let Some(at) = rest.find('/') else {
                break;
            };
            let key = &rest[..at];
            let fallback = &rest[at + 1..];
            text = match self.get(key) {
                Some(value) => value.to_string(),
                None => fallback.to_string(),
            };
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_entries_comments_and_line_breaks() {
        let table = StringTable::parse(
            b"; a comment\r\nCONFIG0001\ttrue\r\nSYSENG0001\tGame settings\r\nSPACE value with space\r\nMULTI\tline one|line two\r\n\r\nEMPTY\r\n",
        );
        assert_eq!(table.len(), 5);
        assert_eq!(table.get("config0001"), Some("true"));
        assert_eq!(table.get("SYSENG0001"), Some("Game settings"));
        assert_eq!(table.get("SPACE"), Some("value with space"));
        assert_eq!(table.get("MULTI"), Some("line one\nline two"));
        assert_eq!(table.get("EMPTY"), Some(""));
        assert_eq!(table.get("missing"), None);
        assert!(!table.utf8);
        assert!(!table.right_to_left);
    }

    #[test]
    fn detects_the_byte_order_mark_and_rtl() {
        let table = StringTable::parse(b"\xEF\xBB\xBF@right-to-left\nA\tb\n");
        assert!(table.utf8);
        assert!(table.right_to_left);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn load_can_extend_or_replace() {
        let mut table = StringTable::parse(b"A\t1\nB\t2\n");
        assert_eq!(table.load(b"B\t3\nC\t4\n", false), 2);
        assert_eq!(table.get("A"), Some("1"));
        assert_eq!(table.get("B"), Some("3"));
        assert_eq!(table.load(b"D\t5\n", true), 1);
        assert_eq!(table.get("A"), None);
        assert_eq!(table.get("D"), Some("5"));
    }

    #[test]
    fn finds_keys() {
        let table = StringTable::parse(b"HELLO\tBonjour\nREF\t/HELLO/x\n");
        assert_eq!(table.key_of("/HELLO/Hello").as_deref(), Some("hello"));
        assert_eq!(table.key_of("/REF/x").as_deref(), Some("hello"));
        assert_eq!(table.key_of("/NOPE/text").as_deref(), Some("nope"));
        assert_eq!(table.key_of("plain"), None);
        assert_eq!(table.key_of("/unterminated"), None);
    }

    #[test]
    fn expands_references() {
        let table = StringTable::parse(b"HELLO\tBonjour\nREF\t/HELLO/x\nLOOP\t/LOOP/y\n");
        assert_eq!(table.expand("/HELLO/Hello"), "Bonjour");
        assert_eq!(table.expand("/hello/Hello"), "Bonjour");
        assert_eq!(table.expand("/NOPE/Hello there"), "Hello there");
        assert_eq!(table.expand("/NOPE/"), "");
        assert_eq!(table.expand("plain"), "plain");
        assert_eq!(table.expand("/unterminated"), "/unterminated");
        assert_eq!(table.expand("/REF/x"), "Bonjour");
        assert_eq!(table.expand("/LOOP/z"), "/LOOP/y");
        assert_eq!(table.expand(""), "");
    }
}
