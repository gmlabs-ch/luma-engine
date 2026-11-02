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
    Null,
}

fn push_arg(a: &mut Assembler, arg: &Arg) {
    match arg {
        Arg::Int(i) => a.push_int(*i),
        Arg::Str(s) => a.push_string(s),
        Arg::Var(v) => a.push_var(v),
        Arg::Null => a.emit(Instruction::PushNull),
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
    // on "PlayIntro": Game.PlayTheora("intro.ogv"); result.afterVideo = true;
    a.event("PlayIntro");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    call(&mut a, "Game", "PlayTheora", &[Arg::Str("intro.ogv")]);
    store(&mut a, "videoStarted");
    a.push_bool(true);
    store(&mut a, "afterVideo");
    a.emit(Instruction::RetEvent);
    // on "PlayIntroPos": Game.PlayTheora("intro.ogv", 0, 4, 6); result.afterVideo2 = true;
    a.event("PlayIntroPos");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    call(
        &mut a,
        "Game",
        "PlayTheora",
        &[Arg::Str("intro.ogv"), Arg::Int(0), Arg::Int(4), Arg::Int(6)],
    );
    a.emit(Instruction::PopEmpty);
    a.push_bool(true);
    store(&mut a, "afterVideo2");
    a.emit(Instruction::RetEvent);
    // on "PlayMissing": result.missing = Game.PlayTheora("none.ogv");
    a.event("PlayMissing");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    call(&mut a, "Game", "PlayTheora", &[Arg::Str("none.ogv")]);
    store(&mut a, "missing");
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

/// A dialogue: two choices, one of them offered once per branch; the
/// pick, its text and the count go to `result`.
fn dialogue_script() -> Vec<u8> {
    let mut a = Assembler::new("dialogue.script");
    a.emit(Instruction::Ret);
    a.event("dlg");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    call(&mut a, "Game", "StartDlgBranch", &[Arg::Str("chat")]);
    a.emit(Instruction::PopEmpty);
    call(
        &mut a,
        "Game",
        "AddResponse",
        &[Arg::Int(1), Arg::Str("Yes")],
    );
    a.emit(Instruction::PopEmpty);
    call(
        &mut a,
        "Game",
        "AddResponseOnce",
        &[Arg::Int(2), Arg::Str("/GREET/x")],
    );
    a.emit(Instruction::PopEmpty);
    call(&mut a, "Game", "GetNumResponses", &[]);
    store(&mut a, "num");
    call(&mut a, "Game", "GetResponse", &[]);
    store(&mut a, "chosen");
    get(&mut a, "Game", "LastResponse");
    store(&mut a, "last");
    a.emit(Instruction::RetEvent);
    a.event("bye");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    call(&mut a, "Game", "EndDlgBranch", &[]);
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::RetEvent);
    a.build()
}

/// The script of a verb entity in a window's container: a click notes
/// it and swaps its sprite.
fn verb_script() -> Vec<u8> {
    let mut a = Assembler::new("verb.script");
    a.emit(Instruction::Ret);
    a.event("LeftClick");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    get(&mut a, "self", "Name");
    store(&mut a, "verbClicked");
    call(&mut a, "self", "SetSprite", &[Arg::Str("art\\red.png")]);
    a.emit(Instruction::PopEmpty);
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

/// Edits the prop's sprite through its frame and sub-frame objects.
fn sprite_edit_script() -> Vec<u8> {
    let mut a = Assembler::new("sprite_edit.script");
    a.def_glob_var("result");
    a.def_glob_var("Scene");
    get(&mut a, "Game", "Scene");
    a.pop_var("Scene");
    // var p = Scene.GetNode("prop"); var s = p.GetSpriteObject();
    a.def_var("p");
    call(&mut a, "Scene", "GetNode", &[Arg::Str("prop")]);
    a.pop_var("p");
    a.def_var("s");
    call(&mut a, "p", "GetSpriteObject", &[]);
    a.pop_var("s");
    // var f = s.GetFrame(0); result.frameType = f.Type; ...
    a.def_var("f");
    call(&mut a, "s", "GetFrame", &[Arg::Int(0)]);
    a.pop_var("f");
    get(&mut a, "f", "Type");
    store(&mut a, "frameType");
    get(&mut a, "f", "Delay");
    store(&mut a, "delayBefore");
    a.push_int(30);
    set(&mut a, "f", "Delay");
    get(&mut a, "f", "Delay");
    store(&mut a, "delayAfter");
    get(&mut a, "f", "NumSubframes");
    store(&mut a, "numSub");
    // var sub = f.GetSubframe(0); sub.MirrorX = true; f.HotspotY = 7
    // (a frame with one sub-frame answers for it).
    a.def_var("sub");
    call(&mut a, "f", "GetSubframe", &[Arg::Int(0)]);
    a.pop_var("sub");
    get(&mut a, "sub", "Type");
    store(&mut a, "subType");
    a.push_bool(true);
    set(&mut a, "sub", "MirrorX");
    get(&mut a, "f", "MirrorX");
    store(&mut a, "mirror");
    a.push_int(7);
    set(&mut a, "f", "HotspotY");
    get(&mut a, "sub", "HotspotY");
    store(&mut a, "hotY");
    call(&mut a, "sub", "GetImage", &[]);
    store(&mut a, "image");
    get(&mut a, "sub", "TransparentColor");
    store(&mut a, "transparent");
    a.push_int(0x00ff_0000);
    set(&mut a, "sub", "AlphaColor");
    get(&mut a, "sub", "AlphaColor");
    store(&mut a, "alpha");
    // var added = s.AddFrame("art\green.png"); added.Delay = 40;
    a.def_var("added");
    call(&mut a, "s", "AddFrame", &[Arg::Str("art\\green.png")]);
    a.pop_var("added");
    a.push_int(40);
    set(&mut a, "added", "Delay");
    get(&mut a, "s", "NumFrames");
    store(&mut a, "numFrames");
    call(&mut a, "s", "GetFrame", &[Arg::Int(9)]);
    store(&mut a, "outOfRange");
    // Events and sounds of a frame.
    call(&mut a, "f", "AddEvent", &[Arg::Str("Blink")]);
    a.emit(Instruction::PopEmpty);
    call(&mut a, "f", "AddEvent", &[Arg::Str("blink")]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "f", "NumEvents");
    store(&mut a, "numEvents");
    call(&mut a, "f", "SetSound", &[Arg::Str("audio\\beep.wav")]);
    store(&mut a, "soundOk");
    call(&mut a, "f", "GetSound", &[]);
    store(&mut a, "sound");
    call(&mut a, "f", "SetSound", &[Arg::Str("audio\\nope.wav")]);
    store(&mut a, "soundBad");
    call(&mut a, "f", "GetSound", &[]);
    store(&mut a, "soundAfter");
    // s.DeleteFrame(1): the added frame's handle follows its frame.
    call(&mut a, "s", "DeleteFrame", &[Arg::Int(1)]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "s", "NumFrames");
    store(&mut a, "numAfterDelete");
    get(&mut a, "added", "Delay");
    store(&mut a, "addedDelay");
    // s.InsertFrame(0, null): an empty frame first; f is now frame 1.
    call(&mut a, "s", "InsertFrame", &[Arg::Int(0), Arg::Null]);
    store(&mut a, "inserted");
    get(&mut a, "s", "NumFrames");
    store(&mut a, "numAfterInsert");
    get(&mut a, "f", "Delay");
    store(&mut a, "fDelay");
    a.def_var("first");
    call(&mut a, "s", "GetFrame", &[Arg::Int(0)]);
    a.pop_var("first");
    get(&mut a, "first", "NumSubframes");
    store(&mut a, "firstSub");
    // s.DeleteFrame(first): by object.
    call(&mut a, "s", "DeleteFrame", &[Arg::Var("first")]);
    a.emit(Instruction::PopEmpty);
    get(&mut a, "s", "NumFrames");
    store(&mut a, "numAtEnd");
    get(&mut a, "f", "Delay");
    store(&mut a, "fDelayAtEnd");
    a.push_bool(true);
    store(&mut a, "done");
    a.emit(Instruction::Ret);
    a.build()
}

/// Notes the frame events of a sprite (`APPLY_EVENT`) and `FrameChanged`.
fn cue_script() -> Vec<u8> {
    let mut a = Assembler::new("cue.script");
    let end = a.label();
    a.jmp(end);
    for (event, key) in [
        ("Tick", "tick"),
        ("Tock", "tock"),
        ("Step", "step"),
        ("FrameChanged", "frameChanged"),
    ] {
        a.event(event);
        a.emit(Instruction::Scope);
        a.def_glob_var("result");
        get(&mut a, "this", "Type");
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

/// Loads the actor, takes an item, starts the music and walks; its
/// events save and load the game.
fn save_script() -> Vec<u8> {
    let mut a = Assembler::new("save.script");
    a.def_glob_var("result");
    a.def_glob_var("actor");
    a.push_int(0);
    store(&mut a, "afterSave");
    call(
        &mut a,
        "Game",
        "LoadActor",
        &[Arg::Str("actors\\bob.actor")],
    );
    a.pop_var("actor");
    a.push_var("actor");
    set(&mut a, "Game", "MainObject");
    call(&mut a, "Game", "TakeItem", &[Arg::Str("key")]);
    a.emit(Instruction::PopEmpty);
    call(
        &mut a,
        "Game",
        "PlayMusic",
        &[Arg::Str("audio\\beep.wav"), Arg::Int(1)],
    );
    a.emit(Instruction::PopEmpty);
    call(&mut a, "actor", "GoTo", &[Arg::Int(50), Arg::Int(24)]);
    a.emit(Instruction::PopEmpty);
    a.push_bool(true);
    store(&mut a, "arrived");
    a.emit(Instruction::Ret);
    // on "SaveIt": result.saved = Game.SaveGame(3, "mid walk");
    // result.afterSave = result.afterSave + 1;
    a.event("SaveIt");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    call(
        &mut a,
        "Game",
        "SaveGame",
        &[Arg::Int(3), Arg::Str("mid walk")],
    );
    store(&mut a, "saved");
    get(&mut a, "result", "afterSave");
    a.push_int(1);
    a.emit(Instruction::Add);
    store(&mut a, "afterSave");
    a.emit(Instruction::RetEvent);
    // on "LoadIt": Game.LoadGame(3); result.afterLoadCall = true;
    a.event("LoadIt");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    call(&mut a, "Game", "LoadGame", &[Arg::Int(3)]);
    store(&mut a, "loadResult");
    a.push_bool(true);
    store(&mut a, "afterLoadCall");
    a.emit(Instruction::RetEvent);
    a.event("BeforeSave");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.push_bool(true);
    store(&mut a, "beforeSave");
    a.emit(Instruction::RetEvent);
    a.event("AfterLoad");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    a.push_bool(true);
    store(&mut a, "loaded");
    a.emit(Instruction::RetEvent);
    a.build()
}

fn settings_script() -> Vec<u8> {
    // Game.RegWriteNumber("Volume", 80); Game.RegWriteString("Lang", "fr");
    // then reads them back, with defaults for a missing key.
    let mut a = Assembler::new("settings.script");
    a.def_glob_var("result");
    call(
        &mut a,
        "Game",
        "RegReadNumber",
        &[Arg::Str("Volume"), Arg::Int(100)],
    );
    store(&mut a, "volumeBefore");
    call(
        &mut a,
        "Game",
        "RegWriteNumber",
        &[Arg::Str("Volume"), Arg::Int(80)],
    );
    a.emit(Instruction::PopEmpty);
    call(
        &mut a,
        "Game",
        "RegWriteString",
        &[Arg::Str("Lang"), Arg::Str("fr")],
    );
    a.emit(Instruction::PopEmpty);
    call(
        &mut a,
        "Game",
        "RegReadNumber",
        &[Arg::Str("volume"), Arg::Int(100)],
    );
    store(&mut a, "volume");
    call(
        &mut a,
        "Game",
        "RegReadString",
        &[Arg::Str("Lang"), Arg::Str("en")],
    );
    store(&mut a, "lang");
    call(
        &mut a,
        "Game",
        "RegReadString",
        &[Arg::Str("Missing"), Arg::Str("none")],
    );
    store(&mut a, "missing");
    call(
        &mut a,
        "Game",
        "RegReadNumber",
        &[Arg::Str("Lang"), Arg::Int(7)],
    );
    store(&mut a, "langAsNumber");
    a.emit(Instruction::Ret);
    a.build()
}

fn services_script() -> Vec<u8> {
    // The game's system properties and services.
    let mut a = Assembler::new("services.script");
    a.def_glob_var("result");
    get(&mut a, "Game", "FPS");
    store(&mut a, "fps");
    get(&mut a, "Game", "WindowsTime");
    store(&mut a, "windowsTime");
    get(&mut a, "Game", "CurrentTime");
    store(&mut a, "currentTime");
    get(&mut a, "Game", "SoundAvailable");
    store(&mut a, "soundAvailable");
    get(&mut a, "Game", "WindowedMode");
    store(&mut a, "windowed");
    get(&mut a, "Game", "Accelerated");
    store(&mut a, "accelerated");
    // Game.SFXVolume = 40; Game.MasterVolume = 70;
    a.push_int(40);
    set(&mut a, "Game", "SFXVolume");
    a.push_int(70);
    set(&mut a, "Game", "MasterVolume");
    get(&mut a, "Game", "SFXVolume");
    store(&mut a, "sfx");
    get(&mut a, "Game", "MasterVolume");
    store(&mut a, "master");
    call(&mut a, "Game", "GetGlobalSFXVolume", &[]);
    store(&mut a, "sfxMethod");
    get(&mut a, "Game", "TextEncoding");
    store(&mut a, "encoding");
    a.push_int(5);
    set(&mut a, "Game", "TextEncoding");
    get(&mut a, "Game", "TextEncoding");
    store(&mut a, "encodingClamped");
    get(&mut a, "Game", "StartupScene");
    store(&mut a, "startupBefore");
    a.push_string("scenes\\hall.scene");
    set(&mut a, "Game", "StartupScene");
    get(&mut a, "Game", "StartupScene");
    store(&mut a, "startup");
    a.push_string("Hi");
    set(&mut a, "Game", "Caption");
    call(&mut a, "Game", "Caption", &[Arg::Int(0)]);
    store(&mut a, "caption");
    call(&mut a, "Game", "GetFileChecksum", &[Arg::Str("string.tab")]);
    store(&mut a, "crc");
    call(
        &mut a,
        "Game",
        "GetFileChecksum",
        &[Arg::Str("string.tab"), Arg::Int(1)],
    );
    store(&mut a, "crcHex");
    call(&mut a, "Game", "GetFileChecksum", &[Arg::Str("nope.tab")]);
    store(&mut a, "crcMissing");
    call(&mut a, "Game", "OpenDocument", &[Arg::Str("")]);
    store(&mut a, "openedNothing");
    call(&mut a, "Game", "RunScript", &[Arg::Str("settings.script")]);
    store(&mut a, "ran");
    call(&mut a, "Game", "RunScript", &[Arg::Str("nope.script")]);
    store(&mut a, "ranMissing");
    a.emit(Instruction::Ret);
    a.build()
}

fn misc_script() -> Vec<u8> {
    // Game.Msg("Hello there"); Game.LockMouseRect(4, 4, 20, 20);
    let mut a = Assembler::new("misc.script");
    a.def_glob_var("result");
    call(&mut a, "Game", "Msg", &[Arg::Str("Hello there")]);
    a.emit(Instruction::PopEmpty);
    call(
        &mut a,
        "Game",
        "LockMouseRect",
        &[Arg::Int(20), Arg::Int(20), Arg::Int(4), Arg::Int(4)],
    );
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::Ret);
    // on "Unlock": Game.LockMouseRect();
    a.event("Unlock");
    a.emit(Instruction::Scope);
    call(&mut a, "Game", "LockMouseRect", &[]);
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::RetEvent);
    // on "DoReset": Game.Reset();
    a.event("DoReset");
    a.emit(Instruction::Scope);
    call(&mut a, "Game", "Reset", &[]);
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::RetEvent);
    // on "NewBox": result.box = Game.LoadInventoryBox("inv2.def");
    // result.noBox = Game.LoadInventoryBox("nope.def");
    a.event("NewBox");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    call(&mut a, "Game", "LoadInventoryBox", &[Arg::Str("inv2.def")]);
    store(&mut a, "box");
    a.emit(Instruction::RetEvent);
    a.build()
}

fn shots_script() -> Vec<u8> {
    // Screenshots and the save thumbnail.
    let mut a = Assembler::new("shots.script");
    a.def_glob_var("result");
    call(&mut a, "Game", "Screenshot", &[Arg::Str("shot")]);
    store(&mut a, "shot1");
    call(&mut a, "Game", "Screenshot", &[Arg::Str("shot")]);
    store(&mut a, "shot2");
    call(
        &mut a,
        "Game",
        "ScreenshotEx",
        &[Arg::Str("thumbs\\t.png"), Arg::Int(8), Arg::Int(4)],
    );
    store(&mut a, "shotEx");
    call(&mut a, "Game", "StoreSaveThumbnail", &[]);
    store(&mut a, "thumb");
    a.emit(Instruction::Ret);
    a.event("Store");
    a.emit(Instruction::Scope);
    call(&mut a, "Game", "StoreSaveThumbnail", &[]);
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::RetEvent);
    a.event("Drop");
    a.emit(Instruction::Scope);
    call(&mut a, "Game", "DeleteSaveThumbnail", &[]);
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::RetEvent);
    // on "SaveIt": result.saved = Game.SaveGame(6, "with thumb");
    a.event("SaveIt");
    a.emit(Instruction::Scope);
    a.def_glob_var("result");
    call(
        &mut a,
        "Game",
        "SaveGame",
        &[Arg::Int(6), Arg::Str("with thumb")],
    );
    store(&mut a, "saved");
    a.emit(Instruction::RetEvent);
    a.build()
}

