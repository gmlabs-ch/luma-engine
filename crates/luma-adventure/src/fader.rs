//! The screen fader: a colour laid over the frame, fading in or out
//! (`Game.FadeOut`, `Game.FadeIn`, the fade of a scene change).
//!
//! Written with reference to `BFader.cpp` and `BTransitionMgr.cpp` of WME
//! Lite (MIT License, Copyright (c) 2011 Jan Nedoma); see `NOTICE`.

use luma_formats::Rgba;

/// How long the fade of a scene change lasts (`FADE_DURATION`).
pub const SCENE_FADE_MS: u64 = 200;

/// The fader's state.
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use super::*;

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
