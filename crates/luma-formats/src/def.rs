//! The text syntax shared by Wintermute Engine definition files.
//!
//! The syntax is documented in `docs/formats/definition-files.md` and was
//! established from the MIT-licensed Wintermute Engine sources
//! (`BParser.cpp`, Copyright (c) 2011 Jan Nedoma) and analysis of game
//! files. See `NOTICE` at the root of the repository.
//!
//! A file is a sequence of statements, each a key followed by an
//! assignment (`KEY = value`), a block (`KEY { ... }`), a list
//! (`KEY { 1, 2, 3 }`) or nothing. The parser keeps scalar values as raw
//! text and converts them on access with the engine's rules
//! (`CBParser::ScanStr`): see [`scan_int`], [`scan_ints`], [`scan_float`]
//! and [`scan_bool`].
//!
//! The original engine builds no tree: each loader walks its block with
//! its own table of keys (`CBParser::GetObject`) and reads the text of a
//! value as it likes, so a scalar may be written in braces
//! (`ALPHA { 128 }`). A [`Node`] therefore keeps the text of a block next
//! to its parsed content, and a loader checks its keys against the
//! original's table with [`check_keys`]: a key that is not in the table
//! is a syntax error that fails the load.
//!
//! ```
//! use luma_formats::{Document, Nodes};
//!
//! let doc = Document::parse_str("SPRITE {\n  LOOPING=TRUE\n  FRAME { DELAY = 100\n IMAGE=\"a.png\" }\n}")?;
//! let sprite = doc.get("sprite").unwrap();
//! assert_eq!(sprite.get_bool("looping"), Some(true));
//! let frame = sprite.get("FRAME").unwrap();
//! assert_eq!(frame.get_int("delay"), Some(100));
//! assert_eq!(frame.get_str("image"), Some("a.png"));
//! # Ok::<(), luma_formats::ParseError>(())
//! ```

use std::fmt;
use std::sync::Arc;

/// How deep blocks may nest. No file of the original engine goes beyond a
/// handful of levels; the bound keeps a hostile file from exhausting the
/// stack.
const MAX_DEPTH: u32 = 64;

/// A parsed definition file: its top-level statements.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    /// Top-level statements, in file order.
    pub nodes: Vec<Node>,
    /// The syntax error that ended the top level early, when one did. The
    /// statements before it are kept: the original engine reads a file
    /// one statement at a time, so a loader that only wants the root
    /// block never sees what follows it (a stray `}` at the end of a
    /// hand-written file), and a loader that reads every top-level
    /// statement fails there. See [`Document::syntax_error`].
    trailing: Option<ParseError>,
}

/// One statement: a key, an optional single-quoted name, and a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The key, as written (keys are compared ignoring ASCII case).
    pub key: String,
    /// The optional `'name'` between the key and the value.
    pub name: Option<String>,
    /// The value.
    pub value: Value,
    /// 1-based line of the key in the source.
    pub line: u32,
    /// The text between the braces of a block or list, as written.
    raw: Raw,
    /// Why the content of a block could not be read as statements, when
    /// it could not (`NAME { "x" }`): the block then holds the statements
    /// read before the error. Reported by [`keys`].
    malformed: Option<Box<ParseError>>,
}

/// A range of the source text, kept by reference.
#[derive(Clone)]
struct Raw {
    src: Arc<str>,
    start: usize,
    end: usize,
}

impl Raw {
    fn text(&self) -> &str {
        self.src.get(self.start..self.end).unwrap_or("")
    }
}

impl fmt::Debug for Raw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.text(), f)
    }
}

impl PartialEq for Raw {
    fn eq(&self, other: &Raw) -> bool {
        self.text() == other.text()
    }
}

impl Eq for Raw {}

/// The value of a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A bare key with no value.
    None,
    /// `KEY = scalar`.
    Scalar(Scalar),
    /// `KEY { a, b, c }`.
    List(Vec<Scalar>),
    /// `KEY { statements }`.
    Block(Vec<Node>),
}

/// A scalar value, kept as raw text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scalar {
    /// The text, without quotes.
    pub text: String,
    /// Whether it was written between double quotes.
    pub quoted: bool,
}

impl Scalar {
    /// Integer value when the text starts with a number: optional sign,
    /// then digits; anything after the digits is ignored. `None` when the
    /// text does not start with a number (the engine's loaders read 0
    /// there: see [`scan_int`]).
    pub fn as_int(&self) -> Option<i64> {
        parse_int(&self.text)
    }

    /// Float value when the text starts with a number: the longest
    /// numeric prefix is used. `None` when the text does not start with a
    /// number (the engine's loaders read 0 there: see [`scan_float`]).
    pub fn as_float(&self) -> Option<f64> {
        parse_float(&self.text)
    }

    /// Boolean value with the engine's rule ([`scan_bool`]); an empty
    /// text is false.
    pub fn as_bool(&self) -> bool {
        scan_bool(&self.text).unwrap_or(false)
    }

    /// Comma-separated integers, as in `TRANSPARENT = 255,0,0`
    /// ([`scan_ints`]).
    pub fn as_ints(&self) -> Vec<i64> {
        scan_ints(&self.text)
    }
}

/// The characters `CBParser::ScanStr` skips around values. A carriage
/// return is not one of them.
fn scan_skip(text: &str) -> &str {
    text.trim_start_matches([' ', '\t', '\n', '\x0c'])
}

/// The characters C's `isspace` knows, which `atoi` and `atof` skip.
fn c_skip(text: &str) -> &str {
    text.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r'])
}

fn parse_int(text: &str) -> Option<i64> {
    let t = c_skip(text);
    let (negative, rest) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let value: i64 = rest[..digits].parse().unwrap_or(i64::MAX);
    Some(if negative { -value } else { value })
}