fn screens_script() -> Vec<u8> {
    // The loading and saving screens and the loading icon.
    let mut a = Assembler::new("screens.script");
    call(
        &mut a,
        "Game",
        "SetLoadingScreen",
        &[Arg::Str("art\\green.png"), Arg::Int(2), Arg::Int(3)],
    );
    a.emit(Instruction::PopEmpty);
    call(
        &mut a,
        "Game",
        "SetSavingScreen",
        &[Arg::Str("art\\red.png"), Arg::Int(4), Arg::Int(5)],
    );
    a.emit(Instruction::PopEmpty);
    call(
        &mut a,
        "Game",
        "DisplayLoadingIcon",
        &[Arg::Str("art\\prop.sprite"), Arg::Int(6), Arg::Int(7)],
    );
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::Ret);
    // on "Persistent": Game.DisplayLoadingIcon("art\prop.sprite", 1, 1, true);
    a.event("Persistent");
    a.emit(Instruction::Scope);
    a.push_bool(true);
    a.push_int(1);
    a.push_int(1);
    a.push_string("art\\prop.sprite");
    a.push_int(4);
    a.push_var("Game");
    a.push_string("DisplayLoadingIcon");
    a.emit(Instruction::CallByExp);
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::RetEvent);
    a.event("Hide");
    a.emit(Instruction::Scope);
    call(&mut a, "Game", "HideLoadingIcon", &[]);
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::RetEvent);
    // on "ClearLoad": Game.SetLoadingScreen(null, 0, 0);
    a.event("ClearLoad");
    a.emit(Instruction::Scope);
    a.push_int(0);
    a.push_int(0);
    a.emit(Instruction::PushNull);
    a.push_int(3);
    a.push_var("Game");
    a.push_string("SetLoadingScreen");
    a.emit(Instruction::CallByExp);
    a.emit(Instruction::PopEmpty);
    a.emit(Instruction::RetEvent);
    a.build()
}

