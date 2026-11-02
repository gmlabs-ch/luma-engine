//! The DirectX `.X` file container, text form: templates, nested data
//! objects and data references, read into a generic object tree.
//!
//! The reader is **template-agnostic**: the text form of a data object is
//! self-describing (numbers, strings, nested objects and `{ references }`
//! with `;` and `,` separators), so each object is read as the flat list
//! of its scalar values plus its children, and the typed loaders of
//! [`crate::model`] consume the values in the order the standard
//! templates define. Template declarations are recorded by name and GUID
//! and their member lists are not interpreted; an object of an unknown
//! template is kept in the tree and ignored by the loaders, as D3DX does.
//! The subset is described in `docs/formats/xfile.md`.
//!
//! Binary files (`bin `) and MSZIP-compressed files (`tzip`, `bzip`) are
//! read by `xfile_binary` into the same tree, so the loaders do
//! not know which form a file came in.
//!
//! The file format is taken from the public DirectX documentation. How the
//! loaded objects are consumed follows the MIT-licensed Wintermute Engine
//! 1.x sources (`XFile.cpp`, `XModel.cpp`, `XFrameNode.cpp`, Copyright
//! (c) 2003-2013 Jan Nedoma). See `NOTICE` at the root of the repository.

use std::fmt;

use crate::xfile_binary;

/// Length of the header: magic, version, format and float size.
pub const HEADER_LEN: usize = 16;

/// The storage format named by the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `txt `: text.
    Text,
    /// `bin `: binary tokens.
    Binary,
    /// `tzip`: MSZIP-compressed text.
    CompressedText,
    /// `bzip`: MSZIP-compressed binary tokens.
    CompressedBinary,
}

impl Format {
    /// The four header characters of the format.
    pub fn tag(self) -> &'static str {
        match self {
            Format::Text => "txt ",
            Format::Binary => "bin ",
            Format::CompressedText => "tzip",
            Format::CompressedBinary => "bzip",
        }
    }
}

/// The 16-byte header, `xof 0303txt 0032` for the sample files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Major version (two digits).
    pub major: u16,
    /// Minor version (two digits).
    pub minor: u16,
    /// Storage format.
    pub format: Format,
    /// Size of a float in bits, 32 or 64. It only matters to the binary
    /// forms, where it is the size of the entries of a float list.
    pub float_size: u16,
}

/// A template declaration: its name and GUID. Members are not kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// The template name.
    pub name: String,
    /// The GUID between `<` and `>`, when declared.
    pub guid: Option<String>,
}

/// A scalar value of a data object.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A number without a fraction or exponent.
    Int(i64),
    /// A number with a fraction or an exponent.
    Float(f64),
    /// A double-quoted string, without the quotes.
    Str(String),
}

/// A data reference, `{ name }`, `{ name <guid> }` or `{ <guid> }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// The referenced object's name.
    pub name: Option<String>,
    /// The referenced object's GUID.
    pub guid: Option<String>,
}

/// A child of a data object.
#[derive(Debug, Clone, PartialEq)]
pub enum Child {
    /// A nested data object.
    Object(DataObject),
    /// A reference to an object defined elsewhere in the file.
    Reference(Reference),
}

/// A data object: `Template [name] { values... children... }`.
#[derive(Debug, Clone, PartialEq)]
pub struct DataObject {
    /// The template name.
    pub template: String,
    /// The optional object name.
    pub name: Option<String>,
    /// The optional GUID of the object.
    pub guid: Option<String>,
    /// The scalar values, in file order, separators dropped.
    pub values: Vec<Value>,
    /// The nested objects and references, in file order.
    pub children: Vec<Child>,
    /// 1-based line of the object's opening in the source; 0 in a binary
    /// file, which has no lines.
    pub line: u32,
}

