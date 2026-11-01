//! The checker on small stories: each kind of report once.

use luma_lumen::Story;
use luma_lumen::check::check;
use luma_lumen::diag::Severity;

fn reports(src: &str) -> Vec<String> {
    let (story, diags) =
        Story::from_sources(vec![("s.lumen".to_string(), src.to_string())], |_| None);
    assert!(diags.is_empty(), "parse: {diags:?}");
    check(&story)
        .into_iter()
        .map(|d| {
            format!(
                "{}{}:{} {}{}",
                if d.severity == Severity::Error {
                    "E"
                } else {
                    "W"
                },
                d.span.line,
                d.span.col,
                d.message,
                d.help.map(|h| format!(" ({h})")).unwrap_or_default()
            )
        })
        .collect()
}

fn has(reports: &[String], text: &str) -> bool {
    reports.iter().any(|r| r.contains(text))
}

#[test]
fn a_clean_story_has_no_report() {
    let r = reports(
        "character zif \"Zif\"\nvar n = 0\n== start\n[zif] Hi.\n~ n += 1\n? Go?\n  * Yes [if n > 0] -> end\n  * No\n    -> start\n== end\n-> END\n",
    );
    assert!(r.is_empty(), "{r:?}");
}

#[test]
fn missing_diverts_get_a_suggestion() {
    let r = reports("== start\n-> flamme\n== flame\n-> start.intro\n== other\n-> flame.nope\n");
    assert!(
        has(
            &r,
            "E2:4 unknown knot or stitch `flamme` (did you mean `flame`?)"
        ),
        "{r:?}"
    );
    assert!(has(&r, "E4:4 knot `start` has no stitch `intro`"), "{r:?}");
    assert!(has(&r, "E6:4 knot `flame` has no stitch `nope`"), "{r:?}");
    assert!(has(&r, "W5:1 no divert reaches knot `other`"), "{r:?}");
    assert!(
        has(&r, "W3:1 no divert reaches knot `flame`"),
        "the broken divert reaches nothing: {r:?}"
    );
}

#[test]
fn choices_that_can_vanish_and_knots_that_run_out() {
    let r = reports(
        "== start\n? Pick\n  - Once -> END\n  - Twice -> END\nAnd then?\n== loop\n?\n  * Stay\n    -> loop\n",
    );
    assert!(
        has(
            &r,
            "W2:1 every option of this choice can be gone or locked at once"
        ),
        "{r:?}"
    );
    assert!(
        has(&r, "W1:1 knot `start` runs out of lines without a divert"),
        "{r:?}"
    );
    assert!(
        !has(&r, "knot `loop` runs out"),
        "a sticky option that diverts keeps the flow: {r:?}"
    );
}

#[test]
fn variables_undeclared_unused_and_mistyped() {
    let r = reports(
        "var count = 0\nvar name = \"x\"\nvar flag = true\nvar idle = 1\n== start\n~ count = \"three\"\n~ name += 1\n~ flag += 1\n~ cuont = 2\nYou have ${count} and ${missing}.\ntemp t = 1\n~ t = \"s\"\n-> END\n",
    );
    assert!(
        has(&r, "E6:3 `count` is int; it cannot take a string"),
        "{r:?}"
    );
    assert!(
        !has(&r, "`name` is string"),
        "strings take anything with +=: {r:?}"
    );
    assert!(has(&r, "E8:3 `flag` is bool; `+=` does not apply"), "{r:?}");
    assert!(
        has(
            &r,
            "E9:3 undeclared variable `cuont` (did you mean `count`?)"
        ),
        "{r:?}"
    );
    assert!(has(&r, "undeclared variable `missing`"), "{r:?}");
    assert!(has(&r, "W4:5 variable `idle` is never used"), "{r:?}");
    assert!(
        has(&r, "E12:3 `t` is int; it cannot take a string"),
        "{r:?}"
    );
}

#[test]
fn commands_are_checked_against_their_signatures() {
    let r = reports(
        "== start\n@bgg lab\n@bg\n@bg lab fade=fast\n@show zif att=left\n@fade sideways 1s\n@wait 1s 2s\n@scene lab later\n@music theme volume=loud\n@emit thing a b c\n-> END\n",
    );
    assert!(
        has(&r, "E2:1 unknown command `@bgg` (did you mean `bg`?)"),
        "{r:?}"
    );
    assert!(has(&r, "E3:1 `@bg` needs 1 argument, got 0"), "{r:?}");
    assert!(has(&r, "E4:1 `@bg fade=fast`: not a duration"), "{r:?}");
    assert!(
        has(&r, "E5:1 `@show` has no `att` option (did you mean `at`?)"),
        "{r:?}"
    );
    assert!(
        has(&r, "E6:1 `@fade sideways`: expected one of in, out"),
        "{r:?}"
    );
    assert!(
        has(&r, "E7:1 `@wait` takes at most 1 argument, got 2"),
        "{r:?}"
    );
    assert!(
        has(&r, "E8:1 `@scene`: the second argument can only be `await`"),
        "{r:?}"
    );
    assert!(has(&r, "E9:1 `@music volume=loud`: not a number"), "{r:?}");
    assert!(!has(&r, "@emit"), "{r:?}");
}

#[test]
fn characters_functions_and_tags() {
    let r = reports(
        "character zif \"Zif\"\n== start\n[zyf] Hi.\n? Pick\n  * Weigh #weigh\n    -> next\n== next\nif picked(wiegh) and seen(nxt) and rnd(1) and random(1):\n  Hi.\n-> END\n",
    );
    assert!(
        has(&r, "E3:2 unknown character `zyf` (did you mean `zif`?)"),
        "{r:?}"
    );
    assert!(
        has(
            &r,
            "`picked` names a tag no option carries: `wiegh` (did you mean `weigh`?)"
        ),
        "{r:?}"
    );
    assert!(
        has(
            &r,
            "`seen` names an unknown knot `nxt` (did you mean `next`?)"
        ),
        "{r:?}"
    );
    assert!(has(&r, "unknown function `rnd`"), "{r:?}");
    assert!(has(&r, "`random` takes 2 arguments"), "{r:?}");
}

#[test]
fn duplicates_and_lines_after_a_divert() {
    let r = reports(
        "var a = 1\nvar a = 2\ncharacter z \"Z\"\ncharacter z \"Zed\"\n== start\n-> END\nNever.\n== start\n-> END\n",
    );
    assert!(has(&r, "E2:5 variable `a` is declared twice"), "{r:?}");
    assert!(has(&r, "E4:1 character `z` is declared twice"), "{r:?}");
    assert!(
        has(
            &r,
            "W7:1 this line comes after a divert and is never reached"
        ),
        "{r:?}"
    );
    assert!(has(&r, "E8:1 knot `start` is declared twice"), "{r:?}");
}
