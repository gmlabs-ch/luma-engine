//! The properties and methods of the native objects, as scripts call
//! them: `Game`, `Scene`, layers, entities, regions, sprites and the
//! keyboard; actors are in `natives_actor.rs`. Methods of later
//! milestones (sound, inventory, windows, saving) are accepted and
//! logged once.
//!
//! Written with reference to the `ScGetProperty`, `ScSetProperty` and
//! `ScCallMethod` methods of `BGame.cpp`, `AdGame.cpp`, `AdScene.cpp`,
//! `AdLayer.cpp`, `BObject.cpp`, `AdObject.cpp`, `AdEntity.cpp`,
//! `AdTalkHolder.cpp`, `AdRegion.cpp`, `BRegion.cpp`, `BSprite.cpp`,
//! `BKeyboardState.cpp` and `BScriptHolder.cpp` of WME Lite (MIT
//! License, Copyright (c) 2011 Jan Nedoma); see `NOTICE`.

use luma_formats::Rgba;
use luma_formats::scene::Direction;
use luma_script::{Context, Effect, NativeId, Return, State, Value, rgba};

use crate::objects::Kind;
use crate::scene_view::{EntityRef, frame_height};
use crate::talk::TalkSkipButton;
use crate::video::VideoPlayback;
use crate::world::World;
use luma_formats::font::TextAlign;

/// `AlphaColor`: the engine's `0xAARRGGBB` integer.
fn color_int(c: Rgba) -> i32 {
    rgba(
        i32::from(c.r),
        i32::from(c.g),
        i32::from(c.b),
        i32::from(c.a),
    )
}

fn color_of(v: i32) -> Rgba {
    Rgba {
        r: ((v >> 16) & 0xff) as u8,
        g: ((v >> 8) & 0xff) as u8,
        b: (v & 0xff) as u8,
        a: ((v >> 24) & 0xff) as u8,
    }
}

fn native_or_null(id: Option<NativeId>) -> Value {
    id.map_or(Value::Null, Value::Native)
}

/// Methods the engine has that later milestones implement.
const GAME_STUBS: &[&str] = &[
    "LockMouseRect",
    "RegWriteNumber",
    "RegWriteString",
    "Screenshot",
    "ScreenshotEx",
    "OpenDocument",
    "SetLoadingScreen",
    "SetSavingScreen",
    "DisplayLoadingIcon",
    "HideLoadingIcon",
    "StoreSaveThumbnail",
    "DeleteSaveThumbnail",
    "ShowStatusLine",
    "HideStatusLine",
    "AddResponse",
    "AddResponseOnce",
    "AddResponseOnceGame",
    "ResetResponse",
    "ClearResponses",
    "StartDlgBranch",
    "EndDlgBranch",
    "LoadResponseBox",
    "LoadInventoryBox",
    "LoadItems",
    "SetSceneViewport",
    "LoadEntity",
    "CreateEntity",
    "CreateItem",
    "DeleteItem",
    "Reset",
    "AccOutputText",
    "DumpTextureStats",
    "EnableScriptProfiling",
    "DisableScriptProfiling",
];

const OBJECT_STUBS: &[&str] = &[
    "SetCursor",
    "RemoveCursor",
    "PlayAnim",
    "PlayAnimAsync",
    "ForceTalkAnim",
    "StickToRegion",
    "CreateParticleEmitter",
    "DeleteParticleEmitter",
    "AddAttachment",
    "RemoveAttachment",
    "AddTalkSprite",
    "RemoveTalkSprite",
    "SetTalkSprite",
    "CreateRegion",
    "DeleteRegion",
    "SetCaption",
];

impl World {
    /// A built-in property of a native, if the native defines it.
    pub(crate) fn native_get(&self, id: NativeId, name: &str) -> Option<Value> {
        let kind = self.native(id)?.kind.clone();
        match kind {
            Kind::Game => self.game_get(name),
            Kind::Keyboard => match name {
                "Type" => Some(Value::from("keyboard")),
                "Key" => Some(Value::from(self.keyboard.key.clone())),
                "Printable" => Some(Value::Bool(self.keyboard.printable)),
                "IsShift" => Some(Value::Bool(self.keyboard.shift)),
                "IsAlt" => Some(Value::Bool(self.keyboard.alt)),
                "IsControl" => Some(Value::Bool(self.keyboard.control)),
                "KeyCode" => Some(Value::Int(self.keyboard.key_code)),
                _ => None,
            },
            Kind::Math => match name {
                "Type" => Some(Value::from("math")),
                "PI" => Some(Value::Float(std::f64::consts::PI)),
                _ => None,
            },
            Kind::Fader => match name {
                "Type" => Some(Value::from("fader")),
                "Ready" => Some(Value::Bool(self.fader.ready)),
                _ => None,
            },
            Kind::Scene => self.scene_get(name),
            Kind::Layer(index) => self.layer_get(index, name),
            Kind::Entity(r) => self.entity_get(r, name),
            Kind::Region { layer, index } => self.region_get(layer, index, name),
            Kind::Sprite(owner) => self.sprite_get(owner, name),
            Kind::Actor => self.actor_get(id, name),
            Kind::Control => self.control_get(id, name),
            Kind::Item => self.item_get(id, name),
            Kind::Inert(kind) => match name {
                "Type" => Some(Value::from(kind.to_ascii_lowercase())),
                "Name" => Some(Value::from(kind)),
                _ => None,
            },
        }
    }

    /// Sets a built-in property; `false` when the native has no such
    /// property and it should be stored as a script property.
    pub(crate) fn native_set(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
        let kind = match self.native(id) {
            Some(native) => native.kind.clone(),
            None => return false,
        };
        match kind {
            Kind::Game => self.game_set(name, value),
            Kind::Scene => self.scene_set(name, value),
            Kind::Layer(index) => self.layer_set(index, name, value),
            Kind::Entity(r) => self.entity_set(r, name, value),
            Kind::Region { layer, index } => self.region_set(layer, index, name, value),
            Kind::Sprite(owner) => self.sprite_set(owner, name, value),
            Kind::Actor => self.actor_set(id, name, value),
            Kind::Control => self.control_set(id, name, value),
            Kind::Item => self.item_set(id, name, value),
            Kind::Keyboard | Kind::Math | Kind::Fader | Kind::Inert(_) => false,
        }
    }

