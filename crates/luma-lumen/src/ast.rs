//! The syntax tree of a Lumen story, as the parser builds it.

use crate::expr::Expr;

/// A place in a source file: the file's index in the story, a 1-based
/// line and column, and the length of the span in characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Span {
    /// Index of the file in [`Story::files`].
    pub file: usize,
    /// Line, from 1.
    pub line: u32,
    /// Column, from 1, in characters.
    pub col: u32,
    /// Length in characters (at least 1 when the span points at something).
    pub len: u32,
}

/// A source file, kept for the diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// The path as given, relative to the story folder when possible.
    pub path: String,
    /// The whole text.
    pub text: String,
}

impl SourceFile {
    /// A line of the file, by its 1-based number.
    pub fn line(&self, number: u32) -> &str {
        self.text
            .lines()
            .nth(number.saturating_sub(1) as usize)
            .unwrap_or("")
    }
}

/// A whole story: the files' headers merged, and every knot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Story {
    /// The files, in loading order.
    pub files: Vec<SourceFile>,
    /// Characters declared in the headers.
    pub characters: Vec<Character>,
    /// Global and persistent variables declared in the headers.
    pub vars: Vec<VarDecl>,
    /// The knots of every file, in order.
    pub knots: Vec<Knot>,
    /// Writer notes (`//! ...`).
    pub notes: Vec<Note>,
    /// The entry knot named by the manifest (`start = "..."`).
    pub start: Option<String>,
}

impl Story {
    /// The knot by name or alias.
    pub fn knot(&self, name: &str) -> Option<&Knot> {
        self.knots
            .iter()
            .find(|k| k.name == name || k.aliases.iter().any(|a| a == name))
    }

    /// The index of the knot by name or alias.
    pub fn knot_index(&self, name: &str) -> Option<usize> {
        self.knots
            .iter()
            .position(|k| k.name == name || k.aliases.iter().any(|a| a == name))
    }

    /// The character by id.
    pub fn character(&self, id: &str) -> Option<&Character> {
        self.characters.iter().find(|c| c.id == id)
    }

    /// The knot the story starts in: the manifest's, else the first one.
    pub fn entry(&self) -> Option<&Knot> {
        match &self.start {
            Some(name) => self.knot(name),
            None => self.knots.first(),
        }
    }
}

/// A writer note, `//! text`, listed by the checker and never compiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// The note's text, without the marker.
    pub text: String,
    /// Where it is.
    pub span: Span,
}

/// A character declaration: `character id "Name" key=value...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Character {
    /// The id lines refer to.
    pub id: String,
    /// The displayed name.
    pub name: String,
    /// The options (`color`, `sprites`, `voice`...), as given.
    pub options: Vec<(String, String)>,
    /// Where it is declared.
    pub span: Span,
}

/// A variable declaration: `var`, `persistent var` or `temp`.
#[derive(Debug, Clone, PartialEq)]
pub struct VarDecl {
    /// The variable's name.
    pub name: String,
    /// Its first value.
    pub value: Expr,
    /// `persistent var`: kept across playthroughs.
    pub persistent: bool,
    /// Where it is declared.
    pub span: Span,
}

/// A knot: `== name`, its own lines, then its stitches.
#[derive(Debug, Clone, PartialEq)]
pub struct Knot {
    /// The name diverts use.
    pub name: String,
    /// Former names (`alias old_name`), for old saved games.
    pub aliases: Vec<String>,
    /// The lines before the first stitch.
    pub body: Vec<Stmt>,
    /// The stitches, in order.
    pub stitches: Vec<Stitch>,
    /// Where the header is.
    pub span: Span,
}

impl Knot {
    /// The stitch by name.
    pub fn stitch(&self, name: &str) -> Option<usize> {
        self.stitches.iter().position(|s| s.name == name)
    }
}

/// A stitch: `= name` inside a knot.
#[derive(Debug, Clone, PartialEq)]
pub struct Stitch {
    /// The name.
    pub name: String,
    /// Its lines.
    pub body: Vec<Stmt>,
    /// Where the header is.
    pub span: Span,
}

/// Who says a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Speaker {
    /// Plain text: narration.
    Narration,
    /// `[id] text` or `[id:pose] text`.
    Character {
        /// The character's id.
        id: String,
        /// The pose to switch to first.
        pose: Option<String>,
    },
    /// `[id~] text`: an inner voice.
    Thought(String),
    /// `[??] text`: a speaker not revealed yet.
    Unknown,
    /// `["Name"] text`: a one-off speaker.
    OneOff(String),
}

/// A statement of a block.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// Dialogue or narration.
    Line(Line),
    /// A stage direction (`@name ...`).
    Command(Command),
    /// A choice (`?` and its options).
    Choice(Choice),
    /// `-> target`.
    Divert(Divert),
    /// `call target`: run a knot and come back.
    Call(Divert),
    /// `return` from a called knot.
    Return(Span),
    /// A `~` assignment or a `temp` declaration.
    Assign(Assign),
    /// `if` / `elif` / `else` blocks.
    If(If),
}

