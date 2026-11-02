//! The `File` class (`CSXFile`).
//!
//! Written with reference to `SXFile.cpp` of Wintermute Engine 1.x
//! (MIT License, Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.
//!
//! Reading goes through the host, which serves the game's files and
//! those scripts wrote; the whole file is read at once, as package files
//! are. Writing goes to a path the host chooses under the game's
//! writable directory; the file is opened for each write, so the object
//! holds no handle and copies and saves like any value. A saved game
//! keeps a file open for reading (its content and position) and closes
//! one open for writing, as the original does.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;

use super::{ClassHost, arg, text_from_bytes, text_to_bytes};
use crate::host::{Context, Return};
use crate::value::{Natives, Value};

/// How a file is open (`AccessMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Mode {
    /// Not open (0).
    #[default]
    Closed,
    /// Reading (1).
    Read,
    /// Writing a new file (2).
    Write,
    /// Appending (3).
    Append,
}

impl Mode {
    fn code(self) -> i32 {
        match self {
            Mode::Closed => 0,
            Mode::Read => 1,
            Mode::Write => 2,
            Mode::Append => 3,
        }
    }
}

/// The content of a file open for reading, and the position in it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct ReadBuffer {
    data: Vec<u8>,
    pos: usize,
}

/// A file open for writing: where it is, and the position.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WriteTarget {
    path: PathBuf,
    pos: u64,
}

/// A `File` object.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(from = "SavedFile", into = "SavedFile")
)]
pub struct FileObject {
    filename: Option<String>,
    mode: Mode,
    text: bool,
    read: Option<ReadBuffer>,
    write: Option<WriteTarget>,
}

/// What a saved game keeps of a file object.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SavedFile {
    filename: Option<String>,
    text: bool,
    read: Option<ReadBuffer>,
}

impl From<FileObject> for SavedFile {
    fn from(file: FileObject) -> SavedFile {
        let reading = file.mode == Mode::Read;
        SavedFile {
            filename: file.filename,
            text: reading && file.text,
            read: if reading { file.read } else { None },
        }
    }
}

impl From<SavedFile> for FileObject {
    fn from(saved: SavedFile) -> FileObject {
        let mode = if saved.read.is_some() {
            Mode::Read
        } else {
            Mode::Closed
        };
        FileObject {
            filename: saved.filename,
            mode,
            text: saved.text,
            read: saved.read,
            write: None,
        }
    }
}

/// The string form: the file name, or `[file object]` without one.
impl fmt::Display for FileObject {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(self.filename.as_deref().unwrap_or("[file object]"))
    }
}

impl FileObject {
    /// `new File(name)`; the name may be given later with `SetFilename`.
    pub fn new(args: &[Value], natives: &dyn Natives) -> FileObject {
        let name = arg(args, 0);
        FileObject {
            filename: (!name.is_null()).then(|| name.get_string(natives)),
            ..FileObject::default()
        }
    }

    /// The file name, if set.
    pub fn filename(&self) -> Option<&str> {
        self.filename.as_deref()
    }

    /// How the file is open.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Whether the file is open as text.
    pub fn is_text(&self) -> bool {
        self.text
    }

    /// Closes the file (`Close`).
    pub fn close(&mut self) {
        self.read = None;
        self.write = None;
        self.mode = Mode::Closed;
        self.text = false;
    }

    /// The position (`Position`).
    pub fn position(&self) -> u64 {
        match (&self.read, &self.write) {
            (Some(read), _) => read.pos as u64,
            (_, Some(write)) => write.pos,
            _ => 0,
        }
    }

    /// The size (`Length`).
    pub fn length(&self) -> u64 {
        match (&self.read, &self.write) {
            (Some(read), _) => read.data.len() as u64,
            (_, Some(write)) => fs::metadata(&write.path).map(|m| m.len()).unwrap_or(0),
            _ => 0,
        }
    }

    /// Seeks (`SetPosition`): a position past the end of a file read
    /// fails; a negative one always does.
    fn set_position(&mut self, pos: i64) -> bool {
        if pos < 0 {
            return false;
        }
        match (&mut self.read, &mut self.write) {
            (Some(read), _) => {
                if pos as usize > read.data.len() {
                    return false;
                }
                read.pos = pos as usize;
                true
            }
            (_, Some(write)) => {
                write.pos = pos as u64;
                true
            }
            _ => false,
        }
    }

    /// Opens the file (`OpenAsText`, `OpenAsBinary`).
    fn open<H: ClassHost>(&mut self, host: &mut H, mode: Mode, text: bool) -> bool {
        self.close();
        let Some(name) = self.filename.clone() else {
            return false;
        };
        match mode {
            Mode::Read => match host.read_file(&name) {
                Some(data) => self.read = Some(ReadBuffer { data, pos: 0 }),
                None => return false,
            },
            Mode::Write | Mode::Append => {
                let Some(path) = host.writable_path(&name) else {
                    return false;
                };
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let opened = if mode == Mode::Write {
                    fs::File::create(&path).map(|_| 0)
                } else {
                    OpenOptions::new()
                        .append(true)
                        .create(true)
                        .open(&path)
                        .and_then(|f| f.metadata())
                        .map(|m| m.len())
                };
                match opened {
                    Ok(pos) => self.write = Some(WriteTarget { path, pos }),
                    Err(_) => return false,
                }
            }
            Mode::Closed => return false,
        }
        self.mode = mode;
        self.text = text;
        true
    }

