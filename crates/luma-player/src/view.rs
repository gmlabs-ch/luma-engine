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

#[cfg(test)]
mod tests {
    use super::*;

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
