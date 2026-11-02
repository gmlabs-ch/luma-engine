//! The script library classes: `String`, `Array`, `Date`, `File` and
//! `MemBuffer`, the objects a script creates with `new`, and the global
//! `Directory` object.
//!
//! Written with reference to `SXString.cpp`, `SXArray.cpp`, `SXDate.cpp`,
//! `SXFile.cpp`, `SXMemBuffer.cpp` and `SXDirectory.cpp` of Wintermute
//! Engine 1.x (MIT License, Copyright (c) 2003-2013 Jan Nedoma); see
//! `NOTICE`.
//!
//! The classes are plain data with the engine's semantics: the host owns
//! the objects (a [`ClassObject`] per native handle), creates them from
//! its constructor function (`new String(...)` reaches
//! [`ClassObject::construct`] through `Host::call_function`; the host
//! makes the one `Directory` object itself and publishes it as a global)
//! and forwards property reads, writes, method calls and conversions to
//! them. What the classes need from the embedding program, files, the
//! clock and new handles, goes through the [`ClassHost`] trait.
//!
//! Strings are sequences of `char`s: every index, length and position
//! counts characters, which is what the original engine does with a
//! UTF-8 game and, since a Latin-1 game's bytes are one character each,
//! with an ANSI game too. Text written to and read from files is
//! Latin-1, as the rest of the engine reads game files.

use std::cmp::Ordering;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::host::{Context, Return};
use crate::value::{NativeId, Natives, Value};

mod array;
mod date;
mod directory;
mod file;
mod membuffer;
mod string;

pub use array::ArrayObject;
pub use date::DateObject;
pub use directory::{DirEntry, DirectoryObject, PATH_SEPARATOR, TEMP_DIRECTORY, mask_matches};
pub use file::{FileObject, Mode};
pub use membuffer::MemBuffer;
pub use string::StringObject;

/// What the library classes need from the program embedding them.
pub trait ClassHost: Natives {
    /// The bytes of a file a script may read (`File.OpenAsText(1)`,
    /// `File.Copy`): a file written by a script first, then the game's
    /// own files. `None` when there is no such file.
    fn read_file(&mut self, name: &str) -> Option<Vec<u8>>;

    /// Where a script may write, delete or copy to a file of this name:
    /// a path under the game's writable directory, or `None` when the
    /// name is not acceptable (an absolute path, a parent directory).
    /// [`relative_path`] does the checking for a host.
    fn writable_path(&mut self, name: &str) -> Option<PathBuf>;

    /// The game's writable directory itself, the one [`writable_path`]
    /// resolves under (`Directory.CurrentDirectory`, the root of
    /// `Directory.TempDirectory`).
    ///
    /// [`writable_path`]: ClassHost::writable_path
    fn writable_root(&self) -> PathBuf;

    /// A directory a script may create, delete or list, by the name it
    /// gives: the writable directory itself for an empty name, else
    /// [`writable_path`]. A host that accepts other spellings of its root
    /// overrides this.
    ///
    /// [`writable_path`]: ClassHost::writable_path
    fn writable_dir(&mut self, name: &str) -> Option<PathBuf> {
        if name.trim_matches(['\\', '/', ' ']).is_empty() || name == "." {
            Some(self.writable_root())
        } else {
            self.writable_path(name)
        }
    }

    /// The game's own entries of a directory (`Directory.GetFiles`,
    /// `GetDirectories`): the files and sub-directories of the packages
    /// and loose files under `dir`, the directory of the game for an
    /// empty name. Listed after what the writable directory holds. The
    /// default knows no game files.
    fn game_entries(&mut self, _dir: &str) -> Vec<DirEntry> {
        Vec::new()
    }

    /// The wall clock for `new Date()` and `Date.SetCurrentTime`: seconds
    /// since 1970-01-01 00:00:00 **in the local time zone**, that is the
    /// Unix time plus the zone's offset from UTC. The default knows no
    /// zone and gives UTC.
    fn local_time(&self) -> i64 {
        utc_now()
    }

    /// Gives a handle to an object a method created (`String.Split`
    /// returns a new `Array`).
    fn create_object(&mut self, object: ClassObject) -> NativeId;