fn hall_init_script() -> Vec<u8> {
    // Records the state the hall's nodes arrive in, then changes it.
    let mut a = Assembler::new("scenes\\hall\\hall_init.script");
    a.def_glob_var("result");
    a.def_glob_var("Scene");
    get(&mut a, "Scene", "PersistentState");
    store(&mut a, "hallPersistent");
    a.def_var("d");
    call(&mut a, "Scene", "GetNode", &[Arg::Str("door")]);
    a.pop_var("d");
    call(&mut a, "d", "GetSprite", &[]);
    store(&mut a, "doorSprite");
    get(&mut a, "d", "Active");
    store(&mut a, "doorActive");
    get(&mut a, "d", "Caption");
    store(&mut a, "doorCaption");
    get(&mut a, "d", "AlphaColor");
    store(&mut a, "doorAlpha");
    a.def_var("x");
    call(&mut a, "Scene", "GetNode", &[Arg::Str("exit")]);
    a.pop_var("x");
    get(&mut a, "x", "Active");
    store(&mut a, "exitActive");
    a.def_var("f");
    call(&mut a, "Scene", "GetNode", &[Arg::Str("fixed")]);
    a.pop_var("f");
    get(&mut a, "f", "Active");
    store(&mut a, "fixedActive");
    call(&mut a, "d", "SetSprite", &[Arg::Str("art\\walk.sprite")]);
    a.emit(Instruction::PopEmpty);
    a.push_bool(false);
    set(&mut a, "d", "Active");
    a.push_string("opened");
    set(&mut a, "d", "Caption");
    a.push_int(0x1122_3344);
    set(&mut a, "d", "AlphaColor");
    a.push_bool(false);
    set(&mut a, "x", "Active");
    a.push_bool(false);
    set(&mut a, "f", "Active");
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
        b"GAME { NAME=\"Test\" SCRIPT=\"game.script\" STRING_TABLE=\"string.tab\" SYSTEM_FONT=\"fonts\\sys.font\" CURSOR=\"sprites\\cursor.sprite\" ACTIVE_CURSOR=\"art\\green.png\" NONINTERACTIVE_CURSOR=\"art\\red.png\" THUMBNAIL_WIDTH=8 THUMBNAIL_HEIGHT=4 } AD_GAME { ITEMS=\"items.items\" INVENTORY_BOX=\"inv.def\" RESPONSE_BOX=\"responses.def\" }"
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
    pkg.add_file("fr.tab", b"GREET\tBonjour\r\n".to_vec());
    pkg.add_file(
        "items2.items",
        b"ITEM { NAME=\"key\" CAPTION=\"/GREET/x\" IMAGE=\"art\\green.png\" }
ITEM { NAME=\"gem\" CAPTION=\"Gem\" IMAGE=\"art\\red.png\" }"
            .to_vec(),
    );
    pkg.add_file("game.script", game_script());
    pkg.add_file("win.script", window_script());
    pkg.add_file("intro.ogv", luma_video::synthetic_clip(32, 32, 10, 10));
    pkg.add_file("intro.sub", b"{2}{5}Hello|intro\n{7}{0}Bye".to_vec());
    pkg.add_file(
        "items.items",
        b"ITEM { NAME=\"key\" CAPTION=\"/GREET/x\" IMAGE=\"art\\green.png\" SPRITE_HOVER=\"art\\red.png\" CURSOR=\"art\\red.png\" CURSOR_HOVER=\"art\\green.png\" PROPERTY { NAME=\"weight\" VALUE=\"3\" } }
ITEM { NAME=\"coin\" IMAGE=\"art\\red.png\" CURSOR=\"art\\green.png\" CURSOR_COMBINED=FALSE }"
            .to_vec(),
    );
    pkg.add_file(
        "inv.def",
        b"INVENTORY_BOX {
  ITEM_WIDTH=8 ITEM_HEIGHT=8 SPACING=0 SCROLL_BY=1 HIDE_SELECTED=TRUE
  AREA { 4, 0, 12, 8 }
  WINDOW {
    X=0 Y=0 WIDTH=16 HEIGHT=8
    BUTTON { NAME=\"prev\" X=0 Y=0 WIDTH=4 HEIGHT=8 PARENT_NOTIFY=TRUE }
    BUTTON { NAME=\"next\" X=12 Y=0 WIDTH=4 HEIGHT=8 PARENT_NOTIFY=TRUE }
  }
}"
        .to_vec(),
    );
    pkg.add_file("menu.script", menu_script());
    pkg.add_file("verb.script", verb_script());
    pkg.add_file("dialogue.script", dialogue_script());
    pkg.add_file(
        "responses.def",
        b"RESPONSE_BOX {
  FONT=\"fonts\\sys.font\" FONT_HOVER=\"fonts\\test.font\" SPACING=1 VERTICAL_ALIGN=\"top\"
  AREA { 2, 2, 30, 30 }
  WINDOW {
    X=0 Y=0 WIDTH=32 HEIGHT=32
    BUTTON { NAME=\"prev\" X=0 Y=0 WIDTH=2 HEIGHT=2 }
    BUTTON { NAME=\"next\" X=30 Y=0 WIDTH=2 HEIGHT=2 }
  }
}"
        .to_vec(),
    );
    pkg.add_file(
        "verb.entity",
        b"ENTITY { NAME=\"pick\" CAPTION=\"/GREET/x\" ACTIVE=TRUE REGISTRABLE=TRUE SPRITE=\"art\\green.png\" SCRIPT=\"verb.script\" }"
            .to_vec(),
    );
    pkg.add_file(
        "cont.window",
        b"WINDOW {
  NAME=\"bar\" X=2 Y=2 WIDTH=20 HEIGHT=16
  ENTITY_CONTAINER { NAME=\"verb\" X=1 Y=1 ENTITY=\"verb.entity\" }
  ENTITY_CONTAINER { NAME=\"empty\" X=10 Y=1 }
}"
        .to_vec(),
    );
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
    pkg.add_file("scenes\\hall\\hall_init.script", hall_init_script());
    pkg.add_file(
        "scenes\\hall.scene",
        b"SCENE {
  NAME=\"Hall\" PERSISTENT_STATE=TRUE
  SCRIPT=\"scenes\\hall\\hall_init.script\"
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=32 HEIGHT=32
    ENTITY { NAME=\"door\" X=4 Y=4 SPRITE=\"art\\prop.sprite\" }
    REGION { NAME=\"exit\" POINT {0,0} POINT {8,0} POINT {8,8} POINT {0,8} }
    ENTITY { NAME=\"fixed\" X=10 Y=10 SPRITE=\"art\\prop.sprite\" SAVE_STATE=FALSE }
  }
  WAYPOINTS { NAME=\"wp\" POINT {16,16} POINT {24,24} }
}"
        .to_vec(),
    );
    pkg.add_file(
        "scenes\\small.scene",
        b"SCENE {
  NAME=\"Small\"
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=32 HEIGHT=20
    ENTITY { NAME=\"spot\" X=20 Y=18 REGION { POINT {8,10} POINT {24,10} POINT {24,18} POINT {8,18} } }
    REGION { NAME=\"floor\" POINT {0,12} POINT {32,12} POINT {32,20} POINT {0,20} }
  }
}"
        .to_vec(),
    );
    pkg.add_file("art\\red.png", png(4, 4, [255, 0, 0, 255]));
    pkg.add_file("art\\green.png", png(4, 4, [0, 255, 0, 255]));
    pkg.add_file(
        "sprites\\cursor.sprite",
        b"SPRITE { LOOPING=TRUE FRAME { DELAY=50 IMAGE=\"art\\red.png\" } FRAME { DELAY=50 IMAGE=\"art\\green.png\" } }".to_vec(),
    );
    pkg.add_file("walk.script", walk_script());
    pkg.add_file("sprite_edit.script", sprite_edit_script());
    pkg.add_file("cue.script", cue_script());
    pkg.add_file(
        "art\\cue.sprite",
        b"SPRITE { LOOPING=TRUE FRAME { DELAY=50 IMAGE=\"art\\red.png\" SOUND=\"audio\\beep.wav\" APPLY_EVENT=\"Tick\" } FRAME { DELAY=50 IMAGE=\"art\\green.png\" KILL_SOUND=TRUE APPLY_EVENT=\"Tock\" } }".to_vec(),
    );
    pkg.add_file("save.script", save_script());
    pkg.add_file("settings.script", settings_script());
    pkg.add_file("services.script", services_script());
    pkg.add_file("misc.script", misc_script());
    pkg.add_file("shots.script", shots_script());
    pkg.add_file("screens.script", screens_script());
    pkg.add_file(
        "inv2.def",
        b"INVENTORY_BOX {
  ITEM_WIDTH=12 ITEM_HEIGHT=12 SPACING=0
  AREA { 0, 0, 24, 12 }
  WINDOW { X=0 Y=0 WIDTH=24 HEIGHT=12 }
}"
        .to_vec(),
    );
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
        b"SPRITE { LOOPING=TRUE FRAME { DELAY=100 IMAGE=\"art\\green.png\" HOTSPOT {0,8} APPLY_EVENT=\"Step\" } }"
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
    ENTITY { NAME=\"keyspot\" X=28 Y=28 ITEM=\"key\" }
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
    assert_eq!(result(&runtime, "nodes"), Value::Int(4));
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
fn frames_and_subframes_are_script_objects_and_survive_a_saved_game() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Room");
    let game_id = runtime.world.game_id();
    assert!(
        runtime
            .world
            .attach(&mut runtime.engine, game_id, "sprite_edit.script")
    );
    let mut log = Vec::new();
    for _ in 0..20 {
        log.extend(runtime.tick(16));
        if result(&runtime, "done") == Value::Bool(true) {
            break;
        }
    }
    assert_eq!(result(&runtime, "done"), Value::Bool(true), "{log:?}");
    assert_eq!(result(&runtime, "frameType"), Value::from("frame"));
    assert_eq!(result(&runtime, "delayBefore"), Value::Int(100));
    assert_eq!(result(&runtime, "delayAfter"), Value::Int(30));
    assert_eq!(result(&runtime, "numSub"), Value::Int(1));
    assert_eq!(result(&runtime, "subType"), Value::from("subframe"));
    assert_eq!(
        result(&runtime, "mirror"),
        Value::Bool(true),
        "a frame with one sub-frame answers for it"
    );
    assert_eq!(result(&runtime, "hotY"), Value::Int(7));
    assert_eq!(result(&runtime, "image"), Value::from("art\\red.png"));
    assert_eq!(
        result(&runtime, "transparent"),
        Value::Int(0xffff_00ffu32 as i32),
        "the default colour key, opaque magenta"
    );
    assert_eq!(result(&runtime, "alpha"), Value::Int(0x00ff_0000));
    assert_eq!(result(&runtime, "numFrames"), Value::Int(3));
    assert_eq!(result(&runtime, "outOfRange"), Value::Null);
    assert!(
        log.iter()
            .any(|l| l.contains("Sprite.GetFrame: Frame index 9 is out of range")),
        "{log:?}"
    );
    assert_eq!(
        result(&runtime, "numEvents"),
        Value::Int(1),
        "no duplicates"
    );
    assert_eq!(result(&runtime, "soundOk"), Value::Bool(true));
    assert_eq!(result(&runtime, "sound"), Value::from("audio\\beep.wav"));
    assert_eq!(result(&runtime, "soundBad"), Value::Bool(false));
    assert_eq!(result(&runtime, "soundAfter"), Value::Null);
    assert_eq!(result(&runtime, "numAfterDelete"), Value::Int(2));
    assert_eq!(
        result(&runtime, "addedDelay"),
        Value::Int(40),
        "a handle follows its frame when one before it is deleted"
    );
    let Value::Native(inserted) = result(&runtime, "inserted") else {
        panic!("InsertFrame returned {:?}", result(&runtime, "inserted"));
    };
    assert_eq!(
        runtime.world.get_property(inserted, "Type"),
        None,
        "the handle of a deleted frame is invalid"
    );
    assert_eq!(result(&runtime, "numAfterInsert"), Value::Int(3));
    assert_eq!(result(&runtime, "fDelay"), Value::Int(30));
    assert_eq!(
        result(&runtime, "firstSub"),
        Value::Int(0),
        "no image: no sub-frame"
    );
    assert_eq!(result(&runtime, "numAtEnd"), Value::Int(2));
    assert_eq!(result(&runtime, "fDelayAtEnd"), Value::Int(30));

    // The edits are in the sprite that is drawn.
    let check = |runtime: &Runtime| {
        let loaded = runtime
            .world
            .scene()
            .unwrap()
            .sprite("art\\prop.sprite")
            .unwrap();
        assert!(loaded.edited);
        let frames = &loaded.sprite.frames;
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].delay_ms, 30);
        assert_eq!(frames[0].apply_events, ["Blink"]);
        assert_eq!(frames[0].sound, None);
        let sub = &frames[0].subframes[0];
        assert!(sub.mirror_x && !sub.mirror_y);
        assert_eq!(sub.hotspot_y, 7);
        assert_eq!(sub.tint.r, 255);
        assert_eq!((sub.tint.g, sub.tint.b, sub.tint.a), (0, 0, 0));
        assert_eq!(frames[1].delay_ms, 40);
        assert_eq!(
            frames[1].subframes[0].image.as_deref(),
            Some("art\\green.png")
        );
    };
    check(&runtime);

    // A saved game keeps the edited sprite; the others load from their
    // files again.
    runtime.save_game(8, "edited", false).unwrap();
    let mut loaded = Runtime::with_audio(Vfs::open_game(&dir).unwrap(), Audio::null()).unwrap();
    loaded.start();
    loaded.tick(16);
    loaded.load_game(8).unwrap();
    loaded.tick(16);
    check(&loaded);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn frames_play_their_sounds_and_raise_their_events() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Room");
    // The prop shows cue.sprite: frame 0 (50 ms) plays a sound and raises
    // "Tick", frame 1 kills the sound and raises "Tock".
    let prop_ref = luma_adventure::EntityRef::Node { layer: 0, index: 0 };
    let files = Vfs::open_game(&dir).unwrap();
    {
        let view = runtime.world.scene_mut().unwrap();
        assert!(view.add_sprite(&files, "art\\cue.sprite"));
        let e = view.entity_mut(prop_ref).unwrap();
        e.active = true;
        e.sprite = Some("art\\cue.sprite".to_string());
        view.reset_anim(prop_ref);
    }
    let prop = runtime.world.node_id(prop_ref).unwrap();
    let sprite = runtime.world.sprite_object_id(prop).unwrap();
    assert!(
        runtime
            .world
            .attach(&mut runtime.engine, prop, "cue.script")
    );
    assert!(
        runtime
            .world
            .attach(&mut runtime.engine, sprite, "cue.script")
    );
    runtime.tick(16);
    assert_eq!(
        runtime.world.frame_sounds_of(prop),
        1,
        "frame 0's sound plays"
    );
    assert_eq!(result(&runtime, "tock"), Value::Null);
    runtime.tick(16);
    assert_eq!(
        result(&runtime, "tick"),
        Value::from("entity"),
        "APPLY_EVENT is raised on the owner"
    );
    assert_eq!(
        result(&runtime, "frameChanged"),
        Value::from("sprite"),
        "FrameChanged is raised on the sprite object"
    );
    for _ in 0..4 {
        runtime.tick(16);
    }
    assert_eq!(result(&runtime, "tock"), Value::from("entity"));
    assert_eq!(
        runtime.world.frame_sounds_of(prop),
        0,
        "a KILL_SOUND frame stops the sprite's sounds"
    );

    // An actor's sprite raises its frame events on the actor.
    let bob = runtime
        .world
        .load_actor(&mut runtime.engine, "actors\\bob.actor", false)
        .unwrap();
    assert!(runtime.world.attach(&mut runtime.engine, bob, "cue.script"));
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(result(&runtime, "step"), Value::from("actor"));
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
fn items_the_inventory_box_and_the_item_cursor() {
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
    // The items loaded with their captions expanded and their
    // properties; the box has its window with two buttons.
    assert_eq!(runtime.world.item_ids().len(), 2);
    let key = runtime.world.item_by_name("KEY").unwrap();
    let coin = runtime.world.item_by_name("coin").unwrap();
    assert_eq!(runtime.world.class_name(key), "item");
    assert_eq!(
        runtime.world.get_property(key, "Caption"),
        Some(Value::from("Hello"))
    );
    assert_eq!(
        runtime.world.get_property(key, "weight"),
        Some(Value::from("3"))
    );
    assert_eq!(
        runtime.world.get_property(game, "TotalNumItems"),
        Some(Value::Int(2))
    );
    let inv_window = runtime.world.inventory().window.unwrap();
    assert_eq!(runtime.world.widgets_of(inv_window).len(), 2);
    assert!(!runtime.world.window_ids().contains(&inv_window));
    let keyspot = runtime.world.find_node("keyspot").unwrap();
    let keyspot_ref = luma_adventure::EntityRef::Node { layer: 0, index: 3 };
    let active = |runtime: &Runtime| {
        runtime
            .world
            .scene()
            .unwrap()
            .entity(keyspot_ref)
            .unwrap()
            .active
    };
    assert_eq!(
        runtime.world.get_property(keyspot, "Item"),
        Some(Value::from("key"))
    );
    assert!(active(&runtime));
    // Taking an item hides the entities tied to it; dropping shows them.
    assert!(runtime.world.take_item("key", None));
    assert!(!runtime.world.take_item("nothing", None));
    assert!(runtime.world.has_item("key") && !runtime.world.has_item("coin"));
    assert_eq!(runtime.world.taken_items(), [key]);
    assert!(!active(&runtime));
    assert_eq!(
        runtime.world.get_property(game, "NumItems"),
        Some(Value::Int(1))
    );
    // The box shown: the key sits in the first cell, under the mouse.
    runtime
        .world
        .set_property(game, "InventoryVisible", &Value::Bool(true));
    runtime.mouse_move(6, 2);
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(runtime.world.active_object(), Some(key));
    assert_eq!(
        runtime.world.item(key).unwrap().current_sprite.as_deref(),
        Some("art\\red.png")
    );
    let prev = runtime.world.widget(inv_window, "prev").unwrap();
    let next = runtime.world.widget(inv_window, "next").unwrap();
    assert!(runtime.world.control(prev).unwrap().disabled);
    assert!(runtime.world.control(next).unwrap().disabled);
    // Selecting the key shows its cursor over the normal one, and hides
    // it from the box.
    runtime
        .world
        .set_property(game, "SelectedItem", &Value::Native(key));
    runtime.tick(16);
    assert_eq!(runtime.world.selected_item(), Some(key));
    assert_eq!(
        runtime.world.cursors().item_cursor.as_deref(),
        Some("art\\red.png")
    );
    assert!(runtime.world.cursors().shown().is_some());
    runtime.tick(16);
    assert_eq!(runtime.world.active_object(), Some(inv_window));
    // Over a scene entity the hover cursor shows; a coin replaces the
    // cursor entirely.
    runtime
        .world
        .set_property(game, "InventoryVisible", &Value::Bool(false));
    // The scene script deactivated the prop; bring it back.
    let prop_ref = luma_adventure::EntityRef::Node { layer: 0, index: 0 };
    runtime
        .world
        .scene_mut()
        .unwrap()
        .entity_mut(prop_ref)
        .unwrap()
        .active = true;
    // The scene script moved it to (5, 6) and scrolled the view.
    runtime.world.skip_to(0, 0);
    runtime.mouse_move(6, 7);
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(
        runtime.world.active_object(),
        runtime.world.find_node("prop")
    );
    assert_eq!(
        runtime.world.cursors().item_cursor.as_deref(),
        Some("art\\green.png")
    );
    runtime.world.take_item("coin", Some("key"));
    assert_eq!(runtime.world.taken_items(), [key, coin]);
    runtime
        .world
        .set_property(game, "SelectedItem", &Value::from("coin"));
    runtime.tick(16);
    assert_eq!(
        runtime.world.cursors().item_cursor.as_deref(),
        Some("art\\green.png")
    );
    assert_eq!(runtime.world.cursors().shown(), None);
    // The box pages: with two items and one cell, next scrolls.
    runtime
        .world
        .set_property(game, "InventoryVisible", &Value::Bool(true));
    runtime.tick(16);
    assert!(!runtime.world.control(next).unwrap().disabled);
    runtime.world.press_button(&mut runtime.engine, next);
    runtime.tick(16);
    assert_eq!(
        runtime.world.get_property(game, "InventoryScrollOffset"),
        Some(Value::Int(1))
    );
    assert!(!runtime.world.control(prev).unwrap().disabled);
    // Dropping the selected item deselects it; a null selection clears.
    assert!(runtime.world.drop_item("coin"));
    assert_eq!(runtime.world.selected_item(), None);
    assert!(!runtime.world.drop_item("coin"));
    assert_eq!(runtime.world.taken_items(), [key]);
    runtime.world.drop_item("key");
    assert!(active(&runtime));
    runtime
        .world
        .set_property(game, "SelectedItem", &Value::Null);
    runtime.tick(16);
    assert_eq!(runtime.world.cursors().item_cursor, None);
}

#[test]
fn videos_freeze_the_game_and_can_be_skipped() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    let game = runtime.world.game_id();
    runtime
        .world
        .set_property(game, "VideoSubtitles", &Value::Bool(true));
    // A missing file: false, and the script goes on.
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "PlayMissing", false);
    let log = runtime.tick(16);
    assert_eq!(result(&runtime, "missing"), Value::Bool(false));
    assert!(
        log.iter()
            .any(|l| l.contains("Error opening video file 'none.ogv'")),
        "{log:?}"
    );
    assert!(!runtime.world.is_video_playing());
    // The intro: the game freezes, the scripts stop, the frames follow
    // the wall clock.
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "PlayIntro", false);
    runtime.tick(16);
    assert!(runtime.world.is_video_playing());
    assert!(runtime.world.is_frozen());
    // The call sleeps and no script runs during the video: even the
    // call's result is stored after it.
    assert_eq!(result(&runtime, "videoStarted"), Value::Null);
    assert_eq!(result(&runtime, "afterVideo"), Value::Null);
    let player = runtime.world.video().unwrap();
    assert_eq!((player.info.width, player.info.height), (32, 32));
    assert_eq!((player.x, player.y, player.zoom), (0, 0, 100.0));
    assert_eq!(player.frame_index(), Some(0));
    assert!(player.subtitler.is_some());
    let frozen_at = runtime.world.timer();
    for _ in 0..20 {
        runtime.tick(16);
    }
    assert_eq!(runtime.world.timer(), frozen_at, "the game timer stops");
    assert_eq!(
        result(&runtime, "afterVideo"),
        Value::Null,
        "scripts do not run"
    );
    let player = runtime.world.video().unwrap();
    assert_eq!(player.frame_index(), Some(3), "320 ms at 10 fps");
    assert_eq!(
        player.subtitler.as_ref().unwrap().shown(),
        Some("Hello\nintro")
    );
    // Drawing gives the video's quad only.
    let mut list = luma_render::DrawList::new(32.0, 32.0);
    runtime.draw(&mut list, None);
    assert!(
        list.quads().is_empty(),
        "no texture uploaded without a renderer"
    );
    // 21 ticks so far since the start.
    let mut ticks = 21;
    while runtime.world.is_video_playing() && ticks < 200 {
        runtime.tick(16);
        ticks += 1;
    }
    assert!(
        (55..=70).contains(&ticks),
        "ten frames at 10 fps: {ticks} ticks"
    );
    assert!(!runtime.world.is_frozen());
    runtime.tick(16);
    assert_eq!(result(&runtime, "videoStarted"), Value::Bool(true));
    assert_eq!(result(&runtime, "afterVideo"), Value::Bool(true));
    assert!(runtime.world.video().is_none());
    // Placed at a position, and skipped by a click.
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "PlayIntroPos", false);
    runtime.tick(16);
    let player = runtime.world.video().unwrap();
    assert_eq!((player.x, player.y), (4, 6));
    assert_eq!(player.playback, luma_adventure::VideoPlayback::Pos);
    runtime.mouse_left_down();
    assert!(!runtime.world.is_video_playing());
    assert!(!runtime.world.is_frozen());
    runtime.mouse_left_up();
    runtime.tick(16);
    assert_eq!(result(&runtime, "afterVideo2"), Value::Bool(true));
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

#[test]
fn persistent_scenes_remember_their_nodes_between_visits() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Room");

    // First visit: the nodes are as the file says; the script changes them.
    runtime.change_scene("scenes/hall.scene").unwrap();
    runtime.tick(16);
    assert_eq!(result(&runtime, "hallPersistent"), Value::Bool(true));
    assert_eq!(
        result(&runtime, "doorSprite"),
        Value::from("art\\prop.sprite")
    );
    assert_eq!(result(&runtime, "doorActive"), Value::Bool(true));
    assert_eq!(result(&runtime, "doorCaption"), Value::from(""));
    assert_eq!(result(&runtime, "doorAlpha"), Value::Int(0));
    assert_eq!(result(&runtime, "exitActive"), Value::Bool(true));
    assert_eq!(result(&runtime, "fixedActive"), Value::Bool(true));
    assert!(runtime.world.scene_states().is_empty());

    // Leaving notes the state; the room is not persistent.
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    let states = runtime.world.scene_states();
    assert_eq!(states.len(), 1, "{states:?}");
    let hall = &states["scenes\\hall.scene"];
    assert_eq!(
        hall.nodes.len(),
        2,
        "the fixed entity is not kept: {hall:?}"
    );
    assert_eq!(
        hall.nodes["door"].sprite.as_deref(),
        Some("art\\walk.sprite")
    );
    assert!(!hall.nodes["door"].active);
    assert!(!hall.nodes["exit"].active);

    // Coming back: the door is open and inactive, the exit inactive, the
    // fixed entity back as in the file, and the room's state not kept.
    runtime.change_scene("scenes\\Hall.scene").unwrap();
    let view = runtime.world.scene().unwrap();
    let door = view
        .entity_refs()
        .into_iter()
        .find_map(|r| view.entity(r).filter(|e| e.name == "door"))
        .unwrap();
    assert_eq!(door.sprite.as_deref(), Some("art\\walk.sprite"));
    assert!(
        view.bank.sprite("art\\walk.sprite").is_some(),
        "the sprite is loaded"
    );
    runtime.tick(16);
    assert_eq!(
        result(&runtime, "doorSprite"),
        Value::from("art\\walk.sprite")
    );
    assert_eq!(result(&runtime, "doorActive"), Value::Bool(false));
    assert_eq!(result(&runtime, "doorCaption"), Value::from("opened"));
    assert_eq!(result(&runtime, "doorAlpha"), Value::Int(0x1122_3344));
    assert_eq!(result(&runtime, "exitActive"), Value::Bool(false));
    assert_eq!(result(&runtime, "fixedActive"), Value::Bool(true));
    assert_eq!(runtime.world.scene_states().len(), 1);
}

#[test]
fn saved_games_resume_the_scripts_the_actors_and_the_sounds() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Room");
    let game = runtime.world.game_id();
    assert!(
        runtime
            .world
            .attach(&mut runtime.engine, game, "save.script")
    );
    for _ in 0..12 {
        runtime.tick(16);
    }
    let actor = runtime.engine.global("actor").unwrap().as_native().unwrap();
    assert!(runtime.world.actor(actor).unwrap().is_walking());
    let (x_saved, y_saved) = {
        let a = runtime.world.actor(actor).unwrap();
        (a.x, a.y)
    };
    assert!(runtime.world.is_music_playing(0));
    assert!(!runtime.world.is_save_slot_used(3));
    assert_eq!(runtime.world.most_recent_save_slot(), -1);

    // Save in the middle of the walk: the handler goes on after the call.
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "SaveIt", false);
    let log = runtime.tick(16);
    assert!(log.iter().any(|l| l.starts_with("Saving game")), "{log:?}");
    assert_eq!(result(&runtime, "saved"), Value::Bool(true));
    assert_eq!(result(&runtime, "beforeSave"), Value::Bool(true));
    assert_eq!(result(&runtime, "afterSave"), Value::Int(1));
    assert!(dir.join("saves").join("save003.dsv").is_file());
    assert!(runtime.world.is_save_slot_used(3));
    assert_eq!(runtime.world.save_slot_description(3), "mid walk");
    assert_eq!(runtime.world.save_slot_description(4), "");
    assert_eq!(runtime.world.most_recent_save_slot(), 3);
    let header = luma_adventure::read_save_header(&dir.join("saves").join("save003.dsv")).unwrap();
    assert_eq!(header.game, "Test");
    assert!(header.timestamp > 1_700_000_000);

    // The walk ends, the game moves on: another item, another scene.
    let mut ticks = 0;
    while result(&runtime, "arrived") != Value::Bool(true) && ticks < 200 {
        runtime.tick(16);
        ticks += 1;
    }
    assert_eq!(result(&runtime, "arrived"), Value::Bool(true));
    assert_eq!(runtime.world.actor(actor).unwrap().x, 50);
    assert!(runtime.world.take_item("coin", None));
    runtime.change_scene("scenes\\hall.scene").unwrap();
    runtime.tick(16);
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Hall");

    // Load: the frame after the call, everything is as it was saved, and
    // the saved handler is the one that goes on.
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "LoadIt", false);
    runtime.tick(16);
    assert_eq!(result(&runtime, "loadResult"), Value::Null, "not yet");
    assert_eq!(runtime.world.scheduled_load(), Some(3));
    let log = runtime.tick(16);
    assert!(log.iter().any(|l| l.starts_with("Loading game")), "{log:?}");
    assert!(
        log.iter()
            .all(|l| !l.contains("Error") && !l.contains("error")),
        "{log:?}"
    );
    assert_eq!(
        result(&runtime, "loaded"),
        Value::Bool(true),
        "AfterLoad ran"
    );
    assert_eq!(result(&runtime, "afterLoadCall"), Value::Null);
    assert_eq!(result(&runtime, "arrived"), Value::Null);
    assert_eq!(
        result(&runtime, "afterSave"),
        Value::Int(1),
        "resumed after SaveGame"
    );
    assert_eq!(result(&runtime, "saved"), Value::Bool(true));
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Room");
    assert_eq!(runtime.world.game_id(), game);
    assert_eq!(
        runtime.engine.global("Game").unwrap().as_native(),
        Some(game)
    );
    assert_eq!(runtime.world.main_object(), Some(actor));
    let a = runtime.world.actor(actor).unwrap();
    assert!(a.is_walking(), "{:?}", a.state);
    assert!(
        (a.x - x_saved).abs() <= 2 && a.y == y_saved,
        "{} {}",
        a.x,
        a.y
    );
    let taken: Vec<String> = runtime
        .world
        .taken_items()
        .iter()
        .map(|id| runtime.world.item(*id).unwrap().def.name.clone())
        .collect();
    assert_eq!(taken, ["key"]);
    assert!(runtime.world.is_music_playing(0));
    assert!(
        runtime
            .world
            .music(0)
            .unwrap()
            .filename
            .ends_with("beep.wav")
    );
    assert!(runtime.world.music_position(0) > 100);
    assert!(
        runtime
            .world
            .scene()
            .unwrap()
            .sprite("art\\prop.sprite")
            .is_some(),
        "the scene's sprites are loaded again"
    );
    assert!(
        runtime
            .world
            .actor(actor)
            .unwrap()
            .bank
            .sprite("art\\walk.sprite")
            .is_some(),
        "the actor's sprites are loaded again"
    );
    assert_eq!(runtime.world.drawables().len(), 1);

    // The walk goes on to its end.
    let mut ticks = 0;
    while result(&runtime, "arrived") != Value::Bool(true) && ticks < 200 {
        runtime.tick(16);
        ticks += 1;
    }
    assert_eq!(result(&runtime, "arrived"), Value::Bool(true));
    assert_eq!(runtime.world.actor(actor).unwrap().x, 50);

    // The slot can be emptied.
    assert!(runtime.world.empty_save_slot(3));
    assert!(!runtime.world.is_save_slot_used(3));
    assert!(!runtime.world.empty_save_slot(3));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn entity_containers_show_their_entity_and_give_it_the_mouse() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    let game = runtime.world.game_id();
    runtime
        .world
        .set_property(game, "Interactive", &Value::Bool(true));
    let bar = runtime
        .world
        .load_window(&mut runtime.engine, "cont.window")
        .unwrap();
    let verb = runtime.world.widget(bar, "verb").unwrap();
    let empty = runtime.world.widget(bar, "empty").unwrap();
    assert_eq!(runtime.world.class_name(verb), "entity container");
    // The container's entity, as GetEntity gives it; the caption went
    // through the string table.
    let entity = runtime.world.container_entity(verb).unwrap();
    assert_eq!(runtime.world.container_entity(empty), None);
    assert_eq!(
        runtime.world.get_property(entity, "Name"),
        Some(Value::from("pick"))
    );
    assert_eq!(
        runtime.world.get_property(entity, "Caption"),
        Some(Value::from("Hello"))
    );
    // The entity is drawn at the container's position in the window and
    // is the object under the mouse there.
    assert_eq!(
        runtime.world.get_property(entity, "Interactive"),
        Some(Value::Bool(true))
    );
    assert_eq!(
        runtime.world.get_property(entity, "Active"),
        Some(Value::Bool(true))
    );
    runtime.mouse_move(4, 4);
    runtime.tick(16);
    assert_eq!(runtime.world.active_object(), Some(entity));
    runtime.mouse_move(15, 4);
    runtime.tick(16);
    assert_ne!(runtime.world.active_object(), Some(entity));
    // A click raises LeftClick on the entity's own script, which swaps
    // its sprite.
    runtime.mouse_move(4, 4);
    runtime.tick(16);
    runtime.mouse_left_down();
    runtime.tick(16);
    runtime.mouse_left_up();
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(result(&runtime, "verbClicked"), Value::from("pick"));
    // Hidden with its container; inactive entities take no mouse.
    runtime
        .world
        .set_property(verb, "Visible", &Value::Bool(false));
    runtime.tick(16);
    assert_ne!(runtime.world.active_object(), Some(entity));
}

