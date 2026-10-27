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
