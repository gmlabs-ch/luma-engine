# Contributing to Luma Engine

Thank you for your interest. Luma Engine is maintained by a very small team,
so please keep the following in mind.

## Before you start

- **Issues are welcome**: bug reports, game compatibility reports, questions.
- **Discuss before coding**: open an issue describing the change you want to
  make before sending a pull request. Unannounced pull requests may be closed.

## The clean-room rule

Luma Engine is dual-licensed under MIT and Apache-2.0. To keep it that way:

- **Do not read, copy or port code from ScummVM** (including its Wintermute
  engine) or from any other GPL-licensed project. This applies to looking at
  that code while working on Luma Engine, not only to copying it.
- **Allowed references**: the MIT-licensed sources of
  [WME Lite](https://github.com/MnemonicWME/wmelite) and
  [Wintermute Engine 1.x](https://github.com/MnemonicWME/wintermute-engine),
  public documentation, and your own analysis of game files.
- Code derived from WME sources must keep the original MIT copyright notice in
  a file header, and be listed in [NOTICE](NOTICE).

## Game data

Never commit game data to this repository, even freeware. Put local test games
under `testdata/local/`, which is ignored by git.

## Code style

- Run `cargo fmt --all` and `cargo clippy --all-targets -- -D warnings`
  before pushing; CI enforces both.
- Document public items. Keep `unsafe` out of engine crates.
- Code, comments, commit messages and documentation are written in English.

## License of contributions

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed under MIT and Apache-2.0, without any
additional terms or conditions.
