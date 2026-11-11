//! The `Directory` object (`CSXDirectory`), the global `Directory`.
//!
//! Written with reference to `SXDirectory.cpp` of Wintermute Engine 1.x
//! (MIT License, Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.
//!
//! The original works on the Windows file system at large: it creates,
//! deletes and lists any directory, reads the process's working directory
//! and the system's temporary directory. Here every path a script gives
//! resolves inside the directory the host allows writes in (the save
//! directory of the adventure layer, which is what `CurrentDirectory`
//! names), and a listing adds the game's own files to what that directory
//! holds, the way `File` reads a file a script wrote first and the game's
//! files after. The object keeps no state of its own: it is one object
//! for the whole game, like `Math`.

use std::fs;
use std::path::{MAIN_SEPARATOR, Path};

use super::{ArrayObject, ClassHost, ClassObject, arg};
use crate::host::Return;
use crate::value::Value;

/// An entry of a directory, as the host lists it for scripts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// The file or directory name, without its path.
    pub name: String,
    /// Whether it is a directory.
    pub is_dir: bool,
}

/// The `Directory` object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DirectoryObject;

/// The path separator scripts see (`PathSeparator`): the original's,
/// which every file name in the engine accepts along with `/`.
pub const PATH_SEPARATOR: &str = "\\";

/// The name of the temporary directory under the host's writable
/// directory (`TempDirectory`).
pub const TEMP_DIRECTORY: &str = "temp";

impl DirectoryObject {
    /// A built-in property (`ScGetProperty`): `None` for a name the class
    /// does not know, which the host looks up among the script
    /// properties, as the base class does.
    pub fn get_property<H: ClassHost + ?Sized>(&self, name: &str, host: &H) -> Option<Value> {
        Some(match name {
            "Type" => Value::from("directory"),
            "PathSeparator" => Value::from(PATH_SEPARATOR),
            "CurrentDirectory" => Value::from(path_text(&host.writable_root(), true)),
            "TempDirectory" => {
                // The system's temporary directory in the original, which
                // always exists; here a directory of the writable root,
                // created on demand. The original's `GetTempPath` ends
                // with a separator, and scripts append file names to it.
                let dir = host.writable_root().join(TEMP_DIRECTORY);
                if fs::create_dir_all(&dir).is_err() {
                    return Some(Value::Null);
                }
                Value::from(path_text(&dir, true))
            }
            _ => return None,
        })
    }

    /// Writes a property (`ScSetProperty`): `true` when the class took
    /// it. `CurrentDirectory` changes the process's working directory in
    /// the original; here scripts' paths resolve inside the writable
    /// directory whatever it is set to, so the set is accepted and
    /// ignored.
    pub fn set_property(&mut self, name: &str) -> bool {
        name == "CurrentDirectory"
    }

