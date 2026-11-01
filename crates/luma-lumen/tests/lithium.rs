//! The opening of the lithium story, the spec's complete example, checks
//! clean and plays end to end on both routes.

use std::path::Path;

use luma_lumen::check::check;
use luma_lumen::{Event, Runner, Story};

fn story() -> Story {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/stories/lithium");
    let (story, diags) = Story::load(&dir).unwrap();
    assert!(diags.is_empty(), "{diags:?}");
    story
}

/// Runs to the next choice or the end, collecting the events on the way.
fn run_to_choice(runner: &mut Runner) -> (Vec<Event>, Option<Vec<String>>) {
    let mut events = Vec::new();
    loop {
        match runner.step().unwrap() {
            Some(Event::Choice { options, .. }) => {
                return (
                    events,
                    Some(
                        options
                            .iter()
                            .map(|o| match &o.locked {
                                Some(reason) => format!("{} (locked: {reason})", o.text),
                                None => o.text.clone(),
                            })
                            .collect(),
                    ),
                );
            }
            Some(event) => events.push(event),
            None => return (events, None),
        }
    }
}

fn texts(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .map(|e| match e {
            Event::Narration(t) => t.clone(),
            Event::Line {
                speaker,
                text,
                thought,
                ..
            } => format!(
                "{speaker}{}: {text}",
                if *thought { " (thinks)" } else { "" }
            ),
            Event::Command { text, parallel, .. } => {
                format!("[{}{text}]", if *parallel { "&" } else { "" })
            }
            Event::Scene { name, .. } => format!("=> scene {name}"),
            Event::Stopped => "STOP".to_string(),
            Event::Ended => "END".to_string(),
            Event::Choice { .. } => "?".to_string(),
        })
        .collect()
}

#[test]
fn the_story_loads_and_checks_clean() {
    let story = story();
    assert_eq!(story.files.len(), 2);
    assert_eq!(story.characters.len(), 2);
    assert_eq!(story.start.as_deref(), Some("labo_nuit"));
    assert_eq!(
        story
            .knots
            .iter()
            .map(|k| k.name.as_str())
            .collect::<Vec<_>>(),
        ["labo_nuit", "conseil", "flamme"]
    );
    let diags = check(&story);
    assert!(diags.is_empty(), "{:#?}", diags);
}

#[test]
fn the_flame_route_plays_end_to_end() {
    let story = story();
    let mut runner = Runner::new(&story, 1).unwrap();
    runner.start(None).unwrap();

    let (events, options) = run_to_choice(&mut runner);
    assert_eq!(
        texts(&events),
        [
            "[bg labo_stockholm_nuit fade=1s]",
            "[ambience fourneau volume=0.3]",
            "[music tension_douce fade=2s]",
            "Stockholm, 1817. Le laboratoire de Berzelius, après minuit.",
            "[show arfwedson at=center pose=penche]",
            "Johan August Arfwedson: Encore cette pierre d'Utö… Elle ne réagit comme aucune autre.",
            "Johan August Arfwedson: Il manque toujours trois pour cent de la masse.",
        ]
    );
    // No burner yet: the flame option is hidden; Berzelius can be asked.
    assert_eq!(
        options.unwrap(),
        [
            "Refaire la pesée",
            "Demander conseil",
            "Regarder par la fenêtre"
        ]
    );

    // Weigh again, twice: the counter shows in the line, the option is gone after.
    runner.choose(0).unwrap();
    let (events, options) = run_to_choice(&mut runner);
    assert_eq!(
        texts(&events),
        [
            "[sfx balance]",
            "Johan August Arfwedson: 1 pesées. Le même résultat, au milligramme près.",
        ]
    );
    assert_eq!(
        options.unwrap(),
        ["Demander conseil", "Regarder par la fenêtre"]
    );

    // The window: a sequence that varies on each visit.
    runner.choose(1).unwrap();
    let (events, _) = run_to_choice(&mut runner);
    assert_eq!(
        texts(&events),
        ["La neige tombe sur Riddarholmen. Rien de nouveau."]
    );
    runner.choose(1).unwrap();
    let (events, _) = run_to_choice(&mut runner);
    assert_eq!(
        texts(&events),
        ["La neige tombe sur Riddarholmen. Toujours rien."]
    );

    // Ask Berzelius: a knot with a parallel move, a thought and a gift.
    runner.choose(0).unwrap();
    let (events, options) = run_to_choice(&mut runner);
    assert_eq!(
        texts(&events),
        [
            "[show berzelius at=right]",
            "Jöns Jacob Berzelius: Vous cherchez un sel que vous ne connaissez pas encore.",
            "Jöns Jacob Berzelius: Recommencez. Lentement, cette fois.",
            "[give bec_bunsen]",
            "[&move berzelius to=porte 1.5s]",
            "Johan August Arfwedson (thinks): Lentement. Comme si j'avais le temps.",
            "[wait]",
            "[hide berzelius]",
        ]
    );
    assert_eq!(runner.inventory().collect::<Vec<_>>(), ["bec_bunsen"]);
    assert_eq!(runner.visits("conseil"), 1);
    // The flame is now possible; asking was a once-only option, so it is gone.
    assert_eq!(
        options.unwrap(),
        ["Tester à la flamme", "Regarder par la fenêtre"]
    );

    runner.choose(0).unwrap();
    let (events, options) = run_to_choice(&mut runner);
    assert!(options.is_none());
    assert_eq!(
        texts(&events),
        [
            "[sfx crepitement]",
            "[camera zoom=1.3 on=flamme 2s]",
            "Une flamme rouge vif. Rouge carmin.",
            "[emit succes premiere_flamme]",
            "Johan August Arfwedson: Un nouvel élément. Il faut lui trouver un nom.",
            "=> scene labo_interactif",
        ]
    );
    assert!(runner.finished());
    assert_eq!(
        runner.global("decouverte"),
        Some(&luma_lumen::expr::Value::Bool(true))
    );
}

#[test]
fn a_seen_knot_locks_the_option_and_the_weighings_count() {
    let story = story();
    // Starting in `conseil` counts as having seen it: back in the lab, the
    // option to ask is still there (not picked) but locked, with its reason.
    let mut runner = Runner::new(&story, 1).unwrap();
    runner.start(Some("conseil")).unwrap();
    let (_, options) = run_to_choice(&mut runner);
    assert_eq!(runner.visits("conseil"), 1);
    assert_eq!(runner.visits("labo_nuit"), 1);
    assert_eq!(
        options.unwrap(),
        [
            "Refaire la pesée",
            "Tester à la flamme",
            "Demander conseil (locked: Il est rentré chez lui.)",
            "Regarder par la fenêtre",
        ]
    );
    let err = runner.choose(2).unwrap_err();
    assert!(err.message.contains("locked"), "{err}");
    // Weigh, then the flame: with one weighing the new element line shows.
    runner.choose(0).unwrap();
    run_to_choice(&mut runner);
    runner.choose(0).unwrap();
    let (events, _) = run_to_choice(&mut runner);
    assert!(
        texts(&events)
            .iter()
            .any(|t| t.contains("Un nouvel élément")),
        "{events:?}"
    );
    assert_eq!(
        runner.global("pesees"),
        Some(&luma_lumen::expr::Value::Int(1))
    );
}
