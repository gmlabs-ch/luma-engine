//! The scanner: bytes to tokens, with `#include` expansion and the line
//! counter the original compiler keeps.
//!
//! Written with reference to `compiler/lang.l` and `compiler/main.c` of
//! Wintermute Engine 1.x (MIT License, Copyright (c) 2003-2013 Jan Nedoma
//! and contributors); see `NOTICE`. The rules are specified in
//! `docs/compiler.md`, section "Lexical structure".
//!
//! Two notions of line live here. The *recorded line* is the original's
//! global counter: it advances on every newline of the main file only, so
//! that every token of an included file carries the line of the
//! `#include` directive; the parser reads it with [`Lexer::line`] when it
//! builds a node, and the code generator writes it in `DBG_LINE`. The
//! *position* of a token ([`Pos`]) is its real line and column in its own
//! file, used for error messages only.

use crate::error::{Error, Pos, Result};

/// The most deeply nested `#include` the original accepts: the 21st level
/// is "Includes nested too deeply (20)".
const MAX_INCLUDE_DEPTH: usize = 20;

/// Longest string constant, in bytes after escape processing.
const MAX_STRING: usize = 4096;

/// A lexical token.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// An identifier: `[A-Za-z_][A-Za-z0-9_@]*`.
    Ident(String),
    /// An integer constant (decimal digits, no sign).
    Int(i32),
    /// A float constant (`digits.digits`).
    Float(f64),
    /// `true` or `false`.
    Bool(bool),
    /// A string constant, after the three `~` escapes; raw bytes.
    Str(Vec<u8>),
    /// `break;`, one token including the semicolon.
    Break,
    /// `continue;`, one token including the semicolon.
    Continue,
    /// A keyword.
    Keyword(Keyword),
    /// An operator or a punctuation sign.
    Punct(Punct),
    /// The end of the main file.
    Eof,
}

/// The reserved words (other than `true`, `false`, `break;`, `continue;`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Keyword {
    If,
    Else,
    While,
    For,
    Switch,
    Case,
    Default,
    Return,
    Function,
    Method,
    Var,
    Global,
    Const,
    On,
    New,
    Null,
    External,
    // Types of `external` declarations (`short` and `int` are `Long`).
    Bool,
    Long,
    Byte,
    String,
    Float,
    Double,
    MemBuffer,
    // Calling conventions of `external` declarations.
    StdCall,
    CDecl,
    ThisCall,
}

/// Operators and punctuation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Punct {
    AndAnd,
    OrOr,
    EqEq,
    EqEqEq,
    Ne,
    NeEq,
    Ge,
    Le,
    Assign,
    Gt,
    Lt,
    Not,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    LBrace,
    RBrace,
    Semi,
    LParen,
    RParen,
    Comma,
    Dot,
    LBracket,
    RBracket,
    Colon,
}

impl Token {
    /// How the token reads in an error message.
    pub fn describe(&self) -> String {
        match self {
            Token::Ident(name) => format!("identifier `{name}`"),
            Token::Int(v) => format!("number `{v}`"),
            Token::Float(v) => format!("number `{v}`"),
            Token::Bool(v) => format!("`{v}`"),
            Token::Str(_) => "string constant".to_string(),
            Token::Break => "`break;`".to_string(),
            Token::Continue => "`continue;`".to_string(),
            Token::Keyword(k) => format!("`{}`", k.text()),
            Token::Punct(p) => format!("`{}`", p.text()),
            Token::Eof => "end of file".to_string(),
        }
    }
}

impl Keyword {
    fn from_text(text: &str) -> Option<Keyword> {
        Some(match text {
            "if" => Keyword::If,
            "else" => Keyword::Else,
            "while" => Keyword::While,
            "for" => Keyword::For,
            "switch" => Keyword::Switch,
            "case" => Keyword::Case,
            "default" => Keyword::Default,
            "return" => Keyword::Return,
            "function" => Keyword::Function,
            "method" => Keyword::Method,
            "var" => Keyword::Var,
            "global" => Keyword::Global,
            "const" => Keyword::Const,
            "on" => Keyword::On,
            "new" => Keyword::New,
            "null" => Keyword::Null,
            "external" => Keyword::External,
            "bool" => Keyword::Bool,
            "long" | "short" | "int" => Keyword::Long,
            "byte" => Keyword::Byte,
            "string" => Keyword::String,
            "float" => Keyword::Float,
            "double" => Keyword::Double,
            "membuffer" => Keyword::MemBuffer,
            "stdcall" => Keyword::StdCall,
            "cdecl" => Keyword::CDecl,
            "thiscall" => Keyword::ThisCall,
            _ => return None,
        })
    }