fn parse_float(text: &str) -> Option<f64> {
    let t = c_skip(text);
    let mut end = 0;
    let b = t.as_bytes();
    if end < b.len() && (b[end] == b'+' || b[end] == b'-') {
        end += 1;
    }
    let int_start = end;
    while end < b.len() && b[end].is_ascii_digit() {
        end += 1;
    }
    let mut digits = end - int_start;
    if end < b.len() && b[end] == b'.' {
        let mut frac_end = end + 1;
        while frac_end < b.len() && b[frac_end].is_ascii_digit() {
            frac_end += 1;
        }
        digits += frac_end - end - 1;
        if digits > 0 {
            end = frac_end;
        }
    }
    if digits == 0 {
        return None;
    }
    if end < b.len() && (b[end] == b'e' || b[end] == b'E') {
        let mut exp_end = end + 1;
        if exp_end < b.len() && (b[exp_end] == b'+' || b[exp_end] == b'-') {
            exp_end += 1;
        }
        let exp_digits = exp_end;
        while exp_end < b.len() && b[exp_end].is_ascii_digit() {
            exp_end += 1;
        }
        if exp_end > exp_digits {
            end = exp_end;
        }
    }
    t[..end].parse().ok()
}

/// `CBParser::ScanStr(text, "%d", &v)`: the integer the engine reads from
/// a value. `None` when the text is empty (the engine then leaves the
/// variable as it was); text that is not a number reads as 0 (`atoi`).
pub fn scan_int(text: &str) -> Option<i64> {
    scan_ints(text).first().copied()
}

/// `CBParser::ScanStr(text, "%d,%d,...", ...)`: the integers the engine
/// reads from a value, in order. The scan stops where a value is not
/// followed by a comma, and the variables after that keep what they had;
/// a part that is not a number reads as 0. After a number the engine
/// skips digits, signs, spaces, tabs and line feeds, but not a carriage
/// return: a list broken over lines with CR LF ends at the first line.
pub fn scan_ints(text: &str) -> Vec<i64> {
    let mut values = Vec::new();
    let mut rest = scan_skip(text);
    while !rest.is_empty() {
        rest = scan_skip(rest);
        values.push(parse_int(rest).unwrap_or(0));
        rest = rest.trim_start_matches(|c: char| {
            c.is_ascii_digit() || matches!(c, '+' | '-' | ' ' | '\t' | '\n' | '\x0c')
        });
        match rest.strip_prefix(',') {
            Some(after) => rest = after,
            None => break,
        }
    }
    values
}

/// `CBParser::ScanStr(text, "%D", list, &count)`: a list of integers of
/// any length (the widths of a bitmap font). Unlike [`scan_ints`] the
/// list ends, without a value, where an item does not start with a digit
/// or a sign.
pub fn scan_int_list(text: &str) -> Vec<i64> {
    let mut values = Vec::new();
    let mut rest = scan_skip(text);
    while rest.starts_with(|c: char| c.is_ascii_digit() || c == '+' || c == '-') {
        values.push(parse_int(rest).unwrap_or(0));
        rest = rest.trim_start_matches(|c: char| c.is_ascii_digit() || c == '+' || c == '-');
        rest = scan_skip(rest);
        match rest.strip_prefix(',') {
            Some(after) => rest = scan_skip(after),
            None => break,
        }
    }
    values
}

/// `CBParser::ScanStr(text, "%f", &v)`: the number the engine reads from
/// a value. `None` when the text is empty; text that is not a number
/// reads as 0 (`atof`).
pub fn scan_float(text: &str) -> Option<f64> {
    scan_floats(text).first().copied()
}

/// `CBParser::ScanStr(text, "%f,%f,...", ...)`: as [`scan_ints`], for
/// floating-point values.
pub fn scan_floats(text: &str) -> Vec<f64> {
    let mut values = Vec::new();
    let mut rest = scan_skip(text);
    while !rest.is_empty() {
        rest = scan_skip(rest);
        values.push(parse_float(rest).unwrap_or(0.0));
        rest = rest.trim_start_matches(|c: char| {
            c.is_ascii_digit()
                || matches!(c, '.' | 'e' | 'E' | '+' | '-' | ' ' | '\t' | '\n' | '\x0c')
        });
        match rest.strip_prefix(',') {
            Some(after) => rest = after,
            None => break,
        }
    }
    values
}

/// `CBParser::ScanStr(text, "%b", &v)`: the boolean the engine reads from
/// a value. `None` when the text is empty (the variable keeps what it
/// had). Otherwise the leading run of letters and digits is compared with
/// `yes`, `true`, `on` and `1` over its own length only, ignoring case:
/// any prefix of one of them is true (`T`, `Y`, `O`, `TR`), and so is a
/// value that starts with no letter or digit at all.
pub fn scan_bool(text: &str) -> Option<bool> {
    let rest = scan_skip(text);
    if rest.is_empty() {
        return None;
    }
    let len = rest.bytes().take_while(u8::is_ascii_alphanumeric).count();
    let run = &rest[..len];
    Some(
        ["yes", "true", "on", "1"]
            .iter()
            .any(|word| word.len() >= len && word[..len].eq_ignore_ascii_case(run)),
    )
}

/// The key of `table` that `CBParser::GetObject` finds for a written
/// key: the first one, in table order, that the written key starts with
/// (ignoring ASCII case). `None` when there is none, and also when the
/// first match is shorter than the written key: the original then takes
/// the rest of the word for the next statement, which no table of the
/// engine accepts.
pub fn token<'t>(table: &[&'t str], key: &str) -> Option<&'t str> {
    let found = table.iter().find(|token| {
        key.len() >= token.len()
            && key.as_bytes()[..token.len()].eq_ignore_ascii_case(token.as_bytes())
    })?;
    (found.len() == key.len()).then_some(*found)
}

