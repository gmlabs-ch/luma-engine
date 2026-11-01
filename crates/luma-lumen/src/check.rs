//! The checker: what the parser cannot see on one line, reported before
//! any playtest. Missing diverts, knots nothing reaches, choices that can
//! all be hidden, variables undeclared or never read, unknown commands or
//! wrong arguments, unknown characters and functions, knots that run out
//! of lines.

use std::collections::{HashMap, HashSet};

use crate::ast::{
    AssignOp, Choice, Command, Knot, Segment, Span, Speaker, Stmt, Story, Target, Text,
};
use crate::diag::{Diagnostic, did_you_mean};
use crate::expr::{BinOp, Expr, NAME_FUNCTIONS, Type, VALUE_FUNCTIONS};
use crate::parse::parse_duration;

/// What a stage direction accepts.
struct Signature {
    name: &'static str,
    /// Fewest and most positional arguments.
    arity: (usize, usize),
    /// Option keys.
    keys: &'static [&'static str],
    /// Positional arguments that are durations, when present.
    durations: &'static [usize],
    /// Words the first argument must be one of, when not empty.
    first: &'static [&'static str],
}

/// The built-in stage directions.
const COMMANDS: &[Signature] = &[
    Signature {
        name: "bg",
        arity: (1, 1),
        keys: &["fade"],
        durations: &[],
        first: &[],
    },
    Signature {
        name: "show",
        arity: (1, 1),
        keys: &["at", "pose", "fade"],
        durations: &[],
        first: &[],
    },
    Signature {
        name: "hide",
        arity: (1, 1),
        keys: &["fade"],
        durations: &[],
        first: &[],
    },
    Signature {
        name: "pose",
        arity: (2, 2),
        keys: &[],
        durations: &[],
        first: &[],
    },
    Signature {
        name: "move",
        arity: (1, 2),
        keys: &["to"],
        durations: &[1],
        first: &[],
    },
    Signature {
        name: "music",
        arity: (1, 1),
        keys: &["fade", "volume"],
        durations: &[],
        first: &[],
    },
    Signature {
        name: "sfx",
        arity: (1, 1),
        keys: &["volume"],
        durations: &[],
        first: &[],
    },
    Signature {
        name: "ambience",
        arity: (1, 1),
        keys: &["volume", "fade"],
        durations: &[],
        first: &[],
    },
    Signature {
        name: "voice",
        arity: (1, 1),
        keys: &[],
        durations: &[],
        first: &["on", "off"],
    },
    Signature {
        name: "fade",
        arity: (1, 2),
        keys: &["color"],
        durations: &[1],
        first: &["in", "out"],
    },
    Signature {
        name: "flash",
        arity: (0, 1),
        keys: &["color"],
        durations: &[0],
        first: &[],
    },
    Signature {
        name: "camera",
        arity: (0, 1),
        keys: &["zoom", "on", "x", "y"],
        durations: &[0],
        first: &[],
    },
    Signature {
        name: "shake",
        arity: (0, 1),
        keys: &["strength"],
        durations: &[0],
        first: &[],
    },
    Signature {
        name: "video",
        arity: (1, 2),
        keys: &[],
        durations: &[],
        first: &[],
    },
    Signature {
        name: "wait",
        arity: (0, 1),
        keys: &[],
        durations: &[0],
        first: &[],
    },
    Signature {
        name: "give",
        arity: (1, 1),
        keys: &[],
        durations: &[],
        first: &[],
    },
    Signature {
        name: "take",
        arity: (1, 1),
        keys: &[],
        durations: &[],
        first: &[],
    },
    Signature {
        name: "scene",
        arity: (1, 2),
        keys: &[],
        durations: &[],
        first: &[],
    },
    Signature {
        name: "emit",
        arity: (1, usize::MAX),
        keys: &[],
        durations: &[],
        first: &[],
    },
];

/// Option keys whose value is a duration, and those whose value is a number.
const DURATION_KEYS: &[&str] = &["fade"];
const NUMBER_KEYS: &[&str] = &["volume", "zoom", "strength", "x", "y"];

/// The names of the built-in stage directions.
pub fn command_names() -> impl Iterator<Item = &'static str> {
    COMMANDS.iter().map(|c| c.name)
}

#[derive(Debug, Clone)]
struct VarInfo {
    kind: Option<Type>,
    /// Read or assigned somewhere after its declaration.
    read: bool,
    span: Span,
}