#[test]
fn dialogue_responses_wait_for_a_pick_and_remember_once_choices() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    let game = runtime.world.game_id();
    runtime
        .world
        .set_property(game, "Interactive", &Value::Bool(true));
    assert!(
        runtime
            .world
            .attach(&mut runtime.engine, game, "dialogue.script")
    );
    runtime.tick(16);
    // The dialogue offers two choices and waits.
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "dlg", false);
    runtime.tick(16);
    assert_eq!(result(&runtime, "num"), Value::Int(2));
    assert_eq!(result(&runtime, "chosen"), Value::Null);
    assert_eq!(
        runtime.world.get_property(game, "ResponsesVisible"),
        Some(Value::Bool(true))
    );
    let window = runtime.world.responses_window().unwrap();
    assert!(runtime.world.control(window).unwrap().visible);
    let buttons: Vec<_> = runtime
        .world
        .widgets_of(window)
        .into_iter()
        .filter(|w| runtime.world.control(*w).unwrap().name == "response")
        .collect();
    assert_eq!(buttons.len(), 2);
    let second = runtime.world.control(buttons[1]).unwrap();
    assert_eq!(second.text, "Hello", "through the string table");
    assert!(second.visible && second.y > 2, "laid out under the first");
    let (x, y) = (second.x + 2, second.y + second.height / 2);
    // Picking the second choice resumes the script with its id.
    runtime.mouse_move(x, y);
    runtime.tick(16);
    assert_eq!(runtime.world.active_object(), Some(buttons[1]));
    runtime.mouse_left_down();
    runtime.tick(16);
    runtime.mouse_left_up();
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(result(&runtime, "chosen"), Value::Int(2));
    assert_eq!(result(&runtime, "last"), Value::from("Hello"));
    assert_eq!(
        runtime.world.get_property(game, "ResponsesVisible"),
        Some(Value::Bool(false))
    );
    assert!(!runtime.world.control(window).unwrap().visible);
    // Asked again in the same branch, the once-only choice is gone; the
    // dialogue then auto-selects nothing and waits with one choice.
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "dlg", false);
    runtime.tick(16);
    assert_eq!(result(&runtime, "num"), Value::Int(1));
    let buttons: Vec<_> = runtime
        .world
        .widgets_of(window)
        .into_iter()
        .filter(|w| runtime.world.control(*w).unwrap().name == "response")
        .collect();
    assert_eq!(buttons.len(), 1);
    let first = runtime.world.control(buttons[0]).unwrap();
    runtime.mouse_move(first.x + 2, first.y + first.height / 2);
    runtime.tick(16);
    runtime.mouse_left_down();
    runtime.tick(16);
    runtime.mouse_left_up();
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(result(&runtime, "chosen"), Value::Int(1));
    // Each dialogue opened its own "chat" branch: ending both forgets
    // the once-only choices.
    for _ in 0..2 {
        runtime
            .engine
            .apply_event(&mut runtime.world, game, "bye", false);
        runtime.tick(16);
    }
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "dlg", false);
    runtime.tick(16);
    assert_eq!(result(&runtime, "num"), Value::Int(2));
}

#[test]
fn a_played_animation_survives_a_saved_game() {
    use luma_adventure::EntityRef;
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Room");
    // "prop" is node 0 of the main layer, at (8, 12); stand.sprite loops,
    // so it plays until something ends it, as a wandering character's does.
    let prop = EntityRef::Node { layer: 0, index: 0 };
    // The camera follows the walking actor: the prop goes where the
    // screen is, and the point tested follows the offset of the frame.
    let (ox, oy) = runtime.world.offset();
    {
        let e = runtime.world.scene_mut().unwrap().entity_mut(prop).unwrap();
        e.x = ox + 8;
        e.y = oy + 12;
    }
    let (px, py) = (ox + 9, oy + 5);
    runtime
        .world
        .scene_mut()
        .unwrap()
        .entity_mut(prop)
        .unwrap()
        .active = true;
    let files = Vfs::open_game(&dir).unwrap();
    assert!(
        runtime
            .world
            .scene_mut()
            .unwrap()
            .play_anim(&files, prop, "art\\stand.sprite")
    );
    runtime.tick(16);
    assert!(runtime.world.scene().unwrap().is_playing_anim(prop));
    let (ox, oy) = runtime.world.offset();
    let under_mouse = runtime.world.object_at(px - ox, py - oy);
    assert!(under_mouse.is_some(), "the played sprite takes the mouse");
    runtime.save_game(7, "anim", false).unwrap();

    // A fresh engine loads the slot: the played sprite is loaded again,
    // so the entity is drawn and takes the mouse as before.
    let mut loaded = Runtime::with_audio(Vfs::open_game(&dir).unwrap(), Audio::null()).unwrap();
    loaded.start();
    loaded.tick(16);
    loaded.load_game(7).unwrap();
    loaded.tick(16);
    let view = loaded.world.scene().unwrap();
    assert!(view.is_playing_anim(prop));
    assert_eq!(
        view.shown_sprite(prop).as_deref(),
        Some("art\\stand.sprite")
    );
    assert!(view.sprite("art\\stand.sprite").is_some(), "loaded again");
    let (ox, oy) = loaded.world.offset();
    assert_eq!(loaded.world.object_at(px - ox, py - oy), under_mouse);

    // A sentence takes over: the played sprite is dropped and the entity
    // is ready once the sentence ends, as a script waiting on it expects.
    let id = loaded.world.find_node("prop").unwrap();
    assert!(loaded.world.talk(
        id,
        "Hi",
        None,
        100,
        None,
        luma_formats::font::TextAlign::Center
    ));
    loaded.tick(16);
    let view = loaded.world.scene().unwrap();
    assert!(
        !view.is_playing_anim(prop),
        "talking replaced the animation"
    );
    assert!(view.is_talking(prop));
    for _ in 0..12 {
        loaded.tick(16);
    }
    let view = loaded.world.scene().unwrap();
    assert!(!view.is_talking(prop));
    assert_eq!(view.shown_sprite(prop).as_deref(), Some("art\\prop.sprite"));
}

#[test]
fn hotspots_of_a_scene_smaller_than_the_screen_are_where_they_are_drawn() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    // A 32x20 scene on a 32x32 screen: the original engine keeps a
    // negative scroll offset (`min(0, 20 - 32)`), draws the layers at
    // the top of the screen anyway, and registers a hotspot's region
    // with the same offset as the layer it is drawn in.
    runtime.change_scene("scenes\\small.scene").unwrap();
    runtime.tick(16);
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Small");
    runtime.world.scroll_to(16, 10);
    for _ in 0..40 {
        runtime.tick(16);
    }
    assert_eq!(runtime.world.offset(), (0, -12));
    let spot = runtime.world.find_node("spot").unwrap();
    assert_eq!(runtime.world.object_at(16, 14), Some(spot));
    assert_eq!(runtime.world.object_at(16, 5), None);
}

