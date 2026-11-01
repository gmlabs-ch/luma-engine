//! Plays OpenQuest, the CC BY test game in `testdata/openquest`, from the
//! opening to the ending through the headless player.
//!
//! The game is played as a chain of runs: each one loads the slot the
//! previous one saved, acts, and saves the next slot, as a player would over
//! several sessions; saving and loading are thus exercised at every step.
//! The steps are the game's solution. The coordinates are screen positions
//! at 320x200: the verb bar is at y=134 (Pick up 101,163; Talk to 101,176;
//! Examine 144,163; Give 144,176; Use 144,189; Open 187,163; Smell 187,176;
//! Move 187,189), the inventory box at (211,153) with a 28x18 slot per taken
//! item, and the dialogue rows are bottom-aligned 16 px apart (y=190, 174,
//! 158, 142). A click during a sentence is lost, as the game is not
//! interactive then, so every step waits for its sentences to end. Zif
//! wanders, so he is clicked where he stands (`--click-on`).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The game, copied to a temporary directory so that the saved games the
/// walkthrough writes stay out of the repository.
struct Game {
    dir: PathBuf,
    /// The slot the last run saved; 0 before the first run.
    slot: u32,
}

impl Game {
    fn copy() -> Game {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/openquest");
        let dir = std::env::temp_dir().join(format!("luma-openquest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(source.join("data.dcp"), dir.join("data.dcp")).unwrap();
        Game { dir, slot: 0 }
    }

    /// Runs the player on the game: loads the slot the previous run saved
    /// (none the first time), runs the steps ten frames apart, saves the
    /// next slot and returns what the player printed. Fails on a player
    /// error, a fatal script error or a save that did not happen.
    fn play(&mut self, steps: &[&str]) -> String {
        let mut args: Vec<String> = vec![
            self.dir.to_str().unwrap().to_string(),
            "--headless".into(),
            "--mute".into(),
            "--ticks".into(),
            "4".into(),
            "--after".into(),
            "10".into(),
        ];
        if self.slot > 0 {
            args.extend([
                "--load".into(),
                self.slot.to_string(),
                "--wait".into(),
                "20".into(),
            ]);
        }
        args.extend(steps.iter().map(|s| s.to_string()));
        self.slot += 1;
        args.extend(["--save".into(), self.slot.to_string()]);
        let out = Command::new(env!("CARGO_BIN_EXE_luma-player"))
            .args(&args)
            .output()
            .expect("run luma-player");
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "step {}: {stderr}\n{stdout}",
            self.slot
        );
        assert!(!stdout.contains("Fatal"), "step {}: {stdout}", self.slot);
        assert!(
            stdout.contains("] saved to "),
            "step {}: {stdout}",
            self.slot
        );
        stdout
    }
}

/// The inventory line of a run's output.
fn inventory(out: &str) -> &str {
    out.lines()
        .find(|l| l.starts_with("inventory:"))
        .unwrap_or_else(|| panic!("no inventory line in {out}"))
}

/// The response box's texts at the end of a run, or an empty string.
fn responses(out: &str) -> &str {
    out.lines()
        .find(|l| l.starts_with("responses:"))
        .unwrap_or("")
}

fn loaded(out: &str, scene: &str) -> bool {
    out.contains(&format!("loaded from '{scene}'"))
}

#[test]
fn plays_openquest_from_the_opening_to_the_ending() {
    let mut game = Game::copy();

    // New Game on the title screen, then the opening cut-scene (about a
    // minute) ends in the first room, with the scanner in the inventory.
    let out = game.play(&["--click-at", "240,70", "--wait", "3700"]);
    assert!(loaded(&out, r"scenes\opening\opening.scene"), "{out}");
    assert!(loaded(&out, r"scenes\room\room.scene"), "{out}");
    assert!(inventory(&out).contains(r#"1 taken ["scanner"]"#), "{out}");

    // Examine the plant: there is ammunition in it.
    game.play(&[
        "--click-at",
        "144,163",
        "--click-at",
        "120,70",
        "--wait",
        "400",
    ]);
    // Talk to Carol: ask her designation, then permission to move the plant.
    let out = game.play(&[
        "--click-at",
        "101,176",
        "--click-at",
        "70,92",
        "--wait",
        "700",
    ]);
    assert!(responses(&out).contains("May I move this plant?"), "{out}");
    game.play(&["--click-at", "60,142", "--wait", "900"]);
    let out = game.play(&["--click-at", "60,174", "--wait", "900"]);
    assert!(!responses(&out).contains("May I move this plant?"), "{out}");
    game.play(&["--click-at", "60,190", "--wait", "300"]);
    // Move the plant, pick up the ammunition, open the cabinet.
    game.play(&[
        "--click-at",
        "187,189",
        "--click-at",
        "120,70",
        "--wait",
        "500",
    ]);
    let out = game.play(&[
        "--click-at",
        "101,163",
        "--click-at",
        "115,91",
        "--wait",
        "300",
    ]);
    assert!(
        inventory(&out).contains(r#"["scanner", "ammunition"]"#),
        "{out}"
    );
    let out = game.play(&[
        "--click-at",
        "187,163",
        "--click-at",
        "156,98",
        "--wait",
        "1200",
    ]);
    assert!(
        inventory(&out).contains(r#"["scanner", "ammunition", "gun", "keycard"]"#),
        "{out}"
    );
    // Use the ammunition with the gun: the ammunition is gone.
    let out = game.play(&[
        "--click-at",
        "144,189",
        "--click-at",
        "254,162",
        "--click-at",
        "282,162",
        "--wait",
        "400",
    ]);
    assert!(
        inventory(&out).contains(r#"3 taken ["scanner", "gun", "keycard"]"#),
        "{out}"
    );
    // Smell the door plate, then try it: the two plates are connected.
    game.play(&[
        "--click-at",
        "187,176",
        "--click-at",
        "249,70",
        "--wait",
        "700",
    ]);
    let out = game.play(&[
        "--click-at",
        "144,189",
        "--click-at",
        "249,70",
        "--wait",
        "500",
        "--global",
        "CanAskZif",
    ]);
    assert!(out.contains("global CanAskZif: Bool(true)"), "{out}");
    // Ask Zif, wherever he wandered, to help with the mechanism.
    let out = game.play(&[
        "--click-at",
        "101,176",
        "--click-on",
        "zif_aimless",
        "--wait",
        "700",
    ]);
    assert!(
        responses(&out).contains("I require your assistance"),
        "{out}"
    );
    game.play(&["--click-at", "60,174", "--wait", "1000"]);
    // Use the plate again: Zif presses the other one, the door opens.
    game.play(&[
        "--click-at",
        "144,189",
        "--click-at",
        "249,70",
        "--wait",
        "2500",
    ]);
    let out = game.play(&["--click-at", "272,80", "--wait", "1500"]);
    assert!(loaded(&out, r"scenes\room2\room2.scene"), "{out}");

    // The keycard in its slot, then the terminal: the triangle puzzle.
    game.play(&[
        "--click-at",
        "144,189",
        "--click-at",
        "282,162",
        "--click-at",
        "182,87",
        "--wait",
        "500",
    ]);
    let out = game.play(&[
        "--click-at",
        "144,189",
        "--click-at",
        "182,72",
        "--wait",
        "600",
    ]);
    assert!(loaded(&out, r"scenes\triangle\triangle.scene"), "{out}");
    // Each piece follows the mouse and snaps when released over its place.
    for piece in [
        "61,100:261,109",
        "58,32:205,88",
        "130,38:285,113",
        "100,26:216,117",
        "22,92:243,109",
        "94,56:282,122",
        "100,86:181,110",
        "160,32:241,42",
        "124,80:228,59",
        "142,56:256,65",
        "22,44:212,70",
        "28,26:228,84",
    ] {
        game.play(&["--drag", piece, "--wait", "30"]);
    }
    let out = game.play(&["--click-at", "76,125", "--wait", "2500"]);
    assert!(loaded(&out, r"scenes\room2\room2.scene"), "{out}");
    // Shoot the node (a miss), then give the gun to Zif: the ending, and
    // the title screen again.
    game.play(&[
        "--click-at",
        "144,189",
        "--click-at",
        "254,162",
        "--click-at",
        "158,18",
        "--wait",
        "900",
    ]);
    let out = game.play(&[
        "--click-at",
        "144,176",
        "--click-at",
        "254,162",
        "--click-at",
        "92,75",
        "--wait",
        "6000",
    ]);
    assert!(loaded(&out, r"scenes\title\title.scene"), "{out}");

    std::fs::remove_dir_all(&game.dir).unwrap();
}
