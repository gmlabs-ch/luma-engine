# Lumen, the scenario format

Lumen is the scenario language of games made for Luma Engine: a plain-text
file that reads like a screenplay, with the flow of a branching story
(knots, diverts, choices) and the staging of a scene (`@` directions) in
reading order. Wintermute games are untouched: they keep running their
compiled scripts. This document is the specification of what the engine
implements today, the first slice: the parser, the checker and a text
runner. The compiler to bytecode and the player integration come later;
the [decisions](#decisions) section says how.

The crate is `luma-lumen`, re-exported as `luma::lumen` behind the `lumen`
feature of `luma` (off by default). The `luma` command drives it:

```sh
luma lumen check story/            # parse and check every .lumen file under story/
luma lumen play story/             # play it as text, choices by number
luma lumen play story/ --start conseil --seed 3 --choose 1,1,1
```

`play` reads the choices from standard input (`q` quits); `--choose` answers
the first ones, for scripts and tests. A story is a folder (every `.lumen`
file under it, in path order, plus a `lumen.toml` manifest when there is
one) or a single file and the files it includes.

## File basics

A file is UTF-8 text with the `.lumen` extension. Everything before the
first knot is the header; then come knots and their stitches.

| Element | Syntax | Rule |
|---------|--------|------|
| Knot | `== lab_night` | A top-level section, addressable from anywhere: `-> lab_night`. Names are `snake_case` ASCII, unique per story. |
| Stitch | `= arrival` | A sub-section of its knot: `-> lab_night.arrival`, or `-> arrival` from inside the knot. Unique per knot. |
| Alias | `alias old_name` | Right under a knot header: a former name the knot still answers to. |
| Comment | `// note`, `/* ... */` | Ignored. |
| Writer note | `//! check the date` | Listed by `luma lumen check`, never run. |
| Include | `include "common/characters.lumen"` | Header only; relative to the story folder. A file is loaded once. |
| Character | `character arfwedson "Johan August Arfwedson" color=#C6FF7A sprites="actors/arfwedson"` | Header only: an id, a quoted name, then `key=value` options kept as given. |
| Variable | `var weighings = 0`, `persistent var endings_seen = 0` | Header only; see [State](#state). |
| Manifest | `lumen.toml` with `start = "lab_night"` | Names the entry knot; without it the first knot of the first file runs. |

Lines are statements; blank lines are ignored. Indentation is in spaces
(tabs are an error) and matters only under a choice, an option and an
`if`. Anywhere else, a line indented under the previous one continues it,
for a long line of text or a long `@` direction; a continuation may not
start like a statement (`-`, `*`, `?`, `@`, `~`, `->`, `[`, `if`...).

## Dialogue and narration

| Form | Meaning |
|------|---------|
| `[arfwedson] text` | The character speaks; the name shown is the declaration's. |
| `[arfwedson:worried] text` | Speaks and switches pose first. |
| `[arfwedson~] text` | Inner voice (shown as a thought). |
| `[??] text` | Unknown speaker, shown as `???`. |
| `["Guard"] text` | One-off speaker with no declaration. |
| plain text | Narration. |

A line may end with `#tags` and a stable id `#l:7f3a`; both are kept and
stripped from the text. Inline tags sit in braces inside the text:

| Tag | Effect in the text runner |
|-----|---------------------------|
| `${weighings}` | The value of an expression, inserted. |
| `{seq: First. \| Again? \| Not again.}` | Varies on each showing: `seq` runs through the items then repeats the last, `cycle` wraps around, `shuffle` picks at random. |
| `{pause 0.6}`, `{speed 0.5}`...`{/speed}`, `{shake}`...`{/shake}`, `{wave}`, `{sfx click}`, `{auto 1.5}` | Kept in the tree for the player; dropped by the text runner. |
| `*word*`, `**word**` | Emphasis: left in the text as written. |

## Stage directions

A line starting with `@` is a direction: `@name`, positional arguments,
then `key=value` options. `&@name` marks a direction that runs in parallel
with what follows. Durations are `2s` or `400ms` (a bare number is seconds).
The checker knows these directions and refuses any other, or wrong
arguments, so that a typo in staging never becomes a silent no-op:

| Direction | Arguments | Options |
|-----------|-----------|---------|
| `@bg name` | 1 | `fade` |
| `@show who`, `@hide who` | 1 | `at`, `pose`, `fade` (`hide`: `fade`) |
| `@pose who pose` | 2 | |
| `@move who [duration]` | 1-2 | `to` |
| `@music name\|stop` | 1 | `fade`, `volume` |
| `@sfx name` | 1 | `volume` |
| `@ambience name\|stop` | 1 | `volume`, `fade` |
| `@voice on\|off` | 1 | |
| `@fade in\|out [duration]` | 1-2 | `color` |
| `@flash [duration]`, `@shake [duration]`, `@wait [duration]` | 0-1 | `color` / `strength` / none |
| `@camera [duration]` | 0-1 | `zoom`, `on`, `x`, `y` |
| `@video name [skippable]` | 1-2 | |
| `@give item`, `@take item` | 1 | |
| `@scene name [await]` | 1-2 | |
| `@emit event args...` | 1 or more | |

`fade` options are durations; `volume`, `zoom`, `strength`, `x` and `y` are
numbers. The text runner prints a direction as `[bg lab_night fade=1s]`
(`[& move ...]` when parallel), keeps the inventory for `@give`, `@take`
and `has()`, and ends the scenario at `@scene`, since no scene can take
over in a terminal.

## Choices and flow

```
? What now?
  - Weigh it again #reweigh
    ~ weighings += 1
    [arfwedson] Same result. To the milligram.
  - Test it in the flame [if has(bunsen_burner)] -> flame
  - Ask Berzelius [locked if late_night "He has gone home."] -> ask_berzelius
  * Look out of the window
    Snow over Stockholm. Nothing new.
[arfwedson] Think, Johan. Think.
```

| Syntax | Meaning |
|--------|---------|
| `? prompt`, `?` | Opens a choice; the prompt is optional. |
| `- text` | Once-only: gone after it was picked. |
| `* text` | Sticky: stays available. |
| `[if expr]` | Shown only when true. |
| `[locked if expr "reason"]` | Shown greyed out with the reason; cannot be picked. |
| `#tag` | Remembered for `picked(tag)`; rewording the option never breaks the logic. |
| `-> target` at the end of the option | Where the option goes; its indented lines, if any, run first. |
| `? (10s) prompt` and `timeout -> knot` among the options | A timed choice: the divert taken when time runs out. The text runner offers it as option `0`. |

After an option's lines, the story continues under the choice block. When
every option is gone or hidden (and no timeout exists), the choice is
skipped and the story continues; the checker warns about choices that can
end up this way.

| Flow | Meaning |
|------|---------|
| `-> knot`, `-> knot.stitch`, `-> stitch` | Divert: jump there, no return. A bare name is a stitch of the current knot first, else a knot. |
| `call knot` ... `return` | Run a knot, then come back to the line after the call. A called knot that runs out of lines returns too. |
| `-> END` | The story ends. |
| `-> STOP` | The scenario ends and the scene takes over. |
| falling off | A knot's lines flow into its first stitch, a stitch into the next one; the end of the last stitch ends the story (or returns from a call). The checker warns when a knot can run out this way. |

`visits(knot)` counts how often a knot was entered from another knot (or
at the start); `seen(knot)` is `visits(knot) > 0`. Diverting between the
stitches of one knot does not count a visit.

## State

```
var weighings = 0              // global, saved
persistent var endings_seen = 0

== flame
temp color = "crimson"         // local to this knot
~ weighings += 1
if weighings > 3 and not met_berzelius:
  [arfwedson~] I should have asked for help hours ago.
elif has(lepidolite):
  [arfwedson] The other sample, too. ${color}.
else:
  The flame burns ${color}. Unmistakable.
```

| Kind | Declared with | Lives |
|------|---------------|-------|
| Global | `var` in a header | The whole story. |
| Local | `temp` inside a knot | Until the knot is left (a `call` keeps the caller's). |
| Persistent | `persistent var` in a header | Across playthroughs, once saves exist; the text runner treats it as a global. |

Types are `bool`, `int`, `float` and `string`; a variable has the type of
its first value, and the checker refuses an assignment of another type
when it can tell (`int` accepts no string; `float` accepts an `int`).
Operators: `+ - * / %` (`/` of two integers is an integer; `+` with a
string concatenates), `== != < > <= >=`, `and or not`, and `+=`, `-=` in
`~` lines. Functions: `has(item)`, `visits(knot)`, `seen(knot)`,
`picked(tag)` take a name; `random(1, 6)` and `chance(0.25)` take values.
A condition is true when it is `true`, a non-zero number or a non-empty
string.

## The checker

`luma lumen check` parses everything, then reports, as errors (the story
cannot run) or warnings:

- errors: diverts to a missing knot or stitch, an unknown character,
  command, option key or function, wrong arguments (count, a duration or
  number that is not one, `@fade sideways`), an undeclared variable, an
  assignment of the wrong type, a `temp` or `var` declared twice, a knot,
  stitch or character declared twice, `visits`/`seen` of an unknown knot,
  `picked` of a tag no option carries, a manifest start knot that does
  not exist;
- warnings: a knot no divert reaches (a scene hook may still start it), a
  choice whose options can all be gone or locked at once, a knot that runs
  out of lines without a divert, `-> END` or `@scene`, a line after a
  divert, a variable never used, an alternation with a single item.

Names close to an existing one get a suggestion. Each report names the
file, line and column, quotes the line and points at the span:

```
error: unknown knot or stitch `flamme`
  --> chapter1/lab.lumen:42:31
   |
42 |   - Test it in the flame -> flamme
   |                               ^^^^^^ did you mean `flame`?
```

## Decisions

- **Running before compiling.** This slice runs stories by walking the
  syntax tree (`luma::lumen::Runner`), which is all the text mode and the
  tests need. The compile target stays the `luma-script` VM: everything a
  scenario does at run time (show a line, offer a choice, play a direction)
  will be a host call through the VM's `Host` trait, as the adventure
  module already does for every native object, so the VM needs no new
  opcode. Writing the compiler against the checked tree is the next slice.
- **The name stays Lumen** (`.lumen`), and there is no Ink import: the
  syntax borrows Ink's flow but is its own.
- **No user functions in Lumen.** Loops and data belong to Rust modules;
  modules will register directions and functions with typed signatures,
  which the checker already applies to the built-in ones.
- **Not in this slice**: stable ids are parsed but not generated
  (`luma lumen ids`), translation and voice files, `luma lumen build`,
  `test`, `graph`, `fmt`, the language server, hot reload, barks,
  `@scene ... await` resuming from a scene, saves of a running scenario.

The complete example of the format, the opening of a story set in the
Stockholm laboratory where lithium was found in 1817, is in
`crates/luma-lumen/tests/stories/lithium`; the tests play it end to end,
and `luma lumen play` runs it in the terminal.