impl Stmt {
    /// Where the statement starts.
    pub fn span(&self) -> Span {
        match self {
            Stmt::Line(l) => l.span,
            Stmt::Command(c) => c.span,
            Stmt::Choice(c) => c.span,
            Stmt::Divert(d) | Stmt::Call(d) => d.span,
            Stmt::Return(s) => *s,
            Stmt::Assign(a) => a.span,
            Stmt::If(i) => i.span,
        }
    }
}

/// A line of dialogue or narration.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Who speaks.
    pub speaker: Speaker,
    /// The text, with its inline tags.
    pub text: Text,
    /// The stable id (`#l:7f3a`).
    pub id: Option<String>,
    /// Other `#tags` at the end of the line.
    pub tags: Vec<String>,
    /// Where it is.
    pub span: Span,
}

/// A stage direction: `@name args key=value`, `&@name` in parallel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The command's name, without `@`.
    pub name: String,
    /// Positional arguments, in order.
    pub args: Vec<String>,
    /// `key=value` options, in order.
    pub options: Vec<(String, String)>,
    /// `&`: does not block the scenario.
    pub parallel: bool,
    /// The direction as written, without `&` and `@`.
    pub source: String,
    /// Where it is.
    pub span: Span,
}

impl Command {
    /// The value of an option.
    pub fn option(&self, key: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// A choice block.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    /// The prompt, when the `?` line has one.
    pub prompt: Option<Text>,
    /// A timed choice: how long, and where to go when time runs out.
    pub timeout: Option<Timeout>,
    /// The options, in order.
    pub options: Vec<ChoiceOption>,
    /// Where the `?` line is.
    pub span: Span,
}

/// `? (10s) prompt` with a `timeout -> target` line.
#[derive(Debug, Clone, PartialEq)]
pub struct Timeout {
    /// Seconds before the divert.
    pub seconds: f64,
    /// Where the story goes then.
    pub target: Divert,
}

/// One option of a choice.
#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceOption {
    /// The text shown.
    pub text: Text,
    /// `*`: stays available after being picked; `-`: once only.
    pub sticky: bool,
    /// `[if expr]`: shown only when true.
    pub condition: Option<Expr>,
    /// `[locked if expr "reason"]`: greyed out, with the reason.
    pub locked: Option<(Expr, String)>,
    /// `#tags`, for `picked(tag)`.
    pub tags: Vec<String>,
    /// The stable id (`#l:...`).
    pub id: Option<String>,
    /// The lines run when picked; a trailing `-> target` is its last
    /// statement.
    pub body: Vec<Stmt>,
    /// Where the option line is.
    pub span: Span,
}

/// Where a divert goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `name`: a stitch of the current knot, else a knot.
    Name(String),
    /// `knot.stitch`.
    Path(String, String),
    /// `END`: the story ends.
    End,
    /// `STOP`: the scenario ends, the scene takes over.
    Stop,
    /// `back`: return to an awaiting scenario from a scene.
    Back,
}

/// `-> target` or the target of a `call`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divert {
    /// Where to.
    pub target: Target,
    /// Where the arrow is.
    pub span: Span,
}

/// The operator of a `~` line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    /// `=`
    Set,
    /// `+=`
    Add,
    /// `-=`
    Sub,
}

/// `~ name op expr`, or `temp name = expr`.
#[derive(Debug, Clone, PartialEq)]
pub struct Assign {
    /// The variable.
    pub name: String,
    /// The operator.
    pub op: AssignOp,
    /// The value.
    pub value: Expr,
    /// `temp`: declares a knot-local variable.
    pub temp: bool,
    /// Where it is.
    pub span: Span,
}

/// `if expr:` with `elif` and `else` blocks.
#[derive(Debug, Clone, PartialEq)]
pub struct If {
    /// The conditions and their blocks, in order.
    pub branches: Vec<(Expr, Vec<Stmt>)>,
    /// The `else` block.
    pub otherwise: Option<Vec<Stmt>>,
    /// Where the `if` is.
    pub span: Span,
}

/// A line's text: plain text and inline tags.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Text(pub Vec<Segment>);

impl Text {
    /// The plain text alone, tags and values dropped.
    pub fn plain(&self) -> String {
        let mut out = String::new();
        for s in &self.0 {
            match s {
                Segment::Text(t) => out.push_str(t),
                Segment::Alt { items, .. } => {
                    if let Some(first) = items.first() {
                        out.push_str(&first.plain());
                    }
                }
                Segment::Value(_) | Segment::Tag { .. } => {}
            }
        }
        out
    }
}

/// A piece of a line.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    /// Plain text (emphasis marks kept as written).
    Text(String),
    /// `${expr}`.
    Value(Expr),
    /// `{name args}` or `{/name}`.
    Tag {
        /// The tag's name.
        name: String,
        /// What follows the name, trimmed.
        args: String,
        /// `{/name}`.
        close: bool,
    },
    /// `{seq: a | b | c}`, `cycle:` or `shuffle:`.
    Alt {
        /// How the next item is chosen.
        kind: AltKind,
        /// The alternatives.
        items: Vec<Text>,
        /// Identifies the alternation for its visit counter.
        key: Span,
    },
}

/// How an alternation picks its item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AltKind {
    /// Each item once, then the last one forever.
    Seq,
    /// Round and round.
    Cycle,
    /// At random.
    Shuffle,
}
