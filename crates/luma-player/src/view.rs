//! Where the game's frame lies in the window: the letterbox mapping
//! between window pixels and game pixels, both ways.

use luma::render::letterbox;

/// A window position, in physical pixels of a drawable area of `size`,
/// in pixels of a game of resolution `game`. A position in the bars
/// around the frame falls outside the game's screen.
pub fn to_game(game: (u32, u32), size: (u32, u32), (x, y): (f64, f64)) -> (i32, i32) {
    let (w, h) = (f64::from(game.0), f64::from(game.1));
    let (vx, vy, vw, vh) = letterbox(w as f32, h as f32, size.0, size.1);
    if vw <= 0.0 || vh <= 0.0 {
        return (0, 0);
    }
    (
        ((x - f64::from(vx)) * w / f64::from(vw)).floor() as i32,
        ((y - f64::from(vy)) * h / f64::from(vh)).floor() as i32,
    )
}

/// The window position of a game pixel (its centre), in physical pixels
/// of a drawable area of `size`: where to put the system cursor so that
/// [`to_game`] gives the pixel back.
pub fn to_window(game: (u32, u32), size: (u32, u32), (x, y): (i32, i32)) -> (f64, f64) {
    let (w, h) = (f64::from(game.0), f64::from(game.1));
    let (vx, vy, vw, vh) = letterbox(w as f32, h as f32, size.0, size.1);
    if w <= 0.0 || h <= 0.0 {
        return (0.0, 0.0);
    }
    (
        f64::from(vx) + (f64::from(x) + 0.5) * f64::from(vw) / w,
        f64::from(vy) + (f64::from(y) + 0.5) * f64::from(vh) / h,
    )
}

/// Where the system cursor is put back when a full-screen game's cursor
/// left the frame for the bars of the letterbox (`CBRenderer::
/// ClipCursor` confines it to the game's rectangle): the nearest window
/// position inside the frame, `None` when the cursor is inside already.
/// The frame's right and bottom edges are outside, as in the clip
/// rectangle the original gives Windows.
pub fn clip_to_frame(game: (u32, u32), size: (u32, u32), (x, y): (f64, f64)) -> Option<(f64, f64)> {
    let (vx, vy, vw, vh) = letterbox(game.0 as f32, game.1 as f32, size.0, size.1);
    if vw <= 0.0 || vh <= 0.0 {
        return None;
    }
    let (left, top) = (f64::from(vx), f64::from(vy));
    let (right, bottom) = (left + f64::from(vw), top + f64::from(vh));
    let inside = x >= left && x < right && y >= top && y < bottom;
    if inside {
        return None;
    }
    // One physical pixel in from the right and bottom edges.
    Some((
        x.clamp(left, (right - 1.0).max(left)),
        y.clamp(top, (bottom - 1.0).max(top)),
    ))
}

/// Whether the system cursor is confined to the frame: while a
/// full-screen game is active (the window has the focus and is shown).
/// The original releases it when the window loses the focus (Windows
/// does) and when the renderer is destroyed.
pub fn clips_cursor(windowed: bool, focused: bool, occluded: bool) -> bool {
    !windowed && focused && !occluded
}

/// Whether the game runs in a window (`CHWManager`): never when the
/// game forbids it (`ALLOW_WINDOWED_MODE = FALSE`); else what the
/// player asks on the command line, else the choice kept in the
/// settings (`Video/Windowed`), else a window.
pub fn windowed_mode(allow_windowed: bool, asked: Option<bool>, saved: Option<bool>) -> bool {
    allow_windowed && asked.or(saved).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_the_command_line_then_the_settings_choose_the_window() {
        assert!(windowed_mode(true, None, None), "a window by default");
        assert!(!windowed_mode(true, None, Some(false)));
        assert!(windowed_mode(true, None, Some(true)));
        assert!(windowed_mode(true, Some(true), Some(false)));
        assert!(!windowed_mode(true, Some(false), Some(true)));
        // A game that forbids the window runs full screen whatever is
        // asked.
        assert!(!windowed_mode(false, None, None));
        assert!(!windowed_mode(false, Some(true), Some(true)));
    }

    #[test]
    fn a_cursor_in_the_bars_is_put_back_on_the_frame() {
        // A 320x200 game in a 1000x500 area: the frame is 800x500 from
        // x 100 to 900 (the right edge, 900, is outside).
        let (game, size) = ((320, 200), (1000, 500));
        assert_eq!(clip_to_frame(game, size, (100.0, 0.0)), None);
        assert_eq!(clip_to_frame(game, size, (500.0, 250.0)), None);
        assert_eq!(clip_to_frame(game, size, (899.5, 499.5)), None);
        assert_eq!(
            clip_to_frame(game, size, (50.0, 250.0)),
            Some((100.0, 250.0))
        );
        assert_eq!(
            clip_to_frame(game, size, (990.0, 20.0)),
            Some((899.0, 20.0))
        );
        assert_eq!(
            clip_to_frame(game, size, (900.0, 500.0)),
            Some((899.0, 499.0))
        );
        assert_eq!(clip_to_frame(game, size, (-5.0, -5.0)), Some((100.0, 0.0)));
        // Bars above and below: the frame is 800x500 from y 50 to 550.
        let size = (800, 600);
        assert_eq!(
            clip_to_frame(game, size, (400.0, 20.0)),
            Some((400.0, 50.0))
        );
        assert_eq!(
            clip_to_frame(game, size, (400.0, 580.0)),
            Some((400.0, 549.0))
        );
        // Whatever lands is where the game sees its own edge pixel.
        let (x, y) = clip_to_frame(game, size, (900.0, 700.0)).unwrap();
        assert_eq!(to_game(game, size, (x, y)), (319, 199));
        assert_eq!(clip_to_frame(game, (0, 0), (5.0, 5.0)), None);
    }

    #[test]
    fn only_an_active_full_screen_game_confines_the_cursor() {
        assert!(clips_cursor(false, true, false));
        assert!(!clips_cursor(true, true, false), "a window is not clipped");
        assert!(!clips_cursor(false, false, false), "released on focus loss");
        assert!(!clips_cursor(false, true, true), "and while hidden");
    }

    #[test]
    fn window_and_game_positions_map_both_ways() {
        // A 320x200 game in a 1000x500 area: scaled 2.5 times, 800x500,
        // with bars of 100 pixels left and right.
        let (game, size) = ((320, 200), (1000, 500));
        assert_eq!(to_game(game, size, (100.0, 0.0)), (0, 0));
        assert_eq!(to_game(game, size, (899.9, 499.9)), (319, 199));
        assert_eq!(to_game(game, size, (50.0, 250.0)), (-20, 100), "in the bar");
        assert_eq!(to_game(game, size, (950.0, 250.0)), (340, 100));
        for point in [(0, 0), (319, 199), (160, 100), (7, 133)] {
            assert_eq!(to_game(game, size, to_window(game, size, point)), point);
        }
        // A window smaller than the game.
        let size = (160, 120);
        for point in [(0, 0), (319, 199), (33, 77)] {
            assert_eq!(to_game(game, size, to_window(game, size, point)), point);
        }
        assert_eq!(to_game(game, (0, 0), (5.0, 5.0)), (0, 0));
    }
}