    /// A method call (`ScCallMethod`); `None` when the class has no such
    /// method.
    pub fn call_method<H: ClassHost>(
        &mut self,
        name: &str,
        args: &[Value],
        host: &mut H,
    ) -> Option<Return> {
        Some(match name {
            // `CBUtils::CreatePath` then `mkdir`: every level is created.
            // `CreatePath` makes the directories up to each separator
            // and the answer is the result of the final `mkdir` on the
            // whole path: true when that call made the directory, false
            // when it existed already, and false for a path ending with
            // a separator, whose last level `CreatePath` has just made.
            "Create" => {
                let path = arg(args, 0).get_string(host);
                let made_by_create_path = path.ends_with(['\\', '/']);
                let created = host.writable_dir(&path).is_some_and(|dir| {
                    let existed = dir.is_dir();
                    fs::create_dir_all(&dir).is_ok()
                        && dir.is_dir()
                        && !existed
                        && !made_by_create_path
                });
                Return::value(created)
            }
            // `rmdir`: only an empty directory goes.
            "Delete" => {
                let path = arg(args, 0).get_string(host);
                let deleted = host
                    .writable_dir(&path)
                    .is_some_and(|dir| fs::remove_dir(dir).is_ok());
                Return::value(deleted)
            }
            "GetFiles" | "GetDirectories" => {
                let path = arg(args, 0).get_string(host);
                let mask = match arg(args, 1) {
                    Value::Null => "*.*".to_string(),
                    other => other.get_string(host),
                };
                let directories = name == "GetDirectories";
                let names: Vec<Value> = list(&path, host)
                    .into_iter()
                    .filter(|e| e.is_dir == directories)
                    // The original skips the `.` and `..` entries of a
                    // directory listing this way, and with them every
                    // hidden directory.
                    .filter(|e| !(directories && e.name.starts_with('.')))
                    .filter(|e| mask_matches(&mask, &e.name))
                    .map(|e| Value::from(e.name))
                    .collect();
                let id = host.create_object(ClassObject::Array(ArrayObject::from_values(names)));
                Return::value(Value::Native(id))
            }
            // The Windows drive letters; there are none to list elsewhere.
            "GetDrives" => {
                let id = host.create_object(ClassObject::Array(ArrayObject::default()));
                Return::value(Value::Native(id))
            }
            _ => return None,
        })
    }
}

/// The entries of a directory: what the writable directory holds, then
/// the game's own entries the host adds, a name appearing once (the
/// writable copy first), sorted without regard to case.
fn list<H: ClassHost>(path: &str, host: &mut H) -> Vec<DirEntry> {
    let mut entries: Vec<DirEntry> = Vec::new();
    if let Some(dir) = host.writable_dir(path) {
        if let Ok(read) = fs::read_dir(&dir) {
            for entry in read.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
                entries.push(DirEntry { name, is_dir });
            }
        }
    }
    for entry in host.game_entries(path) {
        if !entries
            .iter()
            .any(|e| e.name.eq_ignore_ascii_case(&entry.name))
        {
            entries.push(entry);
        }
    }
    entries.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    entries
}

/// Whether a file name matches a Windows wildcard mask (`_findfirst`):
/// `*` stands for any run of characters, `?` for one, letters compare
/// without case, and `*.*` matches every name, extension or not.
pub fn mask_matches(mask: &str, name: &str) -> bool {
    if mask == "*.*" || mask.is_empty() {
        return true;
    }
    let mask: Vec<char> = mask.chars().flat_map(char::to_lowercase).collect();
    let name: Vec<char> = name.chars().flat_map(char::to_lowercase).collect();
    glob(&mask, &name)
}

fn glob(mask: &[char], name: &[char]) -> bool {
    match mask.split_first() {
        None => name.is_empty(),
        Some(('*', rest)) => (0..=name.len()).any(|skip| glob(rest, &name[skip..])),
        Some(('?', rest)) => !name.is_empty() && glob(rest, &name[1..]),
        Some((c, rest)) => name.first() == Some(c) && glob(rest, &name[1..]),
    }
}

/// A path as a script sees it, with the host's separators and, when
/// asked, a trailing one, as the original's `GetTempPath` gives.
fn path_text(path: &Path, trailing_separator: bool) -> String {
    let mut text = path.to_string_lossy().into_owned();
    if trailing_separator && !text.ends_with(['/', '\\']) {
        text.push(MAIN_SEPARATOR);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_match_like_findfirst() {
        assert!(mask_matches("*.*", "readme"));
        assert!(mask_matches("*.*", "a.txt"));
        assert!(mask_matches("*", "a.txt"));
        assert!(mask_matches("*.TXT", "notes.txt"));
        assert!(!mask_matches("*.txt", "notes.text"));
        assert!(mask_matches("save?.dsav", "save1.dsav"));
        assert!(!mask_matches("save?.dsav", "save12.dsav"));
        assert!(mask_matches("s*e*.d*", "save12.dsav"));
        assert!(!mask_matches("b*", "abc"));
        assert!(mask_matches("", "anything"));
    }
}
