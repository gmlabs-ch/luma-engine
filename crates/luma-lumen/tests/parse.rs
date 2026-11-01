//! The parser on small stories: structure and error reporting.

use luma_lumen::ast::{AssignOp, Segment, Speaker, Stmt, Target};
use luma_lumen::diag::Severity;
use luma_lumen::{Diagnostic, Story};

fn parse(src: &str) -> (Story, Vec<Diagnostic>) {
    Story::from_sources(vec![("story.lumen".to_string(), src.to_string())], |_| None)
}

fn errors(diags: &[Diagnostic]) -> Vec<String> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| format!("{}:{}: {}", d.span.line, d.span.col, d.message))
        .collect()
}

#[test]
fn header_knots_stitches_and_lines() {
    let (story, diags) = parse(
        "// header\ncharacter zif \"Commander Zif\" color=#fff\nvar count = 0\npersistent var endings = 0\n\n== lab\nalias old_lab\nThe furnace ticks. //! check the date\n[zif] Three percent.\n[zif:worried] Again. #l:7f3a\n[zif~] Did I rush it?\n[??] Who is there?\n[\"Guard\"] Halt!\n-> lab.work\n\n= work\n@bg lab fade=1s\n&@move zif to=door 1.5s\n~ count += 1\ntemp color = \"red\"\n-> END\n",
    );
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(story.characters[0].id, "zif");
    assert_eq!(story.characters[0].name, "Commander Zif");
    assert_eq!(
        story.characters[0].options,
        [("color".to_string(), "#fff".to_string())]
    );
    assert_eq!(story.vars.len(), 2);
    assert!(story.vars[1].persistent);
    assert_eq!(story.notes.len(), 1);
    assert_eq!(story.notes[0].text, "check the date");
    let knot = &story.knots[0];
    assert_eq!(knot.name, "lab");
    assert_eq!(knot.aliases, ["old_lab"]);
    assert_eq!(knot.body.len(), 7);
    let speakers: Vec<&Speaker> = knot
        .body
        .iter()
        .filter_map(|s| match s {
            Stmt::Line(l) => Some(&l.speaker),
            _ => None,
        })
        .collect();
    assert_eq!(
        speakers,
        [
            &Speaker::Narration,
            &Speaker::Character {
                id: "zif".to_string(),
                pose: None
            },
            &Speaker::Character {
                id: "zif".to_string(),
                pose: Some("worried".to_string())
            },
            &Speaker::Thought("zif".to_string()),
            &Speaker::Unknown,
            &Speaker::OneOff("Guard".to_string()),
        ]
    );
    let Stmt::Line(tagged) = &knot.body[2] else {
        panic!()
    };
    assert_eq!(tagged.id.as_deref(), Some("7f3a"));
    assert_eq!(tagged.text.plain(), "Again.");
    assert!(
        matches!(&knot.body[6], Stmt::Divert(d) if d.target == Target::Path("lab".into(), "work".into()))
    );
    let work = &knot.stitches[0];
    assert_eq!(work.name, "work");
    let Stmt::Command(bg) = &work.body[0] else {
        panic!()
    };
    assert_eq!(
        (bg.name.as_str(), bg.args.as_slice(), bg.option("fade")),
        ("bg", &["lab".to_string()][..], Some("1s"))
    );
    let Stmt::Command(mv) = &work.body[1] else {
        panic!()
    };
    assert!(mv.parallel);
    assert_eq!(mv.args, ["zif", "1.5s"]);
    assert_eq!(mv.option("to"), Some("door"));
    assert!(matches!(&work.body[2], Stmt::Assign(a) if a.op == AssignOp::Add && !a.temp));
    assert!(matches!(&work.body[3], Stmt::Assign(a) if a.temp && a.name == "color"));
    assert!(matches!(&work.body[4], Stmt::Divert(d) if d.target == Target::End));
}