fn clamp_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// The statements of a block as a loader of the original engine reads
/// them: each one with the key of `table` it matches, in file order. The
/// first statement whose key is not in the table ends the sequence with a
/// syntax error (`PARSERR_TOKENNOTFOUND`, `Syntax error in <what>
/// definition`), and so does the place where the content of the block
/// stops being statements. What was read before the error has been
/// yielded: the original has applied it by then.
pub fn keys<'a>(
    node: &'a Node,
    table: &'a [&'static str],
    what: &'a str,
) -> impl Iterator<Item = Result<(&'static str, &'a Node), ParseError>> + 'a {
    keys_of(node.nodes(), node.syntax_error(), table, what)
}

/// [`keys`] for the top level of a file.
pub fn document_keys<'a>(
    doc: &'a Document,
    table: &'a [&'static str],
    what: &'a str,
) -> impl Iterator<Item = Result<(&'static str, &'a Node), ParseError>> + 'a {
    keys_of(&doc.nodes, doc.syntax_error(), table, what)
}

fn keys_of<'a>(
    nodes: &'a [Node],
    malformed: Option<&'a ParseError>,
    table: &'a [&'static str],
    what: &'a str,
) -> impl Iterator<Item = Result<(&'static str, &'a Node), ParseError>> + 'a {
    let mut failed = false;
    let mut index = 0;
    std::iter::from_fn(move || {
        if failed {
            return None;
        }
        let Some(node) = nodes.get(index) else {
            failed = true;
            return malformed.map(|err| {
                Err(ParseError {
                    line: err.line,
                    message: format!("syntax error in {what} definition: {}", err.message),
                })
            });
        };
        index += 1;
        match token(table, &node.key) {
            Some(key) => Some(Ok((key, node))),
            None => {
                failed = true;
                Some(Err(ParseError {
                    line: node.line,
                    message: format!(
                        "syntax error in {what} definition: unknown key `{}`",
                        node.key
                    ),
                }))
            }
        }
    })
}

/// Checks a whole block against a loader's table of keys: the error
/// [`keys`] would end with, if any.
pub fn check_keys(node: &Node, table: &[&'static str], what: &str) -> Result<(), ParseError> {
    keys(node, table, what)
        .find_map(Result::err)
        .map_or(Ok(()), Err)
}

impl fmt::Display for Scalar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.quoted {
            write!(f, "\"{}\"", self.text)
        } else {
            f.write_str(&self.text)
        }
    }
}

/// Lookup helpers shared by documents and blocks.
pub trait Nodes {
    /// The statements to search.
    fn nodes(&self) -> &[Node];

    /// Every statement with this key, in order (case-insensitive).
    fn get_all<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.nodes()
            .iter()
            .filter(move |n| n.key.eq_ignore_ascii_case(key))
    }

    /// The first statement with this key.
    fn get(&self, key: &str) -> Option<&Node> {
        self.nodes()
            .iter()
            .find(|n| n.key.eq_ignore_ascii_case(key))
    }

    /// The first value with this key, as text.
    fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Node::as_str)
    }

    /// The first value with this key, as an integer.
    fn get_int(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(Node::as_int)
    }

    /// The first value with this key, as a float.
    fn get_float(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Node::as_float)
    }

    /// The first value with this key, as a boolean.
    fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(Node::as_bool)
    }

    /// The first value with this key, as a list of integers.
    fn get_ints(&self, key: &str) -> Option<Vec<i64>> {
        self.get(key).and_then(Node::as_ints)
    }
}

impl Nodes for Document {
    fn nodes(&self) -> &[Node] {
        &self.nodes
    }
}

impl Nodes for Node {
    /// The children of a block; empty for any other value.
    fn nodes(&self) -> &[Node] {
        match &self.value {
            Value::Block(nodes) => nodes,
            _ => &[],
        }
    }
}

impl Node {
    /// The scalar value, if the node is an assignment.
    pub fn scalar(&self) -> Option<&Scalar> {
        match &self.value {
            Value::Scalar(s) => Some(s),
            _ => None,
        }
    }

    /// The text of the value as the engine's loaders receive it: what
    /// follows `=` (without the quotes), or everything between the braces
    /// of a block, untrimmed. `None` for a bare key.
    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            Value::None => None,
            Value::Scalar(s) => Some(s.text.as_str()),
            Value::List(_) | Value::Block(_) => Some(self.raw.text()),
        }
    }

    /// The value as an integer ([`scan_int`]): `X = 5` and `X { 5 }` are
    /// the same, text that is not a number is 0, and `None` means the
    /// value is empty (the engine keeps what the variable had).
    pub fn as_int(&self) -> Option<i64> {
        self.as_str().and_then(scan_int)
    }

    /// The value as a float ([`scan_float`]).
    pub fn as_float(&self) -> Option<f64> {
        self.as_str().and_then(scan_float)
    }

    /// The value as a boolean ([`scan_bool`]).
    pub fn as_bool(&self) -> Option<bool> {
        self.as_str().and_then(scan_bool)
    }

    /// The value as integers ([`scan_ints`]): `P { 1, 2 }` and `P = 1,2`
    /// are the same.
    pub fn as_ints(&self) -> Option<Vec<i64>> {
        self.as_str().map(scan_ints)
    }

    /// `ScanStr(params, "%b", &to)`: sets `to` from the value, or leaves
    /// it when the value is empty.
    pub fn read_bool(&self, to: &mut bool) {
        if let Some(value) = self.as_bool() {
            *to = value;
        }
    }

    /// `ScanStr(params, "%d", &to)`: sets `to` from the value (0 when it
    /// is not a number), or leaves it when the value is empty.
    pub fn read_int(&self, to: &mut i32) {
        if let Some(value) = self.as_int() {
            *to = clamp_i32(value);
        }
    }

    /// `ScanStr(params, "%f", &to)`: sets `to` from the value (0 when it
    /// is not a number), or leaves it when the value is empty.
    pub fn read_float(&self, to: &mut f32) {
        if let Some(value) = self.as_float() {
            *to = value as f32;
        }
    }

    /// `ScanStr(params, "%d,%d,...", ...)`: sets the variables the scan
    /// reaches, in order, and leaves the others.
    pub fn read_ints(&self, to: &mut [&mut i32]) {
        let values = self.as_ints().unwrap_or_default();
        for (slot, value) in to.iter_mut().zip(values) {
            **slot = clamp_i32(value);
        }
    }

    /// The text of the value as an owned string, empty for a bare key.
    pub fn text(&self) -> String {
        self.as_str().unwrap_or_default().to_string()
    }

    /// The children, if the node is a block.
    pub fn children(&self) -> Option<&[Node]> {
        match &self.value {
            Value::Block(nodes) => Some(nodes),
            _ => None,
        }
    }

    /// Why the content of the block is not a sequence of statements, when
    /// it is not.
    pub fn syntax_error(&self) -> Option<&ParseError> {
        self.malformed.as_deref()
    }
}