#[test]
fn load_items_reloads_the_items_as_the_original_engine_does() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    let names = |runtime: &Runtime| -> Vec<String> {
        runtime
            .world
            .inventory()
            .items
            .iter()
            .map(|(_, i)| i.def.name.clone())
            .collect()
    };
    assert_eq!(names(&runtime), ["key", "coin"]);
    assert!(runtime.world.take_item("key", None));
    let key = runtime.world.item_by_name("key").unwrap();
    assert_eq!(runtime.world.item(key).unwrap().def.caption, "Hello");

    // Merging: an item of the file replaces the one of the same name,
    // which leaves the inventory (the original engine deletes it first).
    assert!(
        runtime
            .world
            .load_items_from(&mut runtime.engine, "items2.items", true)
    );
    assert_eq!(names(&runtime), ["coin", "key", "gem"]);
    assert!(
        !runtime.world.has_item("key"),
        "the replaced item was dropped"
    );
    assert_ne!(runtime.world.item_by_name("key"), Some(key), "a new object");
    assert!(runtime.world.item(key).is_none());

    // A language menu: the string table changes, then the items reload
    // so that their captions expand with it; what was taken is lost.
    assert!(runtime.world.take_item("coin", None));
    assert!(runtime.world.load_string_table("fr.tab", true));
    assert!(
        runtime
            .world
            .load_items_from(&mut runtime.engine, "items2.items", false)
    );
    assert_eq!(names(&runtime), ["key", "gem"]);
    assert!(runtime.world.taken_items().is_empty());
    let key = runtime.world.item_by_name("key").unwrap();
    assert_eq!(runtime.world.item(key).unwrap().def.caption, "Bonjour");
    assert!(
        !runtime
            .world
            .load_items_from(&mut runtime.engine, "missing.items", false),
        "a missing file fails and changes nothing"
    );
    assert_eq!(names(&runtime), ["key", "gem"]);

    // Single items come and go the same way.
    let gem = runtime.world.item_by_name("gem").unwrap();
    assert!(runtime.world.delete_item(&mut runtime.engine, gem));
    assert!(!runtime.world.delete_item(&mut runtime.engine, gem));
    let rock = runtime.world.create_item(&mut runtime.engine, Some("rock"));
    assert_eq!(runtime.world.item_by_name("rock"), Some(rock));
    assert_eq!(names(&runtime), ["key", "rock"]);
}

/// Calls a method on a native as a script would, from the game's own
/// script, and returns the value it leaves.
fn call_native(
    runtime: &mut Runtime,
    target: luma_script::NativeId,
    name: &str,
    args: &[Value],
) -> Value {
    use luma_script::Host;
    let game = runtime.world.game_id();
    let script = runtime.engine.scripts_of(game)[0];
    let ctx = luma_script::Context {
        engine: &mut runtime.engine,
        script,
        owner: Some(game),
        line: 0,
    };
    runtime
        .world
        .call_method(target, name, args, ctx)
        .map(|r| r.value)
        .unwrap_or(Value::Null)
}

