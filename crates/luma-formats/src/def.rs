//! The text syntax shared by Wintermute Engine definition files.
//!
//! The syntax is documented in `docs/formats/definition-files.md` and was
//! established from the MIT-licensed WME Lite sources (`BParser.cpp`,
//! Copyright (c) 2011 Jan Nedoma) and analysis of game files. See `NOTICE`
//! at the root of the repository.
//!
//! A file is a sequence of statements, each a key followed by an
//! assignment (`KEY = value`), a block (`KEY { ... }`), a list
//! (`KEY { 1, 2, 3 }`) or nothing. The parser keeps scalar values as raw
//! text and converts them on access with the engine's rules.
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

/// A parsed definition file: its top-level statements.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    /// Top-level statements, in file order.
    pub nodes: Vec<Node>,
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
}

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
    /// Integer value with the engine's `atoi` rules: optional sign, then
    /// digits; anything after the digits is ignored. `None` when the text
    /// does not start with a number.
    pub fn as_int(&self) -> Option<i64> {
        parse_int(&self.text)
    }

    /// Float value with the engine's `atof` rules: the longest numeric
    /// prefix is used. `None` when the text does not start with a number.
    pub fn as_float(&self) -> Option<f64> {
        let t = self.text.trim_start();
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

    /// Boolean value with the engine's rules: `TRUE`, `YES`, `ON` and `1`
    /// (ignoring case) are true, everything else is false.
    pub fn as_bool(&self) -> bool {
        let t = self.text.trim();
        ["true", "yes", "on", "1"]
            .iter()
            .any(|w| t.eq_ignore_ascii_case(w))
    }

    /// Comma-separated integers, as in `TRANSPARENT = 255,0,0`.
    pub fn as_ints(&self) -> Vec<i64> {
        self.text
            .split(',')
            .map_while(|part| parse_int(part.trim()))
            .collect()
    }
}

fn parse_int(text: &str) -> Option<i64> {
    let t = text.trim_start();
    let (negative, rest) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let value: i64 = digits.parse().unwrap_or(i64::MAX);
    Some(if negative { -value } else { value })
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

    /// The first scalar value with this key, as text.
    fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Node::as_str)
    }

    /// The first scalar value with this key, as an integer.
    fn get_int(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(Node::as_int)
    }

    /// The first scalar value with this key, as a float.
    fn get_float(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Node::as_float)
    }

    /// The first scalar value with this key, as a boolean.
    fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(Node::as_bool)
    }

    /// The first value with this key, as a list of integers (either a
    /// list block or a comma-separated scalar).
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

    /// The scalar value as text.
    pub fn as_str(&self) -> Option<&str> {
        self.scalar().map(|s| s.text.as_str())
    }

    /// The scalar value as an integer.
    pub fn as_int(&self) -> Option<i64> {
        self.scalar().and_then(Scalar::as_int)
    }

    /// The scalar value as a float.
    pub fn as_float(&self) -> Option<f64> {
        self.scalar().and_then(Scalar::as_float)
    }

    /// The scalar value as a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        self.scalar().map(Scalar::as_bool)
    }

    /// The value as integers: a list block, or a comma-separated scalar.
    pub fn as_ints(&self) -> Option<Vec<i64>> {
        match &self.value {
            Value::List(items) => Some(items.iter().map_while(Scalar::as_int).collect()),
            Value::Scalar(s) => Some(s.as_ints()),
            _ => None,
        }
    }

    /// The children, if the node is a block.
    pub fn children(&self) -> Option<&[Node]> {
        match &self.value {
            Value::Block(nodes) => Some(nodes),
            _ => None,
        }
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
    pub fn parse(bytes: &[u8]) -> Result<Document, ParseError> {
        let text: String = bytes.iter().map(|&b| char::from(b)).collect();
        Self::parse_str(&text)
    }

    /// Parses a definition file from a string.
    pub fn parse_str(text: &str) -> Result<Document, ParseError> {
        let mut parser = Parser {
            src: text.as_bytes(),
            text,
            pos: 0,
            line: 1,
        };
        let nodes = parser.statements(false)?;
        Ok(Document { nodes })
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
        Value::Block(children) => {
            writeln!(f, " {{")?;
            for child in children {
                write_node(f, child, depth + 1)?;
            }
            writeln!(f, "{indent}}}")
        }
    }
}

