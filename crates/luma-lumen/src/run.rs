//! The runner: plays a story one event at a time, for the text mode and
//! the tests. It walks the syntax tree; nothing is compiled.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

use crate::ast::{AltKind, AssignOp, Segment, Span, Speaker, Stmt, Story, Target, Text};
use crate::expr::{self, BinOp, Env, Value};

/// Something the story could not do while running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    /// What happened.
    pub message: String,
    /// The statement that was running, when known.
    pub span: Option<Span>,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.span {
            Some(s) => write!(f, "line {}: {}", s.line, self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for RuntimeError {}

/// What the story shows or does next.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A narration line.
    Narration(String),
    /// A spoken line.
    Line {
        /// The speaker's displayed name (`???` when unknown).
        speaker: String,
        /// The pose switched to, when the line names one.
        pose: Option<String>,
        /// An inner voice.
        thought: bool,
        /// The text, inline tags dropped and values inserted.
        text: String,
    },
    /// A stage direction.
    Command {
        /// The command's name.
        name: String,
        /// The whole direction as written, without `@`.
        text: String,
        /// Runs in parallel (`&`).
        parallel: bool,
    },
    /// The story waits for the player to choose; answer with
    /// [`Runner::choose`].
    Choice {
        /// The prompt, when the `?` line has one.
        prompt: Option<String>,
        /// The options that can be shown, in order.
        options: Vec<ChoiceView>,
        /// Seconds before the timeout divert, for a timed choice.
        timeout: Option<f64>,
    },
    /// `@scene name`: the scenario hands control to an interactive scene
    /// and, in text mode, ends there.
    Scene {
        /// The scene.
        name: String,
        /// `await`: the scenario would resume when the scene diverts back.
        awaits: bool,
    },
    /// `-> STOP`: the scenario ends and the scene takes over.
    Stopped,
    /// `-> END`, or the story ran out of lines.
    Ended,
}

/// An option as shown to the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceView {
    /// The text, values inserted.
    pub text: String,
    /// The reason it is greyed out, when it is locked.
    pub locked: Option<String>,
}

#[derive(Debug, Clone)]
struct Frame {
    knot: usize,
    stitch: Option<usize>,
    /// Nested blocks entered: (statement index, child block index).
    path: Vec<(usize, usize)>,
    /// Next statement in the innermost block.
    index: usize,
    temps: HashMap<String, Value>,
}

#[derive(Debug, Clone)]
struct Pending {
    /// Indexes of the shown options among the choice's.
    options: Vec<usize>,
    /// Whether the timeout can be chosen.
    timeout: bool,
}

/// Plays a story.
pub struct Runner<'a> {
    story: &'a Story,
    globals: HashMap<String, Value>,
    inventory: BTreeSet<String>,
    visits: HashMap<String, u32>,
    picked: HashSet<String>,
    used_once: HashSet<Span>,
    alternations: HashMap<Span, u32>,
    /// The call stack; the last frame runs.
    frames: Vec<Frame>,
    pending: Option<Pending>,
    finished: bool,
    rng: u64,
}

