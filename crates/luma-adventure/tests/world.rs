//! A synthetic game whose scripts drive the world: the game script
//! changes scene, the scene script inspects and modifies entities, an
//! entity script handles events.

use std::sync::atomic::{AtomicUsize, Ordering};

use luma_adventure::{Audio, Kind, Runtime};
use luma_archive::{PackageBuilder, Vfs};
use luma_formats::Image;
use luma_script::asm::Assembler;
use luma_script::{Instruction, Natives, State, Value};

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "luma-world-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    Image {
        width,
        height,
        pixels: color.repeat((width * height) as usize),
        has_alpha_channel: true,
    }
    .to_png()
    .unwrap()
}

/// An argument of a call in the assembled scripts.
enum Arg {
    Int(i32),
    Str(&'static str),
    Var(&'static str),
}

fn push_arg(a: &mut Assembler, arg: &Arg) {
    match arg {
        Arg::Int(i) => a.push_int(*i),
        Arg::Str(s) => a.push_string(s),
        Arg::Var(v) => a.push_var(v),
    }
}

/// `object.Method(args)`, leaving the result on the stack.
fn call(a: &mut Assembler, object: &str, method: &str, args: &[Arg]) {
    for arg in args.iter().rev() {
        push_arg(a, arg);
    }
    a.push_int(args.len() as i32);
    a.push_var(object);
    a.push_string(method);
    a.emit(Instruction::CallByExp);
}

/// `object.Prop`, pushed.
fn get(a: &mut Assembler, object: &str, prop: &str) {
    a.push_var_ref(object);
    a.push_string(prop);
    a.emit(Instruction::PushByExp);
}

/// `object.Prop = <top of stack>`.
fn set(a: &mut Assembler, object: &str, prop: &str) {
    a.push_var_ref(object);
    a.push_string(prop);
    a.emit(Instruction::PopByExp);
}

/// `result.key = <top of stack>` (`result` is an engine global object).
fn store(a: &mut Assembler, key: &str) {
    set(a, "result", key);
}

fn game_script() -> Vec<u8> {
    let mut a = Assembler::new("game.script");
    a.def_glob_var("result");
    a.emit(Instruction::CreateObject);
    a.pop_var("result");
    call(&mut a, "Game", "ExpandString", &[Arg::Str("/GREET/x")]);
    store(&mut a, "greet");
    get(&mut a, "Game", "ScreenWidth");
    store(&mut a, "width");
    get(&mut a, "Game", "Name");
    store(&mut a, "name");
    get(&mut a, "self", "Name");
    store(&mut a, "selfName");
    // var win = Game.LoadWindow("w.window"); win.Visible = false;
    a.def_var("win");
    call(&mut a, "Game", "LoadWindow", &[Arg::Str("w.window")]);
    a.pop_var("win");
    a.push_bool(false);
    set(&mut a, "win", "Visible");
    get(&mut a, "win", "Visible");
    store(&mut a, "winVisible");
    // Game.Interactive = false; result.interactive = Game.Interactive
    a.push_bool(false);
    set(&mut a, "Game", "Interactive");
    get(&mut a, "Game", "Interactive");
    store(&mut a, "interactive");
    // Scene = Game.Scene (a global the scene scripts use)
    a.def_glob_var("Scene");
    get(&mut a, "Game", "Scene");
    a.pop_var("Scene");
    call(&mut a, "Game", "PlaySound", &[Arg::Str("a.ogg")]);
    a.emit(Instruction::PopEmpty);
    call(&mut a, "Game", "PlaySound", &[Arg::Str("b.ogg")]);
    a.emit(Instruction::PopEmpty);
    call(
        &mut a,
        "Game",
        "ChangeScene",
        &[Arg::Str("scenes\\room.scene")],
    );
    a.emit(Instruction::PopEmpty);
    get(&mut a, "Game", "ChangingScene");
    store(&mut a, "changing");
    a.emit(Instruction::Ret);
    a.event("Keypress");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.def_var("kb");
    get(&mut a, "Game", "Keyboard");
    a.pop_var("kb");
    get(&mut a, "kb", "KeyCode");
    store(&mut a, "key");
    get(&mut a, "kb", "Key");
    store(&mut a, "keyText");
    get(&mut a, "kb", "IsShift");
    store(&mut a, "keyShift");
    call(&mut a, "kb", "IsKeyDown", &[Arg::Str("a")]);
    store(&mut a, "aDown");
    a.emit(Instruction::RetEvent);
    // on "OpenMenu": var m = Game.LoadWindow("menu.window");
    // m.GoSystemExclusive(); result.menuClosed = true;
    a.event("OpenMenu");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.def_var("m");
    call(&mut a, "Game", "LoadWindow", &[Arg::Str("menu.window")]);
    a.pop_var("m");
    call(&mut a, "m", "GoSystemExclusive", &[]);
    a.emit(Instruction::PopEmpty);
    a.push_bool(true);
    store(&mut a, "menuClosed");
    a.emit(Instruction::RetEvent);
    a.build()
}

/// The script of the test window: its `ok` button notifies it, and it
/// reads the keys it is given.
fn window_script() -> Vec<u8> {
    let mut a = Assembler::new("win.script");
    a.emit(Instruction::Ret);
    a.event("ok");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.push_bool(true);
    store(&mut a, "okPressed");
    a.emit(Instruction::RetEvent);
    a.event("Keypress");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.def_var("kb");
    get(&mut a, "Game", "Keyboard");
    a.pop_var("kb");
    get(&mut a, "kb", "KeyCode");
    store(&mut a, "winKey");
    a.emit(Instruction::RetEvent);
    a.build()
}

/// The script of the menu: its `take` button closes it.
fn menu_script() -> Vec<u8> {
    let mut a = Assembler::new("menu.script");
    a.emit(Instruction::Ret);
    a.event("take");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    call(&mut a, "self", "Close", &[]);
    a.emit(Instruction::PopEmpty);
    a.push_bool(true);
    store(&mut a, "take");
    a.emit(Instruction::RetEvent);
    a.build()
}

fn scene_init_script() -> Vec<u8> {
    let mut a = Assembler::new("scenes\\room\\scene_init.script");
    a.def_glob_var("result");
    a.def_glob_var("Scene");
    get(&mut a, "Scene", "Name");
    store(&mut a, "scene");
    get(&mut a, "self", "Name");
    store(&mut a, "sceneSelf");
    // var p = Scene.GetNode("PROP"); p.Active = false; ...
    a.def_var("p");
    call(&mut a, "Scene", "GetNode", &[Arg::Str("PROP")]);
    a.pop_var("p");
    get(&mut a, "p", "X");
    store(&mut a, "propX");
    a.push_bool(false);
    set(&mut a, "p", "Active");
    get(&mut a, "p", "Active");
    store(&mut a, "propActive");
    call(&mut a, "p", "SkipTo", &[Arg::Int(5), Arg::Int(6)]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "p", "Y");
    store(&mut a, "propY");
    get(&mut a, "p", "Type");
    store(&mut a, "propType");
    // Script properties on natives persist.
    a.push_int(7);
    set(&mut a, "p", "xResult");
    call(&mut a, "Scene", "GetNode", &[Arg::Str("prop")]);
    a.pop_var("p");
    get(&mut a, "p", "xResult");
    store(&mut a, "xResult");
    // var s = p.GetSpriteObject(); result.frames = s.NumFrames
    a.def_var("s");
    call(&mut a, "p", "GetSpriteObject", &[]);
    a.pop_var("s");
    get(&mut a, "s", "NumFrames");
    store(&mut a, "frames");
    get(&mut a, "s", "Finished");
    store(&mut a, "finishedAtStart");
    // Scene.SkipTo(100, 0); result.offset = Scene.OffsetX
    call(&mut a, "Scene", "SkipTo", &[Arg::Int(100), Arg::Int(0)]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "Scene", "OffsetX");
    store(&mut a, "offset");
    call(
        &mut a,
        "Scene",
        "IsWalkableAt",
        &[Arg::Int(10), Arg::Int(20)],
    );
    store(&mut a, "walkable");
    call(
        &mut a,
        "Scene",
        "IsWalkableAt",
        &[Arg::Int(10), Arg::Int(2)],
    );
    store(&mut a, "walkableAbove");
    // result.region = Scene.GetRegionAt(10, 20).Name
    a.def_var("r");
    call(
        &mut a,
        "Scene",
        "GetRegionAt",
        &[Arg::Int(10), Arg::Int(20)],
    );
    a.pop_var("r");
    get(&mut a, "r", "Name");
    store(&mut a, "region");
    get(&mut a, "r", "Blocked");
    store(&mut a, "regionBlocked");
    // Layers.
    get(&mut a, "Scene", "MainLayer");
    a.pop_var("r");
    get(&mut a, "r", "NumNodes");
    store(&mut a, "nodes");
    // The lamp handles LookAt; raise it.
    a.def_var("lamp");
    call(&mut a, "Scene", "GetNode", &[Arg::Str("lamp")]);
    a.pop_var("lamp");
    call(&mut a, "lamp", "CanHandleEvent", &[Arg::Str("lookat")]);
    store(&mut a, "lampHandles");
    call(&mut a, "lamp", "CanHandleEvent", &[Arg::Str("Take")]);
    store(&mut a, "lampTakes");
    call(&mut a, "lamp", "ApplyEvent", &[Arg::Str("LookAt")]);
    store(&mut a, "applied");
    call(
        &mut a,
        "lamp",
        "IsScriptRunning",
        &[Arg::Str("scenes\\room\\lamp.script")],
    );
    store(&mut a, "lampRunning");
    // A missing node and an undefined method.
    call(&mut a, "Scene", "GetNode", &[Arg::Str("nobody")]);
    store(&mut a, "nobody");
    call(&mut a, "p", "Explode", &[]);
    store(&mut a, "explode");
    get(&mut a, "Game", "PrevScene");
    store(&mut a, "prev");
    a.emit(Instruction::Ret);
    a.build()
}

fn scene_events_script() -> Vec<u8> {
    let mut a = Assembler::new("scenes\\room\\scene_events.script");
    let end = a.label();
    a.jmp(end);
    a.event("SceneShutdown");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.push_bool(true);
    store(&mut a, "shutdown");
    a.emit(Instruction::RetEvent);
    a.event("LeftClick");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.def_glob_var("Scene");
    get(&mut a, "Scene", "MouseX");
    store(&mut a, "clickX");
    get(&mut a, "Scene", "MouseY");
    store(&mut a, "clickY");
    a.emit(Instruction::RetEvent);
    a.bind(end);
    a.emit(Instruction::Ret);
    a.build()
}

fn lamp_script() -> Vec<u8> {
    let mut a = Assembler::new("scenes\\room\\lamp.script");
    let end = a.label();
    a.jmp(end);
    a.event("LookAt");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    get(&mut a, "this", "Name");
    store(&mut a, "looked");
    a.emit(Instruction::RetEvent);
    a.event("BeepDone");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.push_bool(true);
    store(&mut a, "beepDone");
    a.emit(Instruction::RetEvent);
    a.bind(end);
    a.emit(Instruction::Ret);
    a.build()
}

fn floor_script() -> Vec<u8> {
    let mut a = Assembler::new("scenes\\room\\floor.script");
    let end = a.label();
    a.jmp(end);
    a.event("ActorEntry");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    get(&mut a, "this", "Name");
    store(&mut a, "entered");
    a.emit(Instruction::RetEvent);
    a.bind(end);
    a.emit(Instruction::Ret);
    a.build()
}

/// Loads an actor, walks, turns, plays an animation and scrolls, every
/// synchronous call waiting for the actor.
fn walk_script() -> Vec<u8> {
    let mut a = Assembler::new("walk.script");
    a.def_glob_var("result");
    a.def_glob_var("Scene");
    a.def_glob_var("actor");
    get(&mut a, "Game", "Scene");
    a.pop_var("Scene");
    call(
        &mut a,
        "Game",
        "LoadActor",
        &[Arg::Str("actors\\bob.actor")],
    );
    a.pop_var("actor");
    a.push_var("actor");
    set(&mut a, "Game", "MainObject");
    get(&mut a, "actor", "Type");
    store(&mut a, "actorType");
    get(&mut a, "actor", "Name");
    store(&mut a, "actorName");
    get(&mut a, "actor", "Direction");
    store(&mut a, "dir0");
    get(&mut a, "actor", "Mood");
    store(&mut a, "mood");
    call(&mut a, "actor", "GoTo", &[Arg::Int(50), Arg::Int(24)]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "actor", "X");
    store(&mut a, "afterX");
    get(&mut a, "actor", "Y");
    store(&mut a, "afterY");
    get(&mut a, "actor", "Direction");
    store(&mut a, "dir1");
    get(&mut a, "actor", "Height");
    store(&mut a, "height");
    call(&mut a, "actor", "TurnTo", &[Arg::Int(0)]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "actor", "Direction");
    store(&mut a, "dir2");
    a.def_var("lamp");
    call(&mut a, "Scene", "GetNode", &[Arg::Str("lamp")]);
    a.pop_var("lamp");
    call(&mut a, "actor", "GoToObject", &[Arg::Var("lamp")]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "actor", "X");
    store(&mut a, "objX");
    get(&mut a, "actor", "Y");
    store(&mut a, "objY");
    get(&mut a, "actor", "Direction");
    store(&mut a, "dir3");
    call(&mut a, "actor", "IsWalking", &[]);
    store(&mut a, "walking");
    call(&mut a, "actor", "SkipTo", &[Arg::Int(5), Arg::Int(30)]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "actor", "X");
    store(&mut a, "skipX");
    call(&mut a, "actor", "PlayAnim", &[Arg::Str("art\\prop.sprite")]);
    store(&mut a, "animStarted");
    get(&mut a, "actor", "Ready");
    store(&mut a, "animDone");
    call(&mut a, "Scene", "ScrollTo", &[Arg::Var("actor")]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "Scene", "OffsetX");
    store(&mut a, "scrolled");
    call(&mut a, "actor", "GoToObject", &[Arg::Int(3)]);
    store(&mut a, "badGoTo");
    a.push_bool(true);
    store(&mut a, "done");
    a.emit(Instruction::Ret);
    a.build()
}

/// Loads an actor, makes it and an entity talk, checks the waits, the
/// states, fonts and subtitle settings.
fn talk_script() -> Vec<u8> {
    let mut a = Assembler::new("talk.script");
    a.def_glob_var("result");
    a.def_glob_var("Scene");
    a.def_glob_var("actor");
    get(&mut a, "Game", "Scene");
    a.pop_var("Scene");
    call(
        &mut a,
        "Game",
        "LoadActor",
        &[Arg::Str("actors\\bob.actor")],
    );
    a.pop_var("actor");
    call(&mut a, "actor", "SkipTo", &[Arg::Int(100), Arg::Int(24)]);
    a.emit(Instruction::PopEmpty);
    a.push_int(10);
    set(&mut a, "Game", "SubtitlesSpeed");
    get(&mut a, "Game", "SubtitlesSpeed");
    store(&mut a, "speed");
    get(&mut a, "Game", "CurrentTime");
    store(&mut a, "t0");
    // actor.Talk("/GREET/x") waits for the sentence: 1000 ms at least.
    call(&mut a, "actor", "Talk", &[Arg::Str("/GREET/x")]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "Game", "CurrentTime");
    store(&mut a, "t1");
    call(&mut a, "actor", "IsTalking", &[]);
    store(&mut a, "talkingAfter");
    call(&mut a, "actor", "TalkAsync", &[Arg::Str("Hi there")]);
    a.emit(Instruction::PopEmpty);
    call(&mut a, "actor", "IsTalking", &[]);
    store(&mut a, "talkingAsync");
    get(&mut a, "actor", "Ready");
    store(&mut a, "readyWhileTalking");
    a.def_var("lamp");
    call(&mut a, "Scene", "GetNode", &[Arg::Str("lamp")]);
    a.pop_var("lamp");
    call(
        &mut a,
        "lamp",
        "TalkAsync",
        &[Arg::Str("Lamp says"), Arg::Str(""), Arg::Int(500)],
    );
    a.emit(Instruction::PopEmpty);
    call(&mut a, "lamp", "IsTalking", &[]);
    store(&mut a, "lampTalking");
    get(&mut a, "lamp", "Ready");
    store(&mut a, "lampReady");
    call(&mut a, "actor", "StopTalk", &[]);
    store(&mut a, "stopped");
    call(&mut a, "actor", "StopTalk", &[]);
    store(&mut a, "stoppedAgain");
    call(&mut a, "actor", "GetFont", &[]);
    store(&mut a, "font0");
    call(&mut a, "actor", "SetFont", &[Arg::Str("fonts\\test.font")]);
    a.emit(Instruction::PopEmpty);
    call(&mut a, "actor", "GetFont", &[]);
    store(&mut a, "font1");
    a.push_int(20);
    set(&mut a, "actor", "SubtitlesWidth");
    get(&mut a, "actor", "SubtitlesWidth");
    store(&mut a, "subWidth");
    call(&mut a, "actor", "TalkAsync", &[Arg::Str("Wide")]);
    a.emit(Instruction::PopEmpty);
    a.push_bool(true);
    store(&mut a, "done");
    a.emit(Instruction::Ret);
    a.build()
}

/// Plays sounds on the game, a sound entity and the lamp, music on a
/// channel, and a sentence with its speech file.
fn sound_script() -> Vec<u8> {
    let mut a = Assembler::new("sound.script");
    a.def_glob_var("result");
    a.def_glob_var("Scene");
    a.def_glob_var("actor");
    get(&mut a, "Game", "Scene");
    a.pop_var("Scene");
    call(
        &mut a,
        "Game",
        "GetSoundLength",
        &[Arg::Str("audio\\beep.wav")],
    );
    store(&mut a, "len");
    call(&mut a, "Game", "PlaySound", &[Arg::Str("audio\\beep.wav")]);
    store(&mut a, "started");
    call(&mut a, "Game", "IsSoundPlaying", &[]);
    store(&mut a, "playing");
    a.def_var("hum");
    call(&mut a, "Scene", "GetNode", &[Arg::Str("hum")]);
    a.pop_var("hum");
    call(&mut a, "hum", "GetSoundVolume", &[]);
    store(&mut a, "humVol");
    call(&mut a, "hum", "IsSoundPlaying", &[]);
    store(&mut a, "humPlaying0");
    call(&mut a, "hum", "PlaySound", &[]);
    a.emit(Instruction::PopEmpty);
    call(&mut a, "hum", "IsSoundPlaying", &[]);
    store(&mut a, "humPlaying1");
    a.def_var("lamp");
    call(&mut a, "Scene", "GetNode", &[Arg::Str("lamp")]);
    a.pop_var("lamp");
    call(
        &mut a,
        "lamp",
        "PlaySoundEvent",
        &[Arg::Str("audio\\beep.wav"), Arg::Str("BeepDone")],
    );
    store(&mut a, "eventStarted");
    call(
        &mut a,
        "Game",
        "PlayMusicChannel",
        &[Arg::Int(1), Arg::Str("audio\\beep.wav"), Arg::Int(1)],
    );
    store(&mut a, "musicStarted");
    call(&mut a, "Game", "IsMusicChannelPlaying", &[Arg::Int(1)]);
    store(&mut a, "music");
    call(&mut a, "Game", "GetMusicChannel", &[Arg::Int(1)]);
    store(&mut a, "musicFile");
    call(&mut a, "Game", "SetGlobalMusicVolume", &[Arg::Int(40)]);
    a.emit(Instruction::PopEmpty);
    call(&mut a, "Game", "GetGlobalMusicVolume", &[]);
    store(&mut a, "musicVol");
    call(&mut a, "Game", "AddSpeechDir", &[Arg::Str("voices")]);
    store(&mut a, "dirAdded");
    call(
        &mut a,
        "Game",
        "LoadActor",
        &[Arg::Str("actors\\bob.actor")],
    );
    a.pop_var("actor");
    call(&mut a, "actor", "SkipTo", &[Arg::Int(100), Arg::Int(24)]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "Game", "CurrentTime");
    store(&mut a, "t0");
    // The speech file voices\GREET.wav lasts 250 ms: shorter than the
    // text would take.
    call(&mut a, "actor", "Talk", &[Arg::Str("/GREET/x")]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "Game", "CurrentTime");
    store(&mut a, "t1");
    call(&mut a, "Game", "IsMusicChannelPlaying", &[Arg::Int(1)]);
    store(&mut a, "musicLater");
    call(&mut a, "Game", "StopMusicChannel", &[Arg::Int(1)]);
    store(&mut a, "musicStopped");
    call(&mut a, "Game", "IsMusicChannelPlaying", &[Arg::Int(1)]);
    store(&mut a, "musicAfter");
    a.push_bool(true);
    store(&mut a, "done");
    a.emit(Instruction::Ret);
    a.build()
}

/// The prop's script: mouse events and a click.
fn prop_script() -> Vec<u8> {
    let mut a = Assembler::new("scenes\\room\\prop.script");
    let end = a.label();
    a.jmp(end);
    for (event, key) in [
        ("MouseEntry", "entered"),
        ("MouseLeave", "left"),
        ("LeftClick", "clicked"),
        ("RightClick", "rightClicked"),
        ("LeftDoubleClick", "doubleClicked"),
    ] {
        a.event(event);
        a.emit(Instruction::Scope);
        a.def_glob_var("result");
        get(&mut a, "this", "Name");
        store(&mut a, key);
        a.emit(Instruction::RetEvent);
    }
    a.bind(end);
    a.emit(Instruction::Ret);
    a.build()
}

/// Fades the screen out and reads the colour.
fn fade_script() -> Vec<u8> {
    let mut a = Assembler::new("fade.script");
    a.def_glob_var("result");
    get(&mut a, "Game", "CurrentTime");
    store(&mut a, "fadeT0");
    call(
        &mut a,
        "Game",
        "FadeOut",
        &[Arg::Int(100), Arg::Int(10), Arg::Int(20), Arg::Int(30)],
    );
    a.emit(Instruction::PopEmpty);
    get(&mut a, "Game", "CurrentTime");
    store(&mut a, "fadeT1");
    call(&mut a, "Game", "GetFadeColor", &[]);
    store(&mut a, "fadeColor");
    a.push_bool(true);
    store(&mut a, "faded");
    a.emit(Instruction::Ret);
    a.build()
}

fn game() -> std::path::PathBuf {
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 32,32 } }".to_vec(),
    );
    pkg.add_file(
        "default.game",
        b"GAME { NAME=\"Test\" SCRIPT=\"game.script\" STRING_TABLE=\"string.tab\" SYSTEM_FONT=\"fonts\\sys.font\" CURSOR=\"sprites\\cursor.sprite\" ACTIVE_CURSOR=\"art\\green.png\" NONINTERACTIVE_CURSOR=\"art\\red.png\" } AD_GAME { }"
            .to_vec(),
    );
    pkg.add_file("art\\font.png", png(128, 128, [255, 255, 255, 255]));
    pkg.add_file(
        "fonts\\sys.font",
        b"FONT { IMAGE=\"art\\font.png\" COLUMNS=16 TILE_WIDTH=8 TILE_HEIGHT=8 DEFAULT_WIDTH=6 }"
            .to_vec(),
    );
    pkg.add_file(
        "fonts\\test.font",
        b"FONT { IMAGE=\"art\\font.png\" COLUMNS=16 TILE_WIDTH=8 TILE_HEIGHT=10 DEFAULT_WIDTH=4 }"
            .to_vec(),
    );
    pkg.add_file("talk.script", talk_script());
    pkg.add_file("sound.script", sound_script());
    pkg.add_file("audio\\beep.wav", luma_audio::silent_wav(8000, 1, 300));
    pkg.add_file("voices\\GREET.wav", luma_audio::silent_wav(8000, 2, 250));
    pkg.add_file("string.tab", b"GREET\tHello\r\n".to_vec());
    pkg.add_file("game.script", game_script());
    pkg.add_file("win.script", window_script());
    pkg.add_file("menu.script", menu_script());
    pkg.add_file(
        "w.window",
        b"WINDOW {
  NAME=\"w\" X=2 Y=2 WIDTH=20 HEIGHT=16 SCRIPT=\"win.script\"
  BUTTON { NAME=\"ok\" X=1 Y=1 WIDTH=6 HEIGHT=6 IMAGE_HOVER=\"art\\green.png\" PARENT_NOTIFY=TRUE }
  STATIC { NAME=\"label\" TEXT=\"one|two\" X=8 Y=1 WIDTH=10 HEIGHT=8 FONT=\"fonts\\sys.font\" }
  EDIT { NAME=\"name\" X=1 Y=9 WIDTH=18 HEIGHT=6 MAX_LENGTH=3 }
}"
        .to_vec(),
    );
    pkg.add_file(
        "menu.window",
        b"WINDOW {
  NAME=\"menu\" X=10 Y=10 WIDTH=8 HEIGHT=8 MENU=TRUE IMAGE=\"art\\red.png\" SCRIPT=\"menu.script\"
  BUTTON { NAME=\"take\" X=0 Y=0 WIDTH=8 HEIGHT=4 PARENT_NOTIFY=TRUE }
}"
        .to_vec(),
    );
    pkg.add_file("scenes\\room\\scene_init.script", scene_init_script());
    pkg.add_file("scenes\\room\\scene_events.script", scene_events_script());
    pkg.add_file("scenes\\room\\lamp.script", lamp_script());
    pkg.add_file("scenes\\room\\floor.script", floor_script());
    pkg.add_file("scenes\\room\\prop.script", prop_script());
    pkg.add_file("fade.script", fade_script());
    pkg.add_file("art\\red.png", png(4, 4, [255, 0, 0, 255]));
    pkg.add_file("art\\green.png", png(4, 4, [0, 255, 0, 255]));
    pkg.add_file(
        "sprites\\cursor.sprite",
        b"SPRITE { LOOPING=TRUE FRAME { DELAY=50 IMAGE=\"art\\red.png\" } FRAME { DELAY=50 IMAGE=\"art\\green.png\" } }".to_vec(),
    );
    pkg.add_file("walk.script", walk_script());
    pkg.add_file(
        "art\\prop.sprite",
        b"SPRITE { LOOPING=FALSE FRAME { DELAY=100 IMAGE=\"art\\red.png\" } FRAME { DELAY=100 IMAGE=\"art\\green.png\" } }".to_vec(),
    );
    pkg.add_file(
        "art\\walk.sprite",
        b"SPRITE { LOOPING=TRUE FRAME { DELAY=10 IMAGE=\"art\\red.png\" HOTSPOT {0,8} } }".to_vec(),
    );
    pkg.add_file(
        "art\\stand.sprite",
        b"SPRITE { LOOPING=TRUE FRAME { DELAY=100 IMAGE=\"art\\green.png\" HOTSPOT {0,8} } }"
            .to_vec(),
    );
    pkg.add_file(
        "actors\\bob.actor",
        b"ACTOR {
  NAME=\"bob\" X=10 Y=24 SCALABLE=FALSE
  PROPERTY { NAME=\"Mood\" VALUE=\"calm\" }
  WALK { SPRITESET { LEFT=\"art\\walk.sprite\" RIGHT=\"art\\walk.sprite\" } }
  STAND { SPRITESET { LEFT=\"art\\stand.sprite\" } }
}"
        .to_vec(),
    );
    pkg.add_file(
        "scenes\\room.scene",
        b"SCENE {
  NAME=\"Room\"
  SCRIPT=\"scenes\\room\\scene_init.script\"
  SCRIPT=\"scenes\\room\\scene_events.script\"
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=200 HEIGHT=32
    ENTITY { NAME=\"prop\" X=8 Y=12 SPRITE=\"art\\prop.sprite\" SCRIPT=\"scenes\\room\\prop.script\" }
    REGION { NAME=\"floor\" SCRIPT=\"scenes\\room\\floor.script\" POINT {0,16} POINT {200,16} POINT {200,32} POINT {0,32} }
    ENTITY { NAME=\"lamp\" X=20 Y=20 WALK_TO_X=30 WALK_TO_Y=20 WALK_TO_DIR=4 SCRIPT=\"scenes\\room\\lamp.script\" }
  }
  ENTITY { NAME=\"hum\" SUBTYPE=\"sound\" SOUND=\"audio\\beep.wav\" SOUND_VOLUME=50 }
}"
        .to_vec(),
    );
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    dir
}

fn result(runtime: &Runtime, key: &str) -> Value {
    runtime
        .engine
        .global("result")
        .and_then(|r| r.get_property(key, &()))
        .unwrap_or(Value::Null)
}

#[test]
fn scripts_drive_the_game_and_its_scene() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    assert_eq!(runtime.world.strings().get("greet"), Some("Hello"));
    runtime.start();
    assert_eq!(
        runtime.engine.counts().total,
        1,
        "the game script is attached"
    );

    // Tick 1: the game script runs and schedules the scene change.
    let log = runtime.tick(16);
    assert_eq!(result(&runtime, "greet"), Value::from("Hello"));
    assert_eq!(result(&runtime, "width"), Value::Int(32));
    assert_eq!(result(&runtime, "name"), Value::from("Test"));
    assert_eq!(result(&runtime, "selfName"), Value::from("Test"));
    assert_eq!(result(&runtime, "winVisible"), Value::Bool(false));
    assert_eq!(result(&runtime, "interactive"), Value::Bool(false));
    assert!(!runtime.world.is_interactive());
    assert_eq!(result(&runtime, "changing"), Value::Bool(true));
    assert_eq!(runtime.world.scheduled_scene(), Some("scenes\\room.scene"));
    assert!(runtime.world.scene().is_none());
    let stubs: Vec<&String> = log
        .iter()
        .filter(|l| l.contains("not implemented"))
        .collect();
    assert!(stubs.is_empty(), "no stub expected: {log:?}");
    assert_eq!(
        log.iter()
            .filter(|l| l.starts_with("Error opening sound file"))
            .count(),
        2,
        "the two sounds are missing: {log:?}"
    );
    assert!(
        log.iter().all(|l| !l.starts_with("Runtime error")),
        "{log:?}"
    );

    // Tick 2: the scene loads, its scripts run, the lamp handler runs.
    let log = runtime.tick(16);
    assert!(
        log.iter().any(|l| l.contains("scene 'Room' loaded")),
        "{log:?}"
    );
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Room");
    assert_eq!(result(&runtime, "scene"), Value::from("Room"));
    assert_eq!(result(&runtime, "sceneSelf"), Value::from("Room"));
    assert_eq!(result(&runtime, "propX"), Value::Int(8));
    assert_eq!(result(&runtime, "propActive"), Value::Bool(false));
    assert_eq!(result(&runtime, "propY"), Value::Int(6));
    assert_eq!(result(&runtime, "propType"), Value::from("entity"));
    assert_eq!(result(&runtime, "xResult"), Value::Int(7));
    assert_eq!(result(&runtime, "frames"), Value::Int(2));
    assert_eq!(result(&runtime, "finishedAtStart"), Value::Bool(false));
    assert_eq!(result(&runtime, "offset"), Value::Int(84), "100 - 32/2");
    assert_eq!(runtime.world.offset(), (84, 0));
    assert_eq!(result(&runtime, "walkable"), Value::Bool(true));
    assert_eq!(result(&runtime, "walkableAbove"), Value::Bool(false));
    assert_eq!(result(&runtime, "region"), Value::from("floor"));
    assert_eq!(result(&runtime, "regionBlocked"), Value::Bool(false));
    assert_eq!(result(&runtime, "nodes"), Value::Int(3));
    assert_eq!(result(&runtime, "lampHandles"), Value::Bool(true));
    assert_eq!(result(&runtime, "lampTakes"), Value::Bool(false));
    assert_eq!(result(&runtime, "applied"), Value::Bool(true));
    assert_eq!(result(&runtime, "lampRunning"), Value::Bool(true));
    assert_eq!(result(&runtime, "looked"), Value::from("lamp"));
    assert_eq!(result(&runtime, "nobody"), Value::Null);
    assert_eq!(result(&runtime, "explode"), Value::Null);
    assert_eq!(result(&runtime, "prev"), Value::from(""));
    assert!(
        log.iter()
            .any(|l| l.contains("Call to undefined method 'Explode'")),
        "{log:?}"
    );
    let errors = log
        .iter()
        .filter(|l| l.starts_with("Runtime error"))
        .count();
    assert_eq!(errors, 1, "{log:?}");

    // The scene's state is what the scripts left: the prop is inactive at
    // (5, 6), the lamp's script and the scene's event script persist.
    let view = runtime.world.scene().unwrap();
    let prop = view
        .entity(luma_adventure::EntityRef::Node { layer: 0, index: 0 })
        .unwrap();
    assert!(!prop.active);
    assert_eq!((prop.x, prop.y), (5, 6));
    let persistent = runtime
        .engine
        .scripts()
        .iter()
        .filter(|s| s.state() == State::Persistent)
        .count();
    assert_eq!(
        persistent, 6,
        "game, window, scene_events, prop, lamp and floor"
    );
    let lamp = runtime.world.find_node("LAMP").unwrap();
    assert!(matches!(
        runtime.world.objects().get(lamp).unwrap().kind,
        Kind::Entity(_)
    ));
    assert!(runtime.engine.can_handle_event(lamp, "LookAt"));

    // Changing scene again: SceneShutdown runs, the old objects are gone,
    // PrevScene names the old scene.
    runtime.change_scene("scenes\\room.scene").unwrap();
    assert_eq!(result(&runtime, "shutdown"), Value::Bool(true));
    assert!(
        !runtime.world.objects().contains(lamp),
        "old nodes are invalid"
    );
    assert!(runtime.world.objects().contains(runtime.world.scene_id()));
    runtime.tick(16);
    assert_eq!(result(&runtime, "prev"), Value::from("Room"));
    assert_eq!(runtime.world.offset(), (84, 0), "scene_init ran again");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn sprite_objects_follow_the_animation() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.change_scene("scenes\\room.scene").unwrap();
    // Let the scene scripts run (they deactivate the prop); reactivate it.
    runtime.tick(1);
    let prop_ref = luma_adventure::EntityRef::Node { layer: 0, index: 0 };
    runtime
        .world
        .scene_mut()
        .unwrap()
        .entity_mut(prop_ref)
        .unwrap()
        .active = true;
    let prop = runtime.world.node_id(prop_ref).unwrap();
    let sprite = runtime.world.sprite_object_id(prop).unwrap();
    let finished = |runtime: &Runtime| {
        runtime
            .world
            .get_property(sprite, "Finished")
            .unwrap_or(Value::Null)
    };
    let frame = |runtime: &Runtime| {
        runtime
            .world
            .get_property(sprite, "CurrentFrame")
            .unwrap_or(Value::Null)
    };
    runtime.tick(1);
    assert_eq!(frame(&runtime), Value::Int(0));
    assert_eq!(finished(&runtime), Value::Bool(false));
    runtime.tick(150);
    assert_eq!(frame(&runtime), Value::Int(1));
    runtime.tick(150);
    assert_eq!(frame(&runtime), Value::Int(1));
    assert_eq!(finished(&runtime), Value::Bool(true));
    // Reset through the sprite object.
    runtime
        .world
        .set_property(sprite, "CurrentFrame", &Value::Int(0));
    assert_eq!(frame(&runtime), Value::Int(0));
    assert_eq!(finished(&runtime), Value::Bool(false));
    assert_eq!(
        runtime.world.get_property(sprite, "Owner"),
        Some(Value::Native(prop))
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn actors_walk_turn_and_scroll_the_view() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    let game = runtime.world.game_id();
    assert!(
        runtime
            .world
            .attach(&mut runtime.engine, game, "walk.script")
    );
    let mut log = Vec::new();
    let mut ticks = 0;
    while result(&runtime, "done") != Value::Bool(true) && ticks < 400 {
        log.extend(runtime.tick(16));
        ticks += 1;
    }
    assert_eq!(result(&runtime, "done"), Value::Bool(true), "{log:?}");
    assert_eq!(result(&runtime, "actorType"), Value::from("actor"));
    assert_eq!(result(&runtime, "actorName"), Value::from("bob"));
    assert_eq!(result(&runtime, "mood"), Value::from("calm"));
    assert_eq!(
        result(&runtime, "dir0"),
        Value::Int(6),
        "actors start facing left"
    );
    // GoTo waited for the walk: the actor is on the target, facing it.
    assert_eq!(result(&runtime, "afterX"), Value::Int(50));
    assert_eq!(result(&runtime, "afterY"), Value::Int(24));
    assert_eq!(result(&runtime, "dir1"), Value::Int(2));
    assert_eq!(result(&runtime, "height"), Value::Int(8));
    assert_eq!(
        result(&runtime, "dir2"),
        Value::Int(0),
        "TurnTo waited for the turn"
    );
    // GoToObject went to the lamp's walk-to point and direction.
    assert_eq!(result(&runtime, "objX"), Value::Int(30));
    assert_eq!(result(&runtime, "objY"), Value::Int(20));
    assert_eq!(result(&runtime, "dir3"), Value::Int(4));
    assert_eq!(result(&runtime, "walking"), Value::Bool(false));
    assert_eq!(result(&runtime, "skipX"), Value::Int(5));
    assert_eq!(result(&runtime, "animStarted"), Value::Bool(true));
    assert_eq!(result(&runtime, "animDone"), Value::Bool(true));
    // The view followed the main object to the left edge; ScrollTo
    // waited for it.
    assert_eq!(result(&runtime, "scrolled"), Value::Int(0));
    assert_eq!(runtime.world.offset(), (0, 0));
    assert_eq!(result(&runtime, "badGoTo"), Value::Null);
    assert!(
        log.iter()
            .any(|l| l.contains("accepts an entity refrence only")),
        "{log:?}"
    );
    assert_eq!(result(&runtime, "entered"), Value::from("floor"));
    assert!(
        ticks > 40,
        "walking, turning, playing and scrolling took {ticks} ticks"
    );
    let errors = log
        .iter()
        .filter(|l| l.starts_with("Runtime error"))
        .count();
    assert_eq!(errors, 1, "{log:?}");

    // The actor is a native the world knows, drawn with the scene.
    let actor = runtime.engine.global("actor").unwrap().as_native().unwrap();
    assert!(matches!(
        runtime.world.objects().get(actor).unwrap().kind,
        Kind::Actor
    ));
    assert_eq!(runtime.world.actor(actor).unwrap().def.name, "bob");
    assert_eq!(runtime.world.drawables().len(), 1);

    // A scene change keeps the game's actor; UnloadObject removes it.
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    assert!(runtime.world.objects().contains(actor));
    assert_eq!(runtime.world.actor(actor).unwrap().x, 5);
    assert!(runtime.world.unload_object(&mut runtime.engine, actor));
    assert!(!runtime.world.objects().contains(actor));
    assert!(runtime.world.actor(actor).is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn objects_talk_with_fonts_and_subtitles() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    assert_eq!(runtime.world.system_font(), Some("fonts\\sys.font"));
    assert!(runtime.world.fonts().get("FONTS/SYS.FONT").is_some());
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    let game = runtime.world.game_id();
    assert!(
        runtime
            .world
            .attach(&mut runtime.engine, game, "talk.script")
    );
    let mut log = Vec::new();
    let mut ticks = 0;
    while result(&runtime, "done") != Value::Bool(true) && ticks < 400 {
        log.extend(runtime.tick(16));
        ticks += 1;
    }
    assert_eq!(result(&runtime, "done"), Value::Bool(true), "{log:?}");
    assert_eq!(result(&runtime, "speed"), Value::Int(10));
    let (Value::Int(t0), Value::Int(t1)) = (result(&runtime, "t0"), result(&runtime, "t1")) else {
        panic!("times");
    };
    assert!(
        t1 - t0 >= 1000 && t1 - t0 < 1100,
        "Talk waited {} ms",
        t1 - t0
    );
    assert_eq!(result(&runtime, "talkingAfter"), Value::Bool(false));
    assert_eq!(result(&runtime, "talkingAsync"), Value::Bool(true));
    assert_eq!(result(&runtime, "readyWhileTalking"), Value::Bool(false));
    assert_eq!(result(&runtime, "lampTalking"), Value::Bool(true));
    assert_eq!(result(&runtime, "lampReady"), Value::Bool(false));
    assert_eq!(result(&runtime, "stopped"), Value::Bool(true));
    assert_eq!(result(&runtime, "stoppedAgain"), Value::Bool(false));
    assert_eq!(result(&runtime, "font0"), Value::Null);
    assert_eq!(result(&runtime, "font1"), Value::from("fonts\\test.font"));
    assert_eq!(result(&runtime, "subWidth"), Value::Int(20));
    assert!(
        log.iter().all(|l| !l.starts_with("Runtime error")),
        "{log:?}"
    );

    // The actor is saying "Wide" with the test font, 20 px wide, above
    // its head: the actor at (100, 24) is 8 high, the text 10, and the
    // box is clamped to the 32 px screen (the view is not scrolled: the
    // scene script's `Scene` global is unset without the game script).
    runtime.tick(16);
    let actor = runtime.engine.global("actor").unwrap().as_native().unwrap();
    assert!(runtime.world.is_talking(actor));
    let sentences = runtime.world.sentences();
    assert_eq!(sentences.len(), 2, "{sentences:?}");
    let wide = sentences
        .iter()
        .find(|s| s.text == "Wide")
        .expect("the actor's sentence");
    assert_eq!(wide.font, "fonts\\test.font");
    assert_eq!((wide.x, wide.y, wide.width), (12, 1, 20));
    let lamp = sentences
        .iter()
        .find(|s| s.text == "Lamp says")
        .expect("the lamp's sentence");
    assert_eq!(lamp.font, "fonts\\sys.font");
    assert_eq!(
        lamp.width, 16,
        "half the screen for an object in the middle"
    );
    assert_eq!(lamp.y, 0, "a tall wrapped text is clamped to the top");
    // A click while the game is not interactive skips the sentences.
    runtime
        .world
        .set_property(game, "Interactive", &Value::Bool(false));
    for _ in 0..25 {
        runtime.tick(16);
    }
    runtime.world.click_while_busy(false);
    runtime.tick(16);
    assert!(!runtime.world.is_talking(actor));
    assert!(runtime.world.sentences().is_empty());
    // Subtitles off: the actor still talks, nothing is drawn.
    runtime
        .world
        .set_property(game, "Subtitles", &Value::Bool(false));
    assert!(!runtime.world.subtitles_enabled());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn sounds_music_and_speech_play_on_the_game_clock() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    assert!(runtime.world.audio().device().is_none());
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    // The sound entity loaded its sound without playing it.
    let hum = runtime.world.find_node("hum").unwrap();
    assert!(runtime.world.sfx_of(hum).is_some());
    assert!(!runtime.world.is_sfx_playing(hum));
    let game = runtime.world.game_id();
    assert!(
        runtime
            .world
            .attach(&mut runtime.engine, game, "sound.script")
    );
    let mut log = Vec::new();
    let mut ticks = 0;
    while result(&runtime, "done") != Value::Bool(true) && ticks < 200 {
        log.extend(runtime.tick(16));
        ticks += 1;
    }
    assert_eq!(result(&runtime, "done"), Value::Bool(true), "{log:?}");
    assert_eq!(result(&runtime, "len"), Value::Int(300));
    assert_eq!(result(&runtime, "started"), Value::Bool(true));
    assert_eq!(result(&runtime, "playing"), Value::Bool(true));
    assert_eq!(result(&runtime, "humVol"), Value::Int(50));
    assert_eq!(result(&runtime, "humPlaying0"), Value::Bool(false));
    assert_eq!(result(&runtime, "humPlaying1"), Value::Bool(true));
    assert_eq!(result(&runtime, "eventStarted"), Value::Bool(true));
    assert_eq!(result(&runtime, "musicStarted"), Value::Bool(true));
    assert_eq!(result(&runtime, "music"), Value::Bool(true));
    assert_eq!(
        result(&runtime, "musicFile"),
        Value::from("audio\\beep.wav")
    );
    assert_eq!(result(&runtime, "musicVol"), Value::Int(40));
    assert_eq!(result(&runtime, "dirAdded"), Value::Bool(true));
    let (Value::Int(t0), Value::Int(t1)) = (result(&runtime, "t0"), result(&runtime, "t1")) else {
        panic!("times");
    };
    assert!(
        (250..300).contains(&(t1 - t0)),
        "the sentence lasted its speech file: {} ms",
        t1 - t0
    );
    assert_eq!(
        result(&runtime, "musicLater"),
        Value::Bool(true),
        "music loops"
    );
    assert_eq!(result(&runtime, "musicStopped"), Value::Bool(true));
    assert_eq!(result(&runtime, "musicAfter"), Value::Bool(false));
    assert!(
        log.iter().all(|l| !l.starts_with("Runtime error")),
        "{log:?}"
    );
    // The beeps end after 300 ms; the lamp's sound event fires then.
    for _ in 0..10 {
        runtime.tick(16);
    }
    assert!(!runtime.world.is_sfx_playing(game));
    assert!(!runtime.world.is_sfx_playing(hum));
    assert_eq!(result(&runtime, "beepDone"), Value::Bool(true));
    // The speech sound was unloaded with its sentence; the music with
    // its channel.
    assert!(runtime.world.music(1).is_none());
    assert_eq!(
        runtime.world.audio().len(),
        3,
        "game, hum and lamp sounds stay loaded"
    );
    // A scene change drops the sounds of the scene's objects.
    runtime.change_scene("scenes\\room.scene").unwrap();
    assert_eq!(
        runtime.world.audio().len(),
        2,
        "the game's sound and the new hum"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn mouse_keyboard_cursors_and_fades() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    assert_eq!(
        runtime.world.cursors().normal.as_deref(),
        Some("sprites\\cursor.sprite")
    );
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Room");
    // The game script turned the game non-interactive; the scene script
    // deactivated the prop. Undo both.
    let game = runtime.world.game_id();
    runtime
        .world
        .set_property(game, "Interactive", &Value::Bool(true));
    let prop_ref = luma_adventure::EntityRef::Node { layer: 0, index: 0 };
    runtime
        .world
        .scene_mut()
        .unwrap()
        .entity_mut(prop_ref)
        .unwrap()
        .active = true;
    let prop = runtime.world.node_id(prop_ref).unwrap();
    // The scene script moved the prop to (5, 6); its 4x4 sprite covers
    // (5..9, 6..10). Scroll the view back to the origin first.
    runtime.world.skip_to(0, 0);
    runtime.mouse_move(6, 7);
    runtime.tick(16);
    assert_eq!(runtime.world.active_object(), Some(prop));
    runtime.tick(16);
    assert_eq!(result(&runtime, "entered"), Value::from("prop"));
    assert_eq!(
        runtime.world.cursors().shown(),
        Some("art\\green.png"),
        "the active cursor over an object"
    );
    runtime.mouse_left_down();
    runtime.mouse_left_up();
    runtime.tick(16);
    assert_eq!(result(&runtime, "clicked"), Value::from("prop"));
    runtime.mouse_right_down();
    runtime.tick(16);
    assert_eq!(result(&runtime, "rightClicked"), Value::from("prop"));
    // A second click at once is a double click.
    runtime.mouse_left_down();
    runtime.mouse_left_up();
    runtime.tick(16);
    assert_eq!(result(&runtime, "doubleClicked"), Value::from("prop"));
    // Off the prop: the cursor is the default one and animates; a click
    // reaches the scene with the mouse in scene coordinates.
    runtime.mouse_move(20, 30);
    runtime.tick(16);
    assert_eq!(runtime.world.active_object(), None);
    runtime.tick(16);
    assert_eq!(result(&runtime, "left"), Value::from("prop"));
    assert_eq!(
        runtime.world.cursors().shown(),
        Some("sprites\\cursor.sprite")
    );
    let frame0 = runtime.world.cursors().frame();
    runtime.world.skip_to(100, 0);
    runtime.mouse_left_down();
    runtime.tick(16);
    assert_eq!(result(&runtime, "clickX"), Value::Int(20 + 84));
    assert_eq!(result(&runtime, "clickY"), Value::Int(30));
    for _ in 0..5 {
        runtime.tick(16);
    }
    assert_ne!(
        runtime.world.cursors().frame(),
        frame0,
        "the cursor animates"
    );
    // Not interactive: the wait cursor, no active object, no click.
    runtime.world.skip_to(0, 0);
    runtime
        .world
        .set_property(game, "Interactive", &Value::Bool(false));
    runtime.mouse_move(6, 7);
    runtime.tick(16);
    assert_eq!(runtime.world.active_object(), None);
    assert_eq!(runtime.world.cursors().shown(), Some("art\\red.png"));
    runtime.engine.set_global("result", Value::object());
    runtime.mouse_left_down();
    runtime.tick(16);
    assert_eq!(result(&runtime, "clicked"), Value::Null);
    runtime
        .world
        .set_property(game, "Interactive", &Value::Bool(true));
    // Keys: the code, the text and the modifiers reach the Keypress
    // handler; IsKeyDown sees the held key.
    runtime.key_down(65, Some('A'), true, false, false);
    runtime.tick(16);
    assert_eq!(result(&runtime, "key"), Value::Int(65));
    assert_eq!(result(&runtime, "keyText"), Value::from("A"));
    assert_eq!(result(&runtime, "keyShift"), Value::Bool(true));
    assert_eq!(result(&runtime, "aDown"), Value::Bool(true));
    runtime.key_up(65);
    assert!(!runtime.world.keyboard().is_key_down(97));
    // Fades: FadeOut waits for the fade.
    assert!(
        runtime
            .world
            .attach(&mut runtime.engine, game, "fade.script")
    );
    let mut ticks = 0;
    while result(&runtime, "faded") != Value::Bool(true) && ticks < 50 {
        runtime.tick(16);
        ticks += 1;
    }
    assert_eq!(result(&runtime, "faded"), Value::Bool(true));
    let (Value::Int(t0), Value::Int(t1)) = (result(&runtime, "fadeT0"), result(&runtime, "fadeT1"))
    else {
        panic!("times");
    };
    assert!((100..150).contains(&(t1 - t0)), "{} ms", t1 - t0);
    assert_eq!(
        result(&runtime, "fadeColor"),
        Value::Int(luma_script::rgba(10, 20, 30, 255))
    );
    assert_eq!(runtime.world.fader().color().unwrap().a, 255);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn windows_buttons_keys_and_menus() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    let game = runtime.world.game_id();
    runtime
        .world
        .set_property(game, "Interactive", &Value::Bool(true));
    // The game script loaded w.window and hid it.
    assert_eq!(runtime.world.window_ids().len(), 1);
    let win = runtime.world.window_ids()[0];
    let w = runtime.world.control(win).unwrap();
    assert_eq!(w.name, "w");
    assert!(!w.visible);
    assert_eq!((w.x, w.y, w.width, w.height), (2, 2, 20, 16));
    assert_eq!(runtime.world.widgets_of(win).len(), 3);
    let ok = runtime.world.widget(win, "OK").unwrap();
    let label = runtime.world.widget(win, "label").unwrap();
    let edit = runtime.world.widget(win, "name").unwrap();
    assert_eq!(runtime.world.class_name(ok), "button");
    assert_eq!(runtime.world.class_name(label), "static");
    assert_eq!(runtime.world.class_name(edit), "editor");
    assert_eq!(runtime.world.control(label).unwrap().text, "one\ntwo");
    assert_eq!(
        runtime.world.get_property(win, "NumControls"),
        Some(Value::Int(3))
    );
    assert_eq!(
        runtime.world.get_property(ok, "Parent"),
        Some(Value::Native(win))
    );
    assert_eq!(
        runtime.world.get_property(ok, "NextSibling"),
        Some(Value::Native(label))
    );
    // Hidden: nothing under the mouse.
    runtime.mouse_move(5, 5);
    runtime.tick(16);
    assert_eq!(runtime.world.active_object(), None);
    // Shown: the button is under the mouse and hovers.
    runtime
        .world
        .set_property(win, "Visible", &Value::Bool(true));
    runtime.tick(16);
    assert_eq!(runtime.world.active_object(), Some(ok));
    assert_eq!(runtime.world.focused_window(), Some(win));
    runtime.tick(16);
    assert!(runtime.world.control(ok).unwrap().button().unwrap().hover);
    // Click: pressed while held, fired on release, the window notified.
    runtime.mouse_left_down();
    runtime.tick(16);
    assert!(runtime.world.control(ok).unwrap().button().unwrap().press);
    runtime.mouse_left_up();
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(result(&runtime, "okPressed"), Value::Bool(true));
    assert!(!runtime.world.control(ok).unwrap().button().unwrap().press);
    // The editor took the focus; keys go to it, Tab finds no other
    // focusable control and the window's Keypress handler gets it.
    assert!(runtime.world.is_focused(edit));
    runtime.key_down(65, Some('a'), false, false, false);
    runtime.key_down(66, Some('b'), false, false, false);
    runtime.key_down(8, None, false, false, false);
    runtime.key_down(67, Some('c'), false, false, false);
    runtime.key_down(68, Some('d'), false, false, false);
    runtime.key_down(69, Some('e'), false, false, false);
    assert_eq!(runtime.world.control(edit).unwrap().text, "acd");
    runtime.key_down(9, None, false, false, false);
    runtime.tick(16);
    assert_eq!(result(&runtime, "winKey"), Value::Int(9));
    // A menu opened system-exclusive freezes the game: the timer stops
    // and the opening script waits.
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "OpenMenu", false);
    runtime.tick(16);
    let menu = runtime.world.window_ids()[1];
    assert_eq!(runtime.world.control(menu).unwrap().name, "menu");
    assert!(runtime.world.is_frozen());
    assert_eq!(
        runtime.world.get_property(menu, "SystemExclusive"),
        Some(Value::Bool(true))
    );
    assert_eq!(runtime.world.focused_window(), Some(menu));
    let frozen_at = runtime.world.timer();
    runtime.tick(16);
    assert_eq!(runtime.world.timer(), frozen_at);
    assert_eq!(result(&runtime, "menuClosed"), Value::Null);
    // Outside a system-exclusive window its shield takes the clicks and
    // nothing happens; the menu's own button closes it through its
    // script, which unfreezes the game.
    runtime.mouse_move(0, 0);
    runtime.tick(16);
    let shield = runtime.world.active_object().unwrap();
    assert_eq!(runtime.world.class_name(shield), "window");
    runtime.mouse_left_down();
    runtime.tick(16);
    runtime.mouse_left_up();
    runtime.tick(16);
    assert!(runtime.world.is_frozen());
    runtime.mouse_move(12, 12);
    runtime.tick(16);
    let take = runtime.world.widget(menu, "take").unwrap();
    assert_eq!(runtime.world.active_object(), Some(take));
    runtime.mouse_left_down();
    runtime.tick(16);
    runtime.mouse_left_up();
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(result(&runtime, "take"), Value::Bool(true));
    assert!(!runtime.world.is_frozen());
    assert!(!runtime.world.control(menu).unwrap().visible);
    runtime.tick(16);
    assert_eq!(result(&runtime, "menuClosed"), Value::Bool(true));
    assert!(runtime.world.timer() > frozen_at);
    // Shown again as a plain menu, a click outside closes it.
    runtime
        .world
        .set_property(menu, "Visible", &Value::Bool(true));
    runtime.mouse_move(0, 0);
    runtime.tick(16);
    let shield = runtime.world.active_object().unwrap();
    assert_eq!(runtime.world.control(shield).unwrap().name, "close");
    assert_eq!(runtime.world.class_name(shield), "button");
    runtime.mouse_left_down();
    runtime.tick(16);
    runtime.mouse_left_up();
    runtime.tick(16);
    assert!(!runtime.world.control(menu).unwrap().visible);
    // Unloading removes the window and its controls.
    assert!(runtime.world.unload_object(&mut runtime.engine, menu));
    assert_eq!(runtime.world.window_ids(), [win]);
    assert!(runtime.world.control(take).is_none());
    assert!(!runtime.world.objects().contains(take));
    // A missing file gives no window.
    assert!(
        runtime
            .world
            .load_window(&mut runtime.engine, "no.window")
            .is_none()
    );
}

#[test]
fn quitting_and_missing_scenes() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    assert!(!runtime.world.quit_requested());
    assert!(runtime.change_scene("scenes\\nope.scene").is_err());
    runtime.world.schedule_scene("scenes\\nope.scene", true);
    let log = runtime.tick(16);
    assert!(
        log.iter().any(|l| l.contains("cannot change scene")),
        "{log:?}"
    );
    assert!(runtime.world.scene().is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The local test game: its game script runs, then a real scene with
/// its entities and scripts, without a fatal error.
#[test]
#[ignore = "needs the local test game in testdata/local"]
fn runs_the_local_game() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/local/white-chamber");
    if !root.is_dir() {
        eprintln!("skipped: {} is absent", root.display());
        return;
    }
    let vfs = Vfs::open_game(&root).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    let mut log = Vec::new();
    for _ in 0..30 {
        log.extend(runtime.tick(16));
    }
    assert!(
        runtime.world.scene().is_some(),
        "the game script picked a scene"
    );
    runtime
        .change_scene("scenes/storeroom/storeroom.scene")
        .unwrap();
    for _ in 0..120 {
        log.extend(runtime.tick(16));
    }
    assert_eq!(runtime.world.scene().unwrap().scene.name, "StoreRoom");
    assert!(log.iter().all(|l| !l.starts_with("Fatal")), "{log:?}");
    // Speech: the main actor says a line whose Ogg file the game has.
    runtime.world.sounds_mut().add_speech_dir("english");
    let actor = runtime.world.main_object().expect("the main actor");
    assert!(runtime.world.talk(
        actor,
        "/STORER0032/It's an ID card scanner for the gate",
        None,
        0,
        None,
        luma_formats::font::TextAlign::Center,
    ));
    let sentence = runtime
        .world
        .actor(actor)
        .unwrap()
        .sentence
        .clone()
        .unwrap();
    assert!(
        sentence.sound.is_some(),
        "the speech file was found and decoded"
    );
    assert!(
        sentence.duration > 1000 && sentence.duration < 10000,
        "the sentence lasts its sound: {} ms",
        sentence.duration
    );
    eprintln!("speech sound: {} ms", sentence.duration);
    let errors: Vec<&String> = log
        .iter()
        .filter(|l| l.starts_with("Runtime error"))
        .collect();
    eprintln!("{} log lines, {} runtime errors", log.len(), errors.len());
    for line in &log {
        eprintln!("{line}");
    }
    let counts = runtime.engine.counts();
    eprintln!("{counts:?}");
}