/// A syntax error, with the 1-based line where it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line.
    pub line: u32,
    /// What went wrong.
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

impl Document {
    /// Parses a definition file. Bytes are 8-bit text; each byte maps to
    /// the character with the same code, which is exact for ASCII.
    ///
    /// The error is that of the first statement. A syntax error after it
    /// ends the document there and is kept in
    /// [`Document::syntax_error`].
    pub fn parse(bytes: &[u8]) -> Result<Document, ParseError> {
        let text: String = bytes.iter().map(|&b| char::from(b)).collect();
        Self::parse_str(&text)
    }

    /// Parses a definition file from a string.
    pub fn parse_str(text: &str) -> Result<Document, ParseError> {
        let src: Arc<str> = Arc::from(text);
        let mut parser = Parser {
            src: &src,
            pos: 0,
            line: 1,
        };
        let mut nodes = Vec::new();
        let trailing = parser.statements(src.len(), 0, &mut nodes).err();
        match trailing {
            Some(err) if nodes.is_empty() => Err(err),
            trailing => Ok(Document { nodes, trailing }),
        }
    }

    /// The first statement, when its key is `key` (ignoring case): the
    /// root block of a file. The original engine reads one statement and
    /// answers `'<KEY>' keyword expected` when it is another one, so a
    /// root block that is not first is not found.
    pub fn root(&self, key: &str) -> Option<&Node> {
        self.nodes
            .first()
            .filter(|node| node.key.eq_ignore_ascii_case(key))
    }

    /// The first place, in file order, where the file is not a sequence
    /// of statements, at any depth: what a tool that checks a file
    /// reports. The engine's loaders do not use it: each one fails, or
    /// not, on the blocks it reads ([`keys`]), and a scalar written in
    /// braces (`NAME { "x" }`) is reported here although it loads.
    pub fn first_syntax_error(&self) -> Option<&ParseError> {
        fn within(nodes: &[Node]) -> Option<&ParseError> {
            nodes
                .iter()
                .find_map(|node| within(node.nodes()).or(node.malformed.as_deref()))
        }
        within(&self.nodes).or(self.trailing.as_ref())
    }

    /// The syntax error that ended the top level before the end of the
    /// file, if any. Loaders that read every top-level statement (items,
    /// the game file) fail on it; loaders that read one root block ignore
    /// what follows that block, as the original does.
    pub fn syntax_error(&self) -> Option<&ParseError> {
        self.trailing.as_ref()
    }
}

impl fmt::Display for Document {
    /// Writes the document back in canonical form: two-space indentation,
    /// one statement per line.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for node in &self.nodes {
            write_node(f, node, 0)?;
        }
        Ok(())
    }
}

fn write_node(f: &mut fmt::Formatter<'_>, node: &Node, depth: usize) -> fmt::Result {
    let indent = "  ".repeat(depth);
    write!(f, "{indent}{}", node.key)?;
    if let Some(name) = &node.name {
        write!(f, " '{name}'")?;
    }
    match &node.value {
        Value::None => writeln!(f),
        Value::Scalar(s) => writeln!(f, " = {s}"),
        Value::List(items) => {
            let items: Vec<String> = items.iter().map(ToString::to_string).collect();
            writeln!(f, " {{ {} }}", items.join(", "))
        }
        Value::Block(_) if node.malformed.is_some() => {
            writeln!(f, " {{{}}}", node.raw.text())
        }
        Value::Block(children) => {
            writeln!(f, " {{")?;
            for child in children {
                write_node(f, child, depth + 1)?;
            }
            writeln!(f, "{indent}}}")
        }
    }
}

/// The parser, after `CBParser::GetObject`: it works on byte offsets of
/// the source, and every offset it stops at is a character boundary (it
/// only stops at ASCII bytes, or after a whole character).
struct Parser<'a> {
    src: &'a Arc<str>,
    pos: usize,
    line: u32,
}