/// A parsed `.X` file, in any of the four forms.
#[derive(Debug, Clone, PartialEq)]
pub struct XFile {
    /// The header.
    pub header: Header,
    /// The template declarations, in file order.
    pub templates: Vec<Template>,
    /// The top-level data objects, in file order.
    pub objects: Vec<DataObject>,
}

/// Why a file could not be read, with the line.
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    /// What went wrong.
    pub kind: ErrorKind,
    /// 1-based line (0 for the header and for the binary forms).
    pub line: u32,
}

/// The kinds of error.
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorKind {
    /// The file is shorter than the header or does not start with `xof `.
    BadMagic,
    /// The header names a format that is not one of the four.
    UnsupportedFormat(String),
    /// The header's version or float size is not numeric, or a binary
    /// file's float size is neither 32 nor 64.
    BadHeader,
    /// The file ends inside an object, a template, a string, a binary
    /// record or a compressed block.
    UnexpectedEnd,
    /// A token that cannot appear where it was found, or an unknown
    /// binary token code.
    UnexpectedToken(String),
    /// The MSZIP data of a compressed file is not laid out as expected
    /// or does not inflate.
    BadCompression(String),
    /// A typed loader asked an object for a value it does not have.
    MissingValue {
        /// The object's template.
        object: String,
        /// What was expected.
        expected: &'static str,
    },
    /// A typed loader found a value of the wrong kind.
    WrongValue {
        /// The object's template.
        object: String,
        /// What was expected.
        expected: &'static str,
        /// What was found.
        found: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(f, "line {}: ", self.line)?;
        }
        match &self.kind {
            ErrorKind::BadMagic => write!(f, "not a DirectX .X file (no 'xof ' magic)"),
            ErrorKind::UnsupportedFormat(tag) => {
                write!(f, "unsupported .X format '{tag}'")
            }
            ErrorKind::BadHeader => write!(f, "malformed .X header"),
            ErrorKind::UnexpectedEnd => write!(f, "unexpected end of file"),
            ErrorKind::UnexpectedToken(token) => write!(f, "unexpected token '{token}'"),
            ErrorKind::BadCompression(message) => write!(f, "MSZIP: {message}"),
            ErrorKind::MissingValue { object, expected } => {
                write!(
                    f,
                    "{object}: expected {expected}, found the end of the object's values"
                )
            }
            ErrorKind::WrongValue {
                object,
                expected,
                found,
            } => write!(f, "{object}: expected {expected}, found {found}"),
        }
    }
}

impl std::error::Error for Error {}

/// Result of the readers of this module.
pub type Result<T> = std::result::Result<T, Error>;

impl Header {
    /// Reads the 16-byte header.
    pub fn parse(bytes: &[u8]) -> Result<Header> {
        let error = |kind| Error { kind, line: 0 };
        if bytes.len() < HEADER_LEN || &bytes[0..4] != b"xof " {
            return Err(error(ErrorKind::BadMagic));
        }
        let digits = |slice: &[u8]| -> Result<u16> {
            std::str::from_utf8(slice)
                .ok()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| error(ErrorKind::BadHeader))
        };
        let major = digits(&bytes[4..6])?;
        let minor = digits(&bytes[6..8])?;
        let format = match &bytes[8..12] {
            b"txt " => Format::Text,
            b"bin " => Format::Binary,
            b"tzip" => Format::CompressedText,
            b"bzip" => Format::CompressedBinary,
            other => {
                return Err(error(ErrorKind::UnsupportedFormat(
                    String::from_utf8_lossy(other).into_owned(),
                )));
            }
        };
        let float_size = digits(&bytes[12..16])?;
        Ok(Header {
            major,
            minor,
            format,
            float_size,
        })
    }
}

