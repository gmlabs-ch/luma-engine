//! The game definition (`default.game`): the game's name, its scripts,
//! fonts, cursors and interface files.
//!
//! Written with reference to `BGame.cpp` and `AdGame.cpp` of the
//! Wintermute Engine (MIT License, Copyright (c) 2011 Jan Nedoma); see
//! `NOTICE`.

use crate::def::{Document, Node, document_keys, keys};
use crate::error::Result;
use crate::image::Rgba;
use crate::scene::Property;
use crate::sprite::Rect;
use crate::template::Files;

/// The `GAME` and `AD_GAME` blocks of `default.game`.
#[derive(Debug, Clone, PartialEq)]
pub struct GameFile {
    /// `NAME`: the game's name, also its script `Name`.
    pub name: String,
    /// `CAPTION`: the window title.
    pub caption: String,
    /// `SCRIPT`: scripts attached to the game object, in order.
    pub scripts: Vec<String>,
    /// The last `TEMPLATE` file named; it was applied where the key
    /// stands.
    pub template: Option<String>,
    /// The last `TEMPLATE` file that was read: the file name the game
    /// then answers (`Filename`). `None` without a template.
    pub filename: Option<String>,
    /// `SYSTEM_FONT`: the font of system messages.
    pub system_font: Option<String>,
    /// `VIDEO_FONT`: the font of video subtitles.
    pub video_font: Option<String>,
    /// `CURSOR`: the mouse cursor sprite.
    pub cursor: Option<String>,
    /// `ACTIVE_CURSOR`: the cursor over an interactive object.
    pub active_cursor: Option<String>,
    /// `NONINTERACTIVE_CURSOR`: the cursor while scripts run cut-scenes.
    pub noninteractive_cursor: Option<String>,
    /// `SUBTITLES`: whether speech is subtitled.
    pub subtitles: bool,
    /// `SUBTITLES_SPEED`: milliseconds of subtitle per character.
    pub subtitles_speed: i32,
    /// `VIDEO_SUBTITLES`: whether videos are subtitled.
    pub video_subtitles: bool,
    /// `PERSONAL_SAVEGAMES`: saved games per user profile.
    pub personal_savegames: bool,
    /// `LOCAL_SAVE_DIR`: where saved games go.
    pub local_save_dir: Option<String>,
    /// `THUMBNAIL_WIDTH`: width of the thumbnail stored in saved games;
    /// `0` (the default) for none.
    pub thumbnail_width: i32,
    /// `THUMBNAIL_HEIGHT`: see `thumbnail_width`.
    pub thumbnail_height: i32,
    /// `SAVE_IMAGE`: the image shown while the game saves.
    pub save_image: Option<String>,
    /// `SAVE_IMAGE_X`, `SAVE_IMAGE_Y`: where it is drawn.
    pub save_image_x: i32,
    /// See `save_image_x`.
    pub save_image_y: i32,
    /// `LOAD_IMAGE`: the image shown while a saved game loads.
    pub load_image: Option<String>,
    /// `LOAD_IMAGE_X`, `LOAD_IMAGE_Y`: where it is drawn.
    pub load_image_x: i32,
    /// See `load_image_x`.
    pub load_image_y: i32,
    /// `PROPERTY` blocks: script properties of the game object.
    pub properties: Vec<Property>,
    /// `AD_GAME`'s `RESPONSE_BOX`: the dialogue choices definition.
    pub response_box: Option<String>,
    /// `AD_GAME`'s `INVENTORY_BOX`: the inventory definition.
    pub inventory_box: Option<String>,
    /// `AD_GAME`'s `ITEMS`: the inventory items definition.
    pub items: Option<String>,
    /// `AD_GAME`'s `SCENE_VIEWPORT { left, top, right, bottom }`: the
    /// part of the screen the scene is shown in, as written (the engine
    /// then keeps it inside the screen); the whole screen when absent.
    pub scene_viewport: Option<Rect>,
    /// `AD_GAME`'s `TALK_SKIP_BUTTON`: `right`, `both`, anything else is
    /// the left button.
    pub talk_skip_button: Option<String>,
    /// `AD_GAME`'s `VIDEO_SKIP_BUTTON`: `right`, `both`, anything else is
    /// the left button.
    pub video_skip_button: Option<String>,
    /// `AD_GAME`'s `STARTUP_SCENE`: a scene name for the game's scripts
    /// (`Game.StartupScene`); the engine itself never loads it.
    pub startup_scene: Option<String>,
    /// `AD_GAME`'s `DEBUG_STARTUP_SCENE`: in debug mode, the scene the
    /// first `ChangeScene` loads instead of the one asked for.
    pub debug_startup_scene: Option<String>,
    /// `SHADOW_IMAGE`: the image of the 3D actors' simple shadows; the
    /// built-in `shadow.png` when absent.
    pub shadow_image: Option<String>,
    /// `INDICATOR_X`: left edge of the save and load progress bar; `-1`
    /// (the default) for the left edge of the screen.
    pub indicator_x: i32,
    /// `INDICATOR_Y`: top of the progress bar; `-1` (the default) for the
    /// bottom of the screen.
    pub indicator_y: i32,
    /// `INDICATOR_WIDTH`: full width of the progress bar; `-1` (the
    /// default) for the width of the screen.
    pub indicator_width: i32,
    /// `INDICATOR_HEIGHT`: height of the progress bar, 8 by default; 0 or
    /// less hides it.
    pub indicator_height: i32,
    /// `INDICATOR_COLOR { r, g, b, a }`: colour of the progress bar,
    /// translucent red by default.
    pub indicator_color: Rgba,
    /// `COMPAT_KILL_METHOD_THREADS`: scripts waiting for an object are
    /// not reset when the object's scripts are finished (the behaviour
    /// before version 1.9).
    pub compat_kill_method_threads: bool,
}

