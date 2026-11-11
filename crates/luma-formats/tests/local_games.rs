//! Loads every definition file of the local test games with the loaders'
//! own rules: a key the original engine does not know is a syntax error
//! there, so a game that ran on the original must load whole here.
//!
//! The games are not in the repository: the test looks for unpacked
//! copies under `testdata/local/` and skips when there is none.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use luma_formats::{
    Actor, Document, Entity, Files, FontFile, GameFile, InventoryBoxDef, ItemDef, ResponseBoxDef,
    Scene, Sprite, SpriteSet, TiledImage, WindowDef,
};

/// Every file under `root`, by lower-cased path relative to it with `\`
/// separators, as the games name their files.
fn index(root: &Path) -> HashMap<String, PathBuf> {
    let mut files = HashMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('/', "\\")
                    .to_ascii_lowercase();
                files.insert(name, path);
            }
        }
    }
    files
}

fn check_game(root: &Path) -> (usize, Vec<String>) {
    let files = index(root);
    let mut read = |name: &str| {
        let key = name.replace('/', "\\").to_ascii_lowercase();
        files.get(&key).and_then(|path| std::fs::read(path).ok())
    };
    let mut count = 0;
    let mut failures = Vec::new();
    let mut names: Vec<&String> = files.keys().collect();
    names.sort();
    for name in names {
        let Some(ext) = name.rsplit('.').next() else {
            continue;
        };
        let bytes = std::fs::read(&files[name]).unwrap();
        let mut files = Files::new(&mut read);
        let result = match ext {
            "scene" => Scene::parse_with(&bytes, &mut files).map(|_| ()),
            "sprite" => Sprite::parse(&bytes).map(|_| ()),
            "entity" => Entity::parse_with(&bytes, &mut files).map(|_| ()),
            "actor" => Actor::parse_with(&bytes, &mut files).map(|_| ()),
            "spriteset" => SpriteSet::parse_with(&bytes, &mut files).map(|_| ()),
            "window" => WindowDef::parse_with(&bytes, &mut files).map(|_| ()),
            "tiled" | "image" => TiledImage::parse_with(&bytes, &mut files).map(|_| ()),
            "font" => FontFile::parse(&bytes).map(|_| ()),
            "game" => GameFile::parse_with(&bytes, &mut files).map(|_| ()),
            "items" => match ItemDef::parse_all_with(&bytes, &mut files) {
                (_, Some(err)) => Err(err),
                (_, None) => Ok(()),
            },
            "def" => {
                let doc = Document::parse(&bytes).unwrap();
                match doc.nodes.first().map(|n| n.key.to_ascii_uppercase()) {
                    Some(key) if key == "INVENTORY_BOX" => {
                        InventoryBoxDef::parse_with(&bytes, &mut files).map(|_| ())
                    }
                    Some(key) if key == "RESPONSE_BOX" => {
                        ResponseBoxDef::parse_with(&bytes, &mut files).map(|_| ())
                    }
                    _ => continue,
                }
            }
            _ => continue,
        };
        count += 1;
        if let Err(err) = result {
            failures.push(format!("{name}: {err}"));
        }
    }
    (count, failures)
}

#[test]
#[ignore = "needs the local test games in testdata/local"]
fn every_local_definition_loads_with_the_original_rules() {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/local");
    let mut checked = 0;
    for game in ["white-chamber/unpacked", "openquest/unpacked"] {
        let root = local.join(game);
        if !root.is_dir() {
            eprintln!("skipped: {} is absent", root.display());
            continue;
        }
        let (count, failures) = check_game(&root);
        eprintln!("{game}: {count} definition files");
        assert!(failures.is_empty(), "{game}:\n{}", failures.join("\n"));
        checked += count;
    }
    eprintln!("{checked} definition files loaded");
}