impl XFile {
    /// Parses a `.X` file in any of its four forms: text, binary, and
    /// each of them MSZIP-compressed. The tree is the same whatever the
    /// form; `header` says which it was.
    pub fn parse(bytes: &[u8]) -> Result<XFile> {
        let header = Header::parse(bytes)?;
        let body = &bytes[HEADER_LEN..];
        let (templates, objects) = match header.format {
            Format::Text => parse_text(body)?,
            Format::Binary => xfile_binary::parse_body(body, header.float_size)?,
            Format::CompressedText | Format::CompressedBinary => {
                let data = xfile_binary::decompress_mszip(body)?;
                // The compressed data is the data section. A writer that
                // compressed the whole file, header included, is read too
                // as long as that inner header is a plain one.
                let (form, float_size, data) = match Header::parse(&data) {
                    Ok(inner) => (inner.format, inner.float_size, &data[HEADER_LEN..]),
                    Err(_) if header.format == Format::CompressedText => {
                        (Format::Text, header.float_size, &data[..])
                    }
                    Err(_) => (Format::Binary, header.float_size, &data[..]),
                };
                match form {
                    Format::Text => parse_text(data)?,
                    Format::Binary => xfile_binary::parse_body(data, float_size)?,
                    nested => {
                        return Err(Error {
                            kind: ErrorKind::UnsupportedFormat(nested.tag().to_string()),
                            line: 0,
                        });
                    }
                }
            }
        };
        Ok(XFile {
            header,
            templates,
            objects,
        })
    }

    /// The first object with this name, searching the tree depth-first
    /// in file order. Names are case-sensitive.
    pub fn find(&self, name: &str) -> Option<&DataObject> {
        self.objects.iter().find_map(|object| object.find(name))
    }

    /// The top-level objects of a template.
    pub fn objects_of<'a>(&'a self, template: &'a str) -> impl Iterator<Item = &'a DataObject> {
        self.objects
            .iter()
            .filter(move |object| object.template == template)
    }
}

/// Reads the data section of a text file: the bytes after the header.
fn parse_text(body: &[u8]) -> Result<(Vec<Template>, Vec<DataObject>)> {
    let mut lexer = Lexer {
        src: body,
        pos: 0,
        line: 1,
    };
    let mut templates = Vec::new();
    let mut objects = Vec::new();
    while let Some(token) = lexer.next_token()? {
        match token {
            Token::Ident(word) if word == "template" => {
                templates.push(lexer.template()?);
            }
            Token::Ident(template) => objects.push(lexer.object(template)?),
            other => return Err(lexer.unexpected(&other)),
        }
    }
    Ok((templates, objects))
}

impl DataObject {
    /// This object if it has the name, else the first descendant with it.
    pub fn find(&self, name: &str) -> Option<&DataObject> {
        if self.name.as_deref() == Some(name) {
            return Some(self);
        }
        self.objects().find_map(|child| child.find(name))
    }

    /// The nested objects, references skipped.
    pub fn objects(&self) -> impl Iterator<Item = &DataObject> {
        self.children.iter().filter_map(|child| match child {
            Child::Object(object) => Some(object),
            Child::Reference(_) => None,
        })
    }

    /// The nested objects of a template.
    pub fn objects_of<'a>(&'a self, template: &'a str) -> impl Iterator<Item = &'a DataObject> {
        self.objects()
            .filter(move |object| object.template == template)
    }

    /// The first nested object of a template.
    pub fn child(&self, template: &str) -> Option<&DataObject> {
        self.objects().find(|object| object.template == template)
    }

    /// The references among the children.
    pub fn references(&self) -> impl Iterator<Item = &Reference> {
        self.children.iter().filter_map(|child| match child {
            Child::Reference(reference) => Some(reference),
            Child::Object(_) => None,
        })
    }

    /// A cursor over the values, for the typed loaders.
    pub fn values(&self) -> Values<'_> {
        Values {
            object: self,
            pos: 0,
        }
    }
}

/// A cursor over the scalar values of an object.
#[derive(Debug, Clone)]
pub struct Values<'a> {
    object: &'a DataObject,
    pos: usize,
}

