//! The parser: one `.lumen` file to its header declarations and knots.
//!
//! The file is read line by line. Indentation (spaces) only matters under
//! a choice, an option or an `if`; anywhere else a line indented under
//! the previous one continues it. Errors are collected as diagnostics and
//! the parser goes on with the next line, so one pass reports them all.

use crate::ast::{
    Assign, AssignOp, Character, Choice, ChoiceOption, Command, Divert, If, Knot, Line, Note, Span,
    Speaker, Stitch, Stmt, Target, Timeout, VarDecl,
};
use crate::diag::Diagnostic;
use crate::expr::{self, Expr};
use crate::text::parse_text;

/// What one file declares.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FileAst {
    /// `include "path"` lines, with their spans.
    pub includes: Vec<(String, Span)>,
    /// `character` declarations.
    pub characters: Vec<Character>,
    /// `var` and `persistent var` declarations.
    pub vars: Vec<VarDecl>,
    /// The knots, in order.
    pub knots: Vec<Knot>,
    /// `//!` notes.
    pub notes: Vec<Note>,
}

/// A source line after comments are stripped.
#[derive(Debug, Clone)]
struct SrcLine {
    number: u32,
    indent: u32,
    text: String,
}

/// Parses a file. `file` is its index in the story, used in the spans.
pub fn parse_file(file: usize, source: &str, diags: &mut Vec<Diagnostic>) -> FileAst {
    let mut ast = FileAst::default();
    let lines = split_lines(file, source, &mut ast.notes, diags);
    let mut parser = Parser {
        file,
        lines,
        pos: 0,
        diags,
    };
    parser.parse_top(&mut ast);
    ast
}

/// Strips comments, collects notes, measures indentation.
fn split_lines(
    file: usize,
    source: &str,
    notes: &mut Vec<Note>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<SrcLine> {
    // Block comments, replaced by spaces so that columns stay right.
    let mut text = String::with_capacity(source.len());
    let mut in_block = false;
    let mut in_string = false;
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_block {
            if c == '*' && chars.get(i + 1) == Some(&'/') {
                in_block = false;
                text.push_str("  ");
                i += 2;
                continue;
            }
            text.push(if c == '\n' { '\n' } else { ' ' });
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = !in_string;
        }
        if c == '\n' {
            in_string = false;
        }
        if !in_string && c == '/' && chars.get(i + 1) == Some(&'*') {
            in_block = true;
            text.push_str("  ");
            i += 2;
            continue;
        }
        text.push(c);
        i += 1;
    }
    let mut out = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let number = index as u32 + 1;
        let mut indent = 0u32;
        let mut tabs = false;
        let mut body_start = 0;
        for (b, c) in raw.char_indices() {
            match c {
                ' ' => indent += 1,
                '\t' => {
                    indent += 2;
                    tabs = true;
                }
                _ => {
                    body_start = b;
                    break;
                }
            }
            body_start = b + c.len_utf8();
        }
        let body = &raw[body_start..];
        if tabs && !body.is_empty() {
            diags.push(Diagnostic::error(
                Span {
                    file,
                    line: number,
                    col: 1,
                    len: 1,
                },
                "tabs are not allowed for indentation; use two spaces per level",
            ));
        }
        // Line comments outside strings; `//!` is a writer note.
        let mut cut = body.len();
        let mut quoted = false;
        let bytes: Vec<(usize, char)> = body.char_indices().collect();
        for (k, &(b, c)) in bytes.iter().enumerate() {
            if c == '"' {
                quoted = !quoted;
            }
            if !quoted && c == '/' && bytes.get(k + 1).map(|p| p.1) == Some('/') {
                if bytes.get(k + 2).map(|p| p.1) == Some('!') {
                    let note = body[b + 3..].trim();
                    notes.push(Note {
                        text: note.to_string(),
                        span: Span {
                            file,
                            line: number,
                            col: indent + 1 + k as u32,
                            len: note.chars().count().max(1) as u32,
                        },
                    });
                }
                cut = b;
                break;
            }
        }
        let body = body[..cut].trim_end();
        if body.is_empty() {
            continue;
        }
        out.push(SrcLine {
            number,
            indent,
            text: body.to_string(),
        });
    }
    out
}