    /// A method call on a native; `None` when it has no such method.
    pub(crate) fn native_call(
        &mut self,
        id: NativeId,
        name: &str,
        args: &[Value],
        ctx: Context<'_>,
    ) -> Option<Return> {
        let kind = self.native(id)?.kind.clone();
        let mut ctx = ctx;
        if let Some(ret) = self.holder_call(id, name, args, &mut ctx) {
            return Some(ret);
        }
        match kind {
            Kind::Game => self.game_call(name, args, ctx),
            Kind::Keyboard => match name {
                "IsKeyDown" => {
                    let code = match args.first() {
                        Some(Value::String(s)) if !s.is_empty() => {
                            i32::from(s.as_bytes()[0].to_ascii_lowercase())
                        }
                        Some(v) => v.get_int(self),
                        None => 0,
                    };
                    Some(Return::value(self.keyboard.is_key_down(code)))
                }
                _ => None,
            },
            Kind::Math => math_call(name, args).map(Return::value),
            Kind::Fader => None,
            Kind::Scene => self.scene_call(name, args, ctx),
            Kind::Layer(index) => self.layer_call(index, name, args),
            Kind::Entity(r) => self.entity_call(id, r, name, args),
            Kind::Region { layer, index } => self.region_call(layer, index, name, args),
            Kind::Sprite(owner) => self.sprite_call(owner, name),
            Kind::Actor => self.actor_call(id, name, args, ctx),
            Kind::Control => self.control_call(id, name, args, ctx),
            Kind::Item => self.item_call(id, name, args),
            Kind::Inert(kind) => match name {
                "GetWidget" | "GetControl" => {
                    let widget = self.inert("Widget");
                    Some(Return::value(Value::Native(widget)))
                }
                _ => {
                    self.stub(&format!("{kind}.{name}"));
                    Some(Return::null())
                }
            },
        }
    }

