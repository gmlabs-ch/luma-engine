//! The screen fader: a colour laid over the frame, fading in or out
//! (`Game.FadeOut`, `Game.FadeIn`), and the transition manager, which
//! plays the fade of a scene change on a layer of its own.
//!
//! Written with reference to `BFader.cpp` and `BTransitionMgr.cpp` of WME
//! Lite (MIT License, Copyright (c) 2011 Jan Nedoma); see `NOTICE`.

use luma_formats::Rgba;

/// How long the fade of a scene change lasts (`FADE_DURATION`).
pub const SCENE_FADE_MS: u64 = 200;

/// The fader's state.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fader {
    /// Whether the colour is shown.
    pub active: bool,
    /// Whether the fade has reached its target (`WaitFor` resumes).
    pub ready: bool,
    red: u8,
    green: u8,
    blue: u8,
    current_alpha: u8,
    source_alpha: u8,
    target_alpha: u8,
    duration: u64,
    start_time: u64,
    /// Whether the fade runs on the wall clock rather than the game
    /// timer (`SystemFadeOut`).
    system: bool,
}

impl Default for Fader {
    fn default() -> Self {
        Fader {
            active: false,
            ready: true,
            red: 0,
            green: 0,
            blue: 0,
            current_alpha: 0,
            source_alpha: 0,
            target_alpha: 0,
            duration: 1000,
            start_time: 0,
            system: false,
        }
    }
}

impl Fader {
    /// Fades from `color` (with its alpha) to nothing over `duration`
    /// milliseconds (`CBFader::FadeIn`).
    pub fn fade_in(&mut self, color: Rgba, duration: u64, system: bool, timer: u64, clock: u64) {
        self.ready = false;
        self.active = true;
        self.red = color.r;
        self.green = color.g;
        self.blue = color.b;
        self.source_alpha = color.a;
        self.target_alpha = 0;
        self.duration = duration;
        self.system = system;
        self.start_time = if system { clock } else { timer };
    }

    /// Fades from the current colour to `color` over `duration`
    /// milliseconds (`CBFader::FadeOut`).
    pub fn fade_out(&mut self, color: Rgba, duration: u64, system: bool, timer: u64, clock: u64) {
        self.ready = false;
        self.active = true;
        self.red = color.r;
        self.green = color.g;
        self.blue = color.b;
        self.source_alpha = self.current_alpha;
        self.target_alpha = color.a;
        self.duration = duration;
        self.system = system;
        self.start_time = if system { clock } else { timer };
    }

    /// One frame (`CBFader::Update`).
    pub fn update(&mut self, timer: u64, clock: u64) {
        if !self.active {
            return;
        }
        let now = if self.system { clock } else { timer };
        let elapsed = now.saturating_sub(self.start_time);
        if elapsed >= self.duration {
            self.current_alpha = self.target_alpha;
        } else {
            let delta = f64::from(self.target_alpha) - f64::from(self.source_alpha);
            let alpha =
                f64::from(self.source_alpha) + elapsed as f64 / self.duration as f64 * delta;
            self.current_alpha = alpha.clamp(0.0, 255.0) as u8;
        }
        self.ready = elapsed >= self.duration;
        if self.ready && self.current_alpha == 0 {
            self.active = false;
        }
    }

    /// Stops the fade (`Deactivate`).
    pub fn deactivate(&mut self) {
        self.active = false;
        self.ready = true;
    }

    /// The colour laid over the frame, `None` when nothing shows.
    pub fn color(&self) -> Option<Rgba> {
        if !self.active || self.current_alpha == 0 {
            return None;
        }
        Some(Rgba {
            r: self.red,
            g: self.green,
            b: self.blue,
            a: self.current_alpha,
        })
    }

    /// The colour as the engine's `0xAARRGGBB` integer (`GetFadeColor`).
    pub fn color_int(&self) -> i32 {
        luma_script::rgba(
            i32::from(self.red),
            i32::from(self.green),
            i32::from(self.blue),
            i32::from(self.current_alpha),
        )
    }
}

/// A transition of the screen (`TTransitionType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    /// The screen goes to black (`TRANSITION_FADE_OUT`).
    FadeOut,
    /// The screen comes back from black (`TRANSITION_FADE_IN`).
    FadeIn,
}

/// The transition manager (`CBTransitionMgr`): the fade to black before
/// a scene change and the fade from black after it, on the live clock,
/// drawn over everything else, the game's fader included. It is not part
/// of a saved game, as in the original.
#[derive(Debug, Clone, Default)]
pub struct Transition {
    /// The transition running, `None` when ready.
    kind: Option<TransitionKind>,
    /// When it began, on the live clock; taken at its first update.
    started: Option<u64>,
    /// The opacity of the black drawn this frame.
    opacity: Option<u8>,
    /// What `Game.Interactive` goes back to when a transition ends.
    orig_interactive: bool,
    /// Whether the interactive state is put back at the end. Once a
    /// transition was started non-interactive it stays set: the original
    /// never clears it (its `else m_PreserveInteractive;` does nothing),
    /// and it keeps `orig_interactive` in step with every later change
    /// of `Game.Interactive`, so the restore changes nothing then.
    preserve_interactive: bool,
}

