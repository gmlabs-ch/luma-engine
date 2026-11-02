//! Keeps `docs/compatibility.md` consistent: every row has a valid state, the
//! `Count:` line of each section matches its rows, and the summary tables match
//! the sections. Needs no game data; it only reads the document.
//!
//! Counting rule: in a row `| `A`, `B` | kind | **state** | note |` each
//! backticked name counts once, and twice when the kind cell contains
//! `get/set` (one read, one write).

use std::path::PathBuf;

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
struct Counts {
    done: u64,
    todo: u64,
    noop: u64,
}

impl Counts {
    fn total(self) -> u64 {
        self.done + self.todo + self.noop
    }

    fn add(&mut self, other: Counts) {
        self.done += other.done;
        self.todo += other.todo;
        self.noop += other.noop;
    }
}

struct Section {
    title: String,
    counts: Counts,
    /// The numbers of the `- Count:` line, if the section has one.
    written: Option<Vec<u64>>,
}

fn doc_lines() -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/compatibility.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    text.lines().map(str::to_owned).collect()
}

/// The unsigned integers found in `s`, in order.
fn ints(s: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut cur: Option<u64> = None;
    for c in s.chars() {
        if let Some(d) = c.to_digit(10) {
            cur = Some(cur.unwrap_or(0) * 10 + u64::from(d));
        } else if let Some(n) = cur.take() {
            out.push(n);
        }
    }
    out.extend(cur);
    out
}

fn cells(line: &str) -> Vec<&str> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(str::trim).collect()
}

fn is_table_line(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

fn is_separator(line: &str) -> bool {
    is_table_line(line) && line.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// The number of backticked names in a cell.
fn backticked_names(cell: &str) -> u64 {
    (cell.split('`').count() as u64).saturating_sub(1) / 2
}

/// Parses the `###` sections with their member tables.
fn sections(lines: &[String]) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut in_section = false;
    for (n, line) in lines.iter().enumerate() {
        if line.starts_with("## ") {
            in_section = false;
        } else if let Some(title) = line.strip_prefix("### ") {
            let title = title.split(" (").next().unwrap().trim().to_owned();
            out.push(Section {
                title,
                counts: Counts::default(),
                written: None,
            });
            in_section = true;
        } else if !in_section {
            continue;
        } else if let Some(rest) = line.strip_prefix("- Count:") {
            out.last_mut().unwrap().written = Some(ints(rest));
        } else if is_table_line(line) && !is_separator(line) {
            let c = cells(line);
            assert!(
                c.len() >= 4,
                "line {}: a table row needs name, kind, state and note cells: {line}",
                n + 1
            );
            if c[2] == "State" {
                continue; // header row
            }
            let section = out.last_mut().unwrap();
            let names = backticked_names(c[0]);
            let weight = if c[1].contains("get/set") {
                2 * names
            } else {
                names
            };
            match c[2] {
                "**done**" => section.counts.done += weight,
                "**todo**" => section.counts.todo += weight,
                "**no-op**" => section.counts.noop += weight,
                other => panic!(
                    "line {}: section `{}`: state `{other}` is not **done**, **todo** or **no-op** (row: {line})",
                    n + 1,
                    section.title
                ),
            }
        }
    }
    out
}

/// The table rows (separator lines excluded) that follow the heading `heading`.
fn table_after<'a>(lines: &'a [String], heading: &str) -> Vec<&'a str> {
    let start = lines
        .iter()
        .position(|l| l == heading)
        .unwrap_or_else(|| panic!("heading `{heading}` not found"));
    lines[start + 1..]
        .iter()
        .take_while(|l| !l.starts_with("## "))
        .filter(|l| is_table_line(l) && !is_separator(l))
        .map(String::as_str)
        .collect()
}