    /// Writes bytes at the position (or the end, when appending).
    fn write_bytes(&mut self, bytes: &[u8]) -> bool {
        let Some(target) = &mut self.write else {
            return false;
        };
        let result = if self.mode == Mode::Append {
            OpenOptions::new()
                .append(true)
                .open(&target.path)
                .and_then(|mut f| {
                    f.write_all(bytes)?;
                    f.metadata().map(|m| m.len())
                })
        } else {
            OpenOptions::new()
                .write(true)
                .open(&target.path)
                .and_then(|mut f| {
                    f.seek(SeekFrom::Start(target.pos))?;
                    f.write_all(bytes)?;
                    Ok(target.pos + bytes.len() as u64)
                })
        };
        match result {
            Ok(pos) => {
                target.pos = pos;
                true
            }
            Err(_) => false,
        }
    }

    /// The next byte of a file read, if any.
    fn read_byte(&mut self) -> Option<u8> {
        let read = self.read.as_mut()?;
        let byte = *read.data.get(read.pos)?;
        read.pos += 1;
        Some(byte)
    }

    /// The next `n` bytes, all or nothing, as the original's `Read`.
    fn read_exact(&mut self, n: usize) -> Option<Vec<u8>> {
        let read = self.read.as_mut()?;
        let bytes = read.data.get(read.pos..read.pos + n)?.to_vec();
        read.pos += n;
        Some(bytes)
    }

    /// `ReadLine`: up to a line feed (dropped, carriage returns too);
    /// `null` at the end of the file.
    fn read_line(&mut self) -> Value {
        let mut bytes = Vec::new();
        let mut found_newline = false;
        while let Some(b) = self.read_byte() {
            match b {
                b'\n' => {
                    found_newline = true;
                    break;
                }
                b'\r' => {}
                _ => bytes.push(b),
            }
        }
        if !found_newline && bytes.is_empty() {
            Value::Null
        } else {
            Value::String(text_from_bytes(&bytes))
        }
    }

    /// `ReadText(n)`: `n` characters, carriage returns skipped and not
    /// counted; `null` at the end of the file when `n` is positive.
    fn read_text(&mut self, n: i32) -> Value {
        let mut bytes = Vec::new();
        while (bytes.len() as i64) < i64::from(n) {
            match self.read_byte() {
                Some(b'\r') => {}
                Some(b) => bytes.push(b),
                None => break,
            }
        }
        if n > 0 && bytes.is_empty() {
            Value::Null
        } else {
            Value::String(text_from_bytes(&bytes))
        }
    }

    /// `Type`, `Filename`, `Position`, `Length`, `TextMode`,
    /// `AccessMode`; other names are script properties.
    pub fn get_property(&self, name: &str) -> Option<Value> {
        Some(match name {
            "Type" => Value::from("file"),
            "Filename" => Value::String(self.filename.clone().unwrap_or_default()),
            "Position" => Value::Int(self.position() as i32),
            "Length" => Value::Int(self.length() as i32),
            "TextMode" => Value::Bool(self.text),
            "AccessMode" => Value::Int(self.mode.code()),
            _ => return None,
        })
    }