    /// A line for the game's log: the runtime errors the classes report
    /// (worded by [`Context::runtime_error`]) and their warnings.
    fn log(&mut self, message: &str);
}

/// The Unix time, in seconds.
pub fn utc_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// An object of one of the library classes.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ClassObject {
    /// `new String(...)`.
    String(StringObject),
    /// `new Array(...)`.
    Array(ArrayObject),
    /// `new Date(...)`.
    Date(DateObject),
    /// `new File(...)`.
    File(FileObject),
    /// `new MemBuffer(...)`.
    MemBuffer(MemBuffer),
    /// The global `Directory` object; one per game.
    Directory(DirectoryObject),
}

impl ClassObject {
    /// The constructor names (`new Name(...)`); `Directory` is not one,
    /// the host makes the single object with [`ClassObject::directory`].
    pub const NAMES: [&'static str; 5] = ["String", "Array", "Date", "File", "MemBuffer"];

    /// Whether `name` is one of the library classes.
    pub fn is_class(name: &str) -> bool {
        ClassObject::NAMES.contains(&name)
    }

    /// The `Directory` object, for the host to publish as the global of
    /// that name.
    pub fn directory() -> ClassObject {
        ClassObject::Directory(DirectoryObject)
    }

    /// Creates an object (`new Name(args)`); `None` when `name` is not a
    /// library class. The host registers the object and answers the call
    /// with `Return::constructed`.
    pub fn construct<H: ClassHost>(
        name: &str,
        args: &[Value],
        host: &mut H,
    ) -> Option<ClassObject> {
        Some(match name {
            "String" => ClassObject::String(StringObject::new(args, host)),
            "Array" => ClassObject::Array(ArrayObject::new(args, host)),
            "Date" => ClassObject::Date(DateObject::new(args, host)),
            "File" => ClassObject::File(FileObject::new(args, host)),
            "MemBuffer" => ClassObject::MemBuffer(MemBuffer::new(args, host)),
            _ => return None,
        })
    }

    /// The `Type` property: `string`, `array`, `date`, `file`,
    /// `membuffer` or `directory`. Also the class name natives compare by.
    pub fn type_name(&self) -> &'static str {
        match self {
            ClassObject::String(_) => "string",
            ClassObject::Array(_) => "array",
            ClassObject::Date(_) => "date",
            ClassObject::File(_) => "file",
            ClassObject::MemBuffer(_) => "membuffer",
            ClassObject::Directory(_) => "directory",
        }
    }

    /// The string form (`ScToString`): the text of a string, the elements
    /// of an array joined by commas, a date as C's `asctime` prints it,
    /// a file's name, `[membuffer object]`, `[native object]` for the
    /// directory. The integer, float and boolean forms are the base
    /// class's: `0`, `0.0` and `false`.
    pub fn to_string(&self, natives: &dyn Natives) -> String {
        match self {
            ClassObject::String(s) => s.text().to_string(),
            ClassObject::Array(a) => a.to_string(natives),
            ClassObject::Date(d) => d.to_string(),
            ClassObject::File(f) => f.to_string(),
            ClassObject::MemBuffer(_) => "[membuffer object]".to_string(),
            ClassObject::Directory(_) => "[native object]".to_string(),
        }
    }

    /// Orders two objects of the same class (`ScCompare`), given the
    /// order of their handles: strings by their text, dates by their
    /// time, memory buffers equal only when they are the same object
    /// and "greater" otherwise, arrays and files by handle. Objects of
    /// different classes order by class name.
    pub fn compare(&self, other: &ClassObject, handles: Ordering) -> Ordering {
        match (self, other) {
            (ClassObject::String(a), ClassObject::String(b)) => a.text().cmp(b.text()),
            (ClassObject::Date(a), ClassObject::Date(b)) => a.cmp(b),
            (ClassObject::MemBuffer(_), ClassObject::MemBuffer(_)) => MemBuffer::compare(handles),
            _ if self.type_name() == other.type_name() => handles,
            _ => self.type_name().cmp(other.type_name()),
        }
    }