#[test]
fn every_section_count_line_matches_its_rows() {
    let lines = doc_lines();
    let secs = sections(&lines);
    assert!(secs.len() > 40, "only {} sections parsed", secs.len());
    let mut errors = Vec::new();
    for s in &secs {
        let expected = vec![s.counts.done, s.counts.todo, s.counts.noop];
        match &s.written {
            None => errors.push(format!("section `{}`: no `- Count:` line", s.title)),
            Some(w) if *w != expected => errors.push(format!(
                "section `{}`: expected Count {} done, {} todo, {} no-op, written {w:?}",
                s.title, expected[0], expected[1], expected[2]
            )),
            Some(_) => {}
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn summary_table_matches_the_sections() {
    let lines = doc_lines();
    let secs = sections(&lines);
    let rows = table_after(&lines, "## Summary per class");
    let mut errors = Vec::new();
    let mut seen = Vec::new();
    let mut all_row = None;

    for row in rows {
        let c = cells(row);
        if c[0] == "Class" {
            continue;
        }
        let nums: Vec<u64> = c[1..].iter().map(|x| ints(x)[0]).collect();
        assert_eq!(nums.len(), 4, "summary row needs four numbers: {row}");
        let written = Counts {
            done: nums[0],
            todo: nums[1],
            noop: nums[2],
        };
        if written.total() != nums[3] {
            errors.push(format!(
                "summary `{}`: total written {}, but the columns add up to {}",
                c[0],
                nums[3],
                written.total()
            ));
        }
        if c[0] == "**All**" {
            all_row = Some(written);
            continue;
        }
        seen.push(c[0].to_owned());
        match secs.iter().find(|s| s.title == c[0]) {
            None => errors.push(format!("summary `{}`: no such section", c[0])),
            Some(s) if s.counts != written => errors.push(format!(
                "summary `{}`: expected {:?}, written {:?}",
                c[0], s.counts, written
            )),
            Some(_) => {}
        }
    }

    let titles: Vec<String> = secs.iter().map(|s| s.title.clone()).collect();
    if seen != titles {
        errors.push(format!(
            "summary rows {seen:?} differ from the sections {titles:?}"
        ));
    }
    let mut expected_all = Counts::default();
    for s in &secs {
        expected_all.add(s.counts);
    }
    match all_row {
        None => errors.push("summary: no `| **All** | ... |` row".to_owned()),
        Some(w) if w != expected_all => errors.push(format!(
            "summary `All`: expected {expected_all:?}, written {w:?}"
        )),
        Some(_) => {}
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));

    // The sentence about the events section.
    let sentence = lines
        .iter()
        .find(|l| l.starts_with("Of the total, "))
        .expect("no `Of the total, ...` sentence after the summary table");
    let written = ints(sentence);
    let events = secs
        .iter()
        .find(|s| s.title == "Events raised on objects")
        .expect("no events section")
        .counts;
    let expected = vec![
        events.total(),
        events.done,
        events.todo,
        expected_all.done - events.done,
        expected_all.todo - events.todo,
        expected_all.noop - events.noop,
    ];
    assert_eq!(
        written, expected,
        "`Of the total` sentence: written {written:?}, expected {expected:?} \
         (events, events done, events todo, rest done, rest todo, rest no-op)"
    );
}

#[test]
fn groups_table_total_is_the_todo_total() {
    let lines = doc_lines();
    let secs = sections(&lines);
    let todo: u64 = secs.iter().map(|s| s.counts.todo).sum();
    let rows = table_after(&lines, "## Groups the work falls into");
    let mut sum = 0;
    let mut all = None;
    for r in &rows {
        let c = cells(r);
        if c[0] == "**All**" {
            all = Some(ints(c[1])[0]);
        } else if c[0] != "Group" {
            sum += ints(c[1]).first().copied().unwrap_or(0);
        }
    }
    let all = all.expect("no `| **All** | **N** | |` row in the groups table");
    assert_eq!(
        all, todo,
        "groups table `All`: expected {todo} todo, written {all}"
    );
    assert_eq!(
        sum, todo,
        "groups table rows add up to {sum}, expected {todo}"
    );
}