impl Parser<'_> {
    fn byte(&self, at: usize, end: usize) -> Option<u8> {
        if at < end {
            self.src.as_bytes().get(at).copied()
        } else {
            None
        }
    }

    /// Moves to `to`, counting the lines passed.
    fn advance(&mut self, to: usize) {
        let to = to.min(self.src.len());
        if to > self.pos {
            let passed = self.src.as_bytes()[self.pos..to]
                .iter()
                .filter(|&&b| b == b'\n')
                .count();
            self.line = self.line.saturating_add(passed as u32);
            self.pos = to;
        }
    }

    fn skip_whitespace(&mut self, end: usize) {
        let mut at = self.pos;
        while matches!(self.byte(at, end), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            at += 1;
        }
        self.advance(at);
    }

    /// Skips whitespace and `;` comment lines between statements.
    fn skip_blanks(&mut self, end: usize) {
        loop {
            self.skip_whitespace(end);
            if self.byte(self.pos, end) != Some(b';') {
                return;
            }
            let mut at = self.pos;
            while !matches!(self.byte(at, end), None | Some(b'\n')) {
                at += 1;
            }
            self.advance(at);
        }
    }

    /// The offset of the first `stop` byte from `from`, or `end`.
    fn find(&self, from: usize, end: usize, stop: u8) -> usize {
        self.src.as_bytes()[from.min(end)..end]
            .iter()
            .position(|&b| b == stop)
            .map_or(end, |i| from + i)
    }

    /// Parses the statements of `self.pos..end` into `nodes`. On an error
    /// the statements read before it stay in `nodes`.
    fn statements(
        &mut self,
        end: usize,
        depth: u32,
        nodes: &mut Vec<Node>,
    ) -> Result<(), ParseError> {
        loop {
            self.skip_blanks(end);
            match self.byte(self.pos, end) {
                None => return Ok(()),
                Some(c) if c.is_ascii_alphanumeric() || c == b'_' => {
                    nodes.push(self.statement(end, depth)?);
                }
                Some(b'}') => {
                    return Err(ParseError {
                        line: self.line,
                        message: "unexpected `}`".into(),
                    });
                }
                Some(_) => {
                    let c = self.src[self.pos..].chars().next().unwrap_or('?');
                    return Err(ParseError {
                        line: self.line,
                        message: format!("unexpected character `{c}`, expected a key"),
                    });
                }
            }
        }
    }

    fn statement(&mut self, end: usize, depth: u32) -> Result<Node, ParseError> {
        let line = self.line;
        let start = self.pos;
        let mut at = start;
        while matches!(self.byte(at, end), Some(c) if c.is_ascii_alphanumeric() || c == b'_') {
            at += 1;
        }
        let key = self.src[start..at].to_string();
        self.advance(at);
        self.skip_whitespace(end);

        // The optional name: to the next quote, or to the end of the
        // block when it is not closed.
        let name = if self.byte(self.pos, end) == Some(b'\'') {
            let start = self.pos + 1;
            let close = self.find(start, end, b'\'');
            let name = self.src[start..close].to_string();
            self.advance((close + 1).min(end));
            self.skip_whitespace(end);
            Some(name)
        } else {
            None
        };

        let empty = Raw {
            src: Arc::clone(self.src),
            start: self.pos,
            end: self.pos,
        };
        let mut node = Node {
            key,
            name,
            value: Value::None,
            line,
            raw: empty,
            malformed: None,
        };
        match self.byte(self.pos, end) {
            Some(b'=') => {
                self.advance(self.pos + 1);
                node.value = Value::Scalar(self.assignment(end));
            }
            Some(b'{') => {
                // The closing brace is found by counting braces, quotes
                // or not; a block that is not closed runs to the end.
                let start = self.pos + 1;
                let mut open = 1u32;
                let mut close = start;
                while let Some(c) = self.byte(close, end) {
                    match c {
                        b'{' => open += 1,
                        b'}' => {
                            open -= 1;
                            if open == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    close += 1;
                }
                node.raw = Raw {
                    src: Arc::clone(self.src),
                    start,
                    end: close,
                };
                self.advance(start);
                self.block(&mut node, close, depth + 1);
                self.advance((close + 1).min(end));
            }
            _ => {}
        }
        Ok(node)
    }

    /// The value after `=` (`CBParser::GetAssignmentText`): a quoted
    /// string up to the next `"`, or a run that ends at the first space,
    /// control character or character outside ASCII (the original tests
    /// `char <= 0x20` on signed characters), which is consumed.
    fn assignment(&mut self, end: usize) -> Scalar {
        self.skip_whitespace(end);
        if self.byte(self.pos, end) == Some(b'"') {
            let start = self.pos + 1;
            let close = self.find(start, end, b'"');
            let text = self.src[start..close].to_string();
            self.advance((close + 1).min(end));
            return Scalar { text, quoted: true };
        }
        let start = self.pos;
        let mut at = start;
        while matches!(self.byte(at, end), Some(c) if c > 0x20 && c < 0x80) {
            at += 1;
        }
        let text = self.src[start..at].to_string();
        // Skip the terminator: one whole character.
        let skip = self.src[at..end].chars().next().map_or(0, char::len_utf8);
        self.advance(at + skip);
        Scalar {
            text,
            quoted: false,
        }
    }

    /// The content of a block, `self.pos..end`: a list when it starts
    /// with a number, else nested statements. Keys may start with a digit
    /// (`2D_ONLY`), so the whole first token is examined, not only its
    /// first character.
    fn block(&mut self, node: &mut Node, end: usize, depth: u32) {
        self.skip_whitespace(end);
        let token = self.src.as_bytes()[self.pos..end]
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'+' | b'-'))
            .copied()
            .collect::<Vec<u8>>();
        let numeric = !token.is_empty()
            && token
                .iter()
                .all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'+' | b'-'));
        if numeric {
            node.value = Value::List(self.list(end));
            return;
        }
        let mut children = Vec::new();
        let result = if depth > MAX_DEPTH {
            Err(ParseError {
                line: self.line,
                message: "blocks nested too deep".into(),
            })
        } else {
            self.statements(end, depth, &mut children)
        };
        match result {
            Ok(()) => node.value = Value::Block(children),
            Err(err) => {
                // What was read before the error stays: the original has
                // applied it by the time it fails.
                node.value = Value::Block(children);
                node.malformed = Some(Box::new(err));
            }
        }
    }

    /// The items of a list, split at commas and whitespace. The numbers
    /// the engine reads from a list come from its text
    /// ([`Node::as_ints`]); the items are for display.
    fn list(&mut self, end: usize) -> Vec<Scalar> {
        self.src[self.pos..end]
            .split([' ', '\t', '\r', '\n', ','])
            .filter(|item| !item.is_empty())
            .map(|item| Scalar {
                text: item.to_string(),
                quoted: false,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Document {
        Document::parse_str(text).unwrap_or_else(|e| panic!("{e}\n{text}"))
    }

    #[test]
    fn parses_assignments_blocks_and_lists() {
        let doc = parse(
            "; generated by SpriteEdit\r\n\r\nSPRITE {\r\n  NAME=\"\"\r\n  LOOPING=FALSE\r\n  FRAME {\r\n    DELAY = 0\r\n    IMAGE = \"sprites\\system\\cursornormal.png\"\r\n    HOTSPOT {25, 24}\r\n  }\r\n\r\n}\r\n",
        );
        assert_eq!(doc.nodes.len(), 1);
        let sprite = doc.get("SPRITE").unwrap();
        assert_eq!(sprite.line, 3);
        assert_eq!(sprite.get_str("NAME"), Some(""));
        assert_eq!(sprite.get_bool("LOOPING"), Some(false));
        let frame = sprite.get("FRAME").unwrap();
        assert_eq!(frame.line, 6);
        assert_eq!(frame.get_int("DELAY"), Some(0));
        assert_eq!(
            frame.get_str("IMAGE"),
            Some("sprites\\system\\cursornormal.png")
        );
        assert_eq!(frame.get_ints("HOTSPOT"), Some(vec![25, 24]));
        assert_eq!(frame.get("HOTSPOT").unwrap().line, 9);
    }

    #[test]
    fn brace_on_next_line_and_comments_inside_blocks() {
        let doc = parse(
            "FONT\n{\n  IMAGE = \"fonts\\x.bmp\"\n  TRANSPARENT = 255,0,0\n;  ALWAYS_VISIBLE = TRUE\n  ; ----- editor settings\n  AUTO_WIDTH = TRUE\n}\n",
        );
        let font = doc.get("font").unwrap();
        assert_eq!(font.get_ints("transparent"), Some(vec![255, 0, 0]));
        assert!(font.get("always_visible").is_none());
        assert_eq!(font.get_bool("auto_width"), Some(true));
        assert_eq!(font.children().unwrap().len(), 3);
    }

    #[test]
    fn strings_have_no_escapes_and_may_hold_braces_or_semicolons() {
        let doc = parse("W {\n TEXT = \"/\\\"\n GUID=\"{665ADE9A-1727}\"\n A=\"x;y\"\n}\n");
        let w = doc.get("W").unwrap();
        assert_eq!(w.get_str("TEXT"), Some("/\\"));
        assert_eq!(w.get_str("GUID"), Some("{665ADE9A-1727}"));
        assert_eq!(w.get_str("A"), Some("x;y"));
    }

    #[test]
    fn repeated_keys_keep_order() {
        let doc = parse("SCENE {\n SCRIPT=\"a\"\n SCRIPT=\"b\"\n POINT {1,2}\n POINT {3,4}\n}");
        let scene = doc.get("scene").unwrap();
        let scripts: Vec<_> = scene
            .get_all("script")
            .map(|n| n.as_str().unwrap())
            .collect();
        assert_eq!(scripts, ["a", "b"]);
        let points: Vec<_> = scene
            .get_all("point")
            .map(|n| n.as_ints().unwrap())
            .collect();
        assert_eq!(points, [vec![1, 2], vec![3, 4]]);
    }

    #[test]
    fn bare_keys_names_and_empty_blocks() {
        let doc = parse("A\nB 'named' { X=1 }\nC { }\nD = 5");
        assert_eq!(doc.get("A").unwrap().value, Value::None);
        let b = doc.get("B").unwrap();
        assert_eq!(b.name.as_deref(), Some("named"));
        assert_eq!(b.get_int("X"), Some(1));
        assert_eq!(doc.get("C").unwrap().value, Value::Block(vec![]));
        assert_eq!(doc.get_int("D"), Some(5));
    }

    #[test]
    fn keys_may_start_with_a_digit() {
        let doc = parse("FRAME {\n  2D_ONLY=TRUE\n  HOTSPOT {1,2}\n  3D_ONLY = FALSE\n}");
        let frame = doc.get("frame").unwrap();
        assert_eq!(frame.get_bool("2D_ONLY"), Some(true));
        assert_eq!(frame.get_bool("3d_only"), Some(false));
        assert_eq!(frame.get_ints("hotspot"), Some(vec![1, 2]));
        assert_eq!(frame.children().unwrap().len(), 3);
    }

    #[test]
    fn multiple_top_level_blocks() {
        let doc = parse("ITEM { NAME=\"axe\" }\nITEM { NAME=\"key\" }\n");
        let names: Vec<_> = doc
            .get_all("item")
            .map(|n| n.get_str("name").unwrap())
            .collect();
        assert_eq!(names, ["axe", "key"]);
    }

    /// `CBParser::GetAssignmentText` works on the text of the block,
    /// which was cut out at its closing brace first: an unquoted value
    /// cannot run past it.
    #[test]
    fn an_unquoted_value_ends_at_the_closing_brace() {
        let doc = parse("FRAME { DELAY=100}\nP { X=5}\nQ {A=1}{B=2}");
        assert_eq!(doc.get("FRAME").unwrap().get_int("DELAY"), Some(100));
        assert_eq!(doc.get("P").unwrap().get_str("X"), Some("5"));
        assert_eq!(doc.get("Q").unwrap().get_int("A"), Some(1));
    }

    /// The run of an unquoted value ends at any byte the original sees as
    /// `<= 0x20` in a signed character: control characters and every byte
    /// from 0x80 up. The terminator itself is skipped.
    #[test]
    fn an_unquoted_value_ends_at_control_and_high_bytes() {
        let doc = Document::parse(b"A=caf\xE9s B=1\x01C=2\nD=x\x7Fy").unwrap();
        assert_eq!(doc.get_str("A"), Some("caf"));
        // The terminator (0xE9) is consumed, the next statement is `s`.
        assert_eq!(doc.nodes[1].key, "s");
        assert_eq!(doc.get_int("B"), Some(1));
        assert_eq!(doc.get_int("C"), Some(2));
        // 0x7F is above 0x20 and below 0x80: part of the run.
        assert_eq!(doc.get_str("D"), Some("x\x7Fy"));

        // The same file given as a string: the whole character ends the
        // run and is skipped.
        let doc = parse("A=café B=1");
        assert_eq!(doc.get_str("A"), Some("caf"));
        assert_eq!(doc.get_int("B"), Some(1));
    }

    /// The original hands the text between the braces to the loader,
    /// which reads a scalar from it as from an assignment.
    #[test]
    fn a_scalar_may_be_written_in_braces() {
        let doc = parse(
            "E {\n ALPHA { 128 }\n SCALABLE { TRUE }\n NAME { \"x\" }\n CAPTION { two words }\n SCALE { 1.5 }\n}",
        );
        let e = doc.get("E").unwrap();
        assert_eq!(e.get_int("ALPHA"), Some(128));
        assert_eq!(e.get_bool("SCALABLE"), Some(true));
        // The text is not trimmed and keeps its quotes, as in the
        // original (`SetName(params)`).
        assert_eq!(e.get_str("NAME"), Some(" \"x\" "));
        assert!(e.get("NAME").unwrap().syntax_error().is_some());
        assert_eq!(e.get_str("CAPTION"), Some(" two words "));
        assert_eq!(e.get_float("SCALE"), Some(1.5));
        // A block read as a block reports what is wrong with it.
        let err = check_keys(e.get("NAME").unwrap(), &["X"], "E").unwrap_err();
        assert!(
            err.message.contains("syntax error in E definition"),
            "{err}"
        );
    }

    #[test]
    fn scan_bool_compares_the_length_of_the_value_only() {
        for t in [
            "TRUE", "true", "Yes", "ON", "1", "T", "tr", "Y", "ye", "O", "TRUE,", " yes", ",", "-",
        ] {
            assert_eq!(scan_bool(t), Some(true), "{t:?}");
        }
        for t in [
            "FALSE", "no", "0", "2", "maybe", "yess", "10", "truely", "onn",
        ] {
            assert_eq!(scan_bool(t), Some(false), "{t:?}");
        }
        // Nothing to read: the engine leaves the variable alone.
        assert_eq!(scan_bool(""), None);
        assert_eq!(scan_bool(" \t\n"), None);
        // A carriage return is not skipped, and is no letter: true.
        assert_eq!(scan_bool("\r"), Some(true));
    }

    #[test]
    fn scan_numbers_follow_atoi_and_atof() {
        assert_eq!(scan_int("42"), Some(42));
        assert_eq!(scan_int(" -7"), Some(-7));
        assert_eq!(scan_int("+3"), Some(3));
        assert_eq!(scan_int("12abc"), Some(12));
        assert_eq!(scan_int("1.9"), Some(1));
        // Not a number: 0, where an empty text leaves the variable.
        assert_eq!(scan_int("abc"), Some(0));
        assert_eq!(scan_int(""), None);
        assert_eq!(scan_float("1.5"), Some(1.5));
        assert_eq!(scan_float("-.5"), Some(-0.5));
        assert_eq!(scan_float("2e3"), Some(2000.0));
        assert_eq!(scan_float("x"), Some(0.0));
        assert_eq!(scan_float(""), None);
        assert_eq!(scan_floats("1.5, 2 ,3e1"), vec![1.5, 2.0, 30.0]);
    }

    #[test]
    fn scan_ints_stops_where_a_comma_is_missing() {
        assert_eq!(scan_ints("255,0,0"), vec![255, 0, 0]);
        assert_eq!(scan_ints(" 25, 24 "), vec![25, 24]);
        // A part that is not a number is 0 and ends the scan.
        assert_eq!(scan_ints("1, 2 ,x,4"), vec![1, 2, 0]);
        // `1 2`: the 2 is skipped with the digits and spaces after the 1.
        assert_eq!(scan_ints("1 2"), vec![1]);
        assert_eq!(scan_ints("1 2,3"), vec![1, 3]);
        assert_eq!(scan_ints("-1, 2.5"), vec![-1, 2]);
        // After a comma with spaces the engine reads one more value, 0.
        assert_eq!(scan_ints("1,2,3, "), vec![1, 2, 3, 0]);
        assert_eq!(scan_ints("1,2,3,"), vec![1, 2, 3]);
        // A carriage return after a number stops the scan.
        assert_eq!(scan_ints("\n 1,\n 2"), vec![1, 2]);
        assert_eq!(scan_ints("1\r\n,2"), vec![1]);
        assert_eq!(scan_ints("\r\n 1, 2"), vec![1]);
        assert_eq!(scan_ints(""), Vec::<i64>::new());
        // `%D`: no value for what is not a number.
        assert_eq!(scan_int_list(" 1, 2 ,3, "), vec![1, 2, 3]);
        assert_eq!(scan_int_list("1 2"), vec![1]);
        assert_eq!(scan_int_list("1,x"), vec![1]);
        assert_eq!(scan_int_list("1,\r\n2"), vec![1]);
    }

    #[test]
    fn scalar_conversions_tell_numbers_from_text() {
        let s = |t: &str| Scalar {
            text: t.to_string(),
            quoted: false,
        };
        assert_eq!(s("42").as_int(), Some(42));
        assert_eq!(s("abc").as_int(), None);
        assert_eq!(s("").as_int(), None);
        assert_eq!(s("3.").as_float(), Some(3.0));
        assert_eq!(s("x").as_float(), None);
        assert!(s("on").as_bool());
        assert!(!s("").as_bool());
    }

    #[test]
    fn lists_are_read_from_their_text() {
        let doc = parse("A { 1,2,3, }\nB {-1, 2.5 }\nC {1 2}");
        assert_eq!(doc.get_ints("A"), Some(vec![1, 2, 3, 0]));
        assert_eq!(doc.get_ints("B"), Some(vec![-1, 2]));
        // The engine reads 1 and stops; both items are kept for display.
        assert_eq!(doc.get_ints("C"), Some(vec![1]));
        assert_eq!(
            doc.get("C").unwrap().value,
            Value::List(vec![
                Scalar {
                    text: "1".into(),
                    quoted: false
                },
                Scalar {
                    text: "2".into(),
                    quoted: false
                }
            ])
        );
    }

    /// `CBParser::GetSubText`: a name, a string or a block that is not
    /// closed runs to the end of what is being read.
    #[test]
    fn unterminated_text_runs_to_the_end() {
        let doc = parse("A {\n  B = 1\n");
        assert_eq!(doc.get("A").unwrap().get_int("B"), Some(1));

        // The string ends with its block, which was cut out first.
        let doc = parse("A {\n  B = \"open\n}\nC=2");
        assert_eq!(doc.get("A").unwrap().get_str("B"), Some("open\n"));
        assert_eq!(doc.get_int("C"), Some(2));

        let doc = parse("A 'x");
        assert_eq!(doc.get("A").unwrap().name.as_deref(), Some("x"));
    }

    /// Braces are counted without regard to quotes or comments, as the
    /// original does.
    #[test]
    fn braces_are_counted_inside_strings_and_comments() {
        let doc = parse("A {\n T=\"}\"\n X=1\n}\nB=2");
        let a = doc.get("A").unwrap();
        // The block ended at the brace in the string.
        assert_eq!(a.get_str("T"), Some(""));
        assert!(a.get("X").is_none());
        assert!(doc.get("B").is_none());
        assert!(doc.syntax_error().is_some());

        let doc = parse("A {\n ; }\n X=1\n}");
        assert!(doc.get("A").unwrap().get("X").is_none());
    }

    #[test]
    fn errors_carry_line_numbers() {
        let err = Document::parse_str("\n\n  }").unwrap_err();
        assert_eq!(err.line, 3);

        let doc = parse("A {\n  = 1\n}");
        let err = check_keys(doc.get("A").unwrap(), &[], "A").unwrap_err();
        assert_eq!(err.line, 2);
        assert!(err.message.contains("expected a key"), "{err}");
    }

    /// What follows the root block is never read by a loader of the
    /// original that wants one root block.
    #[test]
    fn a_syntax_error_after_the_first_statement_keeps_what_was_read() {
        let doc = parse("SCENE { NAME=\"a\" }\n}\nIGNORED=1");
        assert_eq!(doc.nodes.len(), 1);
        assert_eq!(doc.root("scene").unwrap().get_str("NAME"), Some("a"));
        assert_eq!(doc.syntax_error().unwrap().line, 2);
        assert!(parse("X=1\nSCENE { }").root("SCENE").is_none());

        // A checker sees the first error at any depth.
        let doc = parse("A {\n B {\n  = 1\n }\n}\n}");
        assert_eq!(doc.first_syntax_error().unwrap().line, 3);
        assert_eq!(doc.syntax_error().unwrap().line, 6);
        assert!(parse("A { B=1 }").first_syntax_error().is_none());
    }

    #[test]
    fn keys_match_the_table_by_prefix_in_table_order() {
        let table = ["NAME", "SCALE_LEVEL", "SCALE", "X"];
        assert_eq!(token(&table, "name"), Some("NAME"));
        assert_eq!(token(&table, "SCALE_LEVEL"), Some("SCALE_LEVEL"));
        assert_eq!(token(&table, "Scale"), Some("SCALE"));
        assert_eq!(token(&table, "Y"), None);
        // `NAMES` matches `NAME`, and the `S` left over is no key.
        assert_eq!(token(&table, "NAMES"), None);
        // A shorter key listed first hides the longer one.
        assert_eq!(token(&["SCALE", "SCALE_LEVEL"], "SCALE_LEVEL"), None);

        let doc = parse("R {\n NAME=\"a\"\n X=1\n BOGUS=2\n}");
        let err = check_keys(doc.get("R").unwrap(), &table, "REGION").unwrap_err();
        assert_eq!(err.line, 4);
        assert!(err.message.contains("syntax error in REGION definition"));
        assert!(check_keys(doc.get("R").unwrap(), &["NAME", "X", "BOGUS"], "R").is_ok());
    }

    #[test]
    fn deep_nesting_is_refused_without_overflowing_the_stack() {
        let text = "A {".repeat(100_000);
        let doc = parse(&text);
        let mut node = &doc.nodes[0];
        let mut depth = 0;
        while let Some(child) = node.nodes().first() {
            node = child;
            depth += 1;
        }
        assert!(depth <= MAX_DEPTH as usize + 1, "{depth}");
        assert!(node.syntax_error().is_some());
    }

    #[test]
    fn display_is_canonical_and_round_trips() {
        let doc = parse(
            "; c\nS 'n' {\n  NAME=\"x\"\n  LOOPING = TRUE\n  FRAME\n  {\n    HOTSPOT {25,24}\n    E\n  }\n}\n",
        );
        let text = doc.to_string();
        assert_eq!(
            text,
            "S 'n' {\n  NAME = \"x\"\n  LOOPING = TRUE\n  FRAME {\n    HOTSPOT { 25, 24 }\n    E\n  }\n}\n"
        );
        assert_eq!(parse(&text).to_string(), text);
    }

    #[test]
    fn parses_bytes_as_8_bit_text() {
        let doc = Document::parse(b"A = \"caf\xE9\"").unwrap();
        assert_eq!(doc.get_str("A"), Some("café"));
    }

    /// Parses every definition file of the local reference game, when it
    /// is present (`testdata/local/` is not in the repository).
    #[test]
    fn parses_every_local_game_file() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/local");
        if !root.is_dir() {
            eprintln!("skipped: {} is absent", root.display());
            return;
        }
        let exts = [
            "sprite", "scene", "window", "font", "def", "items", "game", "settings", "entity",
            "actor",
        ];
        let mut stack = vec![root];
        let mut count = 0;
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !exts.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
                    continue;
                }
                let bytes = std::fs::read(&path).unwrap();
                let doc =
                    Document::parse(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert!(!doc.nodes.is_empty(), "{}: empty", path.display());
                // Canonical output must parse back to the same text.
                let text = doc.to_string();
                assert_eq!(Document::parse_str(&text).unwrap().to_string(), text);
                count += 1;
            }
        }
        eprintln!("parsed {count} local definition files");
    }
}
