//! The DirectX `.X` file container, binary form (`bin `) and MSZIP
//! compressed forms (`tzip`, `bzip`), read into the same object tree as
//! the text form of [`crate::xfile`].
//!
//! The binary form is a stream of 16-bit little-endian **tokens**: some
//! carry a record (a name, a string, an integer, a GUID, a list of
//! integers or of floats), the others stand alone (braces, separators, the
//! `template` keyword and the type keywords of template declarations).
//! The data section has the same shape as in text (`Template [name] {
//! [<guid>] values... children... }`), so the same template-agnostic
//! reading applies: the values of an object are kept as the flat list of
//! its integers, floats and strings, templates are recorded by name and
//! GUID and their members skipped.
//!
//! A compressed file keeps the 16-byte header in clear and follows it
//! with a 32-bit uncompressed size and MSZIP blocks: a 16-bit size, the
//! signature `CK`, a raw deflate stream. The blocks are inflated with
//! `flate2`, with the previous output as the dictionary of the next block
//! as MSZIP allows. `docs/formats/xfile.md` says which parts of the layout
//! are established and which are inferred.
//!
//! The token codes and the record layouts are from the public DirectX
//! documentation of the `.X` file format. See `NOTICE` at the root of the
//! repository for the MIT-licensed Wintermute sources that govern how the
//! loaded objects are consumed.

use std::fmt;

use flate2::{Decompress, FlushDecompress, Status};

use crate::xfile::{
    Child, DataObject, Error, ErrorKind, HEADER_LEN, Reference, Result, Template, Value,
};

// Record-bearing tokens.
const TOKEN_NAME: u16 = 1;
const TOKEN_STRING: u16 = 2;
const TOKEN_INTEGER: u16 = 3;
const TOKEN_GUID: u16 = 5;
const TOKEN_INTEGER_LIST: u16 = 6;
const TOKEN_FLOAT_LIST: u16 = 7;
// Stand-alone tokens.
const TOKEN_OBRACE: u16 = 10;
const TOKEN_CBRACE: u16 = 11;
const TOKEN_OPAREN: u16 = 12;
const TOKEN_CPAREN: u16 = 13;
const TOKEN_OBRACKET: u16 = 14;
const TOKEN_CBRACKET: u16 = 15;
const TOKEN_OANGLE: u16 = 16;
const TOKEN_CANGLE: u16 = 17;
const TOKEN_DOT: u16 = 18;
const TOKEN_COMMA: u16 = 19;
const TOKEN_SEMICOLON: u16 = 20;
const TOKEN_TEMPLATE: u16 = 31;
// Type keywords of template declarations: WORD, DWORD, FLOAT, DOUBLE,
// CHAR, UCHAR, SWORD, SDWORD, VOID, LPSTR, UNICODE, CSTRING, ARRAY.
const TOKEN_WORD: u16 = 40;
const TOKEN_ARRAY: u16 = 52;

/// The signature that opens every MSZIP block.
const MSZIP_SIGNATURE: [u8; 2] = *b"CK";
/// The deflate window: how much of the previous output the next block
/// may reference.
const MSZIP_WINDOW: usize = 32 * 1024;
/// Cap on the buffer pre-sized from the declared uncompressed size, so a
/// corrupt size cannot ask for an absurd allocation.
const MAX_SIZE_HINT: usize = 256 * 1024 * 1024;

/// A token of the binary form, with its record.
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Name(String),
    Str(String),
    Int(u32),
    Guid(String),
    IntList(Vec<u32>),
    FloatList(Vec<f64>),
    OpenBrace,
    CloseBrace,
    /// `;` or `,`.
    Separator,
    Template,
    /// A keyword of the template grammar (parentheses, brackets, angle
    /// brackets, the dot and the type names), met only inside templates.
    Keyword(u16),
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Name(s) => write!(f, "{s}"),
            Token::Str(s) => write!(f, "\"{s}\""),
            Token::Int(i) => write!(f, "{i}"),
            Token::Guid(g) => write!(f, "<{g}>"),
            Token::IntList(list) => write!(f, "a list of {} integers", list.len()),
            Token::FloatList(list) => write!(f, "a list of {} floats", list.len()),
            Token::OpenBrace => write!(f, "{{"),
            Token::CloseBrace => write!(f, "}}"),
            Token::Separator => write!(f, ";"),
            Token::Template => write!(f, "template"),
            Token::Keyword(code) => write!(f, "binary token {code:#06x}"),
        }
    }
}

fn error(kind: ErrorKind) -> Error {
    Error { kind, line: 0 }
}

/// Reads the data section of a binary file: the bytes after the 16-byte
/// header (or the decompressed data of a `bzip` file), with the float
/// size the header declares. Returns the templates and the top-level
/// objects; the objects carry no line (`line` is 0).
pub(crate) fn parse_body(body: &[u8], float_size: u16) -> Result<(Vec<Template>, Vec<DataObject>)> {
    let doubles = match float_size {
        32 => false,
        64 => true,
        _ => return Err(error(ErrorKind::BadHeader)),
    };
    let mut tokens = Tokens {
        src: body,
        pos: 0,
        doubles,
    };
    let mut templates = Vec::new();
    let mut objects = Vec::new();
    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Template => templates.push(tokens.template()?),
            Token::Name(template) => objects.push(tokens.object(template)?),
            other => return Err(tokens.unexpected(&other)),
        }
    }
    Ok((templates, objects))
}

struct Tokens<'a> {
    src: &'a [u8],
    pos: usize,
    /// Floats are 64-bit (`0064` in the header) rather than 32-bit.
    doubles: bool,
}