    /// The word as written (`short` and `int` read back as `long`).
    pub fn text(self) -> &'static str {
        match self {
            Keyword::If => "if",
            Keyword::Else => "else",
            Keyword::While => "while",
            Keyword::For => "for",
            Keyword::Switch => "switch",
            Keyword::Case => "case",
            Keyword::Default => "default",
            Keyword::Return => "return",
            Keyword::Function => "function",
            Keyword::Method => "method",
            Keyword::Var => "var",
            Keyword::Global => "global",
            Keyword::Const => "const",
            Keyword::On => "on",
            Keyword::New => "new",
            Keyword::Null => "null",
            Keyword::External => "external",
            Keyword::Bool => "bool",
            Keyword::Long => "long",
            Keyword::Byte => "byte",
            Keyword::String => "string",
            Keyword::Float => "float",
            Keyword::Double => "double",
            Keyword::MemBuffer => "membuffer",
            Keyword::StdCall => "stdcall",
            Keyword::CDecl => "cdecl",
            Keyword::ThisCall => "thiscall",
        }
    }
}

impl Punct {
    /// The sign as written.
    pub fn text(self) -> &'static str {
        match self {
            Punct::AndAnd => "&&",
            Punct::OrOr => "||",
            Punct::EqEq => "==",
            Punct::EqEqEq => "===",
            Punct::Ne => "!=",
            Punct::NeEq => "!==",
            Punct::Ge => ">=",
            Punct::Le => "<=",
            Punct::Assign => "=",
            Punct::Gt => ">",
            Punct::Lt => "<",
            Punct::Not => "!",
            Punct::Plus => "+",
            Punct::Minus => "-",
            Punct::Star => "*",
            Punct::Slash => "/",
            Punct::Percent => "%",
            Punct::LBrace => "{",
            Punct::RBrace => "}",
            Punct::Semi => ";",
            Punct::LParen => "(",
            Punct::RParen => ")",
            Punct::Comma => ",",
            Punct::Dot => ".",
            Punct::LBracket => "[",
            Punct::RBracket => "]",
            Punct::Colon => ":",
        }
    }
}

/// Loads the files named by `#include` directives.
///
/// The name is the text between the quotes, verbatim (backslashes and
/// case as written); the original resolves it like any game file, relative
/// to the game root, so implementations should too. `None` means the file
/// cannot be found; the compiler reports "Error opening include file".
pub trait Includes {
    /// The bytes of the file `name`, or `None` when it does not exist.
    fn load(&mut self, name: &[u8]) -> Option<Vec<u8>>;
}

impl<F: FnMut(&[u8]) -> Option<Vec<u8>>> Includes for F {
    fn load(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        self(name)
    }
}

/// One open file: the main file or an include being expanded.
struct Buffer {
    /// Index of the file name in [`Lexer::files`].
    file: usize,
    data: Vec<u8>,
    pos: usize,
    /// Real line of `pos`, from 1.
    line: u32,
    /// Byte offset where that line starts.
    line_start: usize,
}

impl Buffer {
    fn new(file: usize, data: Vec<u8>) -> Buffer {
        // A UTF-8 byte order mark is skipped, but only when the file is
        // longer than the mark itself, as the original does.
        let pos = if data.len() > 3 && data.starts_with(&[0xEF, 0xBB, 0xBF]) {
            3
        } else {
            0
        };
        Buffer {
            file,
            data,
            pos,
            line: 1,
            line_start: 0,
        }
    }

    fn peek(&self, ahead: usize) -> Option<u8> {
        self.data.get(self.pos + ahead).copied()
    }

    fn starts_with(&self, text: &[u8]) -> bool {
        self.data[self.pos..].starts_with(text)
    }

    fn position(&self) -> Pos {
        Pos {
            file: self.file,
            line: self.line,
            column: (self.pos - self.line_start) as u32 + 1,
        }
    }
}