    /// The methods of the class.
    pub fn call_method<H: ClassHost>(
        &mut self,
        name: &str,
        args: &[Value],
        host: &mut H,
        ctx: &Context<'_>,
    ) -> Option<Return> {
        let string = |n: usize, host: &H| -> String {
            let natives: &dyn Natives = host;
            arg(args, n).get_string(natives)
        };
        let int = |n: usize, host: &H| -> i32 {
            let natives: &dyn Natives = host;
            arg(args, n).get_int(natives)
        };
        let reading_text = self.mode == Mode::Read && self.text;
        let reading_binary = self.mode == Mode::Read && !self.text;
        let writing_text = matches!(self.mode, Mode::Write | Mode::Append) && self.text;
        let writing_binary = matches!(self.mode, Mode::Write | Mode::Append) && !self.text;
        let error = |host: &mut H, what: &str| {
            host.log(&ctx.runtime_error(&format!("File.{name}: {what}")));
        };
        Some(match name {
            "SetFilename" => {
                let filename = string(0, host);
                self.close();
                self.filename = Some(filename);
                Return::null()
            }
            "OpenAsText" | "OpenAsBinary" => {
                let mode_arg = arg(args, 0);
                let mut mode = if mode_arg.is_null() { 1 } else { int(0, host) };
                if !(1..=3).contains(&mode) {
                    error(host, "invalid access mode. Setting read mode.");
                    mode = 1;
                }
                let mode = match mode {
                    1 => Mode::Read,
                    2 => Mode::Write,
                    _ => Mode::Append,
                };
                let opened = self.open(host, mode, name == "OpenAsText");
                Return::value(opened)
            }
            "Close" => {
                self.close();
                Return::null()
            }
            "SetPosition" => {
                if self.mode == Mode::Closed {
                    error(host, "File is not open");
                    Return::value(false)
                } else {
                    let pos = i64::from(int(0, host));
                    Return::value(self.set_position(pos))
                }
            }
            "Delete" => {
                self.close();
                let deleted = self
                    .filename
                    .as_deref()
                    .and_then(|name| host.writable_path(name))
                    .is_some_and(|path| fs::remove_file(path).is_ok());
                Return::value(deleted)
            }
            "Copy" => {
                let dest = string(0, host);
                let overwrite_arg = arg(args, 1);
                let overwrite = overwrite_arg.is_null() || {
                    let natives: &dyn Natives = host;
                    overwrite_arg.get_bool(natives)
                };
                self.close();
                let copied = (|| {
                    let source = host.read_file(self.filename.as_deref()?)?;
                    let path = host.writable_path(&dest)?;
                    if !overwrite && path.exists() {
                        return None;
                    }
                    if let Some(parent) = path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    fs::write(path, source).ok()
                })();
                Return::value(copied.is_some())
            }
            "ReadLine" => {
                if !reading_text {
                    error(host, "File must be open in text mode.");
                    Return::null()
                } else {
                    Return::value(self.read_line())
                }
            }
            "ReadText" => {
                let n = int(0, host);
                if !reading_text {
                    error(host, "File must be open in text mode.");
                    Return::null()
                } else {
                    Return::value(self.read_text(n))
                }
            }
            "WriteLine" | "WriteText" => {
                let mut text = string(0, host);
                if !writing_text {
                    error(host, "File must be open for writing in text mode.");
                    Return::value(false)
                } else {
                    if name == "WriteLine" {
                        text.push('\n');
                    }
                    Return::value(self.write_bytes(&text_to_bytes(&text)))
                }
            }
            "ReadBool" | "ReadByte" | "ReadShort" | "ReadInt" | "ReadLong" | "ReadFloat"
            | "ReadDouble" | "ReadString" => {
                if !reading_binary {
                    error(host, "File must be open for reading in binary mode.");
                    return Some(Return::null());
                }
                let value = match name {
                    "ReadBool" => self.read_exact(1).map(|b| Value::Bool(b[0] != 0)),
                    "ReadByte" => self.read_exact(1).map(|b| Value::Int(i32::from(b[0]))),
                    // The original adds 65536 to the signed value it read.
                    "ReadShort" => self
                        .read_exact(2)
                        .map(|b| Value::Int(65_536 + i32::from(i16::from_le_bytes([b[0], b[1]])))),
                    "ReadInt" | "ReadLong" => self
                        .read_exact(4)
                        .map(|b| Value::Int(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))),
                    "ReadFloat" => self.read_exact(4).map(|b| {
                        Value::Float(f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])))
                    }),
                    "ReadDouble" => self.read_exact(8).map(|b| {
                        Value::Float(f64::from_le_bytes([
                            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                        ]))
                    }),
                    // A 32-bit length, then the bytes.
                    _ => self
                        .read_exact(4)
                        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
                        .and_then(|size| self.read_exact(size))
                        .map(|bytes| Value::String(text_from_bytes(&bytes))),
                };
                Return::value(value.unwrap_or(Value::Null))
            }
            "WriteBool" | "WriteByte" | "WriteShort" | "WriteInt" | "WriteLong" | "WriteFloat"
            | "WriteDouble" | "WriteString" => {
                let value = arg(args, 0);
                let natives: &dyn Natives = host;
                let bytes: Vec<u8> = match name {
                    "WriteBool" => vec![u8::from(value.get_bool(natives))],
                    "WriteByte" => vec![value.get_int(natives) as u8],
                    "WriteShort" => (value.get_int(natives) as i16).to_le_bytes().to_vec(),
                    "WriteInt" | "WriteLong" => value.get_int(natives).to_le_bytes().to_vec(),
                    "WriteFloat" => (value.get_float(natives) as f32).to_le_bytes().to_vec(),
                    "WriteDouble" => value.get_float(natives).to_le_bytes().to_vec(),
                    _ => {
                        let text = text_to_bytes(&value.get_string(natives));
                        let mut bytes = (text.len() as u32).to_le_bytes().to_vec();
                        bytes.extend_from_slice(&text);
                        bytes
                    }
                };
                if !writing_binary {
                    error(host, "File must be open for writing in binary mode.");
                    Return::value(false)
                } else {
                    Return::value(self.write_bytes(&bytes))
                }
            }
            _ => return None,
        })
    }
}