impl<'a> Values<'a> {
    /// How many values are left.
    pub fn remaining(&self) -> usize {
        self.object.values.len() - self.pos
    }

    fn next_value(&mut self, expected: &'static str) -> Result<&'a Value> {
        let value = self.object.values.get(self.pos).ok_or_else(|| Error {
            kind: ErrorKind::MissingValue {
                object: self.object.template.clone(),
                expected,
            },
            line: self.object.line,
        })?;
        self.pos += 1;
        Ok(value)
    }

    fn wrong(&self, expected: &'static str, found: &Value) -> Error {
        Error {
            kind: ErrorKind::WrongValue {
                object: self.object.template.clone(),
                expected,
                found: match found {
                    Value::Int(i) => format!("the integer {i}"),
                    Value::Float(f) => format!("the number {f}"),
                    Value::Str(s) => format!("the string \"{s}\""),
                },
            },
            line: self.object.line,
        }
    }

    /// The next value as an integer.
    pub fn int(&mut self) -> Result<i64> {
        match *self.next_value("an integer")? {
            Value::Int(i) => Ok(i),
            ref other => Err(self.wrong("an integer", other)),
        }
    }

    /// The next value as a count or index (`DWORD`, `WORD`).
    pub fn count(&mut self) -> Result<u32> {
        match *self.next_value("a count")? {
            Value::Int(i) if (0..=i64::from(u32::MAX)).contains(&i) => Ok(i as u32),
            ref other => Err(self.wrong("a count", other)),
        }
    }

    /// The next value as a float; an integer is accepted.
    pub fn float(&mut self) -> Result<f32> {
        match *self.next_value("a number")? {
            Value::Int(i) => Ok(i as f32),
            Value::Float(f) => Ok(f as f32),
            ref other => Err(self.wrong("a number", other)),
        }
    }

    /// The next three floats.
    pub fn vec3(&mut self) -> Result<[f32; 3]> {
        Ok([self.float()?, self.float()?, self.float()?])
    }

    /// The next value as a string.
    pub fn string(&mut self) -> Result<String> {
        match *self.next_value("a string")? {
            Value::Str(ref s) => Ok(s.clone()),
            ref other => Err(self.wrong("a string", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    Guid(String),
    OpenBrace,
    CloseBrace,
    Separator,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Ident(s) => write!(f, "{s}"),
            Token::Int(i) => write!(f, "{i}"),
            Token::Float(x) => write!(f, "{x}"),
            Token::Str(s) => write!(f, "\"{s}\""),
            Token::Guid(g) => write!(f, "<{g}>"),
            Token::OpenBrace => write!(f, "{{"),
            Token::CloseBrace => write!(f, "}}"),
            Token::Separator => write!(f, ";"),
        }
    }
}

struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: u32,
}

impl Lexer<'_> {
    fn error(&self, kind: ErrorKind) -> Error {
        Error {
            kind,
            line: self.line,
        }
    }

    fn unexpected(&self, token: &Token) -> Error {
        self.error(ErrorKind::UnexpectedToken(token.to_string()))
    }

    fn peek_byte(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek_byte()?;
        self.pos += 1;
        if byte == b'\n' {
            self.line += 1;
        }
        Some(byte)
    }

    fn skip_blanks(&mut self) {
        while let Some(byte) = self.peek_byte() {
            match byte {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    self.bump();
                }
                b'#' => self.skip_line(),
                b'/' if self.src.get(self.pos + 1) == Some(&b'/') => self.skip_line(),
                _ => break,
            }
        }
    }

    fn skip_line(&mut self) {
        while let Some(byte) = self.bump() {
            if byte == b'\n' {
                break;
            }
        }
    }

    fn text(&self, start: usize) -> String {
        self.src[start..self.pos]
            .iter()
            .map(|&b| char::from(b))
            .collect()
    }

