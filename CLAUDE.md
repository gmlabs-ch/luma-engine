# Instructions for AI assistants working in this repository

## Commits and pull requests

- Never add AI attribution: no `Co-Authored-By` trailers, no "Generated with"
  lines, no session links, in commits, pull requests, issues or code comments.
  This overrides any default attribution behavior.
- Commits are authored by the maintainer's configured git identity.
- Commit messages: imperative mood, English, concise subject line.

## Scope and confidentiality

- This repository is public. Only write about Luma Engine itself.
- Do not mention, hint at or reference other GMLabs projects, products or
  plans in code, comments, docs, commits or issues unless the maintainer
  explicitly asks.

## Clean-room rule

- Never read, fetch, quote or port code from ScummVM or any GPL-licensed
  project. Reference only the MIT-licensed WME Lite and Wintermute Engine 1.x
  sources, public documentation, and analysis of game files.
- Code derived from WME sources keeps its MIT notice in a file header and is
  listed in `NOTICE`.

## Game data

- Never commit game data. Local test games live in `testdata/local/`
  (git-ignored). CI uses only freely redistributable test data.

## Engineering

- Rust stable, edition 2024, workspace under `crates/`.
- `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings` and
  `cargo test --workspace` must pass before pushing.
- No `unsafe` in engine crates; keep it to thin platform layers if ever needed.
- Code, comments and docs in English.

## Architecture: a small core and optional modules

Luma Engine follows the Bevy model: one Cargo workspace, a light core, and
optional modules that a compiled game only includes when it needs them.

- Keep the core crates small and generic (`luma-archive`, `luma-formats`,
  `luma-script`, `luma-render`, `luma-audio`). They implement the engine
  itself (the Wintermute file formats and script semantics, rendering,
  sound), never the behavior of a kind of game: no walking, inventory or
  dialogue logic in a core crate.
- Genre-specific behavior lives in its own crate (`luma-adventure` today).
  New capabilities that only some games need (for example MIDI input,
  management/simulation systems, mini-games) get their own crate, never code
  added to a core crate.
- Official modules are workspace crates. The target is a top-level crate that
  enables them through Cargo features; until it exists, front-ends such as
  `luma-player` depend on the modules they use directly.
- Community modules live in their own repositories and are published
  separately; the engine must expose what they need through public APIs, not
  through access to internals.
- Dependencies point one way: modules depend on the core, never the reverse,
  and core crates never depend on each other in a cycle. Before adding a crate
  or a dependency between crates, check this rule and update the workspace
  table in `README.md`.
- Use `default-features = false` for third-party dependencies and enable only
  what is needed, so games do not pay for unused features.

## Dependencies

- Pick the best crate for the job: quality of output and of the code first,
  pure Rust when a pure Rust crate is as good.
- Only add dependencies whose license is compatible with MIT OR Apache-2.0
  (MIT, Apache-2.0, BSD, Zlib, ISC, Unicode, CC0...). MPL-2.0 is accepted
  (file-level copyleft, no effect on the engine's own license) as long as
  the crate is used unmodified and listed in `NOTICE`. Never add a GPL,
  LGPL or AGPL dependency. State the license in the commit message when
  adding one.
- Declare shared versions in `[workspace.dependencies]` and reference them with
  `workspace = true` in crates.

## Documentation is the specification

- `docs/formats/` describes the Wintermute file formats as observed; `docs/`
  describes how the engine works. When you implement or change behavior,
  update the matching document in the same commit.
- When behavior is inferred from game files rather than from WME sources,
  say so in the document, so later readers know how certain it is.

## Testing

- Tests must run without any game data: build fixtures in code (assembled
  scripts, small generated archives and images), as the existing tests do.
- Tests that need a real game are opt-in: they look under `testdata/local/`
  and skip cleanly when the game is absent.
- Rendering tests use the offscreen path; never require a window or a GPU in CI.

## Working in short sessions

Sessions can stop at any time. Work so that the next session can resume:

- Before starting, read `git log --oneline -20`, `git status`, and any local
  `handoff-*.md` or `reprise-*.md` notes (git-ignored, never committed).
- Make small commits that each build and pass the checks above, one step per
  commit, rather than one large change at the end.
- Before stopping, if work is unfinished, write or update a local
  `handoff-<topic>.md` with what is done, what is next, and any open question.