    /// A built-in property (`ScGetProperty`). `None` means the class
    /// knows no such property and the host looks among the properties
    /// scripts set on the object, as the base class does for files,
    /// memory buffers and the directory; strings, arrays and dates answer
    /// every name (an unknown one reads `null`). The host answers the
    /// directory's paths.
    pub fn get_property<H: ClassHost + ?Sized>(&self, name: &str, host: &H) -> Option<Value> {
        match self {
            ClassObject::String(s) => Some(s.get_property(name)),
            ClassObject::Array(a) => Some(a.get_property(name)),
            ClassObject::Date(d) => Some(d.get_property(name)),
            ClassObject::File(f) => f.get_property(name),
            ClassObject::MemBuffer(m) => m.get_property(name),
            ClassObject::Directory(d) => d.get_property(name, host),
        }
    }

    /// Writes a property (`ScSetProperty`). Returns `true` when the class
    /// took it (or dropped it: strings, arrays and dates keep no script
    /// properties), `false` when the host should store it as a script
    /// property of the object.
    pub fn set_property<H: ClassHost>(&mut self, name: &str, value: &Value, host: &mut H) -> bool {
        match self {
            ClassObject::String(s) => {
                s.set_property(name, value, host);
                true
            }
            ClassObject::Array(a) => {
                a.set_property(name, value, host);
                true
            }
            ClassObject::Date(_) => true,
            ClassObject::Directory(d) => d.set_property(name),
            ClassObject::File(_) | ClassObject::MemBuffer(_) => false,
        }
    }

    /// A scalar assigned to a variable holding the object (`ScSetString`,
    /// `ScSetInt`, `ScSetBool`): a string becomes the text of a `String`
    /// object; every other class, and every other value, ignores it.
    pub fn assign_scalar(&mut self, value: &Value) {
        if let (ClassObject::String(s), Value::String(text)) = (self, value) {
            s.set_text(text);
        }
    }

    /// A method call (`ScCallMethod`); `None` when the class has no such
    /// method.
    pub fn call_method<H: ClassHost>(
        &mut self,
        name: &str,
        args: &[Value],
        host: &mut H,
        ctx: &Context<'_>,
    ) -> Option<Return> {
        match self {
            ClassObject::String(s) => s.call_method(name, args, host),
            ClassObject::Array(a) => a.call_method(name, args, host),
            ClassObject::Date(d) => d.call_method(name, args, host),
            ClassObject::File(f) => f.call_method(name, args, host, ctx),
            ClassObject::MemBuffer(m) => m.call_method(name, args, host, ctx),
            ClassObject::Directory(d) => d.call_method(name, args, host),
        }
    }
}

/// The `n`th argument, `null` when missing (`CorrectParams`).
fn arg(args: &[Value], n: usize) -> Value {
    args.get(n).cloned().unwrap_or(Value::Null)
}

/// The bytes of a text as the game's files hold it: Latin-1, `?` for a
/// character beyond it.
pub fn text_to_bytes(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect()
}

/// A text from the bytes of a game file (Latin-1).
pub fn text_from_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// A file name a script gives, as a path relative to a directory the
/// host chooses: `\` and `/` both separate, `.` components are dropped.
/// `None` for an empty name, an absolute path (a leading separator or a
/// drive letter) or a `..` component, so that scripts stay inside the
/// directory.
pub fn relative_path(name: &str) -> Option<PathBuf> {
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/') {
        return None;
    }
    let bytes = normalized.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return None;
    }
    let mut path = PathBuf::new();
    for component in normalized.split('/') {
        match component {
            "" | "." => {}
            ".." => return None,
            other => path.push(other),
        }
    }
    if path.as_os_str().is_empty() {
        None
    } else {
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_stay_inside() {
        assert_eq!(
            relative_path("saves\\notes.txt"),
            Some(PathBuf::from("saves/notes.txt"))
        );
        assert_eq!(relative_path("./a//b"), Some(PathBuf::from("a/b")));
        assert_eq!(relative_path("..\\x"), None);
        assert_eq!(relative_path("a/../x"), None);
        assert_eq!(relative_path("/etc/passwd"), None);
        assert_eq!(relative_path("C:\\game\\x"), None);
        assert_eq!(relative_path(""), None);
        assert_eq!(relative_path("."), None);
    }

    #[test]
    fn text_is_latin1() {
        assert_eq!(text_to_bytes("hé€"), b"h\xe9?");
        assert_eq!(text_from_bytes(b"h\xe9"), "hé");
    }
}