impl<'a> Runner<'a> {
    /// A runner over a checked story. `seed` drives `random`, `chance`
    /// and `shuffle`.
    pub fn new(story: &'a Story, seed: u64) -> Result<Runner<'a>, RuntimeError> {
        let mut runner = Runner {
            story,
            globals: HashMap::new(),
            inventory: BTreeSet::new(),
            visits: HashMap::new(),
            picked: HashSet::new(),
            used_once: HashSet::new(),
            alternations: HashMap::new(),
            frames: Vec::new(),
            pending: None,
            finished: false,
            rng: seed.max(1),
        };
        for v in &story.vars {
            let value = expr::eval(&v.value, &mut runner).map_err(|m| RuntimeError {
                message: m,
                span: Some(v.span),
            })?;
            runner.globals.insert(v.name.clone(), value);
        }
        Ok(runner)
    }

    /// Starts at a knot: the one named, else the manifest's, else the
    /// first.
    pub fn start(&mut self, knot: Option<&str>) -> Result<(), RuntimeError> {
        let name = match knot {
            Some(n) => n.to_string(),
            None => self
                .story
                .entry()
                .map(|k| k.name.clone())
                .ok_or_else(|| RuntimeError {
                    message: "the story has no knot".to_string(),
                    span: None,
                })?,
        };
        let index = self.story.knot_index(&name).ok_or_else(|| RuntimeError {
            message: format!("unknown knot `{name}`"),
            span: None,
        })?;
        self.frames.clear();
        self.frames.push(Frame {
            knot: index,
            stitch: None,
            path: Vec::new(),
            index: 0,
            temps: HashMap::new(),
        });
        *self
            .visits
            .entry(self.story.knots[index].name.clone())
            .or_default() += 1;
        self.finished = false;
        self.pending = None;
        Ok(())
    }

    /// Whether the story is over.
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// The items given so far.
    pub fn inventory(&self) -> impl Iterator<Item = &str> {
        self.inventory.iter().map(String::as_str)
    }

    /// A global variable's value.
    pub fn global(&self, name: &str) -> Option<&Value> {
        self.globals.get(name)
    }

    /// How often a knot was entered.
    pub fn visits(&self, knot: &str) -> u32 {
        self.visits.get(knot).copied().unwrap_or(0)
    }

    /// The name of the knot running now.
    pub fn current_knot(&self) -> Option<&str> {
        self.frames
            .last()
            .map(|f| self.story.knots[f.knot].name.as_str())
    }

    /// Runs to the next event. `None` once the story is over. At a
    /// [`Event::Choice`] the runner waits for [`Runner::choose`].
    pub fn step(&mut self) -> Result<Option<Event>, RuntimeError> {
        if self.finished {
            return Ok(None);
        }
        if self.pending.is_some() {
            return Err(RuntimeError {
                message: "a choice is waiting for an answer".to_string(),
                span: None,
            });
        }
        loop {
            let Some(frame) = self.frames.last() else {
                return self.end(Event::Ended);
            };
            let block = block(self.story, frame);
            if frame.index >= block.len() {
                self.leave_block();
                continue;
            }
            let stmt = &block[frame.index];
            let span = stmt.span();
            let index = frame.index;
            self.frames.last_mut().expect("a frame").index += 1;
            match stmt {
                Stmt::Line(line) => {
                    let text = self.render(&line.text).map_err(|m| at(m, span))?;
                    return Ok(Some(match &line.speaker {
                        Speaker::Narration => Event::Narration(text),
                        Speaker::Character { id, pose } => Event::Line {
                            speaker: self.display_name(id),
                            pose: pose.clone(),
                            thought: false,
                            text,
                        },
                        Speaker::Thought(id) => Event::Line {
                            speaker: self.display_name(id),
                            pose: None,
                            thought: true,
                            text,
                        },
                        Speaker::Unknown => Event::Line {
                            speaker: "???".to_string(),
                            pose: None,
                            thought: false,
                            text,
                        },
                        Speaker::OneOff(name) => Event::Line {
                            speaker: name.clone(),
                            pose: None,
                            thought: false,
                            text,
                        },
                    }));
                }
                Stmt::Command(c) => {
                    let text = c.source.clone();
                    match c.name.as_str() {
                        "give" => {
                            if let Some(item) = c.args.first() {
                                self.inventory.insert(item.clone());
                            }
                        }
                        "take" => {
                            if let Some(item) = c.args.first() {
                                self.inventory.remove(item);
                            }
                        }
                        "scene" => {
                            let name = c.args.first().cloned().unwrap_or_default();
                            let awaits = c.args.get(1).is_some_and(|a| a == "await");
                            return self.end(Event::Scene { name, awaits });
                        }
                        _ => {}
                    }
                    return Ok(Some(Event::Command {
                        name: c.name.clone(),
                        text,
                        parallel: c.parallel,
                    }));
                }
                Stmt::Choice(choice) => {
                    let mut shown = Vec::new();
                    let mut views = Vec::new();
                    for (i, option) in choice.options.iter().enumerate() {
                        if !option.sticky && self.used_once.contains(&option.span) {
                            continue;
                        }
                        if let Some(cond) = &option.condition
                            && !expr::eval(cond, self)
                                .map_err(|m| at(m, option.span))?
                                .truthy()
                        {
                            continue;
                        }
                        let locked = match &option.locked {
                            Some((cond, reason))
                                if expr::eval(cond, self)
                                    .map_err(|m| at(m, option.span))?
                                    .truthy() =>
                            {
                                Some(reason.clone())
                            }
                            _ => None,
                        };
                        let text = self.render(&option.text).map_err(|m| at(m, option.span))?;
                        shown.push(i);
                        views.push(ChoiceView { text, locked });
                    }
                    let timeout = choice.timeout.as_ref().map(|t| t.seconds);
                    if views.iter().all(|v| v.locked.is_some()) && timeout.is_none() {
                        // Nothing to pick: the story goes on under the choice.
                        continue;
                    }
                    let prompt = match &choice.prompt {
                        Some(p) => Some(self.render(p).map_err(|m| at(m, span))?),
                        None => None,
                    };
                    self.pending = Some(Pending {
                        options: shown,
                        timeout: timeout.is_some(),
                    });
                    // Point back at the choice, so that the answer can enter it.
                    self.frames.last_mut().expect("a frame").index = index;
                    return Ok(Some(Event::Choice {
                        prompt,
                        options: views,
                        timeout,
                    }));
                }
                Stmt::Divert(d) => {
                    if let Some(event) = self.divert(&d.target, d.span)? {
                        return self.end(event);
                    }
                }
                Stmt::Call(d) => {
                    let (knot, stitch) = self.resolve(&d.target, d.span)?;
                    self.enter(knot, stitch, true);
                }
                Stmt::Return(_) => {
                    self.frames.pop();
                }
                Stmt::Assign(a) => {
                    let value = expr::eval(&a.value, self).map_err(|m| at(m, span))?;
                    let frame = self.frames.last_mut().expect("a frame");
                    if a.temp {
                        frame.temps.insert(a.name.clone(), value);
                        continue;
                    }
                    let slot = if frame.temps.contains_key(&a.name) {
                        frame.temps.get_mut(&a.name)
                    } else {
                        self.globals.get_mut(&a.name)
                    };
                    let Some(slot) = slot else {
                        return Err(at(format!("unknown variable `{}`", a.name), span));
                    };
                    *slot = match a.op {
                        AssignOp::Set => value,
                        AssignOp::Add => expr::binary(BinOp::Add, slot.clone(), value)
                            .map_err(|m| at(m, span))?,
                        AssignOp::Sub => expr::binary(BinOp::Sub, slot.clone(), value)
                            .map_err(|m| at(m, span))?,
                    };
                }
                Stmt::If(i) => {
                    let mut taken = None;
                    for (b, (cond, _)) in i.branches.iter().enumerate() {
                        if expr::eval(cond, self).map_err(|m| at(m, span))?.truthy() {
                            taken = Some(b);
                            break;
                        }
                    }
                    let child = match taken {
                        Some(b) => b,
                        None if i.otherwise.is_some() => i.branches.len(),
                        None => continue,
                    };
                    let frame = self.frames.last_mut().expect("a frame");
                    frame.path.push((index, child));
                    frame.index = 0;
                }
            }
        }
    }

    /// Answers a [`Event::Choice`]: `index` counts the shown options; the
    /// options' count itself means letting the time run out.
    pub fn choose(&mut self, index: usize) -> Result<(), RuntimeError> {
        let Some(pending) = self.pending.take() else {
            return Err(RuntimeError {
                message: "no choice is waiting".to_string(),
                span: None,
            });
        };
        let frame = self.frames.last().expect("a frame");
        let choice_index = frame.index;
        let Stmt::Choice(choice) = &block(self.story, frame)[choice_index] else {
            return Err(RuntimeError {
                message: "the runner lost its choice".to_string(),
                span: None,
            });
        };
        if index == pending.options.len() && pending.timeout {
            let timeout = choice.timeout.as_ref().expect("a timeout");
            let (target, span) = (timeout.target.target.clone(), timeout.target.span);
            self.frames.last_mut().expect("a frame").index += 1;
            if let Some(event) = self.divert(&target, span)? {
                self.finished = true;
                let _ = event;
            }
            return Ok(());
        }
        let Some(&option_index) = pending.options.get(index) else {
            self.pending = Some(pending);
            return Err(RuntimeError {
                message: format!("no option {index}"),
                span: None,
            });
        };
        let option = &choice.options[option_index];
        if let Some((cond, reason)) = &option.locked
            && expr::eval(cond, self)
                .map_err(|m| at(m, option.span))?
                .truthy()
        {
            self.pending = Some(pending);
            return Err(RuntimeError {
                message: format!("that option is locked: {reason}"),
                span: Some(option.span),
            });
        }
        self.used_once.insert(option.span);
        self.picked.extend(option.tags.iter().cloned());
        let frame = self.frames.last_mut().expect("a frame");
        frame.path.push((choice_index, option_index));
        frame.index = 0;
        Ok(())
    }

    fn end(&mut self, event: Event) -> Result<Option<Event>, RuntimeError> {
        self.finished = true;
        Ok(Some(event))
    }

    /// The innermost block is done: back to the parent block, the next
    /// stitch, or out of the knot.
    fn leave_block(&mut self) {
        let story = self.story;
        let frame = self.frames.last_mut().expect("a frame");
        if let Some((parent_index, _)) = frame.path.pop() {
            frame.index = parent_index + 1;
            return;
        }
        let knot = &story.knots[frame.knot];
        let next_stitch = match frame.stitch {
            None if !knot.stitches.is_empty() => Some(0),
            Some(s) if s + 1 < knot.stitches.len() => Some(s + 1),
            _ => None,
        };
        match next_stitch {
            Some(s) => {
                frame.stitch = Some(s);
                frame.index = 0;
            }
            None => {
                // The knot ran out: back to the caller, or the end.
                self.frames.pop();
            }
        }
    }

    /// Follows a divert. `Some` when it ends the story.
    fn divert(&mut self, target: &Target, span: Span) -> Result<Option<Event>, RuntimeError> {
        match target {
            Target::End => Ok(Some(Event::Ended)),
            Target::Stop | Target::Back => Ok(Some(Event::Stopped)),
            _ => {
                let (knot, stitch) = self.resolve(target, span)?;
                self.enter(knot, stitch, false);
                Ok(None)
            }
        }
    }

    fn resolve(&self, target: &Target, span: Span) -> Result<(usize, Option<usize>), RuntimeError> {
        let story = self.story;
        let current = self.frames.last().map(|f| f.knot);
        match target {
            Target::Name(name) => {
                if let Some(k) = current
                    && let Some(s) = story.knots[k].stitch(name)
                {
                    return Ok((k, Some(s)));
                }
                story
                    .knot_index(name)
                    .map(|k| (k, None))
                    .ok_or_else(|| at(format!("unknown knot or stitch `{name}`"), span))
            }
            Target::Path(knot, stitch) => {
                let k = story
                    .knot_index(knot)
                    .ok_or_else(|| at(format!("unknown knot `{knot}`"), span))?;
                let s = story.knots[k]
                    .stitch(stitch)
                    .ok_or_else(|| at(format!("knot `{knot}` has no stitch `{stitch}`"), span))?;
                Ok((k, Some(s)))
            }
            _ => Err(at("not a place in the story".to_string(), span)),
        }
    }

    /// Goes to a knot or stitch: in place for a divert, on a new frame for
    /// a call. Entering another knot counts a visit and drops the temps.
    fn enter(&mut self, knot: usize, stitch: Option<usize>, call: bool) {
        let current = self.frames.last().map(|f| f.knot);
        if call || current != Some(knot) {
            *self
                .visits
                .entry(self.story.knots[knot].name.clone())
                .or_default() += 1;
        }
        let frame = Frame {
            knot,
            stitch,
            path: Vec::new(),
            index: 0,
            temps: HashMap::new(),
        };
        if call {
            self.frames.push(frame);
        } else if current == Some(knot) {
            let top = self.frames.last_mut().expect("a frame");
            top.stitch = stitch;
            top.path.clear();
            top.index = 0;
        } else {
            *self.frames.last_mut().expect("a frame") = frame;
        }
    }

    fn display_name(&self, id: &str) -> String {
        self.story
            .character(id)
            .map_or_else(|| id.to_string(), |c| c.name.clone())
    }

    /// A line's text with its values inserted and its tags dropped.
    fn render(&mut self, text: &Text) -> Result<String, String> {
        let mut out = String::new();
        for segment in &text.0 {
            match segment {
                Segment::Text(t) => out.push_str(t),
                Segment::Value(e) => out.push_str(&expr::eval(e, self)?.to_string()),
                Segment::Tag { .. } => {}
                Segment::Alt { kind, items, key } => {
                    if items.is_empty() {
                        continue;
                    }
                    let count = self.alternations.entry(*key).or_default();
                    let n = items.len();
                    let pick = match kind {
                        AltKind::Seq => (*count as usize).min(n - 1),
                        AltKind::Cycle => *count as usize % n,
                        AltKind::Shuffle => 0,
                    };
                    *count += 1;
                    let pick = if *kind == AltKind::Shuffle {
                        (self.next_random() % n as u64) as usize
                    } else {
                        pick
                    };
                    let item = items[pick].clone();
                    out.push_str(&self.render(&item)?);
                }
            }
        }
        Ok(out)
    }

    fn next_random(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

impl Env for Runner<'_> {
    fn var(&self, name: &str) -> Option<Value> {
        self.frames
            .last()
            .and_then(|f| f.temps.get(name))
            .or_else(|| self.globals.get(name))
            .cloned()
    }

    fn query(&mut self, function: &str, name: &str) -> Result<Value, String> {
        Ok(match function {
            "has" => Value::Bool(self.inventory.contains(name)),
            "visits" => Value::Int(i64::from(self.visits(name))),
            "seen" => Value::Bool(self.visits(name) > 0),
            "picked" => Value::Bool(self.picked.contains(name)),
            _ => return Err(format!("unknown function `{function}`")),
        })
    }

    fn call(&mut self, function: &str, args: Vec<Value>) -> Result<Value, String> {
        match (function, args.as_slice()) {
            ("random", [Value::Int(a), Value::Int(b)]) => {
                let (lo, hi) = (*a.min(b), *a.max(b));
                let span = (hi - lo + 1) as u64;
                Ok(Value::Int(lo + (self.next_random() % span) as i64))
            }
            ("random", _) => Err("`random` takes two integers".to_string()),
            ("chance", [Value::Float(p)]) => Ok(Value::Bool(
                (self.next_random() % 1_000_000) as f64 / 1_000_000.0 < *p,
            )),
            ("chance", [Value::Int(p)]) => Ok(Value::Bool(*p > 0)),
            ("chance", _) => Err("`chance` takes a probability".to_string()),
            _ => Err(format!("unknown function `{function}`")),
        }
    }
}

fn at(message: String, span: Span) -> RuntimeError {
    RuntimeError {
        message,
        span: Some(span),
    }
}

/// The innermost block a frame runs.
fn block<'s>(story: &'s Story, frame: &Frame) -> &'s [Stmt] {
    let knot = &story.knots[frame.knot];
    let mut block: &[Stmt] = match frame.stitch {
        Some(s) => &knot.stitches[s].body,
        None => &knot.body,
    };
    for &(index, child) in &frame.path {
        block = match &block[index] {
            Stmt::If(i) => {
                if child < i.branches.len() {
                    &i.branches[child].1
                } else {
                    i.otherwise.as_deref().unwrap_or(&[])
                }
            }
            Stmt::Choice(c) => &c.options[child].body,
            _ => &[],
        };
    }
    block
}
