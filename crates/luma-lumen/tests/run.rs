//! The runner's flow rules on small stories.

use luma_lumen::expr::Value;
use luma_lumen::{Event, Runner, Story};

fn story(src: &str) -> Story {
    let (story, diags) =
        Story::from_sources(vec![("s.lumen".to_string(), src.to_string())], |_| None);
    assert!(diags.is_empty(), "{diags:?}");
    let reports = luma_lumen::check::check(&story);
    assert!(!luma_lumen::diag::has_errors(&reports), "{reports:?}");
    story
}

/// Plays with the given answers, returning what was shown.
fn play(story: &Story, start: Option<&str>, answers: &[usize]) -> Vec<String> {
    let mut runner = Runner::new(story, 7).unwrap();
    runner.start(start).unwrap();
    let mut out = Vec::new();
    let mut answers = answers.iter();
    while let Some(event) = runner.step().unwrap() {
        match event {
            Event::Narration(t) => out.push(t),
            Event::Line { speaker, text, .. } => out.push(format!("{speaker}: {text}")),
            Event::Command { text, .. } => out.push(format!("[{text}]")),
            Event::Choice {
                options, timeout, ..
            } => {
                out.push(format!(
                    "? {}{}",
                    options
                        .iter()
                        .map(|o| o.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" / "),
                    timeout.map(|t| format!(" ({t}s)")).unwrap_or_default()
                ));
                runner.choose(*answers.next().expect("an answer")).unwrap();
            }
            Event::Scene { name, .. } => out.push(format!("scene {name}")),
            Event::Stopped => out.push("STOP".to_string()),
            Event::Ended => out.push("END".to_string()),
        }
    }
    out
}

#[test]
fn stitches_flow_into_each_other_and_knots_end_the_story() {
    let s = story("== a\nOne.\n= b\nTwo.\n= c\nThree.\n-> d\n== d\nFour.\n");
    assert_eq!(
        play(&s, None, &[]),
        ["One.", "Two.", "Three.", "Four.", "END"]
    );
}

#[test]
fn calls_return_and_keep_the_callers_temps() {
    let s = story(
        "== main\ntemp here = \"main\"\ncall shared\nBack in ${here}.\n-> END\n== shared\nShared.\nreturn\nNever.\n",
    );
    assert_eq!(play(&s, None, &[]), ["Shared.", "Back in main.", "END"]);
}

#[test]
fn choices_once_sticky_hidden_and_skipped() {
    let s = story(
        "var n = 0\n== start\n? Pick\n  - Once\n    ~ n += 1\n  * Again [if n > 0]\n    ~ n += 1\nAfter ${n}.\nif n < 3:\n  -> start\n-> END\n",
    );
    // First visit: only "Once" (Again needs n > 0). Second: Once is gone,
    // Again shows. Third: Again again. Then n is 3 and the story ends.
    assert_eq!(
        play(&s, None, &[0, 0, 0]),
        [
            "? Once", "After 1.", "? Again", "After 2.", "? Again", "After 3.", "END"
        ]
    );
    // When nothing can be shown, the choice is skipped.
    let s = story("var n = 1\n== start\n? Pick\n  - Never [if n > 5]\nSkipped.\n-> END\n");
    assert_eq!(play(&s, None, &[]), ["Skipped.", "END"]);
}

#[test]
fn timed_choices_can_run_out() {
    let s = story(
        "== start\n? (3s) Quick\n  - Jump -> END\n  timeout -> late\n== late\nToo late.\n-> STOP\n",
    );
    assert_eq!(play(&s, None, &[1]), ["? Jump (3s)", "Too late.", "STOP"]);
    assert_eq!(play(&s, None, &[0]), ["? Jump (3s)", "END"]);
}

#[test]
fn alternations_vary_by_visit() {
    let s = story(
        "var i = 0\n== start\n{seq: A | B} {cycle: x | y}\n~ i += 1\nif i < 4:\n  -> start\n-> END\n",
    );
    assert_eq!(play(&s, None, &[]), ["A x", "B y", "B x", "B y", "END"]);
}

#[test]
fn state_and_functions() {
    let s = story(
        "var gold = 2\n== start\n~ gold = gold * 3 + 1\n@give key\nif has(key) and gold == 7 and visits(start) == 1 and not seen(side):\n  Right.\n-> side\n== side\n? Pick\n  - Tagged #tag -> check\n== check\nif picked(tag) and seen(side):\n  Picked.\n-> END\n",
    );
    assert_eq!(
        play(&s, None, &[0]),
        ["[give key]", "Right.", "? Tagged", "Picked.", "END"]
    );
    let mut runner = Runner::new(&s, 1).unwrap();
    runner.start(None).unwrap();
    while runner
        .step()
        .unwrap()
        .is_some_and(|e| !matches!(e, Event::Choice { .. }))
    {}
    assert_eq!(runner.global("gold"), Some(&Value::Int(7)));
    assert_eq!(runner.current_knot(), Some("side"));
}

#[test]
fn runtime_errors_name_the_line() {
    let (s, _) = Story::from_sources(
        vec![(
            "s.lumen".to_string(),
            "var n = 0\n== start\n~ n = n / 0\n-> END\n".to_string(),
        )],
        |_| None,
    );
    let mut runner = Runner::new(&s, 1).unwrap();
    runner.start(None).unwrap();
    let err = runner.step().unwrap_err();
    assert_eq!(err.to_string(), "line 3: division by zero");
}