/// The scanner, with its include stack and the recorded line counter.
pub struct Lexer<'a> {
    stack: Vec<Buffer>,
    /// The names of the files opened so far: the main file first, then
    /// each include by the name in its directive.
    files: Vec<String>,
    /// The original's `lineno`: 1 plus the newlines of the main file
    /// consumed so far.
    counter: u32,
    includes: &'a mut dyn Includes,
    peeked: Option<(Token, Pos)>,
    /// Position of the last token returned or peeked.
    last_pos: Pos,
}

impl<'a> Lexer<'a> {
    /// Starts scanning `source`, named `name` in error messages.
    pub fn new(source: Vec<u8>, name: &str, includes: &'a mut dyn Includes) -> Lexer<'a> {
        Lexer {
            stack: vec![Buffer::new(0, source)],
            files: vec![name.to_string()],
            counter: 1,
            includes,
            peeked: None,
            last_pos: Pos {
                file: 0,
                line: 1,
                column: 1,
            },
        }
    }

    /// The recorded line: the line of the last token fetched, consumed or
    /// peeked. This is what the original stores in every node it builds.
    pub fn line(&self) -> u32 {
        self.counter
    }

    /// The names of the files opened so far, indexed by [`Pos::file`].
    pub fn files(&self) -> &[String] {
        &self.files
    }

    /// Position of the last token returned or peeked.
    pub fn last_pos(&self) -> Pos {
        self.last_pos.clone()
    }

    /// Builds an error at `pos`.
    pub fn error(&self, pos: &Pos, message: impl Into<String>) -> Error {
        Error {
            file: self.files[pos.file].clone(),
            line: pos.line,
            column: pos.column,
            message: message.into(),
        }
    }

    /// The next token, without consuming it. Fetching it advances the
    /// recorded line, as the original's lookahead does.
    pub fn peek(&mut self) -> Result<&Token> {
        if self.peeked.is_none() {
            let token = self.lex()?;
            self.peeked = Some(token);
        }
        Ok(&self.peeked.as_ref().expect("just peeked").0)
    }

    /// Consumes and returns the next token.
    pub fn advance(&mut self) -> Result<Token> {
        let (token, pos) = match self.peeked.take() {
            Some(peeked) => peeked,
            None => self.lex()?,
        };
        self.last_pos = pos;
        Ok(token)
    }

    /// Scans one token from the current buffer, popping finished includes.
    fn lex(&mut self) -> Result<(Token, Pos)> {
        loop {
            let depth = self.stack.len() - 1;
            let buffer = self.stack.last_mut().expect("the main file stays open");
            let Some(c) = buffer.peek(0) else {
                if depth > 0 {
                    self.stack.pop();
                    continue;
                }
                let pos = buffer.position();
                self.last_pos = pos.clone();
                return Ok((Token::Eof, pos));
            };
            let pos = buffer.position();
            match c {
                b' ' | b'\t' => {
                    buffer.pos += 1;
                }
                b'\r' if buffer.peek(1) == Some(b'\n') => {
                    buffer.pos += 2;
                    buffer.line += 1;
                    buffer.line_start = buffer.pos;
                    if depth == 0 {
                        self.counter += 1;
                    }
                }
                b'\n' => {
                    buffer.pos += 1;
                    buffer.line += 1;
                    buffer.line_start = buffer.pos;
                    if depth == 0 {
                        self.counter += 1;
                    }
                }
                b'/' if buffer.peek(1) == Some(b'/') => {
                    while let Some(c) = buffer.peek(0) {
                        if c == b'\n' {
                            break;
                        }
                        buffer.pos += 1;
                    }
                }
                b'/' if buffer.peek(1) == Some(b'*') => {
                    buffer.pos += 2;
                    self.skip_comment(&pos)?;
                }
                b'#' => {
                    if buffer.starts_with(b"#include") {
                        buffer.pos += 8;
                        self.include(&pos)?;
                    } else if buffer.starts_with(b"#region") || buffer.starts_with(b"#endregion") {
                        while let Some(c) = buffer.peek(0) {
                            if c == b'\n' {
                                break;
                            }
                            buffer.pos += 1;
                        }
                    } else {
                        return Err(self.error(&pos, "Invalid character"));
                    }
                }
                b'"' => {
                    buffer.pos += 1;
                    let token = self.string(&pos)?;
                    self.last_pos = pos.clone();
                    return Ok((token, pos));
                }
                b'0'..=b'9' => {
                    let token = Self::number(buffer);
                    self.last_pos = pos.clone();
                    return Ok((token, pos));
                }
                b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                    let token = Self::word(buffer);
                    self.last_pos = pos.clone();
                    return Ok((token, pos));
                }
                _ => {
                    let Some(punct) = Self::punct(buffer) else {
                        return Err(self.error(&pos, "Invalid character"));
                    };
                    self.last_pos = pos.clone();
                    return Ok((Token::Punct(punct), pos));
                }
            }
        }
    }