impl Tokens<'_> {
    fn unexpected(&self, token: &Token) -> Error {
        error(ErrorKind::UnexpectedToken(token.to_string()))
    }

    /// The offset of the reader in the file, for messages.
    fn offset(&self) -> usize {
        self.pos + HEADER_LEN
    }

    fn bytes(&mut self, len: usize) -> Result<&[u8]> {
        let end = self
            .pos
            .checked_add(len)
            .filter(|&end| end <= self.src.len())
            .ok_or_else(|| error(ErrorKind::UnexpectedEnd))?;
        let slice = &self.src[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u16(&mut self) -> Result<u16> {
        let bytes = self.bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        let bytes = self.bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// A counted byte string, read as Latin-1 like the text lexer does.
    fn text(&mut self) -> Result<String> {
        let count = self.u32()? as usize;
        Ok(self.bytes(count)?.iter().map(|&b| char::from(b)).collect())
    }

    fn peek_u16(&self) -> Option<u16> {
        let bytes = self.src.get(self.pos..self.pos + 2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn next_token(&mut self) -> Result<Option<Token>> {
        if self.pos >= self.src.len() {
            return Ok(None);
        }
        let offset = self.offset();
        let token = match self.u16()? {
            TOKEN_NAME => Token::Name(self.text()?),
            TOKEN_STRING => {
                let text = self.text()?;
                // The record ends with the separator that follows the
                // string. A writer that left it out is tolerated.
                if let Some(TOKEN_SEMICOLON | TOKEN_COMMA) = self.peek_u16() {
                    self.pos += 2;
                }
                Token::Str(text)
            }
            TOKEN_INTEGER => Token::Int(self.u32()?),
            TOKEN_GUID => {
                let data1 = self.u32()?;
                let data2 = self.u16()?;
                let data3 = self.u16()?;
                let data4 = self.bytes(8)?;
                Token::Guid(format_guid(data1, data2, data3, data4))
            }
            TOKEN_INTEGER_LIST => {
                let count = self.u32()? as usize;
                let bytes = self.bytes(count.saturating_mul(4))?;
                Token::IntList(
                    bytes
                        .chunks_exact(4)
                        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect(),
                )
            }
            TOKEN_FLOAT_LIST => {
                let count = self.u32()? as usize;
                let doubles = self.doubles;
                let size = if doubles { 8 } else { 4 };
                let bytes = self.bytes(count.saturating_mul(size))?;
                let floats = if doubles {
                    bytes
                        .chunks_exact(8)
                        .map(|c| f64::from_le_bytes(c.try_into().expect("8-byte chunk")))
                        .collect()
                } else {
                    bytes
                        .chunks_exact(4)
                        .map(|c| f64::from(f32::from_le_bytes([c[0], c[1], c[2], c[3]])))
                        .collect()
                };
                Token::FloatList(floats)
            }
            TOKEN_OBRACE => Token::OpenBrace,
            TOKEN_CBRACE => Token::CloseBrace,
            TOKEN_COMMA | TOKEN_SEMICOLON => Token::Separator,
            TOKEN_TEMPLATE => Token::Template,
            code @ (TOKEN_OPAREN
            | TOKEN_CPAREN
            | TOKEN_OBRACKET
            | TOKEN_CBRACKET
            | TOKEN_OANGLE
            | TOKEN_CANGLE
            | TOKEN_DOT
            | TOKEN_WORD..=TOKEN_ARRAY) => Token::Keyword(code),
            code => {
                return Err(error(ErrorKind::UnexpectedToken(format!(
                    "unknown binary token {code:#06x} at offset {offset}"
                ))));
            }
        };
        Ok(Some(token))
    }

    fn expect_token(&mut self) -> Result<Token> {
        self.next_token()?
            .ok_or_else(|| error(ErrorKind::UnexpectedEnd))
    }

    /// Reads `template Name { <guid> members... }` after the `template`
    /// token. Every token of the body is decoded (the records carry
    /// their length) and skipped to the matching brace; the first GUID
    /// is the template's.
    fn template(&mut self) -> Result<Template> {
        let name = match self.expect_token()? {
            Token::Name(name) => name,
            other => return Err(self.unexpected(&other)),
        };
        match self.expect_token()? {
            Token::OpenBrace => {}
            other => return Err(self.unexpected(&other)),
        }
        let mut guid = None;
        let mut depth = 1usize;
        loop {
            match self.expect_token()? {
                Token::OpenBrace => depth += 1,
                Token::CloseBrace => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                Token::Guid(g) if guid.is_none() => guid = Some(g),
                _ => {}
            }
        }
        Ok(Template { name, guid })
    }

    /// Reads a data object after its template name. The object's GUID may
    /// come before the brace (as in text) or right after it (as the
    /// binary grammar puts it).
    fn object(&mut self, template: String) -> Result<DataObject> {
        let mut name = None;
        let mut guid = None;
        loop {
            match self.expect_token()? {
                Token::Name(n) if name.is_none() && guid.is_none() => name = Some(n),
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
            line: 0,
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
                Token::Int(i) => object.values.push(Value::Int(i64::from(i))),
                Token::IntList(list) => object
                    .values
                    .extend(list.into_iter().map(|i| Value::Int(i64::from(i)))),
                Token::FloatList(list) => object.values.extend(list.into_iter().map(Value::Float)),
                Token::Str(s) => object.values.push(Value::Str(s)),
                Token::Name(child_template) => {
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
                Token::Name(name) if reference.name.is_none() => reference.name = Some(name),
                Token::Guid(guid) if reference.guid.is_none() => reference.guid = Some(guid),
                Token::CloseBrace => break,
                other => return Err(self.unexpected(&other)),
            }
        }
        Ok(reference)
    }
}

/// A binary GUID in the text form `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`,
/// lowercase.
fn format_guid(data1: u32, data2: u16, data3: u16, data4: &[u8]) -> String {
    let mut s = format!("{data1:08x}-{data2:04x}-{data3:04x}-");
    for (i, byte) in data4.iter().enumerate() {
        if i == 2 {
            s.push('-');
        }
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

/// Inflates the MSZIP data that follows the header of a `tzip` or `bzip`
/// file: a 32-bit uncompressed size, then blocks of a 16-bit size, the
/// signature `CK` and a raw deflate stream. Returns the decompressed data
/// section.
pub(crate) fn decompress_mszip(body: &[u8]) -> Result<Vec<u8>> {
    let bad = |message: String| error(ErrorKind::BadCompression(message));
    if body.len() < 4 {
        return Err(error(ErrorKind::UnexpectedEnd));
    }
    let declared = u32::from_le_bytes([body[0], body[1], body[2], body[3]]) as usize;
    let mut out: Vec<u8> = Vec::with_capacity(declared.min(MAX_SIZE_HINT));
    let mut pos = 4;
    // A block needs at least its size word and the signature.
    while pos < body.len() {
        if body.len() - pos < 4 {
            return Err(error(ErrorKind::UnexpectedEnd));
        }
        let block_offset = pos + HEADER_LEN;
        let word = |at: usize| u16::from_le_bytes([body[at], body[at + 1]]);
        // The size word, then `CK`. A second size word before the
        // signature (uncompressed and compressed sizes, as a cabinet
        // stores them) is accepted too.
        let (sizes, data_start): (Vec<usize>, usize) = if body[pos + 2..pos + 4] == MSZIP_SIGNATURE
        {
            (vec![usize::from(word(pos))], pos + 4)
        } else if body.len() - pos >= 6 && body[pos + 4..pos + 6] == MSZIP_SIGNATURE {
            (
                vec![usize::from(word(pos)), usize::from(word(pos + 2))],
                pos + 6,
            )
        } else {
            return Err(bad(format!(
                "no CK signature at the block at offset {block_offset}"
            )));
        };
        let dictionary = &out[out.len().saturating_sub(MSZIP_WINDOW)..];
        let (block, consumed) = inflate_block(&body[data_start..], dictionary)?;
        // The size word is the block's uncompressed size, or its
        // compressed size with or without the signature.
        let plausible = |size: usize| {
            size == block.len() || size == consumed || size == consumed + MSZIP_SIGNATURE.len()
        };
        if !sizes.iter().copied().all(plausible) {
            return Err(bad(format!(
                "the block at offset {block_offset} declares {} bytes but inflated {} bytes from {consumed}",
                sizes
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(" and "),
                block.len()
            )));
        }
        out.extend_from_slice(&block);
        pos = data_start + consumed;
    }
    Ok(out)
}

/// Inflates one raw deflate stream from the start of `input`, with
/// `dictionary` (at most the deflate window) as the history it may
/// reference. Returns the output and the number of input bytes the stream
/// used.
///
/// `flate2`'s Rust back end has no preset-dictionary call, so the
/// dictionary is fed to the decoder as a **stored deflate block** that is
/// not final: it enters the history window exactly like output of the
/// stream would, and its copy is dropped from the result.
fn inflate_block(input: &[u8], dictionary: &[u8]) -> Result<(Vec<u8>, usize)> {
    debug_assert!(dictionary.len() <= MSZIP_WINDOW);
    let bad = |e: flate2::DecompressError| error(ErrorKind::BadCompression(e.to_string()));
    let mut decoder = Decompress::new(false);
    let mut out = Vec::with_capacity(dictionary.len() + MSZIP_WINDOW);
    let mut prefix_len = 0;
    if !dictionary.is_empty() {
        let len = dictionary.len() as u16;
        let mut stored = Vec::with_capacity(5 + dictionary.len());
        stored.push(0); // BFINAL = 0, BTYPE = 00 (stored), padding.
        stored.extend_from_slice(&len.to_le_bytes());
        stored.extend_from_slice(&(!len).to_le_bytes());
        stored.extend_from_slice(dictionary);
        prefix_len = stored.len();
        let status = decoder
            .decompress_vec(&stored, &mut out, FlushDecompress::None)
            .map_err(bad)?;
        debug_assert_eq!(status, Status::Ok);
        debug_assert_eq!(decoder.total_in() as usize, prefix_len);
        debug_assert_eq!(out.len(), dictionary.len());
    }
    loop {
        let consumed = decoder.total_in() as usize - prefix_len;
        let produced = out.len();
        if out.len() == out.capacity() {
            out.reserve(MSZIP_WINDOW);
        }
        let status = decoder
            .decompress_vec(&input[consumed..], &mut out, FlushDecompress::None)
            .map_err(bad)?;
        match status {
            Status::StreamEnd => break,
            Status::Ok | Status::BufError => {
                let stuck =
                    decoder.total_in() as usize - prefix_len == consumed && out.len() == produced;
                if stuck && consumed == input.len() {
                    return Err(error(ErrorKind::UnexpectedEnd));
                }
            }
        }
    }
    let consumed = decoder.total_in() as usize - prefix_len;
    out.drain(..dictionary.len());
    Ok((out, consumed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;
    use crate::xfile::{Format, Header, XFile};
    use flate2::{Compress, Compression, FlushCompress};

    /// Writes the token stream of a binary file.
    struct Writer {
        bytes: Vec<u8>,
        doubles: bool,
    }

    impl Writer {
        fn new(doubles: bool) -> Writer {
            let header = if doubles {
                b"xof 0302bin 0064"
            } else {
                b"xof 0302bin 0032"
            };
            Writer {
                bytes: header.to_vec(),
                doubles,
            }
        }

        fn tok(&mut self, code: u16) -> &mut Self {
            self.bytes.extend_from_slice(&code.to_le_bytes());
            self
        }

        fn counted(&mut self, code: u16, text: &str) -> &mut Self {
            self.tok(code);
            self.bytes
                .extend_from_slice(&(text.len() as u32).to_le_bytes());
            self.bytes.extend_from_slice(text.as_bytes());
            self
        }

        fn name(&mut self, text: &str) -> &mut Self {
            self.counted(TOKEN_NAME, text)
        }

        fn string(&mut self, text: &str) -> &mut Self {
            self.counted(TOKEN_STRING, text).tok(TOKEN_SEMICOLON)
        }

        fn int(&mut self, value: u32) -> &mut Self {
            self.tok(TOKEN_INTEGER);
            self.bytes.extend_from_slice(&value.to_le_bytes());
            self
        }

        fn ints(&mut self, values: &[u32]) -> &mut Self {
            self.tok(TOKEN_INTEGER_LIST);
            self.bytes
                .extend_from_slice(&(values.len() as u32).to_le_bytes());
            for v in values {
                self.bytes.extend_from_slice(&v.to_le_bytes());
            }
            self
        }

        fn floats(&mut self, values: &[f64]) -> &mut Self {
            self.tok(TOKEN_FLOAT_LIST);
            self.bytes
                .extend_from_slice(&(values.len() as u32).to_le_bytes());
            for &v in values {
                if self.doubles {
                    self.bytes.extend_from_slice(&v.to_le_bytes());
                } else {
                    self.bytes.extend_from_slice(&(v as f32).to_le_bytes());
                }
            }
            self
        }

        fn guid(&mut self, data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> &mut Self {
            self.tok(TOKEN_GUID);
            self.bytes.extend_from_slice(&data1.to_le_bytes());
            self.bytes.extend_from_slice(&data2.to_le_bytes());
            self.bytes.extend_from_slice(&data3.to_le_bytes());
            self.bytes.extend_from_slice(&data4);
            self
        }

        fn open(&mut self) -> &mut Self {
            self.tok(TOKEN_OBRACE)
        }

        fn close(&mut self) -> &mut Self {
            self.tok(TOKEN_CBRACE)
        }

        /// `{ name }`.
        fn reference(&mut self, name: &str) -> &mut Self {
            self.open().name(name).close()
        }
    }

    const IDENTITY: [f64; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];

    /// The two-bone quad of the model tests (`model::tests::QUAD`), as a
    /// binary token stream: a template with a restriction, nested
    /// objects, strings, integer and float lists, references.
    fn quad_binary(doubles: bool) -> Vec<u8> {
        let mut w = Writer::new(doubles);
        // template Frame { <3d82ab46-62da-11cf-ab39-0020af71e433> [...] }
        w.tok(TOKEN_TEMPLATE)
            .name("Frame")
            .open()
            .guid(
                0x3d82ab46,
                0x62da,
                0x11cf,
                [0xab, 0x39, 0x00, 0x20, 0xaf, 0x71, 0xe4, 0x33],
            )
            .tok(TOKEN_OBRACKET)
            .tok(TOKEN_DOT)
            .tok(TOKEN_DOT)
            .tok(TOKEN_DOT)
            .tok(TOKEN_CBRACKET)
            .close();
        w.name("AnimTicksPerSecond").open().ints(&[4800]).close();
        w.name("Frame").name("Root").open();
        w.name("FrameTransformMatrix")
            .open()
            .floats(&IDENTITY)
            .close();
        w.name("Frame").name("Tip").open();
        let mut tip = IDENTITY;
        tip[12] = 2.0;
        w.name("FrameTransformMatrix").open().floats(&tip).close();
        w.close();
        w.name("Mesh").name("quad").open();
        w.ints(&[4])
            .floats(&[0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0, 0.0, 1.0, 0.0, 0.0, 1.0])
            .ints(&[1])
            .ints(&[4, 0, 1, 2, 3]);
        w.name("MeshNormals")
            .open()
            .ints(&[1])
            .floats(&[0.0, 1.0, 0.0])
            .ints(&[1, 4, 0, 0, 0, 0])
            .close();
        w.name("MeshTextureCoords")
            .open()
            .ints(&[4])
            .floats(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0])
            .close();
        w.name("MeshMaterialList").open().ints(&[1, 1, 0]);
        w.name("Material")
            .open()
            .floats(&[0.8, 0.8, 0.8, 1.0])
            .floats(&[0.0])
            .floats(&[0.0, 0.0, 0.0])
            .floats(&[0.0, 0.0, 0.0]);
        w.name("TextureFilename").open().string("skin.bmp").close();
        w.close(); // Material
        w.close(); // MeshMaterialList
        w.name("XSkinMeshHeader").open().ints(&[1, 2, 2]).close();
        w.name("SkinWeights")
            .open()
            .string("Root")
            .ints(&[2, 0, 3])
            .floats(&[1.0, 1.0])
            .floats(&IDENTITY)
            .close();
        let mut offset = IDENTITY;
        offset[12] = -2.0;
        w.name("SkinWeights")
            .open()
            .string("Tip")
            .ints(&[2, 1, 2])
            .floats(&[1.0, 1.0])
            .floats(&offset)
            .close();
        w.close(); // Mesh
        w.close(); // Frame Root
        w.name("AnimationSet").name("swing").open();
        w.name("Animation").open();
        w.name("AnimationKey")
            .open()
            .ints(&[0, 2])
            .ints(&[0, 4])
            .floats(&[1.0, 0.0, 0.0, 0.0])
            .ints(&[4800, 4])
            .floats(&[0.6, 0.0, 0.8, 0.0])
            .close();
        w.name("AnimationKey")
            .open()
            .ints(&[2, 2])
            .ints(&[0, 3])
            .floats(&[2.0, 0.0, 0.0])
            .ints(&[4800, 3])
            .floats(&[2.0, 0.0, 0.0])
            .close();
        w.name("AnimationOptions").open().int(1).int(0).close();
        w.reference("Tip");
        w.close(); // Animation
        w.name("Animation").open();
        w.name("AnimationKey")
            .open()
            .ints(&[2, 3])
            .ints(&[0, 3])
            .floats(&[0.0, 0.0, 0.0])
            .ints(&[2400, 3])
            .floats(&[0.0, 5.0, 0.0])
            .ints(&[4800, 3])
            .floats(&[0.0, 10.0, 0.0])
            .close();
        w.reference("Root");
        w.close(); // Animation
        w.close(); // AnimationSet
        w.bytes
    }

    /// The same quad in text, floats written as floats so that the value
    /// kinds match the binary lists.
    const QUAD_TEXT: &str = r#"xof 0303txt 0032
template Frame {
 <3d82ab46-62da-11cf-ab39-0020af71e433>
 [...]
}

AnimTicksPerSecond {
 4800;
}

Frame Root {
 FrameTransformMatrix {
  1.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0, 0.0,0.0,1.0,0.0, 0.0,0.0,0.0,1.0;;
 }
 Frame Tip {
  FrameTransformMatrix {
   1.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0, 0.0,0.0,1.0,0.0, 2.0,0.0,0.0,1.0;;
  }
 }
 Mesh quad {
  4;
  0.0;0.0;0.0;,
  2.0;0.0;0.0;,
  2.0;0.0;1.0;,
  0.0;0.0;1.0;;
  1;
  4;0,1,2,3;;

  MeshNormals {
   1;
   0.0;1.0;0.0;;
   1;
   4;0,0,0,0;;
  }

  MeshTextureCoords {
   4;
   0.0;0.0;,
   1.0;0.0;,
   1.0;1.0;,
   0.0;1.0;;
  }

  MeshMaterialList {
   1;
   1;
   0;

   Material {
    0.8;0.8;0.8;1.0;;
    0.0;
    0.0;0.0;0.0;;
    0.0;0.0;0.0;;

    TextureFilename {
     "skin.bmp";
    }
   }
  }

  XSkinMeshHeader {
   1;
   2;
   2;
  }

  SkinWeights {
   "Root";
   2;
   0,
   3;
   1.0,
   1.0;
   1.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0, 0.0,0.0,1.0,0.0, 0.0,0.0,0.0,1.0;;
  }

  SkinWeights {
   "Tip";
   2;
   1,
   2;
   1.0,
   1.0;
   1.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0, 0.0,0.0,1.0,0.0, -2.0,0.0,0.0,1.0;;
  }
 }
}

AnimationSet swing {
 Animation {
  AnimationKey {
   0;
   2;
   0;4;1.0,0.0,0.0,0.0;;,
   4800;4;0.6,0.0,0.8,0.0;;;
  }
  AnimationKey {
   2;
   2;
   0;3;2.0,0.0,0.0;;,
   4800;3;2.0,0.0,0.0;;;
  }
  AnimationOptions {
   1;
   0;
  }
  { Tip }
 }
 Animation {
  AnimationKey {
   2;
   3;
   0;3;0.0,0.0,0.0;;,
   2400;3;0.0,5.0,0.0;;,
   4800;3;0.0,10.0,0.0;;;
  }
  { Root }
 }
}
"#;

    /// The tree with every `line` cleared, to compare a text tree with a
    /// binary one.
    fn without_lines(objects: &[DataObject]) -> Vec<DataObject> {
        objects
            .iter()
            .map(|object| {
                let mut object = object.clone();
                object.line = 0;
                object.children = object
                    .children
                    .iter()
                    .map(|child| match child {
                        Child::Object(o) => {
                            Child::Object(without_lines(std::slice::from_ref(o)).remove(0))
                        }
                        Child::Reference(r) => Child::Reference(r.clone()),
                    })
                    .collect();
                object
            })
            .collect()
    }

    /// Deflates `data` as independent raw streams of at most `block`
    /// bytes, in the MSZIP layout: a 32-bit total, then per block the
    /// 16-bit uncompressed size, `CK` and the stream.
    fn mszip(data: &[u8], block: usize) -> Vec<u8> {
        let mut out = (data.len() as u32).to_le_bytes().to_vec();
        for chunk in data.chunks(block) {
            let mut compressor = Compress::new(Compression::default(), false);
            let mut deflated = Vec::with_capacity(chunk.len() + 64);
            let status = compressor
                .compress_vec(chunk, &mut deflated, FlushCompress::Finish)
                .unwrap();
            assert_eq!(status, Status::StreamEnd);
            out.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
            out.extend_from_slice(&MSZIP_SIGNATURE);
            out.extend_from_slice(&deflated);
        }
        out
    }

    /// A compressed file: the header with the compressed tag, then the
    /// MSZIP data of the body.
    fn compressed(file: &[u8], tag: &[u8; 4], block: usize) -> Vec<u8> {
        let mut out = file[..HEADER_LEN].to_vec();
        out[8..12].copy_from_slice(tag);
        out.extend_from_slice(&mszip(&file[HEADER_LEN..], block));
        out
    }

    #[test]
    fn a_binary_file_gives_the_same_tree_as_its_text_twin() {
        let text = XFile::parse(QUAD_TEXT.as_bytes()).unwrap();
        let binary = XFile::parse(&quad_binary(true)).unwrap();
        assert_eq!(binary.header.format, Format::Binary);
        assert_eq!(binary.header.float_size, 64);
        assert_eq!(binary.templates, text.templates);
        assert_eq!(binary.objects, without_lines(&text.objects));
        assert_eq!(binary.objects.len(), 3);
        // 1 count + 12 floats + 1 count + 5 face values.
        assert_eq!(binary.find("quad").unwrap().values.len(), 19);
        assert_eq!(
            Model::from_xfile(&binary, "quad.x").unwrap(),
            Model::from_xfile(&text, "quad.x").unwrap()
        );
    }

    #[test]
    fn thirty_two_bit_floats_are_widened() {
        let binary = XFile::parse(&quad_binary(false)).unwrap();
        assert_eq!(binary.header.float_size, 32);
        let model = Model::from_xfile(&binary, "quad.x").unwrap();
        assert_eq!(model.meshes[0].bones.len(), 2);
        // 0.8 is not exact in single precision; exact values are.
        let material = binary
            .find("quad")
            .unwrap()
            .child("MeshMaterialList")
            .unwrap();
        let diffuse = &material.child("Material").unwrap().values;
        assert_eq!(diffuse[0], Value::Float(f64::from(0.8f32)));
        assert_eq!(diffuse[3], Value::Float(1.0));
        let mut w = Writer::new(false);
        w.name("X").open().floats(&[0.5, -2.0, 1e10]).close();
        let file = XFile::parse(&w.bytes).unwrap();
        assert_eq!(
            file.objects[0].values,
            vec![Value::Float(0.5), Value::Float(-2.0), Value::Float(1e10)]
        );
    }

    #[test]
    fn guids_single_integers_and_references_are_read() {
        let mut w = Writer::new(false);
        // Frame A { <guid> 7; { B <guid> } { <guid> } }, with the class
        // GUID after the brace as the binary grammar writes it.
        let d4 = [0xab, 0x39, 0x00, 0x20, 0xaf, 0x71, 0xe4, 0x33];
        w.name("Frame")
            .name("A")
            .open()
            .guid(0x3d82ab46, 0x62da, 0x11cf, d4);
        w.int(7).tok(TOKEN_SEMICOLON);
        w.open().name("B").guid(1, 2, 3, [0; 8]).close();
        w.open().guid(0xffffffff, 0xffff, 0xffff, [0xff; 8]).close();
        w.close();
        // Frame <guid> C { }: the GUID before the brace, as in text.
        w.name("Frame")
            .name("C")
            .guid(1, 2, 3, [0; 8])
            .open()
            .close();
        let file = XFile::parse(&w.bytes).unwrap();
        let a = &file.objects[0];
        assert_eq!(a.name.as_deref(), Some("A"));
        assert_eq!(
            a.guid.as_deref(),
            Some("3d82ab46-62da-11cf-ab39-0020af71e433")
        );
        assert_eq!(a.values, vec![Value::Int(7)]);
        let refs: Vec<_> = a.references().collect();
        assert_eq!(refs[0].name.as_deref(), Some("B"));
        assert_eq!(
            refs[0].guid.as_deref(),
            Some("00000001-0002-0003-0000-000000000000")
        );
        assert_eq!(refs[1].name, None);
        assert_eq!(
            refs[1].guid.as_deref(),
            Some("ffffffff-ffff-ffff-ffff-ffffffffffff")
        );
        assert_eq!(file.objects[1].name.as_deref(), Some("C"));
        assert_eq!(
            file.objects[1].guid.as_deref(),
            Some("00000001-0002-0003-0000-000000000000")
        );
    }

    #[test]
    fn a_string_without_its_terminator_is_tolerated() {
        let mut w = Writer::new(false);
        w.name("TextureFilename")
            .open()
            .counted(TOKEN_STRING, "a.bmp")
            .close();
        let file = XFile::parse(&w.bytes).unwrap();
        assert_eq!(file.objects[0].values, vec![Value::Str("a.bmp".into())]);
    }

    #[test]
    fn reports_truncated_and_malformed_binary() {
        let full = quad_binary(false);
        // Cut inside a record, inside a list and between tokens.
        for cut in [
            HEADER_LEN + 3,
            HEADER_LEN + 9,
            100,
            101,
            200,
            full.len() - 2,
        ] {
            let err = XFile::parse(&full[..cut]).unwrap_err();
            assert_eq!(err.kind, ErrorKind::UnexpectedEnd, "cut at {cut}");
            assert_eq!(err.line, 0);
        }
        // An odd trailing byte.
        let err = XFile::parse(b"xof 0302bin 0032\0").unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnexpectedEnd);
        // An unknown token code.
        let mut w = Writer::new(false);
        w.name("Frame").open().tok(0x0063).close();
        let err = XFile::parse(&w.bytes).unwrap_err();
        assert_eq!(
            err.kind,
            ErrorKind::UnexpectedToken("unknown binary token 0x0063 at offset 29".into())
        );
        assert_eq!(
            err.to_string(),
            "unexpected token 'unknown binary token 0x0063 at offset 29'"
        );
        // A closing brace at the top level, a keyword in an object.
        let mut w = Writer::new(false);
        w.close();
        let err = XFile::parse(&w.bytes).unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnexpectedToken("}".into()));
        let mut w = Writer::new(false);
        w.name("Frame").open().tok(TOKEN_ARRAY).close();
        let err = XFile::parse(&w.bytes).unwrap_err();
        assert_eq!(
            err.kind,
            ErrorKind::UnexpectedToken("binary token 0x0034".into())
        );
        // A list whose count overruns the data.
        let mut w = Writer::new(false);
        w.name("Frame").open().tok(TOKEN_INTEGER_LIST);
        w.bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        w.close();
        let err = XFile::parse(&w.bytes).unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnexpectedEnd);
        // A float size that is neither 32 nor 64.
        let err = XFile::parse(b"xof 0302bin 0016").unwrap_err();
        assert_eq!(err.kind, ErrorKind::BadHeader);
    }

    #[test]
    fn a_template_body_is_skipped_token_by_token() {
        let mut w = Writer::new(false);
        // template Vector { <guid> FLOAT x; FLOAT y; array DWORD f[3]; [Frame <guid>] }
        w.tok(TOKEN_TEMPLATE).name("Vector").open();
        w.guid(
            0x3d82ab45,
            0x62da,
            0x11cf,
            [0xab, 0x39, 0x00, 0x20, 0xaf, 0x71, 0xe4, 0x33],
        );
        w.tok(42).name("x").tok(TOKEN_SEMICOLON);
        w.tok(42).name("y").tok(TOKEN_SEMICOLON);
        w.tok(TOKEN_ARRAY)
            .tok(41)
            .name("f")
            .tok(TOKEN_OBRACKET)
            .int(3)
            .tok(TOKEN_CBRACKET)
            .tok(TOKEN_SEMICOLON);
        w.tok(TOKEN_OBRACKET)
            .name("Frame")
            .guid(9, 9, 9, [9; 8])
            .tok(TOKEN_CBRACKET);
        w.close();
        w.name("Vector").open().floats(&[1.0, 2.0]).close();
        let file = XFile::parse(&w.bytes).unwrap();
        assert_eq!(
            file.templates,
            vec![Template {
                name: "Vector".into(),
                guid: Some("3d82ab45-62da-11cf-ab39-0020af71e433".into())
            }]
        );
        assert_eq!(
            file.objects[0].values,
            vec![Value::Float(1.0), Value::Float(2.0)]
        );
        // An unterminated template.
        let mut w = Writer::new(false);
        w.tok(TOKEN_TEMPLATE)
            .name("Vector")
            .open()
            .tok(42)
            .name("x");
        assert_eq!(
            XFile::parse(&w.bytes).unwrap_err().kind,
            ErrorKind::UnexpectedEnd
        );
    }

    #[test]
    fn compressed_binary_gives_the_same_tree() {
        let binary = quad_binary(true);
        let plain = XFile::parse(&binary).unwrap();
        for block in [100, 1000, 1 << 15] {
            let file = compressed(&binary, b"bzip", block);
            let parsed = XFile::parse(&file).unwrap();
            assert_eq!(parsed.header.format, Format::CompressedBinary);
            assert_eq!(parsed.templates, plain.templates);
            assert_eq!(parsed.objects, plain.objects);
        }
        assert!(mszip(&binary[HEADER_LEN..], 100).len() > 20);
    }

    #[test]
    fn compressed_text_gives_the_same_tree_with_its_lines() {
        let text = XFile::parse(QUAD_TEXT.as_bytes()).unwrap();
        let file = compressed(QUAD_TEXT.as_bytes(), b"tzip", 300);
        let parsed = XFile::parse(&file).unwrap();
        assert_eq!(parsed.header.format, Format::CompressedText);
        assert_eq!(parsed.templates, text.templates);
        assert_eq!(parsed.objects, text.objects);
        assert_eq!(parsed.find("quad").unwrap().line, 20);
    }

    #[test]
    fn a_decompressed_body_that_repeats_the_header_is_accepted() {
        let binary = quad_binary(false);
        let plain = XFile::parse(&binary).unwrap();
        let mut file = binary[..HEADER_LEN].to_vec();
        file[8..12].copy_from_slice(b"bzip");
        file.extend_from_slice(&mszip(&binary, 500));
        let parsed = XFile::parse(&file).unwrap();
        assert_eq!(parsed.objects, plain.objects);
        // The inner header must be a plain one.
        let mut nested = binary[..HEADER_LEN].to_vec();
        nested[8..12].copy_from_slice(b"bzip");
        let mut file = nested.clone();
        file.extend_from_slice(&mszip(&nested, 500));
        assert_eq!(
            XFile::parse(&file).unwrap_err().kind,
            ErrorKind::UnsupportedFormat("bzip".into())
        );
    }

    #[test]
    fn a_block_may_reference_the_previous_block() {
        // Block 1: 40 bytes, deflated on their own. Block 2: a fixed
        // Huffman block hand-encoded as "copy 10 bytes from 10 bytes
        // back" and the end of block, which only inflates with the
        // previous block as history.
        let data1: Vec<u8> = (b'a'..=b'z')
            .chain(b'0'..=b'9')
            .chain(b'A'..=b'D')
            .collect();
        assert_eq!(data1.len(), 40);
        let mut body = mszip(&data1, 1 << 15);
        body.extend_from_slice(&10u16.to_le_bytes());
        body.extend_from_slice(&MSZIP_SIGNATURE);
        body.extend_from_slice(&[0x43, 0xB0, 0x00]);
        let out = decompress_mszip(&body).unwrap();
        let mut expected = data1.clone();
        expected.extend_from_slice(&data1[30..]);
        assert_eq!(out, expected);
        // The hand-encoded block alone has nothing to copy from (the
        // decoder then yields zeros rather than an error, so only the
        // content is checked).
        let mut alone = 10u32.to_le_bytes().to_vec();
        alone.extend_from_slice(&10u16.to_le_bytes());
        alone.extend_from_slice(&MSZIP_SIGNATURE);
        alone.extend_from_slice(&[0x43, 0xB0, 0x00]);
        if let Ok(out) = decompress_mszip(&alone) {
            assert_ne!(out, data1[30..].to_vec());
        }
    }

    #[test]
    fn the_history_is_capped_at_the_window() {
        // Three blocks of 20 KiB: the third may only see the last 32 KiB.
        let data: Vec<u8> = (0..60 * 1024).map(|i| (i % 251) as u8).collect();
        let body = mszip(&data, 20 * 1024);
        assert_eq!(decompress_mszip(&body).unwrap(), data);
    }

    #[test]
    fn size_words_are_checked_both_ways() {
        let data = b"the quick brown fox jumps over the lazy dog".to_vec();
        let mut body = mszip(&data, 1 << 15);
        // The block's compressed size, with or without the signature,
        // instead of its uncompressed size.
        let deflated_len = body.len() - 4 - 4;
        body[4..6].copy_from_slice(&(deflated_len as u16).to_le_bytes());
        assert_eq!(decompress_mszip(&body).unwrap(), data);
        body[4..6].copy_from_slice(&((deflated_len + 2) as u16).to_le_bytes());
        assert_eq!(decompress_mszip(&body).unwrap(), data);
        // Two size words before the signature.
        let mut two = 0u32.to_le_bytes().to_vec();
        two.extend_from_slice(&(data.len() as u16).to_le_bytes());
        two.extend_from_slice(&(deflated_len as u16).to_le_bytes());
        two.extend_from_slice(&body[6..]);
        assert_eq!(decompress_mszip(&two).unwrap(), data);
        // A size that matches nothing.
        body[4..6].copy_from_slice(&7u16.to_le_bytes());
        let err = decompress_mszip(&body).unwrap_err();
        assert!(
            matches!(err.kind, ErrorKind::BadCompression(ref m) if m.contains("declares 7 bytes"))
        );
        assert!(
            err.to_string()
                .starts_with("MSZIP: the block at offset 20 declares 7 bytes")
        );
    }

    #[test]
    fn reports_malformed_compressed_data() {
        let binary = quad_binary(false);
        let file = compressed(&binary, b"bzip", 200);
        // Truncated inside a deflate stream and inside a block header.
        for cut in [file.len() - 1, file.len() - 10, HEADER_LEN + 4 + 3] {
            let err = XFile::parse(&file[..cut]).unwrap_err();
            assert_eq!(err.kind, ErrorKind::UnexpectedEnd, "cut at {cut}");
        }
        // Too short for the total size.
        assert_eq!(
            XFile::parse(b"xof 0303bzip0032\0\0").unwrap_err().kind,
            ErrorKind::UnexpectedEnd
        );
        // No signature.
        let mut bad = file.clone();
        bad[HEADER_LEN + 6..HEADER_LEN + 8].copy_from_slice(b"ZZ");
        let err = XFile::parse(&bad).unwrap_err();
        assert_eq!(
            err.kind,
            ErrorKind::BadCompression("no CK signature at the block at offset 20".into())
        );
        // Corrupt deflate data.
        let mut bad = file.clone();
        for byte in &mut bad[HEADER_LEN + 8..HEADER_LEN + 40] {
            *byte = 0xff;
        }
        assert!(matches!(
            XFile::parse(&bad).unwrap_err().kind,
            ErrorKind::BadCompression(_)
        ));
        // Trailing bytes too short for a block are a truncation; a body
        // with no block at all is an empty file.
        let mut padded = file.clone();
        padded.extend_from_slice(&[0, 0, 0]);
        assert_eq!(
            XFile::parse(&padded).unwrap_err().kind,
            ErrorKind::UnexpectedEnd
        );
        let empty = XFile::parse(b"xof 0303tzip0032\0\0\0\0").unwrap();
        assert!(empty.objects.is_empty());
    }

    /// Reads every binary or compressed `.x` file found under
    /// `testdata/local/` (never in the repository). Skips when there is
    /// none; the local games at hand so far ship no `.x` file at all.
    #[test]
    fn reads_the_local_binary_models_when_present() {
        fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    collect(&path, out);
                } else if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("x"))
                {
                    out.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/local");
        let mut paths = Vec::new();
        collect(&root, &mut paths);
        paths.sort();
        let mut read = 0;
        for path in paths {
            let bytes = std::fs::read(&path).unwrap();
            let Ok(header) = Header::parse(&bytes) else {
                continue;
            };
            if header.format == Format::Text {
                continue;
            }
            let file = XFile::parse(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(!file.objects.is_empty(), "{}: no objects", path.display());
            eprintln!(
                "{}: {} ({} templates, {} objects)",
                path.display(),
                header.format.tag(),
                file.templates.len(),
                file.objects.len()
            );
            read += 1;
        }
        if read == 0 {
            eprintln!(
                "skipped: no binary or compressed .x file under {}",
                root.display()
            );
        }
    }
}