impl Transition {
    /// Whether no transition runs (`IsReady`): a scheduled scene change
    /// waits for it.
    pub fn is_ready(&self) -> bool {
        self.kind.is_none()
    }

    /// The transition running.
    pub fn kind(&self) -> Option<TransitionKind> {
        self.kind
    }

    /// Starts a transition (`Start`); nothing while one runs. A
    /// non-interactive one turns `Game.Interactive` off until it ends.
    pub fn start(&mut self, kind: TransitionKind, non_interactive: bool, interactive: &mut bool) {
        if self.kind.is_some() {
            return;
        }
        if non_interactive {
            self.preserve_interactive = true;
            self.orig_interactive = *interactive;
            *interactive = false;
        }
        self.kind = Some(kind);
        self.started = None;
    }

    /// `Game.Interactive` was set (`CBGame::SetInteractive`): the state
    /// a transition goes back to follows.
    pub fn note_interactive(&mut self, interactive: bool) {
        self.orig_interactive = interactive;
    }

    /// One frame (`Update`), at the live clock: the opacity of the black
    /// to draw, and the end of the transition once more than
    /// [`SCENE_FADE_MS`] have passed since its first update; the frame
    /// it ends on is still drawn.
    pub fn update(&mut self, clock: u64, interactive: &mut bool) {
        self.opacity = None;
        let Some(kind) = self.kind else {
            return;
        };
        let start = *self.started.get_or_insert(clock);
        let time = clock.saturating_sub(start);
        // `Alpha`, how much of the frame shows: 255 to 0 fading out, 0
        // to 255 fading in, truncated; the black has the rest.
        let part = (time as f32 / SCENE_FADE_MS as f32 * 255.0) as i32;
        let alpha = match kind {
            TransitionKind::FadeOut => 255 - part,
            TransitionKind::FadeIn => part,
        }
        .clamp(0, 255);
        self.opacity = Some((255 - alpha) as u8);
        if time > SCENE_FADE_MS {
            self.kind = None;
            if self.preserve_interactive {
                *interactive = self.orig_interactive;
            }
        }
    }

    /// The opacity of the black laid over the frame, `None` when no
    /// transition was drawn at the last update.
    pub fn opacity(&self) -> Option<u8> {
        self.opacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transition_runs_on_the_clock_and_gives_the_interactive_state_back() {
        let mut transition = Transition::default();
        let mut interactive = true;
        assert!(transition.is_ready());
        transition.start(TransitionKind::FadeOut, true, &mut interactive);
        assert!(!transition.is_ready() && !interactive);
        // A second start while one runs is ignored.
        transition.start(TransitionKind::FadeIn, false, &mut interactive);
        assert_eq!(transition.kind(), Some(TransitionKind::FadeOut));
        // The clock starts at the first update.
        transition.update(1000, &mut interactive);
        assert_eq!(transition.opacity(), Some(0));
        transition.update(1100, &mut interactive);
        assert_eq!(transition.opacity(), Some(127));
        transition.update(1200, &mut interactive);
        assert_eq!(transition.opacity(), Some(255));
        assert!(!transition.is_ready(), "not past the duration yet");
        transition.update(1201, &mut interactive);
        assert_eq!(transition.opacity(), Some(255), "the last frame is black");
        assert!(transition.is_ready() && interactive);
        transition.update(1210, &mut interactive);
        assert_eq!(transition.opacity(), None);
        // A script that turned the game non-interactive meanwhile keeps
        // it so.
        transition.start(TransitionKind::FadeOut, true, &mut interactive);
        transition.note_interactive(false);
        transition.update(2000, &mut interactive);
        transition.update(2300, &mut interactive);
        assert!(transition.is_ready() && !interactive);
        // Fading in: from black to the frame.
        transition.start(TransitionKind::FadeIn, false, &mut interactive);
        transition.update(3000, &mut interactive);
        assert_eq!(transition.opacity(), Some(255));
        transition.update(3100, &mut interactive);
        assert_eq!(transition.opacity(), Some(128));
        transition.update(3250, &mut interactive);
        assert_eq!(transition.opacity(), Some(0));
        assert!(transition.is_ready());
    }

    #[test]
    fn fades_out_then_in() {
        let mut fader = Fader::default();
        assert!(fader.ready && fader.color().is_none());
        let black = Rgba {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        };
        fader.fade_out(black, 1000, false, 100, 0);
        assert!(!fader.ready);
        fader.update(600, 0);
        assert_eq!(fader.color().unwrap().a, 127);
        assert!(!fader.ready);
        fader.update(1100, 0);
        assert_eq!(fader.color().unwrap().a, 255);
        assert!(fader.ready && fader.active);
        fader.fade_in(black, 200, true, 0, 5000);
        fader.update(0, 5100);
        assert_eq!(fader.color().unwrap().a, 127);
        fader.update(0, 5300);
        assert!(fader.ready && !fader.active);
        assert!(fader.color().is_none());
    }
}