    /// Skips a `/* */` comment whose opening has been consumed. Comments
    /// nest; `//` inside one swallows the rest of its line.
    fn skip_comment(&mut self, start: &Pos) -> Result<()> {
        let depth = self.stack.len() - 1;
        let buffer = self.stack.last_mut().expect("open");
        let mut nesting = 1;
        while nesting > 0 {
            let Some(c) = buffer.peek(0) else {
                return Err(self.error(start, "Unclosed comment."));
            };
            match c {
                b'\r' if buffer.peek(1) == Some(b'\n') => {
                    buffer.pos += 2;
                    buffer.line += 1;
                    buffer.line_start = buffer.pos;
                    if depth == 0 {
                        self.counter += 1;
                    }
                }
                b'\n' => {
                    buffer.pos += 1;
                    buffer.line += 1;
                    buffer.line_start = buffer.pos;
                    if depth == 0 {
                        self.counter += 1;
                    }
                }
                b'*' if buffer.peek(1) == Some(b'/') => {
                    buffer.pos += 2;
                    nesting -= 1;
                }
                b'/' if buffer.peek(1) == Some(b'*') => {
                    buffer.pos += 2;
                    nesting += 1;
                }
                b'/' if buffer.peek(1) == Some(b'/') => {
                    while let Some(c) = buffer.peek(0) {
                        if c == b'\n' {
                            break;
                        }
                        buffer.pos += 1;
                    }
                }
                _ => buffer.pos += 1,
            }
        }
        Ok(())
    }

    /// Handles `#include`, whose word has been consumed: blanks, a quoted
    /// name on the same line, then the file is pushed on the stack.
    fn include(&mut self, directive: &Pos) -> Result<()> {
        let buffer = self.stack.last_mut().expect("open");
        while matches!(buffer.peek(0), Some(b' ' | b'\t')) {
            buffer.pos += 1;
        }
        let name_pos = buffer.position();
        if buffer.peek(0) != Some(b'"') {
            return Err(self.error(&name_pos, "Include filename expected"));
        }
        let start = buffer.pos + 1;
        let mut end = start;
        loop {
            match buffer.data.get(end) {
                Some(b'"') => break,
                Some(b'\n') | None => {
                    return Err(self.error(&name_pos, "Include filename expected"));
                }
                Some(_) => end += 1,
            }
        }
        let name = buffer.data[start..end].to_vec();
        buffer.pos = end + 1;
        if self.stack.len() > MAX_INCLUDE_DEPTH {
            return Err(self.error(
                directive,
                format!("Includes nested too deeply ({MAX_INCLUDE_DEPTH})"),
            ));
        }
        let Some(data) = self.includes.load(&name) else {
            return Err(self.error(
                directive,
                format!(
                    "Error opening include file {}",
                    String::from_utf8_lossy(&name)
                ),
            ));
        };
        self.files.push(String::from_utf8_lossy(&name).into_owned());
        self.stack.push(Buffer::new(self.files.len() - 1, data));
        Ok(())
    }