impl Default for GameFile {
    fn default() -> Self {
        GameFile {
            name: String::new(),
            caption: String::new(),
            scripts: Vec::new(),
            template: None,
            filename: None,
            system_font: None,
            video_font: None,
            cursor: None,
            active_cursor: None,
            noninteractive_cursor: None,
            subtitles: true,
            subtitles_speed: 70,
            video_subtitles: true,
            personal_savegames: false,
            local_save_dir: None,
            thumbnail_width: 0,
            thumbnail_height: 0,
            save_image: None,
            save_image_x: 0,
            save_image_y: 0,
            load_image: None,
            load_image_x: 0,
            load_image_y: 0,
            properties: Vec::new(),
            response_box: None,
            inventory_box: None,
            items: None,
            scene_viewport: None,
            talk_skip_button: None,
            video_skip_button: None,
            startup_scene: None,
            debug_startup_scene: None,
            shadow_image: None,
            indicator_x: -1,
            indicator_y: -1,
            indicator_width: -1,
            indicator_height: 8,
            indicator_color: Rgba {
                r: 255,
                g: 0,
                b: 0,
                a: 128,
            },
            compat_kill_method_threads: false,
        }
    }
}

/// The keys of `CBGame::LoadBuffer`, in the order of its table.
const GAME_KEYS: &[&str] = &[
    "GAME",
    "TEMPLATE",
    "NAME",
    "SYSTEM_FONT",
    "VIDEO_FONT",
    "EVENTS",
    "CURSOR",
    "ACTIVE_CURSOR",
    "NONINTERACTIVE_CURSOR",
    "PERSONAL_SAVEGAMES",
    "SCRIPT",
    "CAPTION",
    "PROPERTY",
    "SUBTITLES_SPEED",
    "SUBTITLES",
    "VIDEO_SUBTITLES",
    "EDITOR_PROPERTY",
    "THUMBNAIL_WIDTH",
    "THUMBNAIL_HEIGHT",
    "INDICATOR_X",
    "INDICATOR_Y",
    "INDICATOR_WIDTH",
    "INDICATOR_HEIGHT",
    "INDICATOR_COLOR",
    "SAVE_IMAGE_X",
    "SAVE_IMAGE_Y",
    "SAVE_IMAGE",
    "LOAD_IMAGE_X",
    "LOAD_IMAGE_Y",
    "LOAD_IMAGE",
    "SHADOW_IMAGE",
    "LOCAL_SAVE_DIR",
    "COMPAT_KILL_METHOD_THREADS",
];

/// The keys of `CAdGame::LoadBuffer`, in the order of its table: the top
/// level of the file and the `AD_GAME` block are read with the same one.
const AD_GAME_KEYS: &[&str] = &[
    "GAME",
    "AD_GAME",
    "RESPONSE_BOX",
    "INVENTORY_BOX",
    "ITEMS",
    "TALK_SKIP_BUTTON",
    "VIDEO_SKIP_BUTTON",
    "SCENE_VIEWPORT",
    "EDITOR_PROPERTY",
    "STARTUP_SCENE",
    "DEBUG_STARTUP_SCENE",
];

impl GameFile {
    /// Parses a `default.game` file. No other file is read: a `TEMPLATE`
    /// key fails to load, which the original ignores.
    pub fn parse(bytes: &[u8]) -> Result<GameFile> {
        GameFile::parse_with(bytes, &mut Files::none())
    }