    /// The methods every script holder has (`CBScriptHolder`).
    fn holder_call(
        &mut self,
        id: NativeId,
        name: &str,
        args: &[Value],
        ctx: &mut Context<'_>,
    ) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        Some(match name {
            "ApplyEvent" => {
                let event = arg(0).get_string(self);
                let handled = ctx.engine.apply_event(self, id, &event, false);
                Return::value(handled)
            }
            "CanHandleEvent" => {
                let event = arg(0).get_string(self);
                Return::value(ctx.engine.can_handle_event(id, &event))
            }
            "CanHandleMethod" => {
                let method = arg(0).get_string(self);
                Return::value(ctx.engine.can_handle_method(id, &method))
            }
            "AttachScript" => {
                let path = arg(0).get_string(self);
                let ok = self.attach(ctx.engine, id, &path);
                Return::value(ok)
            }
            "DetachScript" => {
                let path = arg(0).get_string(self);
                let kill_threads = arg(1).get_bool(self);
                let found = ctx.engine.scripts_of(id).into_iter().find(|s| {
                    ctx.engine
                        .script(*s)
                        .is_some_and(|s| s.filename().eq_ignore_ascii_case(&path))
                });
                match found {
                    Some(script) => {
                        ctx.engine.finish(script, kill_threads);
                        Return::value(true)
                    }
                    None => Return::value(false),
                }
            }
            "IsScriptRunning" => {
                let path = arg(0).get_string(self);
                let running = ctx.engine.scripts_of(id).into_iter().any(|s| {
                    ctx.engine.script(s).is_some_and(|s| {
                        s.filename().eq_ignore_ascii_case(&path)
                            && !matches!(s.state(), State::Finished | State::Error)
                    })
                });
                Return::value(running)
            }
            _ => return None,
        })
    }

    /// The inventory methods every adventure object shares
    /// (`CAdObject::ScCallMethod`): one inventory, the game's.
    pub(crate) fn inventory_call(&mut self, name: &str, args: &[Value]) -> Return {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        match name {
            "TakeItem" => {
                match arg(0) {
                    Value::Null => self.log("TakeItem: item name expected".to_string()),
                    v => {
                        let item = v.get_string(self);
                        let after = match arg(1) {
                            Value::Null => None,
                            a => Some(a.get_string(self)),
                        };
                        if !self.take_item(&item, after.as_deref()) {
                            self.log(format!("Cannot add item '{item}' to inventory"));
                        }
                    }
                }
                Return::null()
            }
            "DropItem" => {
                match arg(0) {
                    Value::Null => self.log("DropItem: item name expected".to_string()),
                    v => {
                        let item = v.get_string(self);
                        if !self.drop_item(&item) {
                            self.log(format!("Cannot remove item '{item}' from inventory"));
                        }
                    }
                }
                Return::null()
            }
            "GetItem" => Return::value(native_or_null(match arg(0) {
                Value::String(name) => self.item_by_name(&name),
                Value::Null => None,
                v => {
                    let index = v.get_int(self);
                    usize::try_from(index)
                        .ok()
                        .and_then(|i| self.inventory.taken.get(i).copied())
                }
            })),
            "HasItem" | "IsItemTaken" => match arg(0) {
                Value::Null => Return::value(false),
                v => {
                    let name = v.get_string(self);
                    Return::value(self.has_item(&name))
                }
            },
            _ => Return::null(),
        }
    }

    // ----------------------------------------------------------------
    // Game
    // ----------------------------------------------------------------

    fn game_get(&self, name: &str) -> Option<Value> {
        Some(match name {
            "Type" => Value::from("game"),
            "Name" => Value::from(self.name.clone()),
            "Caption" => Value::from(self.caption.clone()),
            "CurrentTime" => Value::Int(self.timer() as i32),
            "WindowsTime" => Value::Int(self.timer() as i32),
            "MainObject" => native_or_null(self.main_object),
            "MouseX" => Value::Int(self.mouse.x),
            "MouseY" => Value::Int(self.mouse.y),
            "ActiveObject" => native_or_null(self.active_object()),
            "SelectedItem" => native_or_null(self.selected_item()),
            "InventoryVisible" => Value::Bool(self.inventory.visible),
            "NumItems" => Value::Int(self.inventory.taken.len() as i32),
            "TotalNumItems" => Value::Int(self.inventory.items.len() as i32),
            "InventoryScrollOffset" => Value::Int(self.inventory.scroll_offset),
            "SmartItemCursor" => Value::Bool(self.inventory.smart_item_cursor),
            "InventoryObject" => Value::Native(self.game_id),
            "ScreenWidth" => Value::Int(self.settings().width as i32),
            "ScreenHeight" => Value::Int(self.settings().height as i32),
            "Interactive" => Value::Bool(self.interactive),
            "Keyboard" => Value::Native(self.keyboard_id),
            "Scene" => Value::Native(self.scene_id),
            "PrevScene" | "PreviousScene" => Value::from(self.prev_scene_name.clone()),
            "PrevSceneFilename" | "PreviousSceneFilename" => {
                Value::from(self.prev_scene_filename.clone())
            }
            "ChangingScene" => Value::Bool(self.scheduled_scene.is_some()),
            "Subtitles" => Value::Bool(self.subtitles),
            "SubtitlesSpeed" => Value::Int(self.subtitles_speed),
            "VideoSubtitles" => Value::Bool(self.video_subtitles),
            "VideoSkipButton" => Value::Int(self.video_skip_button.code()),
            "Frozen" => Value::Bool(self.is_frozen()),
            "FocusedWindow" => native_or_null(self.focused_window()),
            "DebugMode"
            | "SoundAvailable"
            | "WindowedMode"
            | "TextRTL"
            | "ResponsesVisible"
            | "SuspendedRendering"
            | "SuppressScriptErrors"
            | "AutorunDisabled"
            | "AutoSaveOnExit"
            | "CursorHidden" => Value::Bool(false),
            "AcceleratedMode" => Value::Bool(true),
            "SFXVolume" | "SpeechVolume" | "MusicVolume" | "MasterVolume" => Value::Int(100),
            "FPS" | "TextEncoding" | "SoundBufferSize" | "AutoSaveSlot" => Value::Int(0),
            "SaveDirectory" => Value::from(self.save_directory().to_string_lossy().into_owned()),
            "MostRecentSaveSlot" => Value::Int(self.most_recent_save_slot()),
            "TalkSkipButton" => Value::Int(self.talk_skip_button.code()),
            "LastResponse" | "LastResponseOrig" => Value::Null,
            "Platform" => Value::from("luma"),
            "DeviceType" => Value::from("desktop"),
            _ => return None,
        })
    }

    fn game_set(&mut self, name: &str, value: &Value) -> bool {
        match name {
            "Name" => self.name = value.get_string(self),
            "Caption" => self.caption = value.get_string(self),
            "Interactive" => self.interactive = value.get_bool(self),
            "MainObject" => self.main_object = value.as_native(),
            "Subtitles" => self.subtitles = value.get_bool(self),
            "SubtitlesSpeed" => self.subtitles_speed = value.get_int(self),
            "MouseX" => self.mouse.x = value.get_int(self),
            "MouseY" => self.mouse.y = value.get_int(self),
            "SelectedItem" => self.select_item(value),
            "InventoryVisible" => self.inventory.visible = value.get_bool(self),
            "InventoryScrollOffset" => self.inventory.scroll_offset = value.get_int(self),
            "SmartItemCursor" => self.inventory.smart_item_cursor = value.get_bool(self),
            "InventoryObject" => {}
            "TalkSkipButton" => {
                self.talk_skip_button = TalkSkipButton::from_code(value.get_int(self));
            }
            "VideoSubtitles" => self.video_subtitles = value.get_bool(self),
            "VideoSkipButton" => {
                self.video_skip_button = TalkSkipButton::from_code(value.get_int(self));
            }
            _ => return false,
        }
        true
    }

    fn game_call(&mut self, name: &str, args: &[Value], ctx: Context<'_>) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        if let Some(ret) = self.game_sound_call(name, args) {
            return Some(ret);
        }
        let game_id = self.game_id;
        if let Some(ret) = self.object_sound_call(game_id, name, args) {
            return Some(ret);
        }
        Some(match name {
            "ChangeScene" => {
                let path = arg(0).get_string(self);
                let fade_in = match arg(2) {
                    Value::Null => true,
                    v => v.get_bool(self),
                };
                self.schedule_scene(&path, fade_in);
                Return::null()
            }
            "PlayTheora" | "PlayVideo" => {
                let filename = arg(0).get_string(self);
                let playback = match arg(1) {
                    Value::Null => VideoPlayback::Stretch,
                    v => VideoPlayback::from_code(v.get_int(self)),
                };
                let (x, y) = (arg(2).get_int(self), arg(3).get_int(self));
                let freeze_music = match arg(4) {
                    Value::Null => true,
                    v => v.get_bool(self),
                };
                let drop_frames = match arg(5) {
                    Value::Null => true,
                    v => v.get_bool(self),
                };
                let subtitle_file = match arg(6) {
                    Value::Null => None,
                    v => Some(v.get_string(self)),
                };
                if name == "PlayVideo" {
                    self.log(
                        "Warning: Game.PlayVideo() is now deprecated. Use Game.PlayTheora() instead."
                            .to_string(),
                    );
                }
                let started = self.play_theora(
                    ctx.engine,
                    &filename,
                    playback,
                    x,
                    y,
                    freeze_music,
                    drop_frames,
                    subtitle_file.as_deref(),
                );
                if started {
                    Return {
                        value: Value::Bool(true),
                        effect: Effect::Sleep(0),
                    }
                } else {
                    Return::value(false)
                }
            }
            "StopTheora" | "StopVideo" => {
                self.stop_video(ctx.engine);
                Return::null()
            }
            "IsVideoPlaying" => Return::value(self.is_video_playing()),
            "LoadWindow" => {
                let path = arg(0).get_string(self);
                Return::value(native_or_null(self.load_window(ctx.engine, &path)))
            }
            "CreateWindow" => {
                let name = match arg(0) {
                    Value::Null => None,
                    v => Some(v.get_string(self)),
                };
                Return::value(Value::Native(self.create_window(name.as_deref())))
            }
            "DeleteWindow" => {
                let deleted = arg(0).as_native().is_some_and(|id| {
                    self.window_ids().contains(&id) && self.unload_object(ctx.engine, id)
                });
                Return::value(deleted)
            }
            "LoadActor" => {
                let path = arg(0).get_string(self);
                Return::value(native_or_null(self.load_actor(ctx.engine, &path, false)))
            }
            "UnloadObject" | "UnloadActor" | "UnloadEntity" | "DeleteEntity" => {
                if let Some(id) = arg(0).as_native() {
                    self.unload_object(ctx.engine, id);
                }
                Return::null()
            }
            "SetMousePos" => {
                let (x, y) = (arg(0).get_int(self), arg(1).get_int(self));
                self.set_mouse(x, y);
                Return::null()
            }
            "FadeOut" | "FadeOutAsync" | "SystemFadeOut" | "SystemFadeOutAsync" | "FadeIn"
            | "FadeInAsync" | "SystemFadeIn" | "SystemFadeInAsync" => {
                let duration = match arg(0) {
                    Value::Null => 500,
                    v => u64::try_from(v.get_int(self)).unwrap_or(0),
                };
                let channel = |n: usize, default: i32| match arg(n) {
                    Value::Null => default,
                    v => v.get_int(self),
                };
                let color = Rgba {
                    r: channel(1, 0).clamp(0, 255) as u8,
                    g: channel(2, 0).clamp(0, 255) as u8,
                    b: channel(3, 0).clamp(0, 255) as u8,
                    a: channel(4, 255).clamp(0, 255) as u8,
                };
                let system = name.starts_with("System");
                let (timer, clock) = (self.timer(), self.clock);
                if name.contains("Out") {
                    self.fader.fade_out(color, duration, system, timer, clock);
                } else {
                    self.fader.fade_in(color, duration, system, timer, clock);
                }
                if name.ends_with("Async") {
                    Return::null()
                } else {
                    Return::effect(Effect::WaitFor(self.fader_id))
                }
            }
            "GetFadeColor" => Return::value(self.fader.color_int()),
            "SetActiveCursor" | "SetWaitCursor" => {
                let path = arg(0).get_string(self);
                let ok = if name == "SetActiveCursor" {
                    self.set_active_cursor(Some(&path))
                } else {
                    self.set_wait_cursor(Some(&path))
                };
                Return::value(ok)
            }
            "RemoveActiveCursor" => {
                self.set_active_cursor(None);
                Return::null()
            }
            "RemoveWaitCursor" => {
                self.set_wait_cursor(None);
                Return::null()
            }
            "HasActiveCursor" => Return::value(self.cursors.active.is_some()),
            "GetActiveCursor" => {
                Return::value(self.cursors.active.clone().map_or(Value::Null, Value::from))
            }
            "GetWaitCursor" => Return::value(
                self.cursors
                    .noninteractive
                    .clone()
                    .map_or(Value::Null, Value::from),
            ),
            "HideCursor" => {
                self.cursors.hidden = true;
                Return::null()
            }
            "ShowCursor" => {
                self.cursors.hidden = false;
                Return::null()
            }
            "ValidObject" => Return::value(
                arg(0)
                    .as_native()
                    .is_some_and(|id| self.objects.contains(id)),
            ),
            "QuitGame" => {
                self.quit = true;
                Return::null()
            }
            "LoadStringTable" => {
                let path = arg(0).get_string(self);
                let clear_old = match arg(1) {
                    Value::Null => true,
                    v => v.get_bool(self),
                };
                Return::value(self.load_string_table(&path, clear_old))
            }
            "ExpandString" => Return::value(self.expand(&arg(0).get_string(self))),
            "Msg" => {
                let text = arg(0).get_string(self);
                self.log(format!("msg: {text}"));
                Return::null()
            }
            "LOG" => {
                let text = arg(0).get_string(self);
                self.log(format!("sc: {text}"));
                Return::null()
            }
            "FileExists" => Return::value(self.vfs().exists(&arg(0).get_string(self))),
            "ClearScriptCache" => Return::null(),
            "SaveGame" => {
                // Done once `true` is on the caller's stack (`after_call`).
                let slot = arg(0).get_int(self);
                let description = arg(1).get_string(self);
                let quick = arg(2).get_bool(self);
                self.pending_save = Some((slot, description, quick));
                Return::value(true)
            }
            "LoadGame" => {
                // Loaded at the start of the next frame; the script sleeps
                // until then and is replaced by the saved one.
                self.schedule_load(arg(0).get_int(self));
                Return {
                    value: Value::Bool(false),
                    effect: Effect::Sleep(0),
                }
            }
            "IsSaveSlotUsed" => Return::value(self.is_save_slot_used(arg(0).get_int(self))),
            "GetSaveSlotDescription" => {
                Return::value(self.save_slot_description(arg(0).get_int(self)))
            }
            "EmptySaveSlot" => {
                self.empty_save_slot(arg(0).get_int(self));
                Return::null()
            }
            "QueryItem" => Return::value(native_or_null(match arg(0) {
                Value::Int(i) => usize::try_from(i)
                    .ok()
                    .and_then(|i| self.item_ids().get(i).copied()),
                v => self.item_by_name(&v.get_string(self)),
            })),
            "GetInventoryWindow" => Return::value(native_or_null(self.inventory.window)),
            "TakeItem" | "DropItem" | "GetItem" | "HasItem" | "IsItemTaken" => {
                return Some(self.inventory_call(name, args));
            }
            "GetActiveCursorObject" | "GetWaitCursorObject" => {
                self.stub(&format!("Game.{name}"));
                Return::null()
            }
            "RegReadString" => {
                self.stub(&format!("Game.{name}"));
                Return::value("")
            }
            "RegReadNumber" | "GetNumResponses" => {
                self.stub(&format!("Game.{name}"));
                Return::value(0)
            }
            "GetResponse" | "GetCurrentDlgBranch" | "GetResponsesWindow" => {
                self.stub(&format!("Game.{name}"));
                Return::null()
            }
            _ if GAME_STUBS.contains(&name) => {
                self.stub(&format!("Game.{name}"));
                Return::null()
            }
            _ => return None,
        })
    }

    // ----------------------------------------------------------------
    // Scene
    // ----------------------------------------------------------------

    fn scene_get(&self, name: &str) -> Option<Value> {
        let view = self.scene();
        Some(match name {
            "Type" => Value::from("scene"),
            "Name" => Value::from(view.map(|v| v.scene.name.clone()).unwrap_or_default()),
            "Filename" => Value::from(view.map(|v| v.path.clone()).unwrap_or_default()),
            "NumLayers" => Value::Int(view.map_or(0, |v| v.scene.layers.len() as i32)),
            "NumWaypointGroups" => {
                Value::Int(view.map_or(0, |v| v.scene.waypoint_groups.len() as i32))
            }
            "NumFreeNodes" => Value::Int(view.map_or(0, |v| v.scene.free_entities.len() as i32)),
            "MainLayer" => native_or_null(
                view.and_then(|v| v.scene.layers.iter().rposition(|l| l.main))
                    .and_then(|i| self.layer_id(i)),
            ),
            "MouseX" => Value::Int(self.mouse.x + self.offset_x),
            "MouseY" => Value::Int(self.mouse.y + self.offset_y),
            "OffsetX" => Value::Int(self.offset_x),
            "OffsetY" => Value::Int(self.offset_y),
            "Width" => Value::Int(view.map_or(0, |v| v.scene.width())),
            "Height" => Value::Int(view.map_or(0, |v| v.scene.height())),
            "AutoScroll" => Value::Bool(self.auto_scroll),
            "PersistentState" => Value::Bool(view.is_some_and(|v| v.scene.persistent_state)),
            "PersistentStateSprites" => {
                Value::Bool(view.is_none_or(|v| v.scene.persistent_state_sprites))
            }
            "ScrollPixelsX" => Value::Int(self.scroll_pixels_x),
            "ScrollPixelsY" => Value::Int(self.scroll_pixels_y),
            "ScrollSpeedX" => Value::Int(self.scroll_time_x as i32),
            "ScrollSpeedY" => Value::Int(self.scroll_time_y as i32),
            _ => return None,
        })
    }

    fn scene_set(&mut self, name: &str, value: &Value) -> bool {
        match name {
            "Name" => {
                let name = value.get_string(self);
                if let Some(view) = self.scene_mut() {
                    view.scene.name = name;
                }
            }
            "OffsetX" => {
                self.offset_x = value.get_int(self);
                self.target_offset_x = self.offset_x;
            }
            "OffsetY" => {
                self.offset_y = value.get_int(self);
                self.target_offset_y = self.offset_y;
            }
            "AutoScroll" => self.auto_scroll = value.get_bool(self),
            "ScrollPixelsX" => self.scroll_pixels_x = value.get_int(self),
            "ScrollPixelsY" => self.scroll_pixels_y = value.get_int(self),
            "ScrollSpeedX" => self.scroll_time_x = value.get_int(self).max(0) as u64,
            "ScrollSpeedY" => self.scroll_time_y = value.get_int(self).max(0) as u64,
            "PersistentState" => {
                let v = value.get_bool(self);
                if let Some(view) = self.scene_mut() {
                    view.scene.persistent_state = v;
                }
            }
            "PersistentStateSprites" => {
                let v = value.get_bool(self);
                if let Some(view) = self.scene_mut() {
                    view.scene.persistent_state_sprites = v;
                }
            }
            _ => return false,
        }
        true
    }

    fn scene_call(&mut self, name: &str, args: &[Value], ctx: Context<'_>) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        Some(match name {
            "LoadActor" => {
                let path = arg(0).get_string(self);
                Return::value(native_or_null(self.load_actor(ctx.engine, &path, true)))
            }
            "UnloadObject" | "UnloadActor" | "UnloadEntity" | "UnloadActor3D" | "DeleteEntity" => {
                if let Some(id) = arg(0).as_native() {
                    self.unload_object(ctx.engine, id);
                }
                Return::null()
            }
            "GetNode" => {
                let node_name = arg(0).get_string(self);
                Return::value(native_or_null(self.find_node(&node_name)))
            }
            "GetLayer" => {
                let id = match arg(0) {
                    Value::Int(i) => usize::try_from(i).ok().and_then(|i| self.layer_id(i)),
                    other => {
                        let layer_name = other.get_string(self);
                        self.scene()
                            .and_then(|v| {
                                v.scene
                                    .layers
                                    .iter()
                                    .position(|l| l.name.eq_ignore_ascii_case(&layer_name))
                            })
                            .and_then(|i| self.layer_id(i))
                    }
                };
                Return::value(native_or_null(id))
            }
            "GetFreeNode" => {
                let id = match arg(0) {
                    Value::Int(i) => usize::try_from(i)
                        .ok()
                        .filter(|&i| {
                            self.scene()
                                .is_some_and(|v| i < v.scene.free_entities.len())
                        })
                        .and_then(|i| self.node_id(EntityRef::Free(i))),
                    other => {
                        let node_name = other.get_string(self);
                        self.scene()
                            .and_then(|v| v.find_free(&node_name))
                            .and_then(|i| self.node_id(EntityRef::Free(i)))
                    }
                };
                Return::value(native_or_null(id))
            }
            "GetRegionAt" => {
                let x = arg(0).get_int(self);
                let y = arg(1).get_int(self);
                let include_decorations = match arg(2) {
                    Value::Null => false,
                    v => v.get_bool(self),
                };
                let found = self.scene().and_then(|view| {
                    let main = view.scene.layers.iter().rposition(|l| l.main)?;
                    let layer = &view.scene.layers[main];
                    layer
                        .nodes
                        .iter()
                        .enumerate()
                        .rev()
                        .find_map(|(i, node)| match node {
                            luma_formats::SceneNode::Region(r)
                                if r.active
                                    && r.contains(x, y)
                                    && (include_decorations || !r.decoration) =>
                            {
                                Some((main, i))
                            }
                            _ => None,
                        })
                });
                Return::value(native_or_null(
                    found.and_then(|(layer, index)| self.region_id(layer, index)),
                ))
            }
            "IsBlockedAt" => {
                let (x, y) = (arg(0).get_int(self), arg(1).get_int(self));
                Return::value(self.scene().is_none_or(|v| v.scene.is_blocked_at(x, y)))
            }
            "IsWalkableAt" => {
                let (x, y) = (arg(0).get_int(self), arg(1).get_int(self));
                Return::value(self.scene().is_some_and(|v| !v.scene.is_blocked_at(x, y)))
            }
            "GetScaleAt" => {
                let (x, y) = (arg(0).get_int(self), arg(1).get_int(self));
                Return::value(f64::from(
                    self.scene().map_or(100.0, |v| v.scene.zoom_at(x, y)),
                ))
            }
            "GetRotationAt" => {
                let x = arg(0).get_int(self);
                Return::value(f64::from(
                    self.scene().map_or(0.0, |v| v.scene.rotation_at(x)),
                ))
            }
            "SkipTo" | "ScrollTo" | "ScrollToAsync" => {
                let (x, y) = match arg(0).as_native() {
                    Some(id) => self.position_of(id).unwrap_or((0, 0)),
                    None => (arg(0).get_int(self), arg(1).get_int(self)),
                };
                match name {
                    "SkipTo" => {
                        self.skip_to(x, y);
                        Return::null()
                    }
                    "ScrollTo" => {
                        self.scroll_to(x, y);
                        Return::effect(Effect::WaitFor(self.scene_id))
                    }
                    _ => {
                        self.scroll_to(x, y);
                        Return::null()
                    }
                }
            }
            "IsScrolling" => Return::value(!self.is_scrolled()),
            "IsPointInViewport" => {
                let (x, y) = (arg(0).get_int(self), arg(1).get_int(self));
                let (w, h) = (self.settings().width as i32, self.settings().height as i32);
                Return::value(x >= 0 && y >= 0 && x < w && y < h)
            }
            "GetWaypointGroup" | "LoadEntity" | "CreateEntity" | "AddLayer" | "InsertLayer" => {
                self.stub(&format!("Scene.{name}"));
                Return::null()
            }
            "DeleteLayer" | "SetViewport" | "FadeOut" | "FadeOutAsync" | "FadeIn"
            | "FadeInAsync" => {
                self.stub(&format!("Scene.{name}"));
                Return::null()
            }
            _ => return None,
        })
    }

    /// The height of an entity's current frame (`CAdObject::GetHeight`):
    /// the largest hotspot `y` of its sub-frames, scaled with the entity.
    pub(crate) fn entity_height(&self, r: EntityRef) -> i32 {
        let Some(view) = self.scene() else { return 0 };
        let Some(entity) = view.entity(r) else {
            return 0;
        };
        let Some(loaded) = entity.sprite.as_deref().and_then(|p| view.sprite(p)) else {
            return 0;
        };
        let zoom = if entity.scalable {
            view.scene.zoom_at(entity.x, entity.y)
        } else {
            100.0
        };
        frame_height(&loaded.sprite, view.frame_of(r), zoom)
    }

    // ----------------------------------------------------------------
    // Layers
    // ----------------------------------------------------------------

    fn layer_get(&self, index: usize, name: &str) -> Option<Value> {
        let layer = self.scene()?.scene.layers.get(index)?;
        Some(match name {
            "Type" => Value::from("layer"),
            "Name" => Value::from(layer.name.clone()),
            "Caption" => Value::from(layer.caption.clone()),
            "NumNodes" => Value::Int(layer.nodes.len() as i32),
            "Width" => Value::Int(layer.width),
            "Height" => Value::Int(layer.height),
            "Main" => Value::Bool(layer.main),
            "CloseUp" => Value::Bool(layer.close_up),
            "Active" => Value::Bool(layer.active),
            _ => return None,
        })
    }

    fn layer_set(&mut self, index: usize, name: &str, value: &Value) -> bool {
        let (text, int, flag) = (
            value.get_string(self),
            value.get_int(self),
            value.get_bool(self),
        );
        let caption = self.expand(&text);
        let Some(layer) = self.scene_mut().and_then(|v| v.scene.layers.get_mut(index)) else {
            return false;
        };
        match name {
            "Name" => layer.name = text,
            "Caption" => layer.caption = caption,
            "CloseUp" => layer.close_up = flag,
            "Width" => layer.width = int,
            "Height" => layer.height = int,
            "Active" => layer.active = flag,
            _ => return false,
        }
        true
    }

    fn layer_call(&mut self, index: usize, name: &str, args: &[Value]) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        Some(match name {
            "GetNode" => {
                let node = match arg(0) {
                    Value::Int(i) => usize::try_from(i).ok(),
                    other => {
                        let node_name = other.get_string(self);
                        self.scene().and_then(|v| {
                            v.scene
                                .layers
                                .get(index)?
                                .nodes
                                .iter()
                                .position(|n| match n {
                                    luma_formats::SceneNode::Entity(e) => {
                                        e.name.eq_ignore_ascii_case(&node_name)
                                    }
                                    luma_formats::SceneNode::Region(r) => {
                                        r.name.eq_ignore_ascii_case(&node_name)
                                    }
                                })
                        })
                    }
                };
                let id = node.and_then(|ni| match self.scene()?.node(index, ni)? {
                    luma_formats::SceneNode::Entity(_) => self.node_id(EntityRef::Node {
                        layer: index,
                        index: ni,
                    }),
                    luma_formats::SceneNode::Region(_) => self.region_id(index, ni),
                });
                Return::value(native_or_null(id))
            }
            "AddRegion" | "AddEntity" | "InsertRegion" | "InsertEntity" | "DeleteNode" => {
                self.stub(&format!("Layer.{name}"));
                Return::null()
            }
            _ => return None,
        })
    }

    // ----------------------------------------------------------------
    // Entities
    // ----------------------------------------------------------------

    fn entity_get(&self, r: EntityRef, name: &str) -> Option<Value> {
        let view = self.scene()?;
        let e = view.entity(r)?;
        Some(match name {
            "Type" => Value::from("entity"),
            "Name" => Value::from(e.name.clone()),
            "Caption" => Value::from(e.caption.clone()),
            "Filename" => Value::Null,
            "X" => Value::Int(e.x),
            "Y" => Value::Int(e.y),
            "Height" => Value::Int(self.entity_height(r)),
            "Ready" => Value::Bool(!view.is_talking(r)),
            "Movable" => Value::Bool(true),
            "Registrable" | "Interactive" => Value::Bool(e.interactive),
            "Zoomable" | "Scalable" => Value::Bool(e.scalable),
            "Rotatable" => Value::Bool(e.rotatable),
            "AlphaColor" => Value::Int(e.tint.map_or(0, color_int)),
            "BlendMode" => Value::Int(0),
            "Scale" => e.scale.map_or(Value::Null, |s| Value::Float(f64::from(s))),
            "ScaleX" | "ScaleY" => Value::Null,
            "RelativeScale" => Value::Float(f64::from(e.relative_scale)),
            "Rotate" => Value::Null,
            "RelativeRotate" => Value::Float(0.0),
            "Colorable" => Value::Bool(e.colorable),
            "SoundPanning" => Value::Bool(e.sound_panning),
            "SaveState" => Value::Bool(e.save_state),
            "NonIntMouseEvents" => Value::Bool(false),
            "AccCaption" => Value::Null,
            "Active" => Value::Bool(e.active),
            "IgnoreItems" => Value::Bool(e.ignore_items),
            "SceneIndependent" => Value::Bool(false),
            "SubtitlesWidth" => Value::Int(view.subtitles.get(&r).map_or(0, |m| m.width)),
            "SubtitlesPosX" => Value::Int(view.subtitles.get(&r).map_or(0, |m| m.x)),
            "SubtitlesPosY" => Value::Int(view.subtitles.get(&r).map_or(0, |m| m.y)),
            "SubtitlesPosRelative" => {
                Value::Bool(view.subtitles.get(&r).is_none_or(|m| m.relative))
            }
            "SubtitlesPosXCenter" => Value::Bool(view.subtitles.get(&r).is_none_or(|m| m.x_center)),
            "NumItems" => Value::Int(self.inventory.taken.len() as i32),
            "NumAttachments" => Value::Int(0),
            "ParticleEmitter" => Value::Null,
            "Item" => e.item.clone().map_or(Value::Null, Value::from),
            "Subtype" => Value::from(match e.kind {
                luma_formats::EntityKind::Sound => "sound",
                luma_formats::EntityKind::Normal => "normal",
            }),
            "WalkToX" => Value::Int(e.walk_to_x),
            "WalkToY" => Value::Int(e.walk_to_y),
            "WalkToDirection" => Value::Int(e.walk_to_dir.map_or(8, |d| i32::from(d.code()))),
            "Region" => Value::Null,
            _ => return None,
        })
    }

    fn entity_set(&mut self, r: EntityRef, name: &str, value: &Value) -> bool {
        let (text, int, flag, float) = (
            value.get_string(self),
            value.get_int(self),
            value.get_bool(self),
            value.get_float(self),
        );
        let caption = self.expand(&text);
        let is_null = value.is_null();
        if name.starts_with("Subtitles") {
            let Some(view) = self.scene_mut() else {
                return false;
            };
            let mods = view.subtitles.entry(r).or_default();
            match name {
                "SubtitlesWidth" => mods.width = int,
                "SubtitlesPosX" => mods.x = int,
                "SubtitlesPosY" => mods.y = int,
                "SubtitlesPosRelative" => mods.relative = flag,
                "SubtitlesPosXCenter" => mods.x_center = flag,
                _ => return false,
            }
            return true;
        }
        let Some(e) = self.scene_mut().and_then(|v| v.entity_mut(r)) else {
            return false;
        };
        match name {
            "Name" => e.name = text,
            "Caption" => e.caption = caption,
            "X" => e.x = int,
            "Y" => e.y = int,
            "Registrable" | "Interactive" => e.interactive = flag,
            "Zoomable" | "Scalable" => e.scalable = flag,
            "Rotatable" => e.rotatable = flag,
            "AlphaColor" => e.tint = Some(color_of(int)),
            "Scale" => e.scale = if is_null { None } else { Some(float as f32) },
            "RelativeScale" => e.relative_scale = float as f32,
            "Colorable" => e.colorable = flag,
            "SoundPanning" => e.sound_panning = flag,
            "SaveState" => e.save_state = flag,
            "Active" => e.active = flag,
            "IgnoreItems" => e.ignore_items = flag,
            "Item" => e.item = if is_null { None } else { Some(text) },
            "WalkToX" => e.walk_to_x = int,
            "WalkToY" => e.walk_to_y = int,
            "WalkToDirection" => e.walk_to_dir = Direction::from_code(i64::from(int)),
            "Movable" | "BlendMode" | "ScaleX" | "ScaleY" | "Rotate" | "RelativeRotate"
            | "NonIntMouseEvents" | "AccCaption" | "SceneIndependent" => {}
            _ => return false,
        }
        true
    }

    fn entity_call(
        &mut self,
        id: NativeId,
        r: EntityRef,
        name: &str,
        args: &[Value],
    ) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        if let Some(ret) = self.object_sound_call(id, name, args) {
            return Some(ret);
        }
        Some(match name {
            "SkipTo" => {
                let (x, y) = (arg(0).get_int(self), arg(1).get_int(self));
                if let Some(e) = self.scene_mut().and_then(|v| v.entity_mut(r)) {
                    e.x = x;
                    e.y = y;
                }
                Return::null()
            }
            "Caption" => {
                let caption = self
                    .scene()
                    .and_then(|v| v.entity(r))
                    .map(|e| e.caption.clone())
                    .unwrap_or_default();
                Return::value(caption)
            }
            "SetSprite" => {
                let path = match arg(0) {
                    Value::Null => None,
                    v => Some(v.get_string(self)),
                };
                let ok = match (&path, self.scene.as_mut()) {
                    (Some(path), Some(view)) => view.add_sprite(&self.vfs, path),
                    _ => true,
                };
                if ok {
                    if let Some(e) = self.scene_mut().and_then(|v| v.entity_mut(r)) {
                        e.sprite = path;
                    }
                    if let Some(view) = self.scene_mut() {
                        view.reset_anim(r);
                    }
                } else {
                    let path = path.unwrap_or_default();
                    self.log(format!("SetSprite method failed for file '{path}'"));
                }
                Return::value(ok)
            }
            "GetSprite" => {
                let sprite = self
                    .scene()
                    .and_then(|v| v.entity(r))
                    .and_then(|e| e.sprite.clone());
                Return::value(sprite.map_or(Value::Null, Value::from))
            }
            "GetSpriteObject" => Return::value(native_or_null(self.sprite_object(id))),
            "Reset" => {
                if let Some(view) = self.scene_mut() {
                    view.reset_anim(r);
                }
                Return::null()
            }
            "Talk" | "TalkAsync" => self.talk_call(id, name == "Talk", args),
            "StopTalk" | "StopTalking" => Return::value(self.stop_talk(id)),
            "IsTalking" => Return::value(self.is_talking(id)),
            "SetFont" => {
                let path = match arg(0) {
                    Value::Null => None,
                    v => Some(v.get_string(self)),
                };
                self.set_font(id, path.as_deref());
                Return::null()
            }
            "GetFont" => Return::value(
                self.scene()
                    .and_then(|v| v.entity(r))
                    .and_then(|e| e.font.clone())
                    .map_or(Value::Null, Value::from),
            ),
            "TakeItem" | "DropItem" | "GetItem" | "HasItem" => {
                return Some(self.inventory_call(name, args));
            }
            "HasCursor" => Return::value(false),
            "GetCursor" | "GetCursorObject" | "GetAttachment" => Return::null(),
            _ if OBJECT_STUBS.contains(&name) => {
                self.stub(&format!("Entity.{name}"));
                Return::null()
            }
            _ => return None,
        })
    }

    /// `Talk(text, sound, duration, stances, align)` on an actor or an
    /// entity; the synchronous form makes the script wait for the
    /// object.
    pub(crate) fn talk_call(&mut self, id: NativeId, sync: bool, args: &[Value]) -> Return {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        let text = arg(0).get_string(self);
        let sound = match arg(1) {
            Value::Null => None,
            v => Some(v.get_string(self)).filter(|s| !s.is_empty()),
        };
        let duration = arg(2).get_int(self);
        let stances = match arg(3) {
            Value::Null => None,
            v => Some(v.get_string(self)),
        };
        let align = match arg(4) {
            Value::Null => TextAlign::Center,
            v => TextAlign::from_code(v.get_int(self)),
        };
        if self.talk(id, &text, sound.as_deref(), duration, stances, align) && sync {
            Return::effect(Effect::WaitForExclusive(id))
        } else {
            Return::null()
        }
    }

    // ----------------------------------------------------------------
    // Regions
    // ----------------------------------------------------------------

    fn region_get(&self, layer: usize, index: usize, name: &str) -> Option<Value> {
        let region = self.scene()?.region(layer, index)?;
        Some(match name {
            "Type" => Value::from("ad region"),
            "Name" => Value::from(region.name.clone()),
            "Caption" => Value::from(region.caption.clone()),
            "Active" => Value::Bool(region.active),
            "Blocked" => Value::Bool(region.blocked),
            "Decoration" => Value::Bool(region.decoration),
            "Scale" => Value::Float(f64::from(region.zoom.unwrap_or(0.0))),
            "AlphaColor" => Value::Int(color_int(region.tint)),
            "NumPoints" => Value::Int(region.points.len() as i32),
            "X" => Value::Int(region.bounds().map_or(0, |b| b.left)),
            "Y" => Value::Int(region.bounds().map_or(0, |b| b.top)),
            "Ready" => Value::Bool(true),
            _ => return None,
        })
    }

    fn region_set(&mut self, layer: usize, index: usize, name: &str, value: &Value) -> bool {
        let (text, int, flag, float) = (
            value.get_string(self),
            value.get_int(self),
            value.get_bool(self),
            value.get_float(self),
        );
        let caption = self.expand(&text);
        let Some(region) = self.scene_mut().and_then(|v| v.region_mut(layer, index)) else {
            return false;
        };
        match name {
            "Name" => region.name = text,
            "Caption" => region.caption = caption,
            "Active" => region.active = flag,
            "Blocked" => region.blocked = flag,
            "Decoration" => region.decoration = flag,
            "Scale" => {
                region.zoom = if float == 0.0 {
                    None
                } else {
                    Some(float as f32)
                }
            }
            "AlphaColor" => region.tint = color_of(int),
            _ => return false,
        }
        true
    }

    fn region_call(
        &mut self,
        layer: usize,
        index: usize,
        name: &str,
        args: &[Value],
    ) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        Some(match name {
            "GetPoint" => {
                let i = usize::try_from(arg(0).get_int(self)).ok();
                let point = self
                    .scene()
                    .and_then(|v| v.region(layer, index))
                    .and_then(|r| i.and_then(|i| r.points.get(i)))
                    .copied();
                match point {
                    Some(p) => {
                        let mut object = Value::object();
                        object.set_property("X", Value::Int(p.x), self);
                        object.set_property("Y", Value::Int(p.y), self);
                        Return::value(object)
                    }
                    None => Return::null(),
                }
            }
            "SkipTo" => {
                let (x, y) = (arg(0).get_int(self), arg(1).get_int(self));
                if let Some(region) = self.scene_mut().and_then(|v| v.region_mut(layer, index)) {
                    if let Some(b) = region.bounds() {
                        let (dx, dy) = (x - b.left, y - b.top);
                        for p in &mut region.points {
                            p.x += dx;
                            p.y += dy;
                        }
                    }
                }
                Return::null()
            }
            "AddPoint" | "InsertPoint" | "SetPoint" | "RemovePoint" => {
                self.stub(&format!("Region.{name}"));
                Return::value(false)
            }
            _ if OBJECT_STUBS.contains(&name) => {
                self.stub(&format!("Region.{name}"));
                Return::null()
            }
            _ => return None,
        })
    }

    // ----------------------------------------------------------------
    // Sprites
    // ----------------------------------------------------------------

    fn sprite_get(&self, owner: NativeId, name: &str) -> Option<Value> {
        let Kind::Entity(r) = self.native(owner)?.kind else {
            return None;
        };
        let view = self.scene()?;
        let entity = view.entity(r)?;
        let loaded = entity.sprite.as_deref().and_then(|p| view.sprite(p));
        let frames = loaded.map_or(0, |l| l.sprite.frames.len());
        let anim = view.anim(r);
        Some(match name {
            "Type" => Value::from("sprite"),
            "Filename" => entity.sprite.clone().map_or(Value::Null, Value::from),
            "NumFrames" => Value::Int(frames as i32),
            "CurrentFrame" => Value::Int(if frames == 0 {
                -1
            } else {
                anim.map_or(0, |a| a.current_frame.min(frames - 1) as i32)
            }),
            "PixelPerfect" => Value::Bool(loaded.is_some_and(|l| l.sprite.precise)),
            "Looping" => Value::Bool(
                anim.and_then(|a| a.looping)
                    .unwrap_or_else(|| loaded.is_some_and(|l| l.sprite.looping)),
            ),
            "Owner" => Value::Native(owner),
            "Finished" => Value::Bool(anim.is_some_and(|a| a.finished)),
            "Paused" => Value::Bool(anim.is_some_and(|a| a.paused)),
            _ => return None,
        })
    }

    fn sprite_set(&mut self, owner: NativeId, name: &str, value: &Value) -> bool {
        let Some(Kind::Entity(r)) = self.native(owner).map(|n| n.kind.clone()) else {
            return false;
        };
        let (int, flag) = (value.get_int(self), value.get_bool(self));
        let Some(anim) = self.scene_mut().and_then(|v| v.anim_mut(r)) else {
            return false;
        };
        match name {
            "CurrentFrame" => {
                anim.current_frame = int.max(0) as usize;
                anim.last_frame_time = 0;
                anim.finished = false;
            }
            "Looping" => anim.looping = Some(flag),
            "PixelPerfect" => {}
            _ => return false,
        }
        true
    }

    fn sprite_call(&mut self, owner: NativeId, name: &str) -> Option<Return> {
        let Some(Kind::Entity(r)) = self.native(owner).map(|n| n.kind.clone()) else {
            return None;
        };
        Some(match name {
            "Reset" => {
                if let Some(view) = self.scene_mut() {
                    view.reset_anim(r);
                }
                Return::null()
            }
            "Pause" | "Play" => {
                let paused = name == "Pause";
                if let Some(anim) = self.scene_mut().and_then(|v| v.anim_mut(r)) {
                    anim.paused = paused;
                }
                Return::null()
            }
            "GetFrame" | "DeleteFrame" | "AddFrame" | "InsertFrame" => {
                self.stub(&format!("Sprite.{name}"));
                Return::null()
            }
            _ => return None,
        })
    }
}