struct Checker<'a> {
    story: &'a Story,
    diags: Vec<Diagnostic>,
    globals: HashMap<String, VarInfo>,
    /// Temps of the knot being checked.
    temps: HashMap<String, VarInfo>,
    tags: HashSet<String>,
    /// Knots reached from each knot, by index.
    edges: Vec<HashSet<usize>>,
    current: usize,
}

/// Checks a parsed story. The diagnostics are sorted by file and line.
pub fn check(story: &Story) -> Vec<Diagnostic> {
    let mut checker = Checker {
        story,
        diags: Vec::new(),
        globals: HashMap::new(),
        temps: HashMap::new(),
        tags: HashSet::new(),
        edges: vec![HashSet::new(); story.knots.len()],
        current: 0,
    };
    checker.declarations();
    checker.collect_tags();
    for index in 0..story.knots.len() {
        checker.current = index;
        checker.temps.clear();
        checker.knot(&story.knots[index]);
    }
    checker.reachability();
    checker.unused();
    let mut diags = checker.diags;
    diags.sort_by_key(|d| (d.span.file, d.span.line, d.span.col));
    diags
}

impl Checker<'_> {
    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.diags.push(Diagnostic::error(span, message));
    }

    fn warning(&mut self, span: Span, message: impl Into<String>) {
        self.diags.push(Diagnostic::warning(span, message));
    }

    fn suggest(
        &mut self,
        mut diag: Diagnostic,
        name: &str,
        candidates: impl Iterator<Item = String>,
    ) {
        let candidates: Vec<String> = candidates.collect();
        if let Some(close) = did_you_mean(name, candidates.iter().map(String::as_str)) {
            diag = diag.with_help(format!("did you mean `{close}`?"));
        }
        self.diags.push(diag);
    }

    fn knot_names(&self) -> impl Iterator<Item = String> + '_ {
        self.story.knots.iter().map(|k| k.name.clone())
    }

    fn declarations(&mut self) {
        let story = self.story;
        if story.knots.is_empty() {
            self.error(Span::default(), "the story has no knot (`== name`)");
        }
        let mut seen: HashMap<&str, Span> = HashMap::new();
        for knot in &story.knots {
            for name in std::iter::once(&knot.name).chain(&knot.aliases) {
                if let Some(first) = seen.get(name.as_str()) {
                    let first = *first;
                    self.error(
                        knot.span,
                        format!(
                            "knot `{name}` is declared twice (first at line {} of {})",
                            first.line,
                            story.files.get(first.file).map_or("?", |f| f.path.as_str())
                        ),
                    );
                } else {
                    seen.insert(name.as_str(), knot.span);
                }
            }
            let mut stitches: HashSet<&str> = HashSet::new();
            for stitch in &knot.stitches {
                if !stitches.insert(stitch.name.as_str()) {
                    self.error(
                        stitch.span,
                        format!(
                            "stitch `{}` is declared twice in knot `{}`",
                            stitch.name, knot.name
                        ),
                    );
                }
            }
        }
        let mut characters: HashSet<&str> = HashSet::new();
        for c in &story.characters {
            if !characters.insert(c.id.as_str()) {
                self.error(c.span, format!("character `{}` is declared twice", c.id));
            }
        }
        for v in &story.vars {
            if self.globals.contains_key(&v.name) {
                self.error(v.span, format!("variable `{}` is declared twice", v.name));
                continue;
            }
            let kind = self.static_type(&v.value);
            self.check_expr(&v.value);
            self.globals.insert(
                v.name.clone(),
                VarInfo {
                    kind,
                    read: false,
                    span: v.span,
                },
            );
        }
        if let Some(start) = &story.start
            && story.knot(start).is_none()
        {
            let diag = Diagnostic::error(
                Span::default(),
                format!("the manifest's start knot `{start}` does not exist"),
            );
            let start = start.clone();
            self.suggest(diag, &start, story.knots.iter().map(|k| k.name.clone()));
        }
    }

    fn collect_tags(&mut self) {
        fn walk(stmts: &[Stmt], tags: &mut HashSet<String>) {
            for s in stmts {
                match s {
                    Stmt::Choice(c) => {
                        for o in &c.options {
                            tags.extend(o.tags.iter().cloned());
                            walk(&o.body, tags);
                        }
                    }
                    Stmt::If(i) => {
                        for (_, b) in &i.branches {
                            walk(b, tags);
                        }
                        if let Some(b) = &i.otherwise {
                            walk(b, tags);
                        }
                    }
                    _ => {}
                }
            }
        }
        for knot in &self.story.knots {
            walk(&knot.body, &mut self.tags);
            for s in &knot.stitches {
                walk(&s.body, &mut self.tags);
            }
        }
    }

    fn knot(&mut self, knot: &Knot) {
        self.block(&knot.body);
        for stitch in &knot.stitches {
            self.block(&stitch.body);
        }
        let last = knot.stitches.last().map_or(&knot.body, |s| &s.body);
        if !ends_flow(last) {
            let span = knot.stitches.last().map_or(knot.span, |s| s.span);
            self.warning(
                span,
                format!(
                    "knot `{}` runs out of lines without a divert, `-> END` or `@scene`; the story stops there",
                    knot.name
                ),
            );
        }
    }

    fn block(&mut self, stmts: &[Stmt]) {
        let mut after_divert = false;
        for stmt in stmts {
            if std::mem::take(&mut after_divert) {
                self.warning(
                    stmt.span(),
                    "this line comes after a divert and is never reached",
                );
            }
            match stmt {
                Stmt::Line(line) => {
                    if let Speaker::Character { id, .. } | Speaker::Thought(id) = &line.speaker
                        && self.story.character(id).is_none()
                    {
                        let diag = Diagnostic::error(
                            Span {
                                len: id.chars().count() as u32,
                                col: line.span.col + 1,
                                ..line.span
                            },
                            format!("unknown character `{id}`"),
                        );
                        let id = id.clone();
                        let names: Vec<String> =
                            self.story.characters.iter().map(|c| c.id.clone()).collect();
                        self.suggest(diag, &id, names.into_iter());
                    }
                    self.text(&line.text);
                }
                Stmt::Command(c) => self.command(c),
                Stmt::Choice(c) => self.choice(c),
                Stmt::Divert(d) | Stmt::Call(d) => {
                    self.target(&d.target, d.span);
                    after_divert = matches!(stmt, Stmt::Divert(_));
                }
                Stmt::Return(_) => {}
                Stmt::Assign(a) => {
                    self.check_expr(&a.value);
                    let value_type = self.static_type(&a.value);
                    if a.temp {
                        if self.temps.contains_key(&a.name) {
                            self.error(
                                a.span,
                                format!("temp `{}` is declared twice in this knot", a.name),
                            );
                        } else if self.globals.contains_key(&a.name) {
                            self.error(
                                a.span,
                                format!("temp `{}` has the name of a global variable", a.name),
                            );
                        } else {
                            self.temps.insert(
                                a.name.clone(),
                                VarInfo {
                                    kind: value_type,
                                    read: false,
                                    span: a.span,
                                },
                            );
                        }
                        continue;
                    }
                    if let Some(info) = self
                        .temps
                        .get_mut(&a.name)
                        .or_else(|| self.globals.get_mut(&a.name))
                    {
                        info.read = true;
                    }
                    let Some(info) = self
                        .temps
                        .get(&a.name)
                        .or_else(|| self.globals.get(&a.name))
                        .cloned()
                    else {
                        let diag =
                            Diagnostic::error(a.span, format!("undeclared variable `{}`", a.name))
                                .with_help(
                                    "declare it with `var` in a file header, or `temp` in the knot",
                                );
                        let name = a.name.clone();
                        let names: Vec<String> = self
                            .globals
                            .keys()
                            .chain(self.temps.keys())
                            .cloned()
                            .collect();
                        self.suggest(diag, &name, names.into_iter());
                        continue;
                    };
                    match (a.op, info.kind, value_type) {
                        (AssignOp::Set, Some(var), Some(val)) if !assignable(var, val) => {
                            self.error(
                                a.span,
                                format!("`{}` is {var}; it cannot take a {val}", a.name),
                            );
                        }
                        (AssignOp::Add, Some(Type::Bool), _)
                        | (AssignOp::Sub, Some(Type::Bool | Type::Str), _) => {
                            self.error(
                                a.span,
                                format!(
                                    "`{}` is {}; `{}` does not apply",
                                    a.name,
                                    info.kind.unwrap_or(Type::Bool),
                                    if a.op == AssignOp::Add { "+=" } else { "-=" }
                                ),
                            );
                        }
                        (
                            AssignOp::Add | AssignOp::Sub,
                            Some(Type::Int | Type::Float),
                            Some(Type::Str | Type::Bool),
                        ) => {
                            self.error(
                                a.span,
                                format!(
                                    "`{}` is a number; it cannot be changed by a {}",
                                    a.name,
                                    value_type.unwrap_or(Type::Str)
                                ),
                            );
                        }
                        _ => {}
                    }
                }
                Stmt::If(i) => {
                    for (cond, block) in &i.branches {
                        self.check_expr(cond);
                        self.block(block);
                    }
                    if let Some(block) = &i.otherwise {
                        self.block(block);
                    }
                }
            }
        }
    }

    fn text(&mut self, text: &Text) {
        for segment in &text.0 {
            match segment {
                Segment::Value(e) => self.check_expr(e),
                Segment::Alt { items, .. } => {
                    for item in items {
                        self.text(item);
                    }
                }
                Segment::Tag { .. } | Segment::Text(_) => {}
            }
        }
    }

    fn choice(&mut self, choice: &Choice) {
        if let Some(prompt) = &choice.prompt {
            self.text(prompt);
        }
        for option in &choice.options {
            self.text(&option.text);
            if let Some(cond) = &option.condition {
                self.check_expr(cond);
            }
            if let Some((cond, _)) = &option.locked {
                self.check_expr(cond);
            }
            self.block(&option.body);
        }
        if let Some(timeout) = &choice.timeout {
            self.target(&timeout.target.target, timeout.target.span);
        }
        let all_fragile = choice
            .options
            .iter()
            .all(|o| !o.sticky || o.condition.is_some() || o.locked.is_some());
        if all_fragile && choice.timeout.is_none() {
            self.diags.push(
                Diagnostic::warning(
                    choice.span,
                    "every option of this choice can be gone or locked at once; the story then skips the choice",
                )
                .with_help("add a sticky `*` option without a condition, or a timeout"),
            );
        }
    }

    fn command(&mut self, c: &Command) {
        let Some(sig) = COMMANDS.iter().find(|s| s.name == c.name) else {
            let diag = Diagnostic::error(c.span, format!("unknown command `@{}`", c.name));
            let name = c.name.clone();
            self.suggest(diag, &name, command_names().map(str::to_string));
            return;
        };
        let n = c.args.len();
        if n < sig.arity.0 {
            self.error(
                c.span,
                format!(
                    "`@{}` needs {} argument{}, got {n}",
                    c.name,
                    sig.arity.0,
                    if sig.arity.0 == 1 { "" } else { "s" }
                ),
            );
        } else if n > sig.arity.1 {
            self.error(
                c.span,
                format!(
                    "`@{}` takes at most {} argument{}, got {n}",
                    c.name,
                    sig.arity.1,
                    if sig.arity.1 == 1 { "" } else { "s" }
                ),
            );
        }
        if !sig.first.is_empty()
            && let Some(first) = c.args.first()
            && !sig.first.contains(&first.as_str())
        {
            self.error(
                c.span,
                format!(
                    "`@{} {first}`: expected one of {}",
                    c.name,
                    sig.first.join(", ")
                ),
            );
        }
        for &i in sig.durations {
            if let Some(arg) = c.args.get(i)
                && parse_duration(arg).is_none()
            {
                self.error(
                    c.span,
                    format!("`@{}`: `{arg}` is not a duration (`2s`, `400ms`)", c.name),
                );
            }
        }
        match c.name.as_str() {
            "video" if c.args.get(1).is_some_and(|a| a != "skippable") => {
                self.error(
                    c.span,
                    "`@video`: the second argument can only be `skippable`",
                );
            }
            "scene" if c.args.get(1).is_some_and(|a| a != "await") => {
                self.error(c.span, "`@scene`: the second argument can only be `await`");
            }
            _ => {}
        }
        for (key, value) in &c.options {
            if !sig.keys.contains(&key.as_str()) {
                let diag =
                    Diagnostic::error(c.span, format!("`@{}` has no `{key}` option", c.name));
                let key = key.clone();
                self.suggest(diag, &key, sig.keys.iter().map(|k| k.to_string()));
                continue;
            }
            if DURATION_KEYS.contains(&key.as_str()) && parse_duration(value).is_none() {
                self.error(
                    c.span,
                    format!(
                        "`@{} {key}={value}`: not a duration (`2s`, `400ms`)",
                        c.name
                    ),
                );
            }
            if NUMBER_KEYS.contains(&key.as_str()) && value.parse::<f64>().is_err() {
                self.error(c.span, format!("`@{} {key}={value}`: not a number", c.name));
            }
        }
    }

    /// Resolves a divert target, records the edge for reachability.
    fn target(&mut self, target: &Target, span: Span) {
        let story = self.story;
        let current = &story.knots[self.current];
        match target {
            Target::End | Target::Stop | Target::Back => {}
            Target::Name(name) => {
                if current.stitch(name).is_some() {
                    return;
                }
                match story.knot_index(name) {
                    Some(k) => {
                        self.edges[self.current].insert(k);
                    }
                    None => {
                        let diag =
                            Diagnostic::error(span, format!("unknown knot or stitch `{name}`"));
                        let candidates: Vec<String> = self
                            .knot_names()
                            .chain(current.stitches.iter().map(|s| s.name.clone()))
                            .collect();
                        self.suggest(diag, name, candidates.into_iter());
                    }
                }
            }
            Target::Path(knot, stitch) => match story.knot_index(knot) {
                Some(k) => {
                    self.edges[self.current].insert(k);
                    if story.knots[k].stitch(stitch).is_none() {
                        let diag = Diagnostic::error(
                            span,
                            format!("knot `{knot}` has no stitch `{stitch}`"),
                        );
                        let candidates: Vec<String> = story.knots[k]
                            .stitches
                            .iter()
                            .map(|s| s.name.clone())
                            .collect();
                        self.suggest(diag, stitch, candidates.into_iter());
                    }
                }
                None => {
                    let diag = Diagnostic::error(span, format!("unknown knot `{knot}`"));
                    let candidates: Vec<String> = self.knot_names().collect();
                    self.suggest(diag, knot, candidates.into_iter());
                }
            },
        }
    }

    fn check_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Bool(_) | Expr::Int(_) | Expr::Float(_) | Expr::Str(_) => {}
            Expr::Not(x) | Expr::Neg(x) => self.check_expr(x),
            Expr::Binary(_, a, b) => {
                self.check_expr(a);
                self.check_expr(b);
            }
            Expr::Var(name, span) => {
                if let Some(info) = self
                    .temps
                    .get_mut(name)
                    .or_else(|| self.globals.get_mut(name))
                {
                    info.read = true;
                } else {
                    let diag = Diagnostic::error(*span, format!("undeclared variable `{name}`"))
                        .with_help("declare it with `var` in a file header, or `temp` in the knot");
                    let names: Vec<String> = self
                        .globals
                        .keys()
                        .chain(self.temps.keys())
                        .cloned()
                        .collect();
                    self.suggest(diag, name, names.into_iter());
                }
            }
            Expr::Call(name, args, span) => {
                if NAME_FUNCTIONS.contains(&name.as_str()) {
                    // The argument is a name, not a variable: checked here,
                    // not as an expression.
                    let (target, tspan) = match args.as_slice() {
                        [Expr::Var(target, tspan)] => (target, *tspan),
                        [Expr::Str(target)] => (target, *span),
                        _ => {
                            self.error(
                                *span,
                                format!("`{name}` takes one name, as in `{name}(x)`"),
                            );
                            return;
                        }
                    };
                    match name.as_str() {
                        "visits" | "seen" if self.story.knot(target).is_none() => {
                            let diag = Diagnostic::error(
                                tspan,
                                format!("`{name}` names an unknown knot `{target}`"),
                            );
                            let candidates: Vec<String> = self.knot_names().collect();
                            self.suggest(diag, target, candidates.into_iter());
                        }
                        "picked" if !self.tags.contains(target) => {
                            let diag = Diagnostic::error(
                                tspan,
                                format!("`picked` names a tag no option carries: `{target}`"),
                            )
                            .with_help("tag an option with `#name` at the end of its line");
                            let candidates: Vec<String> = self.tags.iter().cloned().collect();
                            self.suggest(diag, target, candidates.into_iter());
                        }
                        _ => {}
                    }
                } else if let Some((_, arity)) = VALUE_FUNCTIONS.iter().find(|(n, _)| *n == name) {
                    if args.len() != *arity {
                        self.error(
                            *span,
                            format!(
                                "`{name}` takes {arity} argument{}",
                                if *arity == 1 { "" } else { "s" }
                            ),
                        );
                    }
                    for a in args {
                        self.check_expr(a);
                    }
                } else {
                    let diag = Diagnostic::error(*span, format!("unknown function `{name}`"));
                    let candidates: Vec<String> = NAME_FUNCTIONS
                        .iter()
                        .map(|s| s.to_string())
                        .chain(VALUE_FUNCTIONS.iter().map(|(n, _)| n.to_string()))
                        .collect();
                    self.suggest(diag, name, candidates.into_iter());
                    for a in args {
                        self.check_expr(a);
                    }
                }
            }
        }
    }

    /// The type of an expression when it can be told without running it.
    fn static_type(&self, expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Bool(_) => Some(Type::Bool),
            Expr::Int(_) => Some(Type::Int),
            Expr::Float(_) => Some(Type::Float),
            Expr::Str(_) => Some(Type::Str),
            Expr::Var(name, _) => self
                .temps
                .get(name)
                .or_else(|| self.globals.get(name))
                .and_then(|i| i.kind),
            Expr::Call(name, _, _) => match name.as_str() {
                "visits" | "random" => Some(Type::Int),
                "has" | "seen" | "picked" | "chance" => Some(Type::Bool),
                _ => None,
            },
            Expr::Not(_) => Some(Type::Bool),
            Expr::Neg(x) => self.static_type(x),
            Expr::Binary(op, a, b) => match op {
                BinOp::Eq
                | BinOp::Ne
                | BinOp::Lt
                | BinOp::Gt
                | BinOp::Le
                | BinOp::Ge
                | BinOp::And
                | BinOp::Or => Some(Type::Bool),
                _ => match (self.static_type(a), self.static_type(b)) {
                    (Some(Type::Str), _) | (_, Some(Type::Str)) if *op == BinOp::Add => {
                        Some(Type::Str)
                    }
                    (Some(Type::Int), Some(Type::Int)) => Some(Type::Int),
                    (Some(Type::Int | Type::Float), Some(Type::Int | Type::Float)) => {
                        Some(Type::Float)
                    }
                    _ => None,
                },
            },
        }
    }

    fn reachability(&mut self) {
        let story = self.story;
        let Some(entry) = story.entry() else { return };
        let Some(start) = story.knot_index(&entry.name) else {
            return;
        };
        let mut reached = vec![false; story.knots.len()];
        let mut stack = vec![start];
        while let Some(k) = stack.pop() {
            if std::mem::replace(&mut reached[k], true) {
                continue;
            }
            stack.extend(self.edges[k].iter().copied());
        }
        for (k, knot) in story.knots.iter().enumerate() {
            if !reached[k] {
                self.warning(
                    knot.span,
                    format!(
                        "no divert reaches knot `{}`; only a scene hook could start it",
                        knot.name
                    ),
                );
            }
        }
    }

    fn unused(&mut self) {
        let mut unused: Vec<(String, Span)> = self
            .globals
            .iter()
            .filter(|(_, i)| !i.read)
            .map(|(n, i)| (n.clone(), i.span))
            .collect();
        unused.sort();
        for (name, span) in unused {
            self.warning(span, format!("variable `{name}` is never used"));
        }
    }
}

/// Whether a value of type `val` may be assigned to a variable of type `var`.
fn assignable(var: Type, val: Type) -> bool {
    var == val || matches!((var, val), (Type::Float, Type::Int))
}

/// Whether a block never falls off its end: its last statement leaves.
fn ends_flow(stmts: &[Stmt]) -> bool {
    match stmts.last() {
        Some(Stmt::Divert(_) | Stmt::Return(_)) => true,
        Some(Stmt::Command(c)) => c.name == "scene",
        Some(Stmt::If(i)) => {
            i.otherwise.as_deref().is_some_and(ends_flow)
                && i.branches.iter().all(|(_, b)| ends_flow(b))
        }
        Some(Stmt::Choice(c)) => {
            c.options.iter().all(|o| ends_flow(&o.body))
                && c.options
                    .iter()
                    .any(|o| o.sticky && o.condition.is_none() && o.locked.is_none())
        }
        _ => false,
    }
}