    fn next_token(&mut self) -> Result<Option<Token>> {
        self.skip_blanks();
        let Some(byte) = self.peek_byte() else {
            return Ok(None);
        };
        let token = match byte {
            b'{' => {
                self.bump();
                Token::OpenBrace
            }
            b'}' => {
                self.bump();
                Token::CloseBrace
            }
            b';' | b',' => {
                self.bump();
                Token::Separator
            }
            b'"' => {
                self.bump();
                let start = self.pos;
                loop {
                    match self.peek_byte() {
                        None => return Err(self.error(ErrorKind::UnexpectedEnd)),
                        Some(b'"') => break,
                        Some(_) => {
                            self.bump();
                        }
                    }
                }
                let text = self.text(start);
                self.bump();
                Token::Str(text)
            }
            b'<' => {
                self.bump();
                let start = self.pos;
                loop {
                    match self.peek_byte() {
                        None => return Err(self.error(ErrorKind::UnexpectedEnd)),
                        Some(b'>') => break,
                        Some(_) => {
                            self.bump();
                        }
                    }
                }
                let text = self.text(start);
                self.bump();
                Token::Guid(text)
            }
            b'0'..=b'9' | b'-' | b'+' | b'.' => self.number()?,
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                let start = self.pos;
                while let Some(b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-') =
                    self.peek_byte()
                {
                    self.bump();
                }
                Token::Ident(self.text(start))
            }
            other => {
                return Err(self.error(ErrorKind::UnexpectedToken(char::from(other).to_string())));
            }
        };
        Ok(Some(token))
    }

    fn number(&mut self) -> Result<Token> {
        let start = self.pos;
        let mut is_float = false;
        if let Some(b'-' | b'+') = self.peek_byte() {
            self.bump();
        }
        while let Some(byte) = self.peek_byte() {
            match byte {
                b'0'..=b'9' => {
                    self.bump();
                }
                b'.' | b'e' | b'E' => {
                    is_float = true;
                    self.bump();
                    if matches!(byte, b'e' | b'E') {
                        if let Some(b'-' | b'+') = self.peek_byte() {
                            self.bump();
                        }
                    }
                }
                _ => break,
            }
        }
        let text = self.text(start);
        if is_float {
            text.parse::<f64>()
                .map(Token::Float)
                .map_err(|_| self.error(ErrorKind::UnexpectedToken(text.clone())))
        } else {
            text.parse::<i64>()
                .map(Token::Int)
                .map_err(|_| self.error(ErrorKind::UnexpectedToken(text.clone())))
        }
    }

    fn expect_token(&mut self) -> Result<Token> {
        self.next_token()?
            .ok_or_else(|| self.error(ErrorKind::UnexpectedEnd))
    }