    /// Parses a `default.game` file, reading the `TEMPLATE` files it
    /// names through `files`.
    pub fn parse_with(bytes: &[u8], files: &mut Files<'_>) -> Result<GameFile> {
        GameFile::from_document_with(&Document::parse(bytes)?, files)
    }

    /// Builds the definition from a parsed document.
    pub fn from_document(doc: &Document) -> Result<GameFile> {
        GameFile::from_document_with(doc, &mut Files::none())
    }

    /// Builds the definition from a parsed document, reading the
    /// `TEMPLATE` files it names through `files`.
    pub fn from_document_with(doc: &Document, files: &mut Files<'_>) -> Result<GameFile> {
        let mut game = GameFile::default();
        game.load(doc, files)?;
        Ok(game)
    }

    /// `CAdGame::LoadBuffer`: every top-level statement in order. `GAME`
    /// blocks are read by `CBGame::LoadBuffer`, `AD_GAME` blocks here; any
    /// other key of the table is accepted and ignored, and a key outside
    /// the table is a syntax error that fails the load, at the top level
    /// and in the blocks.
    fn load(&mut self, doc: &Document, files: &mut Files<'_>) -> Result<()> {
        for item in document_keys(doc, AD_GAME_KEYS, "GAME") {
            let (key, node) = item?;
            match key {
                "GAME" => self.load_game(node, files)?,
                "AD_GAME" => {
                    for item in keys(node, AD_GAME_KEYS, "GAME") {
                        let (key, child) = item?;
                        match key {
                            "RESPONSE_BOX" => self.response_box = Some(child.text()),
                            "INVENTORY_BOX" => self.inventory_box = Some(child.text()),
                            "ITEMS" => self.items = Some(child.text()),
                            "SCENE_VIEWPORT" => {
                                let mut rect = self.scene_viewport.unwrap_or_default();
                                child.read_ints(&mut [
                                    &mut rect.left,
                                    &mut rect.top,
                                    &mut rect.right,
                                    &mut rect.bottom,
                                ]);
                                self.scene_viewport = Some(rect);
                            }
                            "TALK_SKIP_BUTTON" => self.talk_skip_button = Some(child.text()),
                            "VIDEO_SKIP_BUTTON" => self.video_skip_button = Some(child.text()),
                            "STARTUP_SCENE" => self.startup_scene = Some(child.text()),
                            "DEBUG_STARTUP_SCENE" => {
                                self.debug_startup_scene = Some(child.text());
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// `CBGame::LoadBuffer`: the keys of a `GAME` block in order.
    fn load_game(&mut self, root: &Node, files: &mut Files<'_>) -> Result<()> {
        for item in keys(root, GAME_KEYS, "GAME") {
            let (key, node) = item?;
            match key {
                "TEMPLATE" => {
                    // `LoadFile` on the game itself: the whole file named
                    // is read here, `AD_GAME` blocks included. A template
                    // that cannot be loaded is skipped (the original's
                    // error code is overwritten by the next statement).
                    let name = node.text();
                    if let Ok(doc) = files.open(&name) {
                        self.filename = Some(name.clone());
                        let _ = files.nested(|files| self.load(&doc, files));
                    }
                    self.template = Some(name);
                }
                "NAME" => self.name = node.text(),
                "CAPTION" => self.caption = node.text(),
                "SCRIPT" => self.scripts.push(node.text()),
                "SYSTEM_FONT" => self.system_font = Some(node.text()),
                "VIDEO_FONT" => self.video_font = Some(node.text()),
                "SHADOW_IMAGE" => self.shadow_image = Some(node.text()),
                "CURSOR" => self.cursor = Some(node.text()),
                "ACTIVE_CURSOR" => self.active_cursor = Some(node.text()),
                "NONINTERACTIVE_CURSOR" => self.noninteractive_cursor = Some(node.text()),
                "SUBTITLES" => node.read_bool(&mut self.subtitles),
                "SUBTITLES_SPEED" => node.read_int(&mut self.subtitles_speed),
                "VIDEO_SUBTITLES" => node.read_bool(&mut self.video_subtitles),
                "PERSONAL_SAVEGAMES" => node.read_bool(&mut self.personal_savegames),
                "LOCAL_SAVE_DIR" => self.local_save_dir = Some(node.text()),
                "THUMBNAIL_WIDTH" => node.read_int(&mut self.thumbnail_width),
                "THUMBNAIL_HEIGHT" => node.read_int(&mut self.thumbnail_height),
                "INDICATOR_X" => node.read_int(&mut self.indicator_x),
                "INDICATOR_Y" => node.read_int(&mut self.indicator_y),
                "INDICATOR_WIDTH" => node.read_int(&mut self.indicator_width),
                "INDICATOR_HEIGHT" => node.read_int(&mut self.indicator_height),
                "INDICATOR_COLOR" => {
                    // The original's four integers are uninitialised; a
                    // short list leaves the others at 0 here.
                    let (mut r, mut g, mut b, mut a) = (0, 0, 0, 0);
                    node.read_ints(&mut [&mut r, &mut g, &mut b, &mut a]);
                    self.indicator_color = Rgba::from_ints(r, g, b, a);
                }
                "SAVE_IMAGE" => self.save_image = Some(node.text()),
                "SAVE_IMAGE_X" => node.read_int(&mut self.save_image_x),
                "SAVE_IMAGE_Y" => node.read_int(&mut self.save_image_y),
                "LOAD_IMAGE" => self.load_image = Some(node.text()),
                "LOAD_IMAGE_X" => node.read_int(&mut self.load_image_x),
                "LOAD_IMAGE_Y" => node.read_int(&mut self.load_image_y),
                "PROPERTY" => self.properties.extend(Property::from_node(node)),
                "COMPAT_KILL_METHOD_THREADS" => {
                    node.read_bool(&mut self.compat_kill_method_threads);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_game_and_ad_game_blocks() {
        let game = GameFile::parse(
            br#"; generated by ProjectMan
GAME {
  NAME="WhiteChamber"
  CAPTION="The White Chamber"
  SCRIPT="scripts\game.script"
  SCRIPT="scripts\extra.script"
  PERSONAL_SAVEGAMES=FALSE
  SYSTEM_FONT="fonts\outline_green.font"
  CURSOR="sprites\system\cur_arrow.sprite"
  ACTIVE_CURSOR="sprites\system\cur_arrow_h.sprite"
  NONINTERACTIVE_CURSOR="sprites\system\cur_wait.sprite"
  SUBTITLES=TRUE
  VIDEO_SUBTITLES=TRUE
  SUBTITLES_SPEED=100
  THUMBNAIL_WIDTH=200
  THUMBNAIL_HEIGHT=150
  SAVE_IMAGE="interface\saving.png"
  SAVE_IMAGE_X=10
  SAVE_IMAGE_Y=20
  LOAD_IMAGE="interface\loading.png"
  PROPERTY { NAME="Difficulty" VALUE="2" }
}
AD_GAME {
  RESPONSE_BOX="interface\responses.def"
  INVENTORY_BOX="interface\inventory.def"
  ITEMS="items\items.items"
  TALK_SKIP_BUTTON="left"
  SCENE_VIEWPORT { 0, 0, 320, 189 }
  STARTUP_SCENE="scenes\intro\intro.scene"
}
"#,
        )
        .unwrap();
        assert_eq!(game.name, "WhiteChamber");
        assert_eq!(game.caption, "The White Chamber");
        assert_eq!(
            game.scripts,
            ["scripts\\game.script", "scripts\\extra.script"]
        );
        assert_eq!(
            game.system_font.as_deref(),
            Some("fonts\\outline_green.font")
        );
        assert_eq!(
            game.cursor.as_deref(),
            Some("sprites\\system\\cur_arrow.sprite")
        );
        assert!(game.subtitles && game.video_subtitles);
        assert_eq!(game.subtitles_speed, 100);
        assert!(!game.personal_savegames);
        assert_eq!((game.thumbnail_width, game.thumbnail_height), (200, 150));
        assert_eq!(game.save_image.as_deref(), Some("interface\\saving.png"));
        assert_eq!((game.save_image_x, game.save_image_y), (10, 20));
        assert_eq!(game.load_image.as_deref(), Some("interface\\loading.png"));
        assert_eq!((game.load_image_x, game.load_image_y), (0, 0));
        assert_eq!(game.properties.len(), 1);
        assert_eq!(game.properties[0].name, "Difficulty");
        assert_eq!(
            game.response_box.as_deref(),
            Some("interface\\responses.def")
        );
        assert_eq!(game.items.as_deref(), Some("items\\items.items"));
        assert_eq!(
            game.scene_viewport,
            Some(Rect {
                left: 0,
                top: 0,
                right: 320,
                bottom: 189
            })
        );
        assert_eq!(game.talk_skip_button.as_deref(), Some("left"));
        assert_eq!(
            game.startup_scene.as_deref(),
            Some("scenes\\intro\\intro.scene")
        );
    }

    #[test]
    fn defaults_and_errors() {
        let game = GameFile::parse(b"GAME { }").unwrap();
        assert_eq!(game, GameFile::default());
        assert!(game.subtitles);
        assert_eq!(game.subtitles_speed, 70);
        assert!(game.video_subtitles, "the original's default is true");
        assert!(GameFile::parse(b"SCENE { }").is_err());
    }

    /// `CBGame::LoadBuffer`, `CAdGame::LoadBuffer`: a key outside the
    /// table is a syntax error, at the top level and in both blocks.
    #[test]
    fn an_unknown_key_fails_the_game_file() {
        // `STRING_TABLE` belongs to `startup.settings`, not to `GAME`.
        assert!(GameFile::parse(b"GAME { STRING_TABLE=\"string.tab\" }").is_err());
        assert!(GameFile::parse(b"GAME { }\nAD_GAME { BOGUS=1 }").is_err());
        assert!(GameFile::parse(b"GAME { }\nBOGUS { }").is_err());
        // The table's other keys are accepted anywhere and ignored.
        let game = GameFile::parse(
            b"STARTUP_SCENE=\"ignored\"\nGAME { EVENTS=\"x\" }\nAD_GAME { GAME=\"x\" }",
        )
        .unwrap();
        assert_eq!(game.startup_scene, None);
    }

    /// Every `GAME` and `AD_GAME` block is read, in order; a `TEMPLATE`
    /// loads a whole game file where it stands.
    #[test]
    fn blocks_and_templates_apply_in_order() {
        let mut read = |name: &str| {
            (name == "base.game").then(|| {
                b"GAME { NAME=\"base\"\n CAPTION=\"Base\"\n SUBTITLES=FALSE }\nAD_GAME { ITEMS=\"base.items\"\n STARTUP_SCENE=\"a.scene\" }"
                    .to_vec()
            })
        };
        let game = GameFile::parse_with(
            b"GAME { CAPTION=\"Before\"\n SUBTITLES_SPEED=5\n TEMPLATE=\"base.game\"\n NAME=\"mine\" }\nAD_GAME { ITEMS=\"my.items\" }\nGAME { VIDEO_SUBTITLES=FALSE }\nAD_GAME { DEBUG_STARTUP_SCENE=\"d.scene\" }",
            &mut Files::new(&mut read),
        )
        .unwrap();
        assert_eq!(
            game.caption, "Base",
            "the template overrides what was before"
        );
        assert_eq!(game.name, "mine", "what follows overrides the template");
        assert_eq!(game.filename.as_deref(), Some("base.game"));
        assert_eq!(game.subtitles_speed, 5);
        assert!(!game.subtitles && !game.video_subtitles);
        assert_eq!(game.items.as_deref(), Some("my.items"));
        assert_eq!(game.startup_scene.as_deref(), Some("a.scene"));
        assert_eq!(game.debug_startup_scene.as_deref(), Some("d.scene"));

        // A template that cannot be read is skipped.
        let game = GameFile::parse(b"GAME { TEMPLATE=\"none.game\"\n NAME=\"n\" }").unwrap();
        assert_eq!(game.name, "n");
    }

    #[test]
    fn reads_the_remaining_game_keys() {
        let game = GameFile::parse(
            b"GAME {\n SHADOW_IMAGE=\"s.png\"\n INDICATOR_X=1\n INDICATOR_Y=2\n INDICATOR_WIDTH=300\n INDICATOR_HEIGHT=4\n INDICATOR_COLOR { 1,2,3,4 }\n COMPAT_KILL_METHOD_THREADS=TRUE\n}\nAD_GAME { SCENE_VIEWPORT { 10, 20, 310, 180 } }",
        )
        .unwrap();
        assert_eq!(game.shadow_image.as_deref(), Some("s.png"));
        assert_eq!(
            (
                game.indicator_x,
                game.indicator_y,
                game.indicator_width,
                game.indicator_height
            ),
            (1, 2, 300, 4)
        );
        assert_eq!(
            game.indicator_color,
            Rgba {
                r: 1,
                g: 2,
                b: 3,
                a: 4
            }
        );
        assert!(game.compat_kill_method_threads);
        let rect = game.scene_viewport.unwrap();
        assert_eq!(
            (rect.left, rect.top, rect.right, rect.bottom),
            (10, 20, 310, 180)
        );
        let defaults = GameFile::default();
        assert_eq!(
            (
                defaults.indicator_x,
                defaults.indicator_y,
                defaults.indicator_width,
                defaults.indicator_height
            ),
            (-1, -1, -1, 8)
        );
    }
}
