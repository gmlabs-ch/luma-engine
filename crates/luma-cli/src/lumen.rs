//! `luma lumen check` and `luma lumen play`.

use std::io::{self, BufRead, Write};
use std::path::Path;

use luma::lumen::check::check;
use luma::lumen::diag::has_errors;
use luma::lumen::{Event, Runner, Severity, Story};

use crate::{Failure, usage_error};

/// `luma lumen <command> ...`.
pub fn run(args: &[String]) -> Result<(), Failure> {
    match args.first().map(String::as_str) {
        Some("check") => match args {
            [_, story] => check_story(Path::new(story)),
            _ => usage_error("lumen check takes exactly one argument: <story>"),
        },
        Some("play") => {
            let mut story = None;
            let mut start = None;
            let mut seed = 1u64;
            let mut answers = Vec::new();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--start" => {
                        start = Some(value(args, &mut i)?.clone());
                    }
                    "--seed" => {
                        seed = value(args, &mut i)?
                            .parse()
                            .map_err(|_| Failure::Usage("--seed expects a number".to_string()))?;
                    }
                    "--choose" => {
                        let list = value(args, &mut i)?;
                        for n in list.split(',') {
                            answers.push(n.trim().parse::<usize>().map_err(|_| {
                                Failure::Usage(format!("--choose expects numbers, got `{n}`"))
                            })?);
                        }
                    }
                    other if other.starts_with("--") => {
                        return usage_error(&format!("unknown option `{other}`"));
                    }
                    other => {
                        if story.replace(other.to_string()).is_some() {
                            return usage_error("lumen play takes one <story>");
                        }
                    }
                }
                i += 1;
            }
            let Some(story) = story else {
                return usage_error("lumen play needs a <story>: a folder or a .lumen file");
            };
            play(Path::new(&story), start.as_deref(), seed, answers)
        }
        _ => usage_error("lumen takes `check <story>` or `play <story>`"),
    }
}

fn value<'a>(args: &'a [String], i: &mut usize) -> Result<&'a String, Failure> {
    *i += 1;
    args.get(*i)
        .ok_or_else(|| Failure::Usage(format!("{} needs a value", args[*i - 1])))
}

/// Loads and checks a story, printing every diagnostic; `Err` when any
/// is an error.
fn load(path: &Path) -> Result<Story, Failure> {
    let (story, mut diags) = Story::load(path)?;
    diags.extend(check(&story));
    diags.sort_by_key(|d| (d.span.file, d.span.line, d.span.col));
    let stderr = io::stderr();
    let mut err = stderr.lock();
    for d in &diags {
        let _ = write!(err, "{}", d.render(&story.files));
    }
    let errors = diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let warnings = diags.len() - errors;
    if has_errors(&diags) {
        return Err(Failure::Error(format!(
            "{errors} error{}, {warnings} warning{}",
            if errors == 1 { "" } else { "s" },
            if warnings == 1 { "" } else { "s" }
        )));
    }
    Ok(story)
}

fn check_story(path: &Path) -> Result<(), Failure> {
    let story = load(path)?;
    let knots = story.knots.len();
    let lines = story
        .files
        .iter()
        .map(|f| f.text.lines().count())
        .sum::<usize>();
    eprintln!(
        "{} file(s), {lines} line(s), {knots} knot(s), {} character(s): no error",
        story.files.len(),
        story.characters.len()
    );
    for note in &story.notes {
        let file = story
            .files
            .get(note.span.file)
            .map_or("?", |f| f.path.as_str());
        println!("note {file}:{}: {}", note.span.line, note.text);
    }
    Ok(())
}

/// Plays the story in the terminal: lines as they come, choices by
/// number from `answers` first, then from standard input.
fn play(path: &Path, start: Option<&str>, seed: u64, answers: Vec<usize>) -> Result<(), Failure> {
    let story = load(path)?;
    let mut runner = Runner::new(&story, seed)?;
    runner.start(start)?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let stdin = io::stdin();
    let mut answers = answers.into_iter();
    while let Some(event) = runner.step()? {
        match event {
            Event::Narration(text) => writeln!(out, "{text}")?,
            Event::Line {
                speaker,
                pose,
                thought,
                text,
            } => {
                let how = match (pose, thought) {
                    (_, true) => " (thinks)".to_string(),
                    (Some(pose), false) => format!(" ({pose})"),
                    (None, false) => String::new(),
                };
                writeln!(out, "{speaker}{how}: {text}")?;
            }
            Event::Command { text, parallel, .. } => {
                writeln!(out, "[{}{text}]", if parallel { "& " } else { "" })?;
            }
            Event::Choice {
                prompt,
                options,
                timeout,
            } => {
                writeln!(out)?;
                if let Some(prompt) = prompt {
                    writeln!(out, "{prompt}")?;
                }
                for (i, option) in options.iter().enumerate() {
                    match &option.locked {
                        Some(reason) => {
                            writeln!(out, "  {}. {}  (locked: {reason})", i + 1, option.text)?
                        }
                        None => writeln!(out, "  {}. {}", i + 1, option.text)?,
                    }
                }
                if let Some(seconds) = timeout {
                    writeln!(out, "  0. (let {seconds} s pass)")?;
                }
                loop {
                    // From --choose first, then from the keyboard; a bad
                    // scripted answer is an error, a typed one is asked again.
                    let (answer, scripted) = match answers.next() {
                        Some(n) => (n, true),
                        None => {
                            write!(out, "> ")?;
                            out.flush()?;
                            let mut line = String::new();
                            if stdin.lock().read_line(&mut line)? == 0 {
                                writeln!(out)?;
                                return Ok(());
                            }
                            let line = line.trim();
                            if line == "q" || line == "quit" {
                                return Ok(());
                            }
                            match line.parse::<usize>() {
                                Ok(n) => (n, false),
                                Err(_) => {
                                    writeln!(out, "a number, please (q to quit)")?;
                                    continue;
                                }
                            }
                        }
                    };
                    let index = if answer == 0 && timeout.is_some() {
                        options.len()
                    } else if answer >= 1 && answer <= options.len() {
                        answer - 1
                    } else {
                        if scripted {
                            return Err(Failure::Error(format!("--choose: no option {answer}")));
                        }
                        writeln!(out, "no option {answer}")?;
                        continue;
                    };
                    match runner.choose(index) {
                        Ok(()) => break,
                        Err(err) if scripted => {
                            return Err(Failure::Error(format!("--choose: {}", err.message)));
                        }
                        Err(err) => writeln!(out, "{}", err.message)?,
                    }
                }
                writeln!(out)?;
            }
            Event::Scene { name, awaits } => {
                writeln!(
                    out,
                    "[scene {name}{}] -- the scene takes over; the scenario ends here",
                    if awaits { " await" } else { "" }
                )?;
            }
            Event::Stopped => writeln!(out, "-- STOP: the scene takes over")?,
            Event::Ended => writeln!(out, "-- END")?,
        }
    }
    Ok(())
}