struct Parser<'d> {
    file: usize,
    lines: Vec<SrcLine>,
    pos: usize,
    diags: &'d mut Vec<Diagnostic>,
}

/// Markers a continuation line may not start with: they are statements.
const STATEMENT_STARTS: &[&str] = &[
    "- ", "* ", "?", "@", "&@", "~", "->", "[", "if ", "elif ", "else", "call ", "return", "==",
    "= ", "temp ", "timeout",
];

fn is_knot_header(text: &str) -> bool {
    text.starts_with("==")
}

fn is_stitch_header(text: &str) -> bool {
    text.starts_with('=') && !text.starts_with("==")
}

fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c == '_')
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

impl Parser<'_> {
    fn peek(&self) -> Option<&SrcLine> {
        self.lines.get(self.pos)
    }

    fn span(&self, line: &SrcLine, offset: usize, len: usize) -> Span {
        Span {
            file: self.file,
            line: line.number,
            col: line.indent + 1 + offset as u32,
            len: len.max(1) as u32,
        }
    }

    fn line_span(&self, line: &SrcLine) -> Span {
        self.span(line, 0, line.text.chars().count())
    }

    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.diags.push(Diagnostic::error(span, message));
    }

    fn parse_top(&mut self, ast: &mut FileAst) {
        while let Some(line) = self.peek().cloned() {
            if line.indent != 0 {
                let span = self.line_span(&line);
                self.error(span, "unexpected indentation outside a knot");
                self.pos += 1;
                continue;
            }
            if is_knot_header(&line.text) {
                let knot = self.parse_knot(&line);
                ast.knots.push(knot);
                continue;
            }
            if is_stitch_header(&line.text) {
                let span = self.line_span(&line);
                self.error(
                    span,
                    "a stitch outside a knot; stitches follow a `== knot` header",
                );
                self.pos += 1;
                continue;
            }
            self.pos += 1;
            self.parse_header_line(&line, ast);
        }
    }

    fn parse_header_line(&mut self, line: &SrcLine, ast: &mut FileAst) {
        let text = line.text.as_str();
        if let Some(rest) = text.strip_prefix("include ") {
            let rest = rest.trim();
            match unquote(rest) {
                Some(path) => ast
                    .includes
                    .push((path, self.span(line, 8, rest.chars().count()))),
                None => {
                    let span = self.span(line, 8, rest.chars().count());
                    self.error(span, "`include` takes a quoted path");
                }
            }
        } else if let Some(rest) = text.strip_prefix("character ") {
            if let Some(c) = self.parse_character(line, rest) {
                ast.characters.push(c);
            }
        } else if let Some(rest) = text.strip_prefix("persistent var ") {
            if let Some(v) = self.parse_var(line, rest, 15, true) {
                ast.vars.push(v);
            }
        } else if let Some(rest) = text.strip_prefix("var ") {
            if let Some(v) = self.parse_var(line, rest, 4, false) {
                ast.vars.push(v);
            }
        } else {
            let span = self.line_span(line);
            self.error(
                span,
                "only `include`, `var`, `persistent var` and `character` lines may come before the first knot",
            );
        }
    }

    fn parse_character(&mut self, line: &SrcLine, rest: &str) -> Option<Character> {
        let tokens = tokenize(rest);
        let mut it = tokens.into_iter();
        let Some((id, _)) = it.next() else {
            let span = self.line_span(line);
            self.error(span, "`character` needs an id and a quoted name");
            return None;
        };
        if !is_name(&id) {
            let span = self.span(line, 10, id.chars().count());
            self.error(
                span,
                format!("`{id}` is not a valid character id (snake_case, ASCII)"),
            );
        }
        let Some((name, quoted)) = it.next() else {
            let span = self.line_span(line);
            self.error(span, "`character` needs a quoted name after the id");
            return None;
        };
        if !quoted {
            let span = self.line_span(line);
            self.error(span, "the character's name must be quoted");
        }
        let mut options = Vec::new();
        for (tok, quoted) in it {
            match (quoted, tok.split_once('=')) {
                (false, Some((k, v))) => {
                    options.push((k.to_string(), unquote(v).unwrap_or_else(|| v.to_string())))
                }
                _ => {
                    let span = self.line_span(line);
                    self.error(
                        span,
                        format!(
                            "unexpected `{tok}` in a character declaration; options are `key=value`"
                        ),
                    );
                }
            }
        }
        Some(Character {
            id,
            name,
            options,
            span: self.line_span(line),
        })
    }

    /// `name = expr` after `var `, `persistent var ` or `temp `.
    fn parse_var(
        &mut self,
        line: &SrcLine,
        rest: &str,
        offset: usize,
        persistent: bool,
    ) -> Option<VarDecl> {
        let Some((name, value)) = rest.split_once('=') else {
            let span = self.line_span(line);
            self.error(span, "a variable declaration is `name = value`");
            return None;
        };
        let name = name.trim();
        if !is_name(name) {
            let span = self.span(line, offset, name.chars().count());
            self.error(
                span,
                format!("`{name}` is not a valid variable name (snake_case, ASCII)"),
            );
            return None;
        }
        let value_offset = offset + rest.find('=').unwrap_or(0) + 1;
        let value = self.parse_expr(line, value.trim(), value_offset + leading_spaces(value))?;
        Some(VarDecl {
            name: name.to_string(),
            value,
            persistent,
            span: self.span(line, offset, name.chars().count()),
        })
    }

    fn parse_expr(&mut self, line: &SrcLine, src: &str, offset: usize) -> Option<Expr> {
        if src.is_empty() {
            let span = self.span(line, offset, 1);
            self.error(span, "expected an expression");
            return None;
        }
        match expr::parse(src, self.span(line, offset, src.chars().count())) {
            Ok(e) => Some(e),
            Err(err) => {
                let span = self.span(line, offset + err.offset as usize, 1);
                self.error(span, err.message);
                None
            }
        }
    }

    fn parse_knot(&mut self, header: &SrcLine) -> Knot {
        self.pos += 1;
        let name = header.text.trim_start_matches('=').trim().to_string();
        let span = self.span(header, 0, header.text.chars().count());
        if !is_name(&name) {
            let name_span = self.span(
                header,
                header.text.len() - header.text.trim_start_matches('=').trim_start().len(),
                name.chars().count(),
            );
            self.error(
                name_span,
                format!("`{name}` is not a valid knot name (snake_case, ASCII)"),
            );
        }
        let mut aliases = Vec::new();
        while let Some(line) = self.peek().cloned() {
            if line.indent == 0
                && let Some(alias) = line.text.strip_prefix("alias ")
            {
                aliases.push(alias.trim().to_string());
                self.pos += 1;
                continue;
            }
            break;
        }
        let body = self.parse_block(0);
        let mut stitches = Vec::new();
        while let Some(line) = self.peek().cloned() {
            if line.indent == 0 && is_stitch_header(&line.text) {
                self.pos += 1;
                let name = line.text[1..].trim().to_string();
                let span = self.line_span(&line);
                if !is_name(&name) {
                    self.error(
                        span,
                        format!("`{name}` is not a valid stitch name (snake_case, ASCII)"),
                    );
                }
                let body = self.parse_block(0);
                stitches.push(Stitch { name, body, span });
            } else {
                break;
            }
        }
        Knot {
            name,
            aliases,
            body,
            stitches,
            span,
        }
    }

    /// The statements at `indent`, until a line less indented or, at the
    /// top level, a knot or stitch header.
    fn parse_block(&mut self, indent: u32) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        while let Some(line) = self.peek().cloned() {
            if line.indent < indent {
                break;
            }
            if line.indent == 0 && (is_knot_header(&line.text) || is_stitch_header(&line.text)) {
                break;
            }
            if line.indent > indent {
                let span = self.line_span(&line);
                self.error(
                    span,
                    "unexpected indentation: nothing above this line takes a block",
                );
                self.pos += 1;
                continue;
            }
            let text = line.text.as_str();
            if is_knot_header(text) || is_stitch_header(text) {
                let span = self.line_span(&line);
                self.error(
                    span,
                    "a knot or stitch header must start at the first column",
                );
                self.pos += 1;
                continue;
            }
            if text.starts_with("- ") || text.starts_with("* ") || text == "-" || text == "*" {
                let span = self.line_span(&line);
                self.error(
                    span,
                    "an option outside a choice: options are indented under a `?` line",
                );
                self.pos += 1;
                continue;
            }
            if text.starts_with("elif ") || text == "else:" || text.starts_with("else ") {
                let span = self.line_span(&line);
                self.error(span, "`elif` or `else` without an `if` above it");
                self.pos += 1;
                continue;
            }
            if text.starts_with("timeout") {
                let span = self.line_span(&line);
                self.error(
                    span,
                    "`timeout` belongs under a timed choice, `? (10s) prompt`",
                );
                self.pos += 1;
                continue;
            }
            self.pos += 1;
            if let Some(stmt) = self.parse_stmt(&line, indent) {
                stmts.push(stmt);
            }
        }
        stmts
    }

    /// The block under a line at `parent_indent`, which must be indented
    /// deeper; empty, with an error, when nothing follows.
    fn parse_child_block(&mut self, parent: &SrcLine, parent_indent: u32, what: &str) -> Vec<Stmt> {
        match self.peek() {
            Some(next) if next.indent > parent_indent => {
                let indent = next.indent;
                self.parse_block(indent)
            }
            _ => {
                let span = self.line_span(parent);
                self.error(span, format!("expected an indented block after {what}"));
                Vec::new()
            }
        }
    }

    /// Joins the lines indented under a line of text or a command.
    fn continuation(&mut self, indent: u32, text: &mut String) {
        while let Some(next) = self.peek() {
            if next.indent <= indent {
                break;
            }
            if STATEMENT_STARTS.iter().any(|s| next.text.starts_with(s)) {
                let next = next.clone();
                let span = self.line_span(&next);
                self.error(
                    span,
                    "unexpected indentation: a statement cannot continue the line above",
                );
                self.pos += 1;
                continue;
            }
            text.push(' ');
            text.push_str(&next.text);
            self.pos += 1;
        }
    }

    fn parse_stmt(&mut self, line: &SrcLine, indent: u32) -> Option<Stmt> {
        let text = line.text.as_str();
        if let Some(rest) = text.strip_prefix("->") {
            self.no_block(indent, "a divert");
            return self.parse_target(line, rest, 2).map(Stmt::Divert);
        }
        if let Some(rest) = text.strip_prefix("call ") {
            self.no_block(indent, "a call");
            return self.parse_target(line, rest, 5).map(Stmt::Call);
        }
        if text == "return" {
            self.no_block(indent, "`return`");
            return Some(Stmt::Return(self.line_span(line)));
        }
        if let Some(rest) = text.strip_prefix('~') {
            self.no_block(indent, "a `~` line");
            return self.parse_assign(line, rest, 1, false);
        }
        if let Some(rest) = text.strip_prefix("temp ") {
            self.no_block(indent, "a `temp` declaration");
            return self.parse_assign(line, rest, 5, true);
        }
        if let Some(cond) = text.strip_prefix("if ") {
            return self.parse_if(line, indent, cond);
        }
        if let Some(rest) = text.strip_prefix('?') {
            return self.parse_choice(line, indent, rest);
        }
        if text.starts_with('@') || text.starts_with("&@") {
            let mut full = text.to_string();
            self.continuation(indent, &mut full);
            return self.parse_command(line, &full).map(Stmt::Command);
        }
        let mut full = text.to_string();
        self.continuation(indent, &mut full);
        Some(Stmt::Line(self.parse_line(line, &full)))
    }

    /// Reports lines indented under a statement that takes no block.
    fn no_block(&mut self, indent: u32, what: &str) {
        while let Some(next) = self.peek() {
            if next.indent <= indent {
                break;
            }
            let next = next.clone();
            let span = self.line_span(&next);
            self.error(span, format!("unexpected indentation under {what}"));
            self.pos += 1;
        }
    }

    fn parse_target(&mut self, line: &SrcLine, rest: &str, offset: usize) -> Option<Divert> {
        let name = rest.trim();
        let offset = offset + leading_spaces(rest);
        let span = self.span(line, offset, name.chars().count());
        if name.is_empty() {
            self.error(span, "expected a knot or stitch name after `->`");
            return None;
        }
        let target = match name {
            "END" => Target::End,
            "STOP" => Target::Stop,
            "back" => Target::Back,
            _ => {
                match name.split_once('.') {
                    Some((knot, stitch)) => {
                        if !is_name(knot) || !is_name(stitch) {
                            self.error(
                                span,
                                format!("`{name}` is not a valid `knot.stitch` target"),
                            );
                            return None;
                        }
                        Target::Path(knot.to_string(), stitch.to_string())
                    }
                    None => {
                        if !is_name(name) {
                            self.error(span, format!("`{name}` is not a valid knot or stitch name (snake_case, ASCII)"));
                            return None;
                        }
                        Target::Name(name.to_string())
                    }
                }
            }
        };
        Some(Divert { target, span })
    }

    fn parse_assign(
        &mut self,
        line: &SrcLine,
        rest: &str,
        offset: usize,
        temp: bool,
    ) -> Option<Stmt> {
        let ops: [(&str, AssignOp); 3] = [
            ("+=", AssignOp::Add),
            ("-=", AssignOp::Sub),
            ("=", AssignOp::Set),
        ];
        let (at, op_text, op) = ops
            .iter()
            .filter_map(|(o, op)| rest.find(o).map(|at| (at, *o, *op)))
            .min_by_key(|(at, o, _)| (*at, std::cmp::Reverse(o.len())))?;
        let name = rest[..at].trim();
        let name_offset = offset + leading_spaces(rest);
        if name.is_empty() || !is_name(name) {
            let span = self.span(line, name_offset, name.chars().count());
            self.error(
                span,
                if temp {
                    "a `temp` declaration is `temp name = value`".to_string()
                } else {
                    format!("`{name}` is not a valid variable name")
                },
            );
            return None;
        }
        if temp && op != AssignOp::Set {
            let span = self.span(line, offset + at, op_text.len());
            self.error(span, "a `temp` declaration uses `=`");
            return None;
        }
        let value_src = &rest[at + op_text.len()..];
        let value = self.parse_expr(
            line,
            value_src.trim(),
            offset + at + op_text.len() + leading_spaces(value_src),
        )?;
        Some(Stmt::Assign(Assign {
            name: name.to_string(),
            op,
            value,
            temp,
            span: self.span(line, name_offset, name.chars().count()),
        }))
    }

    fn parse_if(&mut self, line: &SrcLine, indent: u32, cond: &str) -> Option<Stmt> {
        let span = self.line_span(line);
        let Some(cond) = cond.strip_suffix(':') else {
            self.error(span, "an `if` line ends with `:`");
            self.no_block(indent, "a malformed `if`");
            return None;
        };
        let mut branches = Vec::new();
        let mut otherwise = None;
        let cond_expr = self.parse_expr(line, cond.trim(), 3 + leading_spaces(cond));
        let block = self.parse_child_block(line, indent, "`if`");
        branches.push((cond_expr.unwrap_or(Expr::Bool(false)), block));
        while let Some(next) = self.peek().cloned() {
            if next.indent != indent {
                break;
            }
            if let Some(cond) = next.text.strip_prefix("elif ") {
                self.pos += 1;
                let Some(cond) = cond.strip_suffix(':') else {
                    let span = self.line_span(&next);
                    self.error(span, "an `elif` line ends with `:`");
                    continue;
                };
                let cond_expr = self.parse_expr(&next, cond.trim(), 5 + leading_spaces(cond));
                let block = self.parse_child_block(&next, indent, "`elif`");
                branches.push((cond_expr.unwrap_or(Expr::Bool(false)), block));
            } else if next.text == "else:" {
                self.pos += 1;
                otherwise = Some(self.parse_child_block(&next, indent, "`else`"));
                break;
            } else if next.text.starts_with("else") {
                self.pos += 1;
                let span = self.line_span(&next);
                self.error(span, "an `else` line is exactly `else:`");
                otherwise = Some(self.parse_child_block(&next, indent, "`else`"));
                break;
            } else {
                break;
            }
        }
        Some(Stmt::If(If {
            branches,
            otherwise,
            span,
        }))
    }

    fn parse_choice(&mut self, line: &SrcLine, indent: u32, rest: &str) -> Option<Stmt> {
        let span = self.line_span(line);
        let mut rest = rest.trim();
        let mut seconds = None;
        if let Some(after) = rest.strip_prefix('(') {
            match after.split_once(')') {
                Some((dur, tail)) => {
                    match parse_duration(dur.trim()) {
                        Some(s) => seconds = Some(s),
                        None => {
                            let span =
                                self.span(line, 1 + leading_spaces(rest), dur.chars().count() + 2);
                            self.error(span, format!("`{dur}` is not a duration (`10s`, `500ms`)"));
                        }
                    }
                    rest = tail.trim();
                }
                None => {
                    self.error(span, "unterminated `(` in the choice line");
                    rest = "";
                }
            }
        }
        let prompt = if rest.is_empty() {
            None
        } else {
            let offset = line.text.len() - rest.len();
            Some(parse_text(
                rest,
                self.span(
                    line,
                    line.text[..offset].chars().count(),
                    rest.chars().count(),
                ),
                self.diags,
            ))
        };
        let mut options = Vec::new();
        let mut timeout = None;
        let Some(first) = self.peek().cloned() else {
            self.error(
                span,
                "a choice needs at least one option, indented under it",
            );
            return None;
        };
        if first.indent <= indent {
            self.error(
                span,
                "a choice needs at least one option, indented under it",
            );
            return None;
        }
        let option_indent = first.indent;
        while let Some(next) = self.peek().cloned() {
            if next.indent < option_indent {
                break;
            }
            if next.indent > option_indent {
                let span = self.line_span(&next);
                self.error(
                    span,
                    "unexpected indentation: an option's lines follow its `-` or `*` line",
                );
                self.pos += 1;
                continue;
            }
            let text = next.text.as_str();
            if text.starts_with("- ") || text.starts_with("* ") {
                self.pos += 1;
                if let Some(option) = self.parse_option(&next, option_indent) {
                    options.push(option);
                }
            } else if let Some(rest) = text.strip_prefix("timeout") {
                self.pos += 1;
                let rest = rest.trim();
                let Some(arrow) = rest.strip_prefix("->") else {
                    let span = self.line_span(&next);
                    self.error(span, "a timeout line is `timeout -> target`");
                    continue;
                };
                if seconds.is_none() {
                    let span = self.line_span(&next);
                    self.error(span, "`timeout` needs a timed choice: `? (10s) prompt`");
                }
                let offset = next.text.len() - arrow.len();
                if let Some(target) =
                    self.parse_target(&next, arrow, next.text[..offset].chars().count())
                {
                    timeout = Some(Timeout {
                        seconds: seconds.unwrap_or(0.0),
                        target,
                    });
                }
            } else {
                break;
            }
        }
        if options.is_empty() {
            self.error(
                span,
                "a choice needs at least one option (`- text` or `* text`)",
            );
            return None;
        }
        if seconds.is_some() && timeout.is_none() {
            self.error(
                span,
                "a timed choice needs a `timeout -> target` line among its options",
            );
        }
        Some(Stmt::Choice(Choice {
            prompt,
            timeout,
            options,
            span,
        }))
    }

    fn parse_option(&mut self, line: &SrcLine, option_indent: u32) -> Option<ChoiceOption> {
        let sticky = line.text.starts_with('*');
        let span = self.line_span(line);
        let rest = &line.text[1..];
        // The trailing divert.
        let (mut body_text, divert) = match find_arrow(rest) {
            Some(at) => {
                let target = self.parse_target(line, &rest[at + 2..], 1 + at + 2);
                (rest[..at].to_string(), target)
            }
            None => (rest.to_string(), None),
        };
        // `[if expr]` and `[locked if expr "reason"]`.
        let mut condition = None;
        let mut locked = None;
        while let Some(open) = find_bracket(&body_text) {
            let Some(close) = body_text[open..].find(']') else {
                let span = self.span(line, 1 + body_text[..open].chars().count(), 1);
                self.error(span, "unterminated `[` in the option");
                break;
            };
            let close = open + close;
            let inner = body_text[open + 1..close].trim().to_string();
            let inner_offset = 1
                + body_text[..open + 1].chars().count()
                + leading_spaces(&body_text[open + 1..close]);
            if let Some(cond) = inner.strip_prefix("locked if ") {
                let cond = cond.trim();
                match cond
                    .rfind('"')
                    .and_then(|end| cond[..end].rfind('"').map(|start| (start, end)))
                {
                    Some((start, end)) => {
                        let reason = cond[start + 1..end].to_string();
                        let expr_src = cond[..start].trim();
                        if let Some(e) = self.parse_expr(
                            line,
                            expr_src,
                            inner_offset + 10 + leading_spaces(&cond[..start]),
                        ) {
                            locked = Some((e, reason));
                        }
                    }
                    None => {
                        let span = self.span(line, inner_offset, inner.chars().count());
                        self.error(span, "`[locked if expr \"reason\"]` needs a quoted reason");
                    }
                }
            } else if let Some(cond) = inner.strip_prefix("if ") {
                if let Some(e) =
                    self.parse_expr(line, cond.trim(), inner_offset + 3 + leading_spaces(cond))
                {
                    condition = Some(e);
                }
            } else {
                let span = self.span(line, inner_offset, inner.chars().count());
                self.error(span, format!("unknown option modifier `[{inner}]`; use `[if expr]` or `[locked if expr \"reason\"]`"));
            }
            body_text.replace_range(open..=close, "");
        }
        let (text_src, id, tags) = split_tags(&body_text);
        let text_offset = 1 + leading_spaces(&body_text);
        let text = parse_text(
            text_src,
            self.span(line, text_offset, text_src.chars().count()),
            self.diags,
        );
        if text_src.is_empty() {
            self.error(span, "an option needs a text");
        }
        let mut body = match self.peek() {
            Some(next) if next.indent > option_indent => {
                let indent = next.indent;
                self.parse_block(indent)
            }
            _ => Vec::new(),
        };
        if let Some(d) = divert {
            body.push(Stmt::Divert(d));
        }
        Some(ChoiceOption {
            text,
            sticky,
            condition,
            locked,
            tags,
            id,
            body,
            span,
        })
    }

    fn parse_command(&mut self, line: &SrcLine, full: &str) -> Option<Command> {
        let parallel = full.starts_with('&');
        let rest = full.trim_start_matches('&').trim_start_matches('@');
        let tokens = tokenize(rest);
        let Some((name, _)) = tokens.first().cloned() else {
            let span = self.line_span(line);
            self.error(span, "a command needs a name after `@`");
            return None;
        };
        if !is_name(&name) {
            let span = self.span(line, usize::from(parallel) + 1, name.chars().count());
            self.error(span, format!("`{name}` is not a valid command name"));
        }
        let mut args = Vec::new();
        let mut options = Vec::new();
        for (tok, quoted) in tokens.into_iter().skip(1) {
            match (quoted, tok.split_once('=')) {
                (false, Some((k, v))) if is_name(k) => {
                    options.push((k.to_string(), unquote(v).unwrap_or_else(|| v.to_string())));
                }
                _ => args.push(tok),
            }
        }
        Some(Command {
            name,
            args,
            options,
            parallel,
            source: rest.split_whitespace().collect::<Vec<_>>().join(" "),
            span: self.span(
                line,
                0,
                usize::from(parallel)
                    + 1
                    + rest
                        .split_whitespace()
                        .next()
                        .map_or(0, |n| n.chars().count()),
            ),
        })
    }

    fn parse_line(&mut self, line: &SrcLine, full: &str) -> Line {
        let span = self.line_span(line);
        let (speaker, rest, offset) = if full.starts_with('[') {
            match full.find(']') {
                Some(close) => {
                    let inner = full[1..close].trim();
                    let speaker = if inner == "??" {
                        Speaker::Unknown
                    } else if let Some(name) = unquote(inner) {
                        Speaker::OneOff(name)
                    } else if let Some(id) = inner.strip_suffix('~') {
                        Speaker::Thought(id.trim().to_string())
                    } else if let Some((id, pose)) = inner.split_once(':') {
                        Speaker::Character {
                            id: id.trim().to_string(),
                            pose: Some(pose.trim().to_string()),
                        }
                    } else {
                        Speaker::Character {
                            id: inner.to_string(),
                            pose: None,
                        }
                    };
                    let id = match &speaker {
                        Speaker::Character { id, .. } | Speaker::Thought(id) => Some(id.as_str()),
                        _ => None,
                    };
                    if let Some(id) = id
                        && !is_name(id)
                    {
                        let span = self.span(line, 1, id.chars().count());
                        self.error(span, format!("`{id}` is not a valid character id"));
                    }
                    (speaker, &full[close + 1..], close + 1)
                }
                None => {
                    self.error(span, "unterminated `[` at the start of the line");
                    (Speaker::Narration, full, 0)
                }
            }
        } else {
            (Speaker::Narration, full, 0)
        };
        let (text_src, id, tags) = split_tags(rest);
        let text_offset = full[..offset].chars().count() + leading_spaces(rest);
        let text = parse_text(
            text_src,
            self.span(line, text_offset, text_src.chars().count()),
            self.diags,
        );
        if text_src.is_empty() {
            self.error(span, "a line needs a text");
        }
        Line {
            speaker,
            text,
            id,
            tags,
            span,
        }
    }
}

