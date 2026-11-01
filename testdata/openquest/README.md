# OpenQuest (Wintermute port): the test game

A two-room adventure used by the test suite and CI. It is redistributed
here under the [Creative Commons Attribution 4.0 International](LICENSE)
licence, which allows that.

- **Authors**: OpenQuest by Michael Sheail (2007, for Adventure Game
  Studio); Wintermute port by Jenni McMurray, published at
  <https://jennibee.itch.io/openquest-wintermute>.
- **What it covers**: a SCUMM-style verb bar built from entity containers,
  an inventory box, a response box for dialogues, item combinations,
  cut-scenes, a drag-and-drop puzzle, saving and loading, looping entity
  animations.
- **Changes from the published game** (allowed by the licence, and marked
  here as it requires): `data.dcp` is repacked with `luma pack` from the
  original package's files, minus the Wintermute runtime (`game.exe`,
  `settings.exe`) and with the four music tracks under `music/` replaced
  by 100 ms of silence, which cuts the package from 82 MB to about 3 MB.
  Everything else is byte for byte the original.

Play it with `luma-player testdata/openquest`. The walkthrough test in
`crates/luma-player/tests/openquest.rs` plays it from the opening to the
ending through the headless player and documents the solution. The package
is reproducible: unpack the original with `luma unpack`, delete `music/`,
put the silent files back and run `luma pack` on the directory.