#[test]
fn objects_have_their_own_cursor_caption_slots_and_flags() {
    use luma_adventure::EntityRef;
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
    let prop_ref = EntityRef::Node { layer: 0, index: 0 };
    runtime
        .world
        .scene_mut()
        .unwrap()
        .entity_mut(prop_ref)
        .unwrap()
        .active = true;
    let prop = runtime.world.node_id(prop_ref).unwrap();
    let text = |v: Value| v.get_string(&());

    // The object's own cursor (`SetCursor`) is shown over it instead of
    // the active cursor; a cursor that does not load leaves the old one.
    assert_eq!(
        call_native(&mut runtime, prop, "HasCursor", &[]),
        Value::Bool(false)
    );
    assert_eq!(
        call_native(&mut runtime, prop, "GetCursor", &[]),
        Value::Null
    );
    assert_eq!(
        call_native(
            &mut runtime,
            prop,
            "SetCursor",
            &[Value::from("sprites\\cursor.sprite")]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        call_native(
            &mut runtime,
            prop,
            "SetCursor",
            &[Value::from("missing.sprite")]
        ),
        Value::Bool(false)
    );
    assert_eq!(
        text(call_native(&mut runtime, prop, "GetCursor", &[])),
        "sprites\\cursor.sprite"
    );
    assert_eq!(
        call_native(&mut runtime, prop, "HasCursor", &[]),
        Value::Bool(true)
    );
    let cursor = call_native(&mut runtime, prop, "GetCursorObject", &[])
        .as_native()
        .expect("a sprite object");
    assert_eq!(
        runtime.world.get_property(cursor, "Filename"),
        Some(Value::from("sprites\\cursor.sprite"))
    );
    assert_eq!(
        runtime.world.get_property(cursor, "NumFrames"),
        Some(Value::Int(2))
    );
    assert_eq!(
        runtime.world.get_property(cursor, "Owner"),
        Some(Value::Native(prop))
    );
    runtime.world.skip_to(0, 0);
    runtime.mouse_move(6, 7);
    runtime.tick(16);
    assert_eq!(runtime.world.active_object(), Some(prop));
    assert_eq!(
        runtime.world.cursors().shown(),
        Some("sprites\\cursor.sprite")
    );
    call_native(&mut runtime, prop, "RemoveCursor", &[]);
    assert_eq!(
        call_native(&mut runtime, prop, "HasCursor", &[]),
        Value::Bool(false)
    );
    assert!(
        !runtime.world.objects().contains(cursor)
            || runtime.world.get_property(cursor, "Filename").is_none(),
        "the cursor object has nothing to show"
    );
    runtime.tick(16);
    assert_eq!(
        runtime.world.cursors().shown(),
        Some("art\\green.png"),
        "back to the active cursor"
    );

    // Caption slots: slot 1 is the caption, 2 to 7 are extra, all
    // expanded through the string table.
    call_native(
        &mut runtime,
        prop,
        "SetCaption",
        &[Value::from("/GREET/x"), Value::Int(2)],
    );
    call_native(
        &mut runtime,
        prop,
        "SetCaption",
        &[Value::from("Prop"), Value::Int(0)],
    );
    assert_eq!(
        text(call_native(&mut runtime, prop, "Caption", &[Value::Int(2)])),
        "Hello"
    );
    assert_eq!(
        text(call_native(&mut runtime, prop, "Caption", &[])),
        "Prop"
    );
    assert_eq!(
        runtime.world.get_property(prop, "Caption"),
        Some(Value::from("Prop"))
    );
    assert_eq!(
        text(call_native(&mut runtime, prop, "Caption", &[Value::Int(3)])),
        ""
    );
    assert_eq!(
        text(call_native(&mut runtime, prop, "Caption", &[Value::Int(9)])),
        ""
    );

    // The flags are kept per object; `NonIntMouseEvents` keeps the
    // object under the mouse while the game is not interactive.
    assert_eq!(
        runtime.world.get_property(prop, "Movable"),
        Some(Value::Bool(true))
    );
    runtime
        .world
        .set_property(prop, "Movable", &Value::Bool(false));
    assert_eq!(
        runtime.world.get_property(prop, "Movable"),
        Some(Value::Bool(false))
    );
    assert_eq!(
        runtime.world.get_property(prop, "SceneIndependent"),
        Some(Value::Bool(false))
    );
    runtime
        .world
        .set_property(prop, "SceneIndependent", &Value::Bool(true));
    assert_eq!(
        runtime.world.get_property(prop, "SceneIndependent"),
        Some(Value::Bool(true))
    );
    let window = runtime.world.window_ids()[0];
    assert_eq!(
        runtime.world.get_property(window, "NonIntMouseEvents"),
        Some(Value::Bool(true)),
        "windows take the mouse while the game is not interactive"
    );
    assert_eq!(
        runtime.world.get_property(prop, "NonIntMouseEvents"),
        Some(Value::Bool(false))
    );
    runtime
        .world
        .set_property(game, "Interactive", &Value::Bool(false));
    runtime.tick(16);
    assert_eq!(runtime.world.active_object(), None);
    runtime
        .world
        .set_property(prop, "NonIntMouseEvents", &Value::Bool(true));
    runtime.tick(16);
    assert_eq!(runtime.world.active_object(), Some(prop));

    // The state is part of a saved game.
    runtime.save_game(11, "flags", false).unwrap();
    let mut loaded = Runtime::with_audio(Vfs::open_game(&dir).unwrap(), Audio::null()).unwrap();
    loaded.start();
    loaded.tick(16);
    loaded.load_game(11).unwrap();
    assert_eq!(
        loaded.world.get_property(prop, "Movable"),
        Some(Value::Bool(false))
    );
    assert_eq!(
        loaded.world.get_property(prop, "NonIntMouseEvents"),
        Some(Value::Bool(true))
    );
    assert_eq!(
        text(call_native(&mut loaded, prop, "Caption", &[Value::Int(2)])),
        "Hello"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn talk_sprite_sets_forced_talk_anims_and_sprite_objects() {
    use luma_adventure::EntityRef;
    use luma_formats::font::TextAlign;
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    let prop_ref = EntityRef::Node { layer: 0, index: 0 };
    runtime
        .world
        .scene_mut()
        .unwrap()
        .entity_mut(prop_ref)
        .unwrap()
        .active = true;
    let prop = runtime.world.node_id(prop_ref).unwrap();
    let shown = |runtime: &Runtime| runtime.world.scene().unwrap().shown_sprite(prop_ref);
    let talk_sprites = |runtime: &Runtime| {
        let e = runtime.world.scene().unwrap().entity(prop_ref).unwrap();
        (e.talk_sprites.clone(), e.talk_special_sprites.clone())
    };

    // AddTalkSprite: a file that does not load fails; the others join the
    // generic or the special set and the next sentence plays them.
    assert_eq!(
        call_native(
            &mut runtime,
            prop,
            "AddTalkSprite",
            &[Value::from("missing.sprite"), Value::Bool(false)]
        ),
        Value::Bool(false)
    );
    assert_eq!(
        call_native(
            &mut runtime,
            prop,
            "AddTalkSprite",
            &[Value::from("art\\walk.sprite"), Value::Bool(false)]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        call_native(
            &mut runtime,
            prop,
            "AddTalkSprite",
            &[Value::from("art\\stand.sprite"), Value::Bool(true)]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        talk_sprites(&runtime),
        (
            vec!["art\\walk.sprite".to_string()],
            vec!["art\\stand.sprite".to_string()]
        )
    );
    assert!(
        runtime
            .world
            .talk(prop, "Hi", None, 500, None, TextAlign::Center)
    );
    runtime.tick(16);
    assert_eq!(shown(&runtime).as_deref(), Some("art\\walk.sprite"));
    assert!(runtime.world.talk(
        prop,
        "Hi",
        None,
        500,
        Some("stand".into()),
        TextAlign::Center
    ));
    runtime.tick(16);
    assert_eq!(
        shown(&runtime).as_deref(),
        Some("art\\stand.sprite"),
        "a special stance by name"
    );

    // SetTalkSprite replaces a set, and what the entity is playing from it.
    assert!(
        runtime
            .world
            .talk(prop, "Hi", None, 500, None, TextAlign::Center)
    );
    runtime.tick(16);
    assert_eq!(shown(&runtime).as_deref(), Some("art\\walk.sprite"));
    assert_eq!(
        call_native(
            &mut runtime,
            prop,
            "SetTalkSprite",
            &[Value::from("art\\prop.sprite"), Value::Bool(false)]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        talk_sprites(&runtime).0,
        vec!["art\\prop.sprite".to_string()]
    );
    assert_eq!(shown(&runtime).as_deref(), Some("art\\prop.sprite"));
    assert_eq!(
        call_native(
            &mut runtime,
            prop,
            "RemoveTalkSprite",
            &[Value::from("ART\\PROP.SPRITE"), Value::Bool(false)]
        ),
        Value::Bool(true)
    );
    assert!(talk_sprites(&runtime).0.is_empty());
    runtime.world.stop_talk(prop);

    // ForceTalkAnim: the next sentence starts with that sprite, then the
    // sets are used again; the sentence after is back to normal.
    assert_eq!(
        call_native(
            &mut runtime,
            prop,
            "AddTalkSprite",
            &[Value::from("art\\walk.sprite"), Value::Bool(false)]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        call_native(
            &mut runtime,
            prop,
            "ForceTalkAnim",
            &[Value::from("art\\stand.sprite")]
        ),
        Value::Bool(true)
    );
    assert!(
        runtime
            .world
            .talk(prop, "Forced", None, 500, None, TextAlign::Center)
    );
    runtime.tick(16);
    assert_eq!(shown(&runtime).as_deref(), Some("art\\stand.sprite"));
    assert!(
        runtime
            .world
            .talk(prop, "Normal", None, 500, None, TextAlign::Center)
    );
    runtime.tick(16);
    assert_eq!(shown(&runtime).as_deref(), Some("art\\walk.sprite"));
    assert_eq!(
        call_native(
            &mut runtime,
            prop,
            "ForceTalkAnim",
            &[Value::from("missing.sprite")]
        ),
        Value::Bool(true)
    );
    assert!(
        runtime
            .world
            .talk(prop, "Dropped", None, 500, None, TextAlign::Center)
    );
    let log = runtime.tick(16);
    assert!(
        log.iter().any(|l| l.contains("error loading talk sprite")),
        "{log:?}"
    );
    assert_eq!(shown(&runtime).as_deref(), Some("art\\walk.sprite"));

    // An actor's sprite object stands for the sprite `SetSprite` gave it;
    // an item's for its image; both animate.
    let actor = runtime
        .world
        .load_actor(&mut runtime.engine, "actors\\bob.actor", false)
        .unwrap();
    assert_eq!(
        call_native(&mut runtime, actor, "GetSpriteObject", &[]),
        Value::Null
    );
    assert_eq!(
        call_native(
            &mut runtime,
            actor,
            "SetSprite",
            &[Value::from("art\\prop.sprite")]
        ),
        Value::Bool(true)
    );
    let sprite = call_native(&mut runtime, actor, "GetSpriteObject", &[])
        .as_native()
        .expect("the actor's sprite object");
    assert_eq!(
        runtime.world.get_property(sprite, "Filename"),
        Some(Value::from("art\\prop.sprite"))
    );
    assert_eq!(
        runtime.world.get_property(sprite, "NumFrames"),
        Some(Value::Int(2))
    );
    assert_eq!(
        runtime.world.get_property(sprite, "Owner"),
        Some(Value::Native(actor))
    );
    for _ in 0..20 {
        runtime.tick(16);
    }
    assert_eq!(
        runtime.world.get_property(sprite, "CurrentFrame"),
        Some(Value::Int(1))
    );
    assert_eq!(
        runtime.world.get_property(sprite, "Finished"),
        Some(Value::Bool(true))
    );
    let key = runtime.world.item_by_name("key").unwrap();
    let image = call_native(&mut runtime, key, "GetSpriteObject", &[])
        .as_native()
        .expect("the item's sprite object");
    assert_eq!(
        runtime.world.get_property(image, "Filename"),
        Some(Value::from("art\\green.png"))
    );
    assert_eq!(
        runtime.world.get_property(image, "NumFrames"),
        Some(Value::Int(1))
    );
    // ForceTalkAnim on an actor plays the file as the first stance too.
    assert_eq!(
        call_native(
            &mut runtime,
            actor,
            "ForceTalkAnim",
            &[Value::from("art\\walk.sprite")]
        ),
        Value::Bool(true)
    );
    assert!(
        runtime
            .world
            .talk(actor, "Hey", None, 500, None, TextAlign::Center)
    );
    runtime.tick(16);
    assert_eq!(
        runtime.world.actor(actor).unwrap().current_sprite(),
        Some("art\\walk.sprite")
    );
    // Actors keep their talk sets in the file: the method only loads.
    assert_eq!(
        call_native(
            &mut runtime,
            actor,
            "AddTalkSprite",
            &[Value::from("missing.sprite"), Value::Bool(false)]
        ),
        Value::Bool(false)
    );
    assert_eq!(
        call_native(
            &mut runtime,
            actor,
            "AddTalkSprite",
            &[Value::from("art\\stand.sprite"), Value::Bool(false)]
        ),
        Value::Bool(true)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn regions_are_edited_entities_get_regions_and_waypoint_groups_switch_off() {
    use luma_adventure::EntityRef;
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

    // The floor region's polygon is edited point by point; an index
    // outside the polygon is refused.
    let floor = runtime.world.region_id(0, 1).unwrap();
    let points = |runtime: &Runtime| {
        runtime
            .world
            .scene()
            .unwrap()
            .region(0, 1)
            .unwrap()
            .points
            .clone()
    };
    let point = |runtime: &mut Runtime, i: i32| {
        let p = call_native(runtime, floor, "GetPoint", &[Value::Int(i)]);
        (
            p.get_property("X", &()).unwrap_or(Value::Null),
            p.get_property("Y", &()).unwrap_or(Value::Null),
        )
    };
    assert_eq!(point(&mut runtime, 0), (Value::Int(0), Value::Int(16)));
    assert_eq!(
        call_native(
            &mut runtime,
            floor,
            "AddPoint",
            &[Value::Int(10), Value::Int(40)]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        call_native(
            &mut runtime,
            floor,
            "InsertPoint",
            &[Value::Int(0), Value::Int(1), Value::Int(1)]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        call_native(
            &mut runtime,
            floor,
            "SetPoint",
            &[Value::Int(0), Value::Int(2), Value::Int(3)]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        call_native(
            &mut runtime,
            floor,
            "SetPoint",
            &[Value::Int(9), Value::Int(2), Value::Int(3)]
        ),
        Value::Bool(false)
    );
    assert_eq!(
        runtime.world.get_property(floor, "NumPoints"),
        Some(Value::Int(6))
    );
    assert_eq!(point(&mut runtime, 0), (Value::Int(2), Value::Int(3)));
    assert_eq!(
        call_native(&mut runtime, floor, "RemovePoint", &[Value::Int(0)]),
        Value::Bool(true)
    );
    assert_eq!(
        call_native(&mut runtime, floor, "RemovePoint", &[Value::Int(9)]),
        Value::Bool(false)
    );
    assert_eq!(points(&runtime).len(), 5);
    assert_eq!(
        points(&runtime)[4],
        luma_formats::scene::Point { x: 10, y: 40 }
    );

    // An entity without a region gets one with `CreateRegion`, empty
    // until points are added; the region is then its clickable area.
    let lamp_ref = EntityRef::Node { layer: 0, index: 2 };
    let lamp = runtime.world.node_id(lamp_ref).unwrap();
    {
        let e = runtime
            .world
            .scene_mut()
            .unwrap()
            .entity_mut(lamp_ref)
            .unwrap();
        e.active = true;
        e.interactive = true;
    }
    assert_eq!(
        runtime.world.get_property(lamp, "Region"),
        Some(Value::Null)
    );
    assert_eq!(
        call_native(&mut runtime, lamp, "DeleteRegion", &[]),
        Value::Bool(false)
    );
    let region = call_native(&mut runtime, lamp, "CreateRegion", &[])
        .as_native()
        .expect("a region object");
    assert_eq!(
        runtime.world.get_property(lamp, "Region"),
        Some(Value::Native(region))
    );
    assert_eq!(
        call_native(&mut runtime, lamp, "CreateRegion", &[]),
        Value::Native(region),
        "an entity keeps the region it has"
    );
    assert_eq!(
        runtime.world.get_property(region, "Type"),
        Some(Value::from("region"))
    );
    assert_eq!(
        runtime.world.get_property(region, "NumPoints"),
        Some(Value::Int(0))
    );
    assert_eq!(
        runtime.world.get_property(region, "Blocked"),
        None,
        "a plain region has no scene-region members"
    );
    for (x, y) in [(20, 20), (26, 20), (26, 26), (20, 26)] {
        call_native(
            &mut runtime,
            region,
            "AddPoint",
            &[Value::Int(x), Value::Int(y)],
        );
    }
    runtime
        .world
        .set_property(region, "Name", &Value::from("hot"));
    assert_eq!(
        runtime.world.get_property(region, "Name"),
        Some(Value::from("hot"))
    );
    runtime.world.skip_to(0, 0);
    runtime.mouse_move(23, 23);
    runtime.tick(16);
    assert_eq!(runtime.world.active_object(), Some(lamp));
    assert_eq!(
        call_native(&mut runtime, lamp, "DeleteRegion", &[]),
        Value::Bool(true)
    );
    assert_eq!(
        runtime.world.get_property(lamp, "Region"),
        Some(Value::Null)
    );
    assert!(!runtime.world.objects().contains(region));
    runtime.tick(16);
    assert_eq!(runtime.world.active_object(), None);

    // A region from the file is a region object from the start, and the
    // edits are part of a saved game.
    runtime.change_scene("scenes\\small.scene").unwrap();
    let spot = runtime.world.find_node("spot").unwrap();
    let spot_region = runtime
        .world
        .get_property(spot, "Region")
        .and_then(|v| v.as_native())
        .expect("the file's region");
    assert_eq!(
        runtime.world.get_property(spot_region, "NumPoints"),
        Some(Value::Int(4))
    );
    call_native(
        &mut runtime,
        spot_region,
        "AddPoint",
        &[Value::Int(8), Value::Int(14)],
    );
    runtime.save_game(12, "regions", false).unwrap();
    let mut loaded = Runtime::with_audio(Vfs::open_game(&dir).unwrap(), Audio::null()).unwrap();
    loaded.start();
    loaded.tick(16);
    loaded.load_game(12).unwrap();
    assert_eq!(
        loaded.world.get_property(spot_region, "NumPoints"),
        Some(Value::Int(5))
    );

    // Waypoint groups are objects: switching one off takes its points
    // out of path finding; the handle dies with the scene.
    runtime.change_scene("scenes\\hall.scene").unwrap();
    let scene = runtime.world.scene_id();
    assert_eq!(
        runtime.world.get_property(scene, "NumWaypointGroups"),
        Some(Value::Int(1))
    );
    assert_eq!(
        call_native(&mut runtime, scene, "GetWaypointGroup", &[Value::Int(1)]),
        Value::Null
    );
    let group = call_native(&mut runtime, scene, "GetWaypointGroup", &[Value::Int(0)])
        .as_native()
        .expect("a waypoint group object");
    assert_eq!(
        runtime.world.get_property(group, "Type"),
        Some(Value::from("waypoint-group"))
    );
    assert_eq!(
        runtime.world.get_property(group, "Name"),
        Some(Value::from("wp"))
    );
    assert_eq!(
        runtime.world.get_property(group, "Active"),
        Some(Value::Bool(true))
    );
    let waypoints = |runtime: &Runtime| {
        luma_adventure::actor::scene_obstacles(&runtime.world.scene().unwrap().scene)
            .1
            .len()
    };
    assert_eq!(waypoints(&runtime), 2);
    runtime
        .world
        .set_property(group, "Active", &Value::Bool(false));
    assert_eq!(
        runtime.world.get_property(group, "Active"),
        Some(Value::Bool(false))
    );
    assert_eq!(waypoints(&runtime), 0);
    runtime.change_scene("scenes\\room.scene").unwrap();
    assert!(!runtime.world.objects().contains(group));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn scripts_create_edit_and_delete_scene_content() {
    use luma_adventure::{DrawItem, EntityRef};
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
    let scene = runtime.world.scene_id();
    let main = runtime.world.layer_id(0).unwrap();
    let name = |runtime: &Runtime, id| runtime.world.get_property(id, "Name");

    // Free entities are created empty or loaded from a file; removing
    // one keeps the handles of the others good.
    let created = call_native(&mut runtime, scene, "CreateEntity", &[Value::from("dyn")])
        .as_native()
        .expect("a created entity");
    assert_eq!(name(&runtime, created), Some(Value::from("dyn")));
    assert_eq!(
        runtime.world.get_property(created, "Scalable"),
        Some(Value::Bool(true))
    );
    assert_eq!(
        runtime.world.get_property(scene, "NumFreeNodes"),
        Some(Value::Int(2))
    );
    runtime.world.set_property(created, "X", &Value::Int(10));
    runtime.world.set_property(created, "Y", &Value::Int(20));
    assert_eq!(
        call_native(
            &mut runtime,
            created,
            "SetSprite",
            &[Value::from("art\\green.png")]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        call_native(
            &mut runtime,
            scene,
            "LoadEntity",
            &[Value::from("missing.entity")]
        ),
        Value::Null
    );
    let pick = call_native(
        &mut runtime,
        scene,
        "LoadEntity",
        &[Value::from("verb.entity")],
    )
    .as_native()
    .expect("the loaded entity");
    assert_eq!(name(&runtime, pick), Some(Value::from("pick")));
    assert_eq!(
        runtime.world.get_property(pick, "Caption"),
        Some(Value::from("Hello"))
    );
    assert_eq!(
        call_native(&mut runtime, pick, "GetSprite", &[]),
        Value::from("art\\green.png")
    );
    assert_eq!(
        runtime.engine.scripts_of(pick).len(),
        1,
        "its script is attached"
    );
    assert_eq!(
        call_native(&mut runtime, scene, "GetFreeNode", &[Value::from("pick")]),
        Value::Native(pick)
    );
    runtime.world.skip_to(0, 0);
    runtime.mouse_move(11, 21);
    runtime.tick(16);
    assert_eq!(
        runtime.world.active_object(),
        Some(created),
        "drawn and under the mouse"
    );
    call_native(
        &mut runtime,
        scene,
        "UnloadObject",
        &[Value::Native(created)],
    );
    assert!(!runtime.world.objects().contains(created));
    assert_eq!(
        runtime.world.get_property(scene, "NumFreeNodes"),
        Some(Value::Int(2))
    );
    assert_eq!(
        name(&runtime, pick),
        Some(Value::from("pick")),
        "renumbered, still good"
    );
    assert_eq!(
        call_native(&mut runtime, scene, "GetFreeNode", &[Value::Int(1)]),
        Value::Native(pick)
    );

    // Nodes join and leave a layer; the handles of the others follow.
    let floor = runtime.world.region_id(0, 1).unwrap();
    let lamp = runtime
        .world
        .node_id(EntityRef::Node { layer: 0, index: 2 })
        .unwrap();
    let r0 = call_native(
        &mut runtime,
        main,
        "InsertRegion",
        &[Value::Int(0), Value::from("r0")],
    )
    .as_native()
    .expect("a region node");
    assert_eq!(
        runtime.world.get_property(main, "NumNodes"),
        Some(Value::Int(5))
    );
    assert_eq!(runtime.world.region_id(0, 0), Some(r0));
    assert_eq!(runtime.world.region_id(0, 2), Some(floor));
    assert_eq!(name(&runtime, floor), Some(Value::from("floor")));
    assert_eq!(name(&runtime, lamp), Some(Value::from("lamp")));
    assert_eq!(
        call_native(&mut runtime, main, "GetNode", &[Value::Int(3)]),
        Value::Native(lamp)
    );
    let added = call_native(&mut runtime, main, "AddEntity", &[Value::from("added")])
        .as_native()
        .expect("an entity node");
    assert_eq!(
        runtime.world.get_property(added, "Scalable"),
        Some(Value::Bool(false))
    );
    assert_eq!(
        call_native(&mut runtime, main, "GetNode", &[Value::Int(5)]),
        Value::Native(added)
    );
    call_native(
        &mut runtime,
        main,
        "InsertEntity",
        &[Value::Int(99), Value::from("tail")],
    );
    assert_eq!(
        runtime.world.get_property(main, "NumNodes"),
        Some(Value::Int(7))
    );
    assert_eq!(
        call_native(&mut runtime, main, "DeleteNode", &[Value::Native(r0)]),
        Value::Bool(true)
    );
    assert_eq!(
        call_native(&mut runtime, main, "DeleteNode", &[Value::Int(42)]),
        Value::Bool(false)
    );
    assert_eq!(
        runtime.world.get_property(main, "NumNodes"),
        Some(Value::Int(6))
    );
    assert!(!runtime.world.objects().contains(r0));
    assert_eq!(runtime.world.region_id(0, 1), Some(floor));
    assert_eq!(name(&runtime, lamp), Some(Value::from("lamp")));

    // Layers too; the main layer stays.
    let fx = call_native(&mut runtime, scene, "AddLayer", &[Value::from("fx")])
        .as_native()
        .expect("a layer");
    assert_eq!(
        runtime.world.get_property(scene, "NumLayers"),
        Some(Value::Int(2))
    );
    assert_eq!(
        runtime.world.get_property(fx, "Width"),
        Some(Value::Int(200))
    );
    let bg = call_native(
        &mut runtime,
        scene,
        "InsertLayer",
        &[Value::Int(0), Value::from("bg")],
    )
    .as_native()
    .expect("a layer");
    assert_eq!(
        runtime.world.get_property(scene, "NumLayers"),
        Some(Value::Int(3))
    );
    assert_eq!(runtime.world.layer_id(1), Some(main));
    assert_eq!(
        runtime.world.get_property(scene, "MainLayer"),
        Some(Value::Native(main))
    );
    assert_eq!(name(&runtime, floor), Some(Value::from("floor")));
    assert_eq!(name(&runtime, lamp), Some(Value::from("lamp")));
    assert_eq!(
        call_native(&mut runtime, scene, "DeleteLayer", &[Value::Native(main)]),
        Value::Bool(false)
    );
    assert_eq!(
        call_native(&mut runtime, scene, "DeleteLayer", &[Value::Native(bg)]),
        Value::Bool(true)
    );
    assert_eq!(
        call_native(&mut runtime, scene, "DeleteLayer", &[Value::Int(5)]),
        Value::Bool(false)
    );
    assert_eq!(
        runtime.world.get_property(scene, "NumLayers"),
        Some(Value::Int(2))
    );
    assert_eq!(runtime.world.layer_id(0), Some(main));
    assert_eq!(name(&runtime, floor), Some(Value::from("floor")));

    // An actor stuck to the floor is drawn in the floor's slot even when
    // it stands outside it.
    let actor = runtime
        .world
        .load_actor(&mut runtime.engine, "actors\\bob.actor", false)
        .unwrap();
    runtime.world.set_property(actor, "Y", &Value::Int(5));
    runtime.tick(16);
    let slots = |runtime: &Runtime| {
        let order = runtime.world.draw_order();
        let actor_at = order
            .iter()
            .position(|i| matches!(i, DrawItem::Other { index: 0, .. }))
            .expect("the actor is drawn");
        let lamp_at = order
            .iter()
            .position(|i| *i == DrawItem::Node { layer: 0, index: 2 })
            .expect("the lamp is drawn");
        (actor_at, lamp_at)
    };
    let (actor_at, lamp_at) = slots(&runtime);
    assert!(actor_at > lamp_at, "outside every region: drawn last");
    assert_eq!(
        call_native(&mut runtime, actor, "StickToRegion", &[Value::from("nope")]),
        Value::Bool(false)
    );
    assert_eq!(
        call_native(
            &mut runtime,
            actor,
            "StickToRegion",
            &[Value::from("FLOOR")]
        ),
        Value::Bool(true)
    );
    let (actor_at, lamp_at) = slots(&runtime);
    assert!(actor_at < lamp_at, "stuck to the floor: drawn with it");
    assert_eq!(
        call_native(&mut runtime, actor, "StickToRegion", &[Value::Null]),
        Value::Bool(true)
    );
    let (actor_at, lamp_at) = slots(&runtime);
    assert!(actor_at > lamp_at);
    assert_eq!(
        call_native(
            &mut runtime,
            actor,
            "StickToRegion",
            &[Value::Native(floor)]
        ),
        Value::Bool(true)
    );

    // Everything created lives in the saved game.
    runtime.save_game(13, "built", false).unwrap();
    let mut loaded = Runtime::with_audio(Vfs::open_game(&dir).unwrap(), Audio::null()).unwrap();
    loaded.start();
    loaded.tick(16);
    loaded.load_game(13).unwrap();
    loaded.tick(16);
    assert_eq!(
        loaded.world.get_property(scene, "NumLayers"),
        Some(Value::Int(2))
    );
    assert_eq!(
        loaded.world.get_property(main, "NumNodes"),
        Some(Value::Int(6))
    );
    assert_eq!(
        loaded.world.get_property(scene, "NumFreeNodes"),
        Some(Value::Int(2))
    );
    assert_eq!(name(&loaded, pick), Some(Value::from("pick")));
    assert_eq!(name(&loaded, added), Some(Value::from("added")));
    assert!(
        loaded
            .world
            .scene()
            .unwrap()
            .sprite("art\\green.png")
            .is_some(),
        "loaded again"
    );
    let (actor_at, lamp_at) = slots(&loaded);
    assert!(actor_at < lamp_at, "still stuck to the floor");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn game_entities_outlive_scenes_and_attachments_follow_their_owner() {
    use luma_adventure::{DrawItem, EntityRef};
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
    let scene = runtime.world.scene_id();
    let name = |runtime: &Runtime, id| runtime.world.get_property(id, "Name");

    // An entity of the game is drawn among the scene's free objects and
    // stays when the scene changes.
    let hud = call_native(&mut runtime, game, "CreateEntity", &[Value::from("hud")])
        .as_native()
        .expect("a game entity");
    assert_eq!(
        runtime.world.get_property(hud, "Type"),
        Some(Value::from("entity"))
    );
    assert_eq!(name(&runtime, hud), Some(Value::from("hud")));
    runtime.world.set_property(hud, "X", &Value::Int(2));
    runtime.world.set_property(hud, "Y", &Value::Int(2));
    assert_eq!(
        call_native(
            &mut runtime,
            hud,
            "SetSprite",
            &[Value::from("art\\green.png")]
        ),
        Value::Bool(true)
    );
    runtime.world.skip_to(0, 0);
    runtime.mouse_move(3, 3);
    runtime.tick(16);
    assert_eq!(runtime.world.active_object(), Some(hud));
    assert_eq!(
        call_native(
            &mut runtime,
            game,
            "LoadEntity",
            &[Value::from("missing.entity")]
        ),
        Value::Null
    );
    let loaded = call_native(
        &mut runtime,
        game,
        "LoadEntity",
        &[Value::from("verb.entity")],
    )
    .as_native()
    .expect("a loaded game entity");
    assert_eq!(name(&runtime, loaded), Some(Value::from("pick")));
    assert_eq!(
        runtime.world.get_property(scene, "NumFreeNodes"),
        Some(Value::Int(1)),
        "not the scene's"
    );
    runtime.change_scene("scenes\\hall.scene").unwrap();
    runtime.tick(16);
    assert_eq!(name(&runtime, hud), Some(Value::from("hud")));
    assert_eq!(name(&runtime, loaded), Some(Value::from("pick")));
    assert_eq!(
        runtime.world.get_property(scene, "NumFreeNodes"),
        Some(Value::Int(0))
    );

    // Attachments are entities drawn around their owner, scaled with it,
    // and hit as it; GetAttachment counts the ones drawn before first.
    let actor = runtime
        .world
        .load_actor(&mut runtime.engine, "actors\\bob.actor", false)
        .unwrap();
    assert_eq!(
        call_native(
            &mut runtime,
            actor,
            "AddAttachment",
            &[
                Value::from("missing.entity"),
                Value::Bool(true),
                Value::Int(0),
                Value::Int(0)
            ]
        ),
        Value::Bool(false)
    );
    assert_eq!(
        call_native(
            &mut runtime,
            actor,
            "AddAttachment",
            &[
                Value::from("verb.entity"),
                Value::Bool(false),
                Value::Int(4),
                Value::Int(0)
            ]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        call_native(
            &mut runtime,
            actor,
            "AddAttachment",
            &[
                Value::from("verb.entity"),
                Value::Bool(true),
                Value::Int(0),
                Value::Int(-10)
            ]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        runtime.world.get_property(actor, "NumAttachments"),
        Some(Value::Int(2))
    );
    let pre = call_native(&mut runtime, actor, "GetAttachment", &[Value::Int(0)])
        .as_native()
        .expect("the attachment drawn before");
    let post = call_native(&mut runtime, actor, "GetAttachment", &[Value::Int(1)])
        .as_native()
        .expect("the attachment drawn after");
    assert_ne!(pre, post);
    assert_eq!(runtime.world.get_property(pre, "Y"), Some(Value::Int(-10)));
    assert_eq!(runtime.world.get_property(post, "X"), Some(Value::Int(4)));
    assert_eq!(
        call_native(&mut runtime, actor, "GetAttachment", &[Value::from("PICK")]),
        Value::Native(post)
    );
    assert_eq!(
        call_native(&mut runtime, actor, "GetAttachment", &[Value::Int(5)]),
        Value::Null
    );
    runtime.tick(16);
    let others = runtime
        .world
        .draw_order()
        .into_iter()
        .filter(|i| matches!(i, DrawItem::Other { .. }))
        .count();
    assert_eq!(
        others, 3,
        "the actor, the two game entities; attachments are not free objects"
    );
    let drawables = runtime.world.drawables();
    let bob = drawables
        .iter()
        .find(|d| d.x == 10 && d.y == 24)
        .expect("the actor's drawable");
    assert_eq!(bob.before.len(), 1);
    assert_eq!(bob.after.len(), 1);
    assert_eq!((bob.before[0].x, bob.before[0].y), (10, 14));
    assert_eq!((bob.after[0].x, bob.after[0].y), (14, 24));
    runtime.mouse_move(11, 15);
    runtime.tick(16);
    assert_eq!(
        runtime.world.active_object(),
        Some(actor),
        "an attachment is hit as its owner"
    );
    assert_eq!(
        call_native(
            &mut runtime,
            actor,
            "RemoveAttachment",
            &[Value::from("nothing")]
        ),
        Value::Bool(false)
    );
    assert_eq!(
        call_native(
            &mut runtime,
            actor,
            "RemoveAttachment",
            &[Value::Native(pre)]
        ),
        Value::Bool(true)
    );
    assert!(!runtime.world.objects().contains(pre));
    assert_eq!(
        runtime.world.get_property(actor, "NumAttachments"),
        Some(Value::Int(1))
    );
    assert_eq!(
        name(&runtime, post),
        Some(Value::from("pick")),
        "the other is renumbered, still good"
    );
    assert_eq!(
        call_native(
            &mut runtime,
            actor,
            "RemoveAttachment",
            &[Value::from("pick")]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        runtime.world.get_property(actor, "NumAttachments"),
        Some(Value::Int(0))
    );

    // An attachment of a scene entity goes with the scene; one of a game
    // entity stays, and both live in a saved game.
    let door = runtime
        .world
        .node_id(EntityRef::Node { layer: 0, index: 0 })
        .unwrap();
    assert_eq!(
        call_native(
            &mut runtime,
            door,
            "AddAttachment",
            &[
                Value::from("verb.entity"),
                Value::Bool(true),
                Value::Int(0),
                Value::Int(0)
            ]
        ),
        Value::Bool(true)
    );
    let door_hat = call_native(&mut runtime, door, "GetAttachment", &[Value::Int(0)])
        .as_native()
        .unwrap();
    assert_eq!(
        call_native(
            &mut runtime,
            hud,
            "AddAttachment",
            &[
                Value::from("verb.entity"),
                Value::Bool(false),
                Value::Int(1),
                Value::Int(1)
            ]
        ),
        Value::Bool(true)
    );
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    assert!(
        !runtime.world.objects().contains(door_hat),
        "gone with the scene"
    );
    assert_eq!(
        runtime.world.get_property(hud, "NumAttachments"),
        Some(Value::Int(1))
    );
    runtime.save_game(14, "hud", false).unwrap();
    let mut saved = Runtime::with_audio(Vfs::open_game(&dir).unwrap(), Audio::null()).unwrap();
    saved.start();
    saved.tick(16);
    saved.load_game(14).unwrap();
    saved.tick(16);
    assert_eq!(name(&saved, hud), Some(Value::from("hud")));
    assert_eq!(
        saved.world.get_property(hud, "NumAttachments"),
        Some(Value::Int(1))
    );
    assert!(
        saved.world.game_view().sprite("art\\green.png").is_some(),
        "loaded again"
    );
    let drawables = saved.world.drawables();
    let hud_draw = drawables
        .iter()
        .find(|d| d.x == 2 && d.y == 2)
        .expect("the hud");
    assert_eq!(hud_draw.after.len(), 1);

    // Game.UnloadObject removes a game entity with its attachments.
    call_native(&mut runtime, game, "UnloadObject", &[Value::Native(hud)]);
    assert!(!runtime.world.objects().contains(hud));
    assert_eq!(name(&runtime, loaded), Some(Value::from("pick")));
    assert_eq!(runtime.world.game_view().entity_refs().len(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn objects_have_their_own_inventories_and_the_box_switches_between_them() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    let game = runtime.world.game_id();
    let actor = runtime
        .world
        .load_actor(&mut runtime.engine, "actors\\bob.actor", false)
        .unwrap();
    let key = runtime.world.item_by_name("key").unwrap();
    let coin = runtime.world.item_by_name("coin").unwrap();

    // The game's own inventory and the actor's are separate lists;
    // IsItemTaken sees both.
    call_native(&mut runtime, game, "TakeItem", &[Value::from("key")]);
    call_native(&mut runtime, actor, "TakeItem", &[Value::from("coin")]);
    assert_eq!(runtime.world.taken_items(), &[key]);
    assert_eq!(runtime.world.taken_items_of(actor), &[coin]);
    assert_eq!(
        runtime.world.get_property(game, "NumItems"),
        Some(Value::Int(1))
    );
    assert_eq!(
        runtime.world.get_property(actor, "NumItems"),
        Some(Value::Int(1))
    );
    assert_eq!(
        call_native(&mut runtime, game, "HasItem", &[Value::from("coin")]),
        Value::Bool(false)
    );
    assert_eq!(
        call_native(&mut runtime, actor, "HasItem", &[Value::from("coin")]),
        Value::Bool(true)
    );
    assert_eq!(
        call_native(&mut runtime, game, "IsItemTaken", &[Value::from("coin")]),
        Value::Bool(true)
    );
    assert_eq!(
        call_native(&mut runtime, actor, "GetItem", &[Value::Int(0)]),
        Value::Native(coin)
    );
    assert_eq!(
        call_native(&mut runtime, game, "GetItem", &[Value::Int(0)]),
        Value::Native(key)
    );
    assert!(runtime.world.is_item_taken("coin"));
    // The scene entity tied to a taken item hides whoever took it.
    call_native(&mut runtime, actor, "TakeItem", &[Value::from("key")]);
    let keyspot = runtime.world.find_node("keyspot").unwrap();
    assert_eq!(
        runtime.world.get_property(keyspot, "Active"),
        Some(Value::Bool(false))
    );
    assert_eq!(runtime.world.taken_items_of(actor), &[coin, key]);
    assert_eq!(
        call_native(&mut runtime, actor, "DropItem", &[Value::from("key")]),
        Value::Null
    );
    assert_eq!(runtime.world.taken_items_of(actor), &[coin]);
    assert_eq!(runtime.world.taken_items(), &[key], "the game's copy stays");

    // The box shows the game's inventory until InventoryObject names
    // another; the scroll position is kept per inventory.
    assert_eq!(
        runtime.world.get_property(game, "InventoryObject"),
        Some(Value::Native(game))
    );
    runtime
        .world
        .set_property(game, "InventoryVisible", &Value::Bool(true));
    runtime
        .world
        .set_property(game, "InventoryScrollOffset", &Value::Int(1));
    runtime
        .world
        .set_property(game, "InventoryObject", &Value::Native(actor));
    assert_eq!(
        runtime.world.get_property(game, "InventoryObject"),
        Some(Value::Native(actor))
    );
    assert_eq!(
        runtime.world.get_property(game, "InventoryScrollOffset"),
        Some(Value::Int(0))
    );
    runtime.tick(16);
    let cells: Vec<luma_script::NativeId> = runtime
        .world
        .inventory()
        .items
        .iter()
        .filter(|(_, item)| item.current_sprite.is_some())
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(cells, vec![coin], "the box shows the actor's coin");
    runtime
        .world
        .set_property(game, "InventoryObject", &Value::Null);
    assert_eq!(
        runtime.world.get_property(game, "InventoryObject"),
        Some(Value::Native(game))
    );
    assert_eq!(
        runtime.world.get_property(game, "InventoryScrollOffset"),
        Some(Value::Int(1))
    );
    runtime
        .world
        .set_property(game, "InventoryObject", &Value::Native(game));
    assert_eq!(
        runtime.world.get_property(game, "InventoryObject"),
        Some(Value::Native(game))
    );

    // Deleting an item takes it out of every inventory; unloading the
    // owner shown brings the box back to the game's.
    runtime
        .world
        .set_property(game, "InventoryObject", &Value::Native(actor));
    assert!(runtime.world.delete_item(&mut runtime.engine, coin));
    assert!(runtime.world.taken_items_of(actor).is_empty());
    runtime.save_game(15, "pockets", false).unwrap();
    assert!(runtime.world.unload_object(&mut runtime.engine, actor));
    assert_eq!(
        runtime.world.get_property(game, "InventoryObject"),
        Some(Value::Native(game))
    );

    // A saved game keeps each inventory and the owner shown.
    let mut loaded = Runtime::with_audio(Vfs::open_game(&dir).unwrap(), Audio::null()).unwrap();
    loaded.start();
    loaded.tick(16);
    loaded.load_game(15).unwrap();
    assert_eq!(
        loaded.world.get_property(game, "InventoryObject"),
        Some(Value::Native(actor))
    );
    assert_eq!(loaded.world.taken_items(), &[key]);
    call_native(&mut loaded, actor, "TakeItem", &[Value::from("key")]);
    assert_eq!(loaded.world.taken_items_of(actor), &[key]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn two_d_only_subframes_are_neither_drawn_nor_hit() {
    // `2D_ONLY` sub-frames are for the original engine's software
    // renderer and `3D_ONLY` ones for its accelerated renderer
    // (`CBFrame::Draw`); Luma always draws with the GPU, so the first
    // are skipped, hit areas included, and the second always shown.
    let dir = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "startup.settings",
        b"SETTINGS { GAME=\"default.game\" RESOLUTION { 32,32 } }".to_vec(),
    );
    pkg.add_file("default.game", b"GAME { NAME=\"Flags\" }".to_vec());
    pkg.add_file("art\\red.png", png(4, 4, [255, 0, 0, 255]));
    pkg.add_file("art\\green.png", png(4, 4, [0, 255, 0, 255]));
    pkg.add_file(
        "art\\flags.sprite",
        b"SPRITE { FRAME { IMAGE=\"art\\red.png\" 2D_ONLY=TRUE SUBFRAME { IMAGE=\"art\\green.png\" HOTSPOT {-8, 0} 3D_ONLY=TRUE } } }".to_vec(),
    );
    pkg.add_file(
        "scenes\\flags.scene",
        b"SCENE {
  NAME=\"Flags\"
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=32 HEIGHT=32
    ENTITY { NAME=\"flags\" X=2 Y=2 SPRITE=\"art\\flags.sprite\" }
  }
}"
        .to_vec(),
    );
    std::fs::write(dir.join("data.dcp"), pkg.build()).unwrap();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.change_scene("scenes\\flags.scene").unwrap();
    runtime.tick(16);
    runtime.tick(16);
    let flags = runtime.world.find_node("flags").unwrap();
    assert_eq!(
        runtime.world.object_at(3, 3),
        None,
        "the 2D_ONLY sub-frame (red, at 2..6) is not drawn"
    );
    assert_eq!(
        runtime.world.object_at(11, 3),
        Some(flags),
        "the 3D_ONLY sub-frame (green, at 10..14) is"
    );
    let frame = &runtime
        .world
        .scene()
        .unwrap()
        .sprite("art\\flags.sprite")
        .unwrap()
        .sprite
        .frames[0];
    let drawn: Vec<_> = luma_adventure::drawn_subframes(frame)
        .map(|s| s.image.clone())
        .collect();
    assert_eq!(drawn, [Some("art\\green.png".to_string())]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn private_settings_persist_in_the_save_directory() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    let game = runtime.world.game_id();
    assert!(
        runtime
            .world
            .attach(&mut runtime.engine, game, "settings.script")
    );
    runtime.tick(16);
    assert_eq!(result(&runtime, "volumeBefore"), Value::Int(100));
    assert_eq!(result(&runtime, "volume"), Value::Int(80));
    assert_eq!(result(&runtime, "lang"), Value::from("fr"));
    assert_eq!(result(&runtime, "missing"), Value::from("none"));
    assert_eq!(result(&runtime, "langAsNumber"), Value::Int(0));
    let ini = std::fs::read_to_string(dir.join("saves").join("settings.ini")).unwrap();
    assert_eq!(ini, "[PrivateSettings]\nVolume=80\nLang=fr\n");

    // A second run of the game reads the values back.
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    assert_eq!(runtime.world.read_setting("volume").as_deref(), Some("80"));
    assert_eq!(runtime.world.read_setting("Lang").as_deref(), Some("fr"));
    assert_eq!(runtime.world.read_setting("Missing"), None);
}

#[test]
fn system_properties_checksums_and_run_script() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    // 70 frames of 16 ms: the first whole second counted 63 frames.
    for _ in 0..70 {
        runtime.tick(16);
    }
    assert_eq!(runtime.world.fps(), 63);
    let game = runtime.world.game_id();
    assert!(
        runtime
            .world
            .attach(&mut runtime.engine, game, "services.script")
    );
    let log = runtime.tick(16);
    assert_eq!(result(&runtime, "fps"), Value::Int(63));
    assert_eq!(result(&runtime, "windowsTime"), Value::Int(71 * 16));
    assert_eq!(result(&runtime, "currentTime"), Value::Int(71 * 16));
    assert_eq!(result(&runtime, "soundAvailable"), Value::Bool(false));
    assert_eq!(result(&runtime, "windowed"), Value::Bool(false));
    assert_eq!(result(&runtime, "accelerated"), Value::Bool(true));
    assert_eq!(result(&runtime, "sfx"), Value::Int(40));
    assert_eq!(result(&runtime, "sfxMethod"), Value::Int(40));
    assert_eq!(result(&runtime, "master"), Value::Int(70));
    assert_eq!(runtime.world.audio().volume(luma_audio::SoundType::Sfx), 40);
    assert_eq!(
        log.iter()
            .filter(|l| l.contains("SFXVolume attribute is obsolete"))
            .count(),
        1,
        "{log:?}"
    );
    assert_eq!(result(&runtime, "encoding"), Value::Int(0));
    assert_eq!(result(&runtime, "encodingClamped"), Value::Int(1));
    assert_eq!(result(&runtime, "startupBefore"), Value::Null);
    assert_eq!(
        result(&runtime, "startup"),
        Value::from("scenes\\hall.scene")
    );
    assert_eq!(runtime.world.startup_scene(), Some("scenes\\hall.scene"));
    assert_eq!(result(&runtime, "caption"), Value::from("Hi"));
    let crc = crc32fast::hash(b"GREET\tHello\r\n");
    assert_eq!(result(&runtime, "crc"), Value::Int(crc as i32));
    assert_eq!(result(&runtime, "crcHex"), Value::from(format!("{crc:x}")));
    assert_eq!(result(&runtime, "crcMissing"), Value::Null);
    assert_eq!(result(&runtime, "openedNothing"), Value::Bool(false));
    assert_eq!(result(&runtime, "ran"), Value::Bool(true));
    assert_eq!(result(&runtime, "ranMissing"), Value::Bool(false));
    assert!(
        log.iter()
            .any(|l| l.contains("'RunScript' method is now obsolete")),
        "{log:?}"
    );
    // The script RunScript attached ran on the game.
    runtime.tick(16);
    assert_eq!(result(&runtime, "lang"), Value::from("fr"));

    // A UTF-8 string table switches the encoding; the windowed flag is
    // the front-end's to set.
    runtime.world.set_windowed(true);
    assert_eq!(
        runtime.world.get_property(game, "WindowedMode"),
        Some(Value::Bool(true))
    );
}

#[test]
fn quick_messages_mouse_lock_reset_and_inventory_box_reload() {
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    runtime.tick(16);
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Room");
    let game = runtime.world.game_id();
    assert!(
        runtime
            .world
            .attach(&mut runtime.engine, game, "misc.script")
    );
    runtime.tick(16);

    // Msg: on screen for three seconds of the game timer.
    let messages = runtime.world.quick_messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, "Hello there");
    assert_eq!(
        messages[0].height, 16,
        "two lines of the system font on a 32-pixel screen"
    );
    for _ in 0..200 {
        runtime.tick(16);
    }
    assert!(runtime.world.quick_messages().is_empty());

    // LockMouseRect: the edges were given in the wrong order.
    assert_eq!(runtime.world.mouse_lock(), Some((4, 4, 20, 20)));
    runtime.mouse_move(0, 0);
    assert_eq!((runtime.world.mouse().x, runtime.world.mouse().y), (4, 4));
    runtime.mouse_move(30, 10);
    assert_eq!((runtime.world.mouse().x, runtime.world.mouse().y), (20, 10));
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "Unlock", false);
    runtime.tick(16);
    assert_eq!(runtime.world.mouse_lock(), None);
    runtime.mouse_move(0, 0);
    assert_eq!((runtime.world.mouse().x, runtime.world.mouse().y), (0, 0));

    // LoadInventoryBox replaces the box and its window.
    let old_window = runtime.world.inventory().window.unwrap();
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "NewBox", false);
    runtime.tick(16);
    assert_eq!(result(&runtime, "box"), Value::Bool(true));
    let inventory = runtime.world.inventory();
    assert_eq!(inventory.def.as_ref().unwrap().item_width, 12);
    assert_ne!(inventory.window, Some(old_window));
    assert!(!runtime.world.objects().contains(old_window));

    // Reset: the inventory, the remembered scenes, the globals go; the
    // scene left next is not remembered.
    assert!(runtime.world.take_item("key", None));
    runtime.engine.set_global("flag", Value::Int(5));
    runtime.change_scene("scenes\\hall.scene").unwrap();
    runtime.tick(16);
    runtime.change_scene("scenes\\room.scene").unwrap();
    runtime.tick(16);
    assert!(
        runtime
            .world
            .scene_states()
            .contains_key("scenes\\hall.scene")
    );
    runtime.change_scene("scenes\\hall.scene").unwrap();
    runtime.tick(16);
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "DoReset", false);
    runtime.tick(16);
    assert!(runtime.world.inventory().taken.is_empty());
    assert_eq!(runtime.world.inventory().items.len(), 2, "items reloaded");
    assert!(runtime.world.scene_states().is_empty());
    assert_eq!(runtime.engine.global("flag"), Some(&Value::Null));
    assert!(runtime.engine.global("Game").unwrap().is_native());
    assert_eq!(runtime.world.scene().unwrap().scene.name, "Hall");
    runtime.change_scene("scenes\\room.scene").unwrap();
    assert!(
        runtime.world.scene_states().is_empty(),
        "the hall was not noted"
    );
    runtime.change_scene("scenes\\hall.scene").unwrap();
    runtime.change_scene("scenes\\room.scene").unwrap();
    assert!(
        runtime
            .world
            .scene_states()
            .contains_key("scenes\\hall.scene")
    );
}

#[test]
fn screenshots_and_save_thumbnails_come_from_the_next_frame() {
    use luma_adventure::capture::Capture;
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    let game = runtime.world.game_id();
    assert!(!runtime.world.wants_frame());
    assert!(
        runtime
            .world
            .attach(&mut runtime.engine, game, "shots.script")
    );
    runtime.tick(16);
    assert_eq!(result(&runtime, "shot1"), Value::Bool(true));
    assert_eq!(result(&runtime, "shot2"), Value::Bool(true));
    assert_eq!(result(&runtime, "shotEx"), Value::Bool(true));
    assert_eq!(result(&runtime, "thumb"), Value::Bool(true));
    assert!(runtime.world.wants_frame());
    let captures = runtime.world.captures();
    assert_eq!(captures.len(), 4);
    assert_eq!(
        captures[0],
        Capture::Screenshot {
            path: dir.join("shot000.bmp"),
            size: None
        }
    );
    assert_eq!(
        captures[1],
        Capture::Screenshot {
            path: dir.join("shot001.bmp"),
            size: None
        }
    );
    assert_eq!(
        captures[2],
        Capture::Screenshot {
            path: dir.join("thumbs/t.png"),
            size: Some((8, 4))
        }
    );
    assert_eq!(captures[3], Capture::Thumbnail);
    assert!(runtime.world.save_thumbnail().is_none());

    // The front-end renders a frame and hands it over.
    let frame = Image {
        width: 32,
        height: 32,
        pixels: [200u8, 10, 10, 255].repeat(32 * 32),
        has_alpha_channel: true,
    };
    runtime.world.deliver_frame(&frame);
    let log = runtime.world.take_log();
    assert!(!runtime.world.wants_frame());
    assert_eq!(
        log.iter()
            .filter(|l| l.starts_with("Screenshot written"))
            .count(),
        3,
        "{log:?}"
    );
    let key = luma_formats::DEFAULT_COLOR_KEY;
    let shot =
        luma_formats::image::decode(&std::fs::read(dir.join("shot000.bmp")).unwrap(), key).unwrap();
    assert_eq!((shot.width, shot.height), (32, 32));
    let p = shot.pixel(3, 3).unwrap();
    assert_eq!((p.r, p.g, p.b), (200, 10, 10));
    assert!(dir.join("shot001.bmp").is_file());
    let small = luma_formats::image::decode(
        &std::fs::read(dir.join("thumbs").join("t.png")).unwrap(),
        key,
    )
    .unwrap();
    assert_eq!((small.width, small.height), (8, 4));
    assert!(runtime.world.save_thumbnail().is_some());

    // The next save carries the thumbnail, and only that one.
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "SaveIt", false);
    runtime.tick(16);
    assert_eq!(result(&runtime, "saved"), Value::Bool(true));
    let header = luma_adventure::read_save_header(&dir.join("saves").join("save006.dsv")).unwrap();
    let thumb = luma_formats::image::decode(header.thumbnail.as_deref().unwrap(), key).unwrap();
    assert_eq!((thumb.width, thumb.height), (8, 4));
    let p = thumb.pixel(0, 0).unwrap();
    assert_eq!((p.r, p.g, p.b), (200, 10, 10));
    assert!(runtime.world.save_thumbnail().is_none());

    // A thumbnail stored then deleted is not in the next save.
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "Store", false);
    runtime.tick(16);
    assert_eq!(runtime.world.captures(), [Capture::Thumbnail]);
    runtime.world.deliver_frame(&frame);
    assert!(runtime.world.save_thumbnail().is_some());
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "Drop", false);
    runtime.tick(16);
    assert!(runtime.world.save_thumbnail().is_none());
    runtime.save_game(7, "no thumb", false).unwrap();
    let header = luma_adventure::read_save_header(&dir.join("saves").join("save007.dsv")).unwrap();
    assert_eq!(header.thumbnail, None);
}

#[test]
fn loading_screens_and_the_loading_icon() {
    use luma_adventure::ScreenImage;
    let dir = game();
    let vfs = Vfs::open_game(&dir).unwrap();
    let mut runtime = Runtime::with_audio(vfs, Audio::null()).unwrap();
    runtime.start();
    runtime.tick(16);
    assert_eq!(runtime.world.loading_screen(), None);
    let game = runtime.world.game_id();
    assert!(
        runtime
            .world
            .attach(&mut runtime.engine, game, "screens.script")
    );
    runtime.tick(16);
    assert_eq!(
        runtime.world.loading_screen(),
        Some(&ScreenImage {
            path: "art\\green.png".into(),
            x: 2,
            y: 3
        })
    );
    assert_eq!(
        runtime.world.saving_screen(),
        Some(&ScreenImage {
            path: "art\\red.png".into(),
            x: 4,
            y: 5
        })
    );
    // The icon shows for the frame that follows the call, then goes.
    let icon = runtime.world.loading_icon().unwrap();
    assert_eq!((icon.x, icon.y, icon.persistent), (6, 7, false));
    assert!(icon.shown);
    runtime.tick(16);
    assert!(runtime.world.loading_icon().is_none());

    // A persistent icon stays and animates until hidden.
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "Persistent", false);
    runtime.tick(16);
    for _ in 0..10 {
        runtime.tick(16);
    }
    let icon = runtime.world.loading_icon().unwrap();
    assert!(icon.persistent);
    assert_eq!(icon.anim.current_frame, 1, "the prop sprite's second frame");
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "Hide", false);
    runtime.tick(16);
    assert!(runtime.world.loading_icon().is_none());

    // The screens travel with a saved game; null removes one.
    runtime.save_game(8, "screens", false).unwrap();
    runtime
        .engine
        .apply_event(&mut runtime.world, game, "ClearLoad", false);
    runtime.tick(16);
    assert_eq!(runtime.world.loading_screen(), None);
    runtime.load_game(8).unwrap();
    assert_eq!(
        runtime.world.loading_screen().map(|s| s.path.as_str()),
        Some("art\\green.png")
    );

    // A game file's own images are set at start.
    let dir2 = temp_dir();
    let mut pkg = PackageBuilder::new("data");
    pkg.add_file(
        "default.game",
        b"GAME { NAME=\"T\" LOAD_IMAGE=\"a.png\" LOAD_IMAGE_X=5 LOAD_IMAGE_Y=6 SAVE_IMAGE=\"a.png\" }".to_vec(),
    );
    pkg.add_file("a.png", png(4, 4, [0, 0, 255, 255]));
    std::fs::write(dir2.join("data.dcp"), pkg.build()).unwrap();
    let runtime = Runtime::with_audio(Vfs::open_game(&dir2).unwrap(), Audio::null()).unwrap();
    assert_eq!(
        runtime.world.loading_screen(),
        Some(&ScreenImage {
            path: "a.png".into(),
            x: 5,
            y: 6
        })
    );
    assert_eq!(
        runtime.world.saving_screen().map(|s| (s.x, s.y)),
        Some((0, 0))
    );
}
