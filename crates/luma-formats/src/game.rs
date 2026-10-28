//! The game definition (`default.game`): the game's name, its scripts,
//! fonts, cursors and interface files.
//!
//! Written with reference to `BGame.cpp` and `AdGame.cpp` of WME Lite
//! (MIT License, Copyright (c) 2011 Jan Nedoma); see `NOTICE`.

use crate::def::{Document, Node, Nodes};
use crate::error::{Error, Result};
use crate::scene::Property;

/// The `GAME` and `AD_GAME` blocks of `default.game`.
#[derive(Debug, Clone, PartialEq)]
pub struct GameFile {
    /// `NAME`: the game's name, also its script `Name`.
    pub name: String,
    /// `CAPTION`: the window title.
    pub caption: String,
    /// `SCRIPT`: scripts attached to the game object, in order.
    pub scripts: Vec<String>,
    /// `STRING_TABLE`: a string table loaded at startup.
    pub string_table: Option<String>,
    /// `SYSTEM_FONT`: the font of system messages.
    pub system_font: Option<String>,
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
    /// `PROPERTY` blocks: script properties of the game object.
    pub properties: Vec<Property>,
    /// `AD_GAME`'s `RESPONSE_BOX`: the dialogue choices definition.
    pub response_box: Option<String>,
    /// `AD_GAME`'s `INVENTORY_BOX`: the inventory definition.
    pub inventory_box: Option<String>,
    /// `AD_GAME`'s `ITEMS`: the inventory items definition.
    pub items: Option<String>,
    /// `AD_GAME`'s `TALK_SKIP_BUTTON`: `left`, `right`, `both` or `none`.
    pub talk_skip_button: Option<String>,
    /// `AD_GAME`'s `STARTUP_SCENE`: the first scene, when the script does
    /// not pick one.
    pub startup_scene: Option<String>,
}

impl Default for GameFile {
    fn default() -> Self {
        GameFile {
            name: String::new(),
            caption: String::new(),
            scripts: Vec::new(),
            string_table: None,
            system_font: None,
            cursor: None,
            active_cursor: None,
            noninteractive_cursor: None,
            subtitles: true,
            subtitles_speed: 70,
            video_subtitles: false,
            personal_savegames: false,
            local_save_dir: None,
            properties: Vec::new(),
            response_box: None,
            inventory_box: None,
            items: None,
            talk_skip_button: None,
            startup_scene: None,
        }
    }
}

fn text(node: &Node) -> Option<String> {
    node.as_str().map(str::to_string)
}

impl GameFile {
    /// Parses a `default.game` file.
    pub fn parse(bytes: &[u8]) -> Result<GameFile> {
        GameFile::from_document(&Document::parse(bytes)?)
    }

    /// Builds the definition from a parsed document with a `GAME` block.
    pub fn from_document(doc: &Document) -> Result<GameFile> {
        let root = doc.get("GAME").ok_or_else(|| Error::Invalid {
            reason: "'GAME' keyword expected".into(),
        })?;
        let mut game = GameFile::default();
        for node in root.nodes() {
            match node.key.to_ascii_uppercase().as_str() {
                "NAME" => game.name = text(node).unwrap_or_default(),
                "CAPTION" => game.caption = text(node).unwrap_or_default(),
                "SCRIPT" => game.scripts.extend(text(node)),
                "STRING_TABLE" => game.string_table = text(node),
                "SYSTEM_FONT" => game.system_font = text(node),
                "CURSOR" => game.cursor = text(node),
                "ACTIVE_CURSOR" => game.active_cursor = text(node),
                "NONINTERACTIVE_CURSOR" => game.noninteractive_cursor = text(node),
                "SUBTITLES" => game.subtitles = node.as_bool().unwrap_or(true),
                "SUBTITLES_SPEED" => {
                    game.subtitles_speed = node.as_int().unwrap_or(70) as i32;
                }
                "VIDEO_SUBTITLES" => game.video_subtitles = node.as_bool().unwrap_or(false),
                "PERSONAL_SAVEGAMES" => {
                    game.personal_savegames = node.as_bool().unwrap_or(false);
                }
                "LOCAL_SAVE_DIR" => game.local_save_dir = text(node),
                "PROPERTY" => game.properties.extend(Property::from_node(node)?),
                _ => {}
            }
        }
        if let Some(ad) = doc.get("AD_GAME") {
            for node in ad.nodes() {
                match node.key.to_ascii_uppercase().as_str() {
                    "RESPONSE_BOX" => game.response_box = text(node),
                    "INVENTORY_BOX" => game.inventory_box = text(node),
                    "ITEMS" => game.items = text(node),
                    "TALK_SKIP_BUTTON" => game.talk_skip_button = text(node),
                    "STARTUP_SCENE" => game.startup_scene = text(node),
                    _ => {}
                }
            }
        }
        Ok(game)
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
  STRING_TABLE="string.tab"
  PROPERTY { NAME="Difficulty" VALUE="2" }
}
AD_GAME {
  RESPONSE_BOX="interface\responses.def"
  INVENTORY_BOX="interface\inventory.def"
  ITEMS="items\items.items"
  TALK_SKIP_BUTTON="left"
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
        assert_eq!(game.string_table.as_deref(), Some("string.tab"));
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
        assert_eq!(game.properties.len(), 1);
        assert_eq!(game.properties[0].name, "Difficulty");
        assert_eq!(
            game.response_box.as_deref(),
            Some("interface\\responses.def")
        );
        assert_eq!(game.items.as_deref(), Some("items\\items.items"));
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
        assert!(GameFile::parse(b"SCENE { }").is_err());
    }
}