    /// Reads a string constant whose opening quote has been consumed.
    fn string(&mut self, start: &Pos) -> Result<Token> {
        let buffer = self.stack.last_mut().expect("open");
        let mut out = Vec::new();
        loop {
            let Some(c) = buffer.peek(0) else {
                return Err(self.error(start, "Unterminated string constant"));
            };
            if c == b'"' {
                buffer.pos += 1;
                return Ok(Token::Str(out));
            }
            if c == b'\n' || (c == b'\r' && buffer.peek(1) == Some(b'\n')) {
                return Err(self.error(start, "Unterminated string constant"));
            }
            if out.len() >= MAX_STRING {
                return Err(self.error(start, "String constant too long"));
            }
            match (c, buffer.peek(1)) {
                (b'~', Some(b'n')) => {
                    out.push(b'\n');
                    buffer.pos += 2;
                }
                (b'~', Some(b'~' | b'"')) => {
                    out.push(buffer.peek(1).expect("checked"));
                    buffer.pos += 2;
                }
                _ => {
                    out.push(c);
                    buffer.pos += 1;
                }
            }
        }
    }

    /// Reads an integer or a float constant: `0`, `[1-9][0-9]*` or
    /// `[0-9]+.[0-9]*`, the longest match winning (`007` is `0` alone and
    /// leaves `07` for the next token).
    fn number(buffer: &mut Buffer) -> Token {
        let start = buffer.pos;
        let mut end = start;
        while matches!(buffer.data.get(end), Some(b'0'..=b'9')) {
            end += 1;
        }
        if buffer.data.get(end) == Some(&b'.') {
            end += 1;
            while matches!(buffer.data.get(end), Some(b'0'..=b'9')) {
                end += 1;
            }
            let text = std::str::from_utf8(&buffer.data[start..end]).expect("ASCII digits");
            buffer.pos = end;
            return Token::Float(text.parse().unwrap_or(0.0));
        }
        if buffer.data[start] == b'0' {
            end = start + 1;
        }
        let text = std::str::from_utf8(&buffer.data[start..end]).expect("ASCII digits");
        buffer.pos = end;
        // The original converts with `atoi`; values beyond `i32` saturate.
        let value = text
            .parse::<i64>()
            .map(|v| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
            .unwrap_or(i32::MAX);
        Token::Int(value)
    }

    /// Reads an identifier, a keyword, `true`/`false`, or `break;` and
    /// `continue;` (which include the semicolon).
    fn word(buffer: &mut Buffer) -> Token {
        let start = buffer.pos;
        let mut end = start;
        while matches!(
            buffer.data.get(end),
            Some(b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'@')
        ) {
            end += 1;
        }
        let text = std::str::from_utf8(&buffer.data[start..end]).expect("ASCII");
        let followed_by_semi = buffer.data.get(end) == Some(&b';');
        let token = match text {
            "break" if followed_by_semi => {
                end += 1;
                Token::Break
            }
            "continue" if followed_by_semi => {
                end += 1;
                Token::Continue
            }
            "true" => Token::Bool(true),
            "false" => Token::Bool(false),
            _ => match Keyword::from_text(text) {
                Some(keyword) => Token::Keyword(keyword),
                None => Token::Ident(text.to_string()),
            },
        };
        buffer.pos = end;
        token
    }

    /// Reads an operator or a punctuation sign, longest match first.
    fn punct(buffer: &mut Buffer) -> Option<Punct> {
        const TABLE: [(&[u8], Punct); 27] = [
            (b"===", Punct::EqEqEq),
            (b"!==", Punct::NeEq),
            (b"&&", Punct::AndAnd),
            (b"||", Punct::OrOr),
            (b"==", Punct::EqEq),
            (b"!=", Punct::Ne),
            (b">=", Punct::Ge),
            (b"<=", Punct::Le),
            (b"=", Punct::Assign),
            (b">", Punct::Gt),
            (b"<", Punct::Lt),
            (b"!", Punct::Not),
            (b"+", Punct::Plus),
            (b"-", Punct::Minus),
            (b"*", Punct::Star),
            (b"/", Punct::Slash),
            (b"%", Punct::Percent),
            (b"{", Punct::LBrace),
            (b"}", Punct::RBrace),
            (b";", Punct::Semi),
            (b"(", Punct::LParen),
            (b")", Punct::RParen),
            (b",", Punct::Comma),
            (b".", Punct::Dot),
            (b"[", Punct::LBracket),
            (b"]", Punct::RBracket),
            (b":", Punct::Colon),
        ];
        for (text, punct) in TABLE {
            if buffer.starts_with(text) {
                buffer.pos += text.len();
                return Some(punct);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_includes(_: &[u8]) -> Option<Vec<u8>> {
        None
    }

    fn tokens(source: &str) -> Vec<(Token, u32)> {
        let mut includes = no_includes;
        let mut lexer = Lexer::new(source.as_bytes().to_vec(), "t.script", &mut includes);
        let mut out = Vec::new();
        loop {
            let token = lexer.advance().unwrap();
            let line = lexer.line();
            if token == Token::Eof {
                out.push((token, line));
                return out;
            }
            out.push((token, line));
        }
    }

    #[test]
    fn keywords_identifiers_and_the_break_token() {
        let got = tokens("for format break; break ; continue;x true trueish int");
        let kinds: Vec<Token> = got.into_iter().map(|(t, _)| t).collect();
        assert_eq!(
            kinds,
            vec![
                Token::Keyword(Keyword::For),
                Token::Ident("format".into()),
                Token::Break,
                Token::Ident("break".into()),
                Token::Punct(Punct::Semi),
                Token::Continue,
                Token::Ident("x".into()),
                Token::Bool(true),
                Token::Ident("trueish".into()),
                Token::Keyword(Keyword::Long),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn numbers() {
        let got: Vec<Token> = tokens("0 42 007 5. 0.5 00.5 .5 1e5")
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        assert_eq!(
            got,
            vec![
                Token::Int(0),
                Token::Int(42),
                Token::Int(0),
                Token::Int(0),
                Token::Int(7),
                Token::Float(5.0),
                Token::Float(0.5),
                Token::Float(0.5),
                Token::Punct(Punct::Dot),
                Token::Int(5),
                Token::Int(1),
                Token::Ident("e5".into()),
                Token::Eof,
            ]
        );
        assert_eq!(tokens("99999999999")[0].0, Token::Int(i32::MAX));
    }

    #[test]
    fn strings_and_escapes() {
        let got = tokens(r#""a~nb~~c~"d~x\e""#);
        assert_eq!(got[0].0, Token::Str(b"a\nb~c\"d~x\\e".to_vec()));
        let mut includes = no_includes;
        let mut lexer = Lexer::new(b"\"abc\ndef\"".to_vec(), "t.script", &mut includes);
        let err = lexer.advance().unwrap_err();
        assert_eq!(err.message, "Unterminated string constant");
        assert_eq!((err.line, err.column), (1, 1));
        let long = format!("\"{}\"", "x".repeat(4097));
        let mut lexer = Lexer::new(long.into_bytes(), "t.script", &mut includes);
        assert_eq!(
            lexer.advance().unwrap_err().message,
            "String constant too long"
        );
        let ok = format!("\"{}\"", "x".repeat(4096));
        let mut lexer = Lexer::new(ok.into_bytes(), "t.script", &mut includes);
        assert!(matches!(lexer.advance().unwrap(), Token::Str(s) if s.len() == 4096));
    }

    #[test]
    fn operators_longest_match() {
        let got: Vec<Token> = tokens("=== == = !== != ! <= < >= > && ||")
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        assert_eq!(
            got,
            vec![
                Token::Punct(Punct::EqEqEq),
                Token::Punct(Punct::EqEq),
                Token::Punct(Punct::Assign),
                Token::Punct(Punct::NeEq),
                Token::Punct(Punct::Ne),
                Token::Punct(Punct::Not),
                Token::Punct(Punct::Le),
                Token::Punct(Punct::Lt),
                Token::Punct(Punct::Ge),
                Token::Punct(Punct::Gt),
                Token::Punct(Punct::AndAnd),
                Token::Punct(Punct::OrOr),
                Token::Eof,
            ]
        );
        let mut includes = no_includes;
        let mut lexer = Lexer::new(b"a & b".to_vec(), "t.script", &mut includes);
        lexer.advance().unwrap();
        let err = lexer.advance().unwrap_err();
        assert_eq!(err.message, "Invalid character");
        assert_eq!((err.line, err.column), (1, 3));
    }

    #[test]
    fn comments_nest_and_count_lines() {
        let got = tokens("a /* one\n /* two */ // */ still\n */ b\n// c\nd #region x y\ne");
        assert_eq!(got[0], (Token::Ident("a".into()), 1));
        assert_eq!(got[1], (Token::Ident("b".into()), 3));
        assert_eq!(got[2], (Token::Ident("d".into()), 5));
        assert_eq!(got[3], (Token::Ident("e".into()), 6));
        assert_eq!(got[4], (Token::Eof, 6));
        let mut includes = no_includes;
        let mut lexer = Lexer::new(b"/* open".to_vec(), "t.script", &mut includes);
        assert_eq!(lexer.advance().unwrap_err().message, "Unclosed comment.");
    }

    #[test]
    fn crlf_and_lone_cr() {
        let got = tokens("a\r\nb\n\nc\n");
        assert_eq!(got[0].1, 1);
        assert_eq!(got[1].1, 2);
        assert_eq!(got[2].1, 4);
        assert_eq!(got[3], (Token::Eof, 5));
        let mut includes = no_includes;
        let mut lexer = Lexer::new(b"a\rb".to_vec(), "t.script", &mut includes);
        lexer.advance().unwrap();
        assert_eq!(lexer.advance().unwrap_err().message, "Invalid character");
    }

    #[test]
    fn includes_freeze_the_counter() {
        let mut includes = |name: &[u8]| -> Option<Vec<u8>> {
            match name {
                b"a.inc" => Some(b"\xEF\xBB\xBFx\n\n#include \"b.inc\"\ny\n".to_vec()),
                b"b.inc" => Some(b"z".to_vec()),
                _ => None,
            }
        };
        let mut lexer = Lexer::new(
            b"p\n#include \"a.inc\"\nq\n".to_vec(),
            "main.script",
            &mut includes,
        );
        let mut got = Vec::new();
        loop {
            let token = lexer.advance().unwrap();
            got.push((token.clone(), lexer.line()));
            if token == Token::Eof {
                break;
            }
        }
        assert_eq!(
            got,
            vec![
                (Token::Ident("p".into()), 1),
                (Token::Ident("x".into()), 2),
                (Token::Ident("z".into()), 2),
                (Token::Ident("y".into()), 2),
                (Token::Ident("q".into()), 3),
                (Token::Eof, 4),
            ]
        );
        assert_eq!(lexer.files(), &["main.script", "a.inc", "b.inc"]);

        let mut lexer = Lexer::new(
            b"\n#include \"missing.inc\"".to_vec(),
            "main.script",
            &mut includes,
        );
        let err = lexer.advance().unwrap_err();
        assert_eq!(err.message, "Error opening include file missing.inc");
        assert_eq!((err.line, err.column), (2, 1));
        let mut lexer = Lexer::new(b"#include x".to_vec(), "main.script", &mut includes);
        assert_eq!(
            lexer.advance().unwrap_err().message,
            "Include filename expected"
        );
        let mut lexer = Lexer::new(b"#define X".to_vec(), "main.script", &mut includes);
        assert_eq!(lexer.advance().unwrap_err().message, "Invalid character");
    }

    #[test]
    fn errors_in_includes_name_the_include() {
        let mut includes =
            |_: &[u8]| -> Option<Vec<u8>> { Some(b"ok\n  \"unterminated\n".to_vec()) };
        let mut lexer = Lexer::new(
            b"\n\n#include \"inc.inc\"".to_vec(),
            "main.script",
            &mut includes,
        );
        lexer.advance().unwrap();
        let err = lexer.advance().unwrap_err();
        assert_eq!(err.file, "inc.inc");
        assert_eq!((err.line, err.column), (2, 3));
    }

    #[test]
    fn nesting_limit() {
        let mut includes = |_: &[u8]| -> Option<Vec<u8>> { Some(b"#include \"x\"".to_vec()) };
        let mut lexer = Lexer::new(b"#include \"x\"".to_vec(), "main.script", &mut includes);
        let err = lexer.advance().unwrap_err();
        assert_eq!(err.message, "Includes nested too deeply (20)");
    }

    #[test]
    fn peek_advances_the_recorded_line() {
        let mut includes = no_includes;
        let mut lexer = Lexer::new(b"a\n\nb".to_vec(), "t.script", &mut includes);
        lexer.advance().unwrap();
        assert_eq!(lexer.line(), 1);
        assert_eq!(lexer.peek().unwrap(), &Token::Ident("b".into()));
        assert_eq!(lexer.line(), 3);
        assert_eq!(lexer.advance().unwrap(), Token::Ident("b".into()));
        assert_eq!(lexer.line(), 3);
    }
}