#[test]
fn choices_and_if_blocks_nest() {
    let (story, diags) = parse(
        "var late = false\n== lab\n? What now?\n  - Weigh it #reweigh\n    ~ late = true\n    Same result.\n  * Flame [if has(burner)] -> flame\n  - Ask [locked if late \"Gone home.\"] -> ask\nif late and not has(burner):\n  Think.\nelif late:\n  Hurry.\nelse:\n  Rest.\n  -> END\n-> END\n== flame\n-> END\n== ask\n-> END\n",
    );
    assert!(diags.is_empty(), "{diags:?}");
    let Stmt::Choice(choice) = &story.knots[0].body[0] else {
        panic!()
    };
    assert_eq!(choice.prompt.as_ref().unwrap().plain(), "What now?");
    assert_eq!(choice.options.len(), 3);
    let weigh = &choice.options[0];
    assert!(!weigh.sticky);
    assert_eq!(weigh.tags, ["reweigh"]);
    assert_eq!(weigh.text.plain(), "Weigh it");
    assert_eq!(weigh.body.len(), 2);
    let flame = &choice.options[1];
    assert!(flame.sticky && flame.condition.is_some());
    assert_eq!(flame.text.plain(), "Flame");
    assert!(
        matches!(flame.body.last(), Some(Stmt::Divert(d)) if d.target == Target::Name("flame".into()))
    );
    let ask = &choice.options[2];
    assert_eq!(ask.locked.as_ref().unwrap().1, "Gone home.");
    let Stmt::If(cond) = &story.knots[0].body[1] else {
        panic!()
    };
    assert_eq!(cond.branches.len(), 2);
    assert_eq!(cond.otherwise.as_ref().unwrap().len(), 2);
}

#[test]
fn long_lines_continue_when_indented() {
    let (story, diags) = parse(
        "== lab\n[zif] A very long line\n  that goes on\n  and on.\n@emit achievement\n  first_flame\n-> END\n",
    );
    assert!(diags.is_empty(), "{diags:?}");
    let Stmt::Line(line) = &story.knots[0].body[0] else {
        panic!()
    };
    assert_eq!(line.text.plain(), "A very long line that goes on and on.");
    let Stmt::Command(c) = &story.knots[0].body[1] else {
        panic!()
    };
    assert_eq!(c.args, ["achievement", "first_flame"]);
}

#[test]
fn inline_values_and_timed_choices() {
    let (story, diags) = parse(
        "var n = 1\n== lab\n? (10s) Quick!\n  - Jump -> END\n  timeout -> late\n== late\nYou waited ${n} seconds. {cycle: A | B}\n-> END\n",
    );
    assert!(diags.is_empty(), "{diags:?}");
    let Stmt::Choice(choice) = &story.knots[0].body[0] else {
        panic!()
    };
    let timeout = choice.timeout.as_ref().unwrap();
    assert_eq!(timeout.seconds, 10.0);
    assert_eq!(timeout.target.target, Target::Name("late".into()));
    let Stmt::Line(line) = &story.knots[1].body[0] else {
        panic!()
    };
    assert!(matches!(&line.text.0[1], Segment::Value(_)));
    assert!(matches!(&line.text.0[3], Segment::Alt { items, .. } if items.len() == 2));
}

#[test]
fn errors_name_the_line_and_the_problem() {
    let (_, diags) = parse(
        "what is this\n== Lab Night\n= intro\n  Indented for nothing.\n- stray option\n? Pick\nelse:\n@\n~ 3 = x\n[zif text\nif x\n  Hi.\n\tTabbed.\n-> \n",
    );
    let errors = errors(&diags);
    assert!(
        errors.iter().any(|e| e.starts_with("1:1: only `include`")),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("`Lab Night` is not a valid knot name")),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|e| e.contains("unexpected indentation")),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("an option outside a choice")),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("a choice needs at least one option")),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("`elif` or `else` without an `if`")),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|e| e.contains("a command needs a name")),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("not a valid variable name")),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|e| e.contains("unterminated `[`")),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("an `if` line ends with `:`")),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|e| e.contains("tabs are not allowed")),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("expected a knot or stitch name after `->`")),
        "{errors:?}"
    );
}

#[test]
fn comments_and_blank_lines_are_ignored() {
    let (story, diags) = parse(
        "/* a block\n   comment */\n== lab // the lab\n\n   \nHello. // said\n[zif] \"Quoted // not a comment\"\n-> END\n",
    );
    assert!(
        diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
            == 0,
        "{diags:?}"
    );
    assert_eq!(story.knots[0].body.len(), 3);
    let Stmt::Line(l) = &story.knots[0].body[1] else {
        panic!()
    };
    assert_eq!(l.text.plain(), "\"Quoted // not a comment\"");
}