fn leading_spaces(s: &str) -> usize {
    s.len() - s.trim_start().len()
}

/// `"text"` without its quotes, when it is quoted.
fn unquote(s: &str) -> Option<String> {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        Some(s[1..s.len() - 1].to_string())
    } else {
        None
    }
}

/// Splits on spaces, keeping quoted strings together; the flag tells
/// whether the token started with a quote (`key="value"` does not).
fn tokenize(s: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut was_quoted = false;
    for c in s.chars() {
        match c {
            '"' => {
                if cur.is_empty() && !quoted {
                    was_quoted = true;
                }
                quoted = !quoted;
            }
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() || was_quoted {
                    out.push((std::mem::take(&mut cur), was_quoted));
                    was_quoted = false;
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() || was_quoted {
        out.push((cur, was_quoted));
    }
    out
}

/// `2s`, `400ms` or a bare number of seconds, in seconds.
pub fn parse_duration(s: &str) -> Option<f64> {
    if let Some(ms) = s.strip_suffix("ms") {
        return ms.trim().parse::<f64>().ok().map(|v| v / 1000.0);
    }
    if let Some(sec) = s.strip_suffix('s') {
        return sec.trim().parse().ok();
    }
    s.parse().ok()
}

/// The byte index of the last `->` outside brackets, braces and quotes.
fn find_arrow(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut quoted = false;
    let mut found = None;
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'"' => quoted = !quoted,
            b'[' | b'{' if !quoted => depth += 1,
            b']' | b'}' if !quoted => depth -= 1,
            b'-' if !quoted && depth == 0 && bytes.get(i + 1) == Some(&b'>') => found = Some(i),
            _ => {}
        }
    }
    found
}

/// The byte index of the first `[` outside quotes and braces.
fn find_bracket(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut quoted = false;
    for (i, b) in s.bytes().enumerate() {
        match b {
            b'"' => quoted = !quoted,
            b'{' if !quoted => depth += 1,
            b'}' if !quoted => depth -= 1,
            b'[' if !quoted && depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Takes the trailing `#tags` off a text: the text, the `#l:` id, the
/// other tags.
fn split_tags(s: &str) -> (&str, Option<String>, Vec<String>) {
    let mut text = s.trim();
    let mut id = None;
    let mut tags = Vec::new();
    while let Some(at) = text.rfind(" #") {
        let tag = &text[at + 2..];
        if tag.is_empty() || tag.contains(' ') || tag.contains('}') {
            break;
        }
        match tag.strip_prefix("l:") {
            Some(l) => id = Some(l.to_string()),
            None => tags.insert(0, tag.to_string()),
        }
        text = text[..at].trim_end();
    }
    (text, id, tags)
}