struct Parser<'a> {
    src: &'a [u8],
    text: &'a str,
    pos: usize,
    line: u32,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        if c == b'\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        self.error_at(self.line, message)
    }

    fn error_at(&self, line: u32, message: impl Into<String>) -> ParseError {
        ParseError {
            line,
            message: message.into(),
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.bump();
        }
    }

    /// Skips whitespace and `;` comments between statements.
    fn skip_blanks(&mut self) {
        loop {
            self.skip_whitespace();
            if self.peek() == Some(b';') {
                while !matches!(self.peek(), None | Some(b'\n')) {
                    self.bump();
                }
            } else {
                return;
            }
        }
    }

    /// Parses statements until the end of input (`nested == false`) or
    /// until the closing brace of the enclosing block, which is consumed.
    fn statements(&mut self, nested: bool) -> Result<Vec<Node>, ParseError> {
        let mut nodes = Vec::new();
        loop {
            self.skip_blanks();
            match self.peek() {
                None if nested => return Err(self.error("unexpected end of file, expected `}`")),
                None => return Ok(nodes),
                Some(b'}') if nested => {
                    self.bump();
                    return Ok(nodes);
                }
                Some(b'}') => return Err(self.error("unexpected `}`")),
                Some(c) if c.is_ascii_alphabetic() || c == b'_' => nodes.push(self.statement()?),
                Some(c) => {
                    return Err(self.error(format!(
                        "unexpected character `{}`, expected a key",
                        char::from(c)
                    )));
                }
            }
        }
    }

    fn statement(&mut self) -> Result<Node, ParseError> {
        let line = self.line;
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == b'_') {
            self.bump();
        }
        let key = self.text[start..self.pos].to_string();
        self.skip_whitespace();

        let name = if self.peek() == Some(b'\'') {
            let line = self.line;
            self.bump();
            let start = self.pos;
            while !matches!(self.peek(), None | Some(b'\'')) {
                self.bump();
            }
            if self.bump().is_none() {
                return Err(self.error_at(line, format!("unterminated name after `{key}`")));
            }
            let name = self.text[start..self.pos - 1].to_string();
            self.skip_whitespace();
            Some(name)
        } else {
            None
        };

        let value = match self.peek() {
            Some(b'=') => {
                self.bump();
                Value::Scalar(self.assignment(&key)?)
            }
            Some(b'{') => {
                self.bump();
                self.block()?
            }
            _ => Value::None,
        };
        Ok(Node {
            key,
            name,
            value,
            line,
        })
    }

    /// The value after `=`: a quoted string up to the next `"`, or a run of
    /// non-whitespace characters.
    fn assignment(&mut self, key: &str) -> Result<Scalar, ParseError> {
        self.skip_whitespace();
        if self.peek() == Some(b'"') {
            let line = self.line;
            self.bump();
            let start = self.pos;
            while !matches!(self.peek(), None | Some(b'"')) {
                self.bump();
            }
            if self.bump().is_none() {
                return Err(self.error_at(line, format!("unterminated string in `{key}`")));
            }
            return Ok(Scalar {
                text: self.text[start..self.pos - 1].to_string(),
                quoted: true,
            });
        }
        let start = self.pos;
        while matches!(self.peek(), Some(c) if !matches!(c, b' ' | b'\t' | b'\r' | b'\n')) {
            self.bump();
        }
        Ok(Scalar {
            text: self.text[start..self.pos].to_string(),
            quoted: false,
        })
    }

    /// After `{`: a list when the content is scalars separated by commas,
    /// else nested statements.
    fn block(&mut self) -> Result<Value, ParseError> {
        self.skip_whitespace();
        match self.peek() {
            Some(c) if c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.') => {
                self.list().map(Value::List)
            }
            _ => self.statements(true).map(Value::Block),
        }
    }

    fn list(&mut self) -> Result<Vec<Scalar>, ParseError> {
        let mut items = Vec::new();
        loop {
            self.skip_whitespace();
            let start = self.pos;
            while matches!(self.peek(), Some(c) if !matches!(c, b' ' | b'\t' | b'\r' | b'\n' | b',' | b'}' | b'{' | b'='))
            {
                self.bump();
            }
            if self.pos > start {
                items.push(Scalar {
                    text: self.text[start..self.pos].to_string(),
                    quoted: false,
                });
            }
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                }
                Some(b'}') => {
                    self.bump();
                    return Ok(items);
                }
                None => return Err(self.error("unexpected end of file in list, expected `}`")),
                Some(c @ (b'{' | b'=')) => {
                    return Err(
                        self.error(format!("unexpected character `{}` in list", char::from(c)))
                    );
                }
                // Items separated by whitespace only: the engine reads the
                // first one and stops; we keep them all.
                Some(_) => {}
            }
        }
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
    fn multiple_top_level_blocks() {
        let doc = parse("ITEM { NAME=\"axe\" }\nITEM { NAME=\"key\" }\n");
        let names: Vec<_> = doc
            .get_all("item")
            .map(|n| n.get_str("name").unwrap())
            .collect();
        assert_eq!(names, ["axe", "key"]);
    }

    #[test]
    fn scalar_conversions_follow_the_engine() {
        let s = |t: &str| Scalar {
            text: t.to_string(),
            quoted: false,
        };
        assert_eq!(s("42").as_int(), Some(42));
        assert_eq!(s("-7").as_int(), Some(-7));
        assert_eq!(s("+3").as_int(), Some(3));
        assert_eq!(s("12abc").as_int(), Some(12));
        assert_eq!(s("1.9").as_int(), Some(1));
        assert_eq!(s("abc").as_int(), None);
        assert_eq!(s("").as_int(), None);
        assert_eq!(s("1.5").as_float(), Some(1.5));
        assert_eq!(s("-.5").as_float(), Some(-0.5));
        assert_eq!(s("2e3").as_float(), Some(2000.0));
        assert_eq!(s("3.").as_float(), Some(3.0));
        assert_eq!(s("x").as_float(), None);
        for t in ["TRUE", "true", "Yes", "ON", "1"] {
            assert!(s(t).as_bool(), "{t}");
        }
        for t in ["FALSE", "no", "0", "2", "", "maybe"] {
            assert!(!s(t).as_bool(), "{t}");
        }
        assert_eq!(s("255,0,0").as_ints(), vec![255, 0, 0]);
        assert_eq!(s("1, 2 ,x,4").as_ints(), vec![1, 2]);
    }

    #[test]
    fn list_edge_cases() {
        let doc = parse("A { 1,2,3, }\nB {-1, 2.5 }\nC {1 2}");
        assert_eq!(doc.get_ints("A"), Some(vec![1, 2, 3]));
        assert_eq!(doc.get_ints("B"), Some(vec![-1, 2]));
        // "1 2" is one item then a missing comma: the engine reads 1 and
        // stops; we keep both items so nothing is silently lost.
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

    #[test]
    fn errors_carry_line_numbers() {
        let err = Document::parse_str("A {\n  B = 1\n").unwrap_err();
        assert_eq!(err.line, 3);
        assert!(err.message.contains("expected `}`"), "{err}");

        let err = Document::parse_str("A {\n  B = \"open\n}\n").unwrap_err();
        assert_eq!(err.line, 2);
        assert!(err.message.contains("unterminated string"), "{err}");

        let err = Document::parse_str("\n\n  }").unwrap_err();
        assert_eq!(err.line, 3);

        let err = Document::parse_str("A {\n  = 1\n}").unwrap_err();
        assert_eq!(err.line, 2);
        assert!(err.message.contains("expected a key"), "{err}");

        let err = Document::parse_str("A { 1, 2 = }").unwrap_err();
        assert!(err.message.contains("in list"), "{err}");

        let err = Document::parse_str("A 'x").unwrap_err();
        assert!(err.message.contains("unterminated name"), "{err}");
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