/// The `Math` object's methods (`SXMath`): every one takes floats and
/// returns a float, as the original engine does (`Floor` included).
fn math_call(name: &str, args: &[Value]) -> Option<Value> {
    let arg = |i: usize| -> f64 {
        match args.get(i) {
            Some(Value::Int(n)) => f64::from(*n),
            Some(Value::Float(f)) => *f,
            Some(Value::Bool(b)) => f64::from(u8::from(*b)),
            Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
            _ => 0.0,
        }
    };
    let x = arg(0);
    let result = match name {
        "Abs" => x.abs(),
        "Acos" => x.acos(),
        "Asin" => x.asin(),
        "Atan" => x.atan(),
        "Atan2" => x.atan2(arg(1)),
        "Ceil" => x.ceil(),
        "Cos" => x.cos(),
        "Cosh" => x.cosh(),
        "Exp" => x.exp(),
        "Floor" => x.floor(),
        "Log" => x.ln(),
        "Log10" => x.log10(),
        "Pow" => x.powf(arg(1)),
        "Sin" => x.sin(),
        "Sinh" => x.sinh(),
        "Tan" => x.tan(),
        "Tanh" => x.tanh(),
        "Sqrt" => x.sqrt(),
        "DegToRad" => x.to_radians(),
        "RadToDeg" => x.to_degrees(),
        _ => return None,
    };
    Some(Value::Float(result))
}