    /// Reads `template Name { ... }` after the `template` keyword. The body
    /// is skipped byte by byte to the matching brace (it holds member
    /// declarations and `[...]` restrictions that are not tokens of the
    /// data section); the first `<guid>` in it is the template's.
    fn template(&mut self) -> Result<Template> {
        let name = match self.expect_token()? {
            Token::Ident(name) => name,
            other => return Err(self.unexpected(&other)),
        };
        match self.expect_token()? {
            Token::OpenBrace => {}
            other => return Err(self.unexpected(&other)),
        }
        let mut guid = None;
        let mut depth = 1usize;
        loop {
            let Some(byte) = self.bump() else {
                return Err(self.error(ErrorKind::UnexpectedEnd));
            };
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                b'"' => {
                    while let Some(inner) = self.bump() {
                        if inner == b'"' {
                            break;
                        }
                    }
                }
                b'<' => {
                    let start = self.pos;
                    while let Some(inner) = self.peek_byte() {
                        if inner == b'>' {
                            break;
                        }
                        self.bump();
                    }
                    if guid.is_none() {
                        guid = Some(self.text(start));
                    }
                }
                _ => {}
            }
        }
        Ok(Template { name, guid })
    }

    /// Reads a data object after its template name. The object's GUID may
    /// come before the brace (as the sample files write it) or first
    /// inside it (as the DirectX grammar puts it).
    fn object(&mut self, template: String) -> Result<DataObject> {
        let line = self.line;
        let mut name = None;
        let mut guid = None;
        loop {
            match self.expect_token()? {
                Token::Ident(n) if name.is_none() && guid.is_none() => name = Some(n),
                Token::Guid(g) if guid.is_none() => guid = Some(g),
                Token::OpenBrace => break,
                other => return Err(self.unexpected(&other)),
            }
        }
        let mut object = DataObject {
            template,
            name,
            guid,
            values: Vec::new(),
            children: Vec::new(),
            line,
        };
        loop {
            match self.expect_token()? {
                Token::CloseBrace => break,
                Token::Separator => {}
                Token::Guid(g)
                    if object.guid.is_none()
                        && object.values.is_empty()
                        && object.children.is_empty() =>
                {
                    object.guid = Some(g);
                }
                Token::Int(i) => object.values.push(Value::Int(i)),
                Token::Float(f) => object.values.push(Value::Float(f)),
                Token::Str(s) => object.values.push(Value::Str(s)),
                Token::Ident(child_template) => {
                    let child = self.object(child_template)?;
                    object.children.push(Child::Object(child));
                }
                Token::OpenBrace => {
                    let reference = self.reference()?;
                    object.children.push(Child::Reference(reference));
                }
                other => return Err(self.unexpected(&other)),
            }
        }
        Ok(object)
    }

    /// Reads a reference after its opening brace.
    fn reference(&mut self) -> Result<Reference> {
        let mut reference = Reference {
            name: None,
            guid: None,
        };
        loop {
            match self.expect_token()? {
                Token::Ident(name) if reference.name.is_none() => reference.name = Some(name),
                Token::Guid(guid) if reference.guid.is_none() => reference.guid = Some(guid),
                Token::CloseBrace => break,
                other => return Err(self.unexpected(&other)),
            }
        }
        Ok(reference)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &str = "xof 0303txt 0032
template Frame {
 <3d82ab46-62da-11cf-ab39-0020af71e433>
 [...]
}
# a comment
AnimTicksPerSecond {
 4800;
}

Frame Root { // trailing comment
 FrameTransformMatrix {
  1.0,0,0,0, 0,1,0,0, 0,0,1,0, 2.5,-3,1e2,1;;
 }
 Mesh {
  3;
  0.0;0.0;0.0;,
  1.0;0.0;0.0;,
  0.0;1.0;0.0;;
  1;
  3;0,1,2;;
  MeshTextureCoords {
   3;
   0.0;0.0;, 1.0;0.0;, 0.0;1.0;;
  }
 }
 Frame Child <9e415a43-7ba6-4a73-8743-b73d47e88476> {
 }
}
AnimationSet walk {
 Animation {
  AnimationKey {
   4;
   0;
   ;
  }
  { Child }
 }
}
";

    #[test]
    fn reads_the_header() {
        let header = Header::parse(b"xof 0303txt 0032").unwrap();
        assert_eq!(
            header,
            Header {
                major: 3,
                minor: 3,
                format: Format::Text,
                float_size: 32
            }
        );
        assert_eq!(
            Header::parse(b"xof 0302bin 0064").unwrap().format,
            Format::Binary
        );
        assert_eq!(
            Header::parse(b"xof 0303tzip0032").unwrap().format,
            Format::CompressedText
        );
        assert_eq!(
            Header::parse(b"xof 0303bzip0032").unwrap().format,
            Format::CompressedBinary
        );
        assert_eq!(
            Header::parse(b"xof 03").unwrap_err().kind,
            ErrorKind::BadMagic
        );
        assert_eq!(
            Header::parse(b"MAIN3DS chunk data").unwrap_err().kind,
            ErrorKind::BadMagic
        );
        assert_eq!(
            Header::parse(b"xof 03x3txt 0032").unwrap_err().kind,
            ErrorKind::BadHeader
        );
        assert_eq!(
            Header::parse(b"xof 0303abcd0032").unwrap_err().kind,
            ErrorKind::UnsupportedFormat("abcd".into())
        );
    }

    #[test]
    fn rejects_unknown_formats_clearly() {
        let err = XFile::parse(b"xof 0303abcd0032\0\0\0").unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnsupportedFormat("abcd".into()));
        assert_eq!(err.to_string(), "unsupported .X format 'abcd'");
    }

    #[test]
    fn a_guid_may_follow_the_opening_brace() {
        let file = XFile::parse(
            b"xof 0303txt 0032\nFrame A { <9e415a43-7ba6-4a73-8743-b73d47e88476> 1; }",
        )
        .unwrap();
        assert_eq!(
            file.objects[0].guid.as_deref(),
            Some("9e415a43-7ba6-4a73-8743-b73d47e88476")
        );
        assert_eq!(file.objects[0].values, vec![Value::Int(1)]);
        // Only first in the body, and only once.
        let err = XFile::parse(b"xof 0303txt 0032\nFrame A { 1; <abc> }").unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnexpectedToken("<abc>".into()));
        let err = XFile::parse(b"xof 0303txt 0032\nFrame A <abc> { <def> }").unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnexpectedToken("<def>".into()));
    }

    #[test]
    fn reads_templates_objects_and_references() {
        let file = XFile::parse(SMALL.as_bytes()).unwrap();
        assert_eq!(file.templates.len(), 1);
        assert_eq!(file.templates[0].name, "Frame");
        assert_eq!(
            file.templates[0].guid.as_deref(),
            Some("3d82ab46-62da-11cf-ab39-0020af71e433")
        );

        assert_eq!(file.objects.len(), 3);
        let ticks = &file.objects[0];
        assert_eq!(ticks.template, "AnimTicksPerSecond");
        assert_eq!(ticks.name, None);
        assert_eq!(ticks.values, vec![Value::Int(4800)]);
        assert_eq!(ticks.line, 7);

        let root = &file.objects[1];
        assert_eq!(root.template, "Frame");
        assert_eq!(root.name.as_deref(), Some("Root"));
        assert!(root.values.is_empty());
        assert_eq!(root.children.len(), 3);
        let matrix = root.child("FrameTransformMatrix").unwrap();
        assert_eq!(matrix.values.len(), 16);
        assert_eq!(matrix.values[0], Value::Float(1.0));
        assert_eq!(matrix.values[1], Value::Int(0));
        assert_eq!(matrix.values[13], Value::Int(-3));
        assert_eq!(matrix.values[14], Value::Float(100.0));
        let mesh = root.child("Mesh").unwrap();
        // 1 count + 9 floats + 1 count + 4 face values.
        assert_eq!(mesh.values.len(), 15);
        let uv = mesh.child("MeshTextureCoords").unwrap();
        assert_eq!(uv.values.len(), 7);
        let child = root.objects_of("Frame").next().unwrap();
        assert_eq!(child.name.as_deref(), Some("Child"));
        assert_eq!(
            child.guid.as_deref(),
            Some("9e415a43-7ba6-4a73-8743-b73d47e88476")
        );
        assert!(child.children.is_empty());

        let set = &file.objects[2];
        assert_eq!(set.name.as_deref(), Some("walk"));
        let anim = set.child("Animation").unwrap();
        let key = anim.child("AnimationKey").unwrap();
        assert_eq!(key.values, vec![Value::Int(4), Value::Int(0)]);
        let refs: Vec<_> = anim.references().collect();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name.as_deref(), Some("Child"));
        assert_eq!(refs[0].guid, None);

        assert_eq!(file.find("Child").unwrap().template, "Frame");
        assert_eq!(file.find("walk").unwrap().template, "AnimationSet");
        assert!(file.find("child").is_none());
        assert_eq!(file.objects_of("Frame").count(), 1);
    }

    #[test]
    fn the_value_cursor_checks_kinds() {
        let file = XFile::parse(SMALL.as_bytes()).unwrap();
        let mesh = file.find("Root").unwrap().child("Mesh").unwrap();
        let mut values = mesh.values();
        assert_eq!(values.count().unwrap(), 3);
        assert_eq!(values.vec3().unwrap(), [0.0, 0.0, 0.0]);
        assert_eq!(values.remaining(), 11);
        let err = values.string().unwrap_err();
        assert_eq!(
            err.kind,
            ErrorKind::WrongValue {
                object: "Mesh".into(),
                expected: "a string",
                found: "the number 1".into()
            }
        );
        assert_eq!(err.line, 15);
        let mut values = file.objects[0].values();
        assert_eq!(values.float().unwrap(), 4800.0);
        let err = values.int().unwrap_err();
        assert!(matches!(err.kind, ErrorKind::MissingValue { .. }));
        assert_eq!(
            err.to_string(),
            "line 7: AnimTicksPerSecond: expected an integer, found the end of the object's values"
        );
    }

    #[test]
    fn reports_malformed_text() {
        let err = XFile::parse(b"xof 0303txt 0032\nFrame Root {\n").unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnexpectedEnd);
        assert_eq!(err.line, 3);
        let err = XFile::parse(b"xof 0303txt 0032\nFrame Root { \"open").unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnexpectedEnd);
        let err = XFile::parse(b"xof 0303txt 0032\n}").unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnexpectedToken("}".into()));
        let err = XFile::parse(b"xof 0303txt 0032\nFrame { 1.2.3; }").unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnexpectedToken("1.2.3".into()));
        let err = XFile::parse(b"xof 0303txt 0032\nFrame { @ }").unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnexpectedToken("@".into()));
        let err = XFile::parse(b"xof 0303txt 0032\nFrame { { a b } }").unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnexpectedToken("b".into()));
    }

    #[test]
    fn an_empty_file_has_no_objects() {
        let file = XFile::parse(b"xof 0303txt 0032\n# nothing\n").unwrap();
        assert!(file.objects.is_empty());
        assert!(file.templates.is_empty());
    }

    #[test]
    fn negative_and_exponent_numbers_are_read() {
        let file = XFile::parse(b"xof 0303txt 0032\nX { -5; +7; -0.5; 2E-3; .5; }").unwrap();
        assert_eq!(
            file.objects[0].values,
            vec![
                Value::Int(-5),
                Value::Int(7),
                Value::Float(-0.5),
                Value::Float(0.002),
                Value::Float(0.5)
            ]
        );
    }

    /// Reads the models of the Wintermute 3D demo when they are present
    /// under `testdata/local/wme_demo_3d/` (never in the repository).
    #[test]
    fn reads_the_demo_models_when_present() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/local/wme_demo_3d");
        let path = dir.join("trinity.x");
        if !path.is_file() {
            eprintln!("skipped: {} is absent", path.display());
            return;
        }
        let file = XFile::parse(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(file.templates.len(), 14);
        // AnimTicksPerSecond, the root frame and seven animation sets.
        assert_eq!(file.objects.len(), 9);
        assert_eq!(file.objects_of("AnimationSet").count(), 7);
        let root = file.find("Bip01").unwrap();
        assert_eq!(root.template, "Frame");
        let mesh = root.child("Mesh").unwrap();
        assert_eq!(mesh.values().count().unwrap(), 825);
        assert_eq!(mesh.objects_of("SkinWeights").count(), 40);
        for name in ["teapot.x", "teapot_taken.x"] {
            let path = dir.join(name);
            if path.is_file() {
                XFile::parse(&std::fs::read(&path).unwrap()).unwrap();
            }
        }
    }
}
