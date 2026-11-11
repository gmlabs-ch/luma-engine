//! Key translation: what the window system reports of a key, turned
//! into what the engine's home platform sends for it (see "The keyboard"
//! in `docs/game-loop.md`).
//!
//! Windows names a key by a virtual key code that does not depend on
//! what the key types: the period key is 190 (`VK_OEM_PERIOD`) and the
//! character `.` (46) comes apart, so that it is never taken for the
//! Delete key (also 46). Letters follow the keyboard layout; the other
//! keys are named here by their place on a US keyboard, which is what
//! the common layouts give them. The table is written from the public
//! list of virtual key codes, not from the engine's sources, which only
//! compare the codes.

use luma::render::winit::keyboard::{Key, KeyCode, KeyLocation, NamedKey, PhysicalKey};

/// `VK_F10`, the one key that reaches the game while Alt is held.
const VK_F10: i32 = 121;

/// The Windows virtual key code of a key (`Keyboard.KeyCode` of a key
/// that types nothing, `Keyboard.IsKeyDown`), or `None` for a key
/// Windows has no code for.
pub fn virtual_key(logical: &Key, physical: PhysicalKey) -> Option<i32> {
    let place = match physical {
        PhysicalKey::Code(code) => Some(code),
        PhysicalKey::Unidentified(_) => None,
    };
    match logical {
        Key::Named(named) => named_key(*named),
        Key::Character(text) => {
            // With Num Lock on, the keypad types digits but keeps codes
            // of its own.
            if let Some(code) = place.and_then(keypad_key) {
                return Some(code);
            }
            let c = text.chars().next()?;
            if c.is_ascii_alphabetic() {
                return Some(i32::from(c.to_ascii_uppercase() as u8));
            }
            place.and_then(placed_key).or(match c {
                '0'..='9' => Some(i32::from(c as u8)),
                ' ' => Some(32),
                _ => None,
            })
        }
        _ => place.and_then(|code| keypad_key(code).or_else(|| placed_key(code))),
    }
}

/// The code of the left or right variant of Shift, Control or Alt
/// (`VK_LSHIFT` 160 to `VK_RMENU` 165), which `Keyboard.IsKeyDown` also
/// answers for.
pub fn sided_key(code: i32, location: KeyLocation) -> Option<i32> {
    let base = match code {
        16 => 160,
        17 => 162,
        18 => 164,
        _ => return None,
    };
    match location {
        KeyLocation::Left => Some(base),
        KeyLocation::Right => Some(base + 1),
        _ => None,
    }
}

/// Whether a key going down reaches the game. With Alt held and Control
/// up, Windows sends `WM_SYSKEYDOWN`, of which the engine only takes
/// F10: the other keys, Alt itself included, raise no `Keypress` and
/// type nothing. `alt_types` lifts the rule where Alt is the key that
/// types the third character of a key (Option on macOS), as AltGr,
/// which Windows reports as Control and Alt together, does.
pub fn reaches_game(code: Option<i32>, alt: bool, control: bool, alt_types: bool) -> bool {
    let system_key = code == Some(18) || (alt && !control);
    !system_key || alt_types || code == Some(VK_F10)
}

fn named_key(named: NamedKey) -> Option<i32> {
    Some(match named {
        NamedKey::Backspace => 8,
        NamedKey::Tab => 9,
        NamedKey::Clear => 12,
        NamedKey::Enter => 13,
        NamedKey::Shift => 16,
        NamedKey::Control => 17,
        NamedKey::Alt | NamedKey::AltGraph => 18,
        NamedKey::Pause => 19,
        NamedKey::CapsLock => 20,
        NamedKey::Escape => 27,
        NamedKey::Space => 32,
        NamedKey::PageUp => 33,
        NamedKey::PageDown => 34,
        NamedKey::End => 35,
        NamedKey::Home => 36,
        NamedKey::ArrowLeft => 37,
        NamedKey::ArrowUp => 38,
        NamedKey::ArrowRight => 39,
        NamedKey::ArrowDown => 40,
        NamedKey::PrintScreen => 44,
        NamedKey::Insert => 45,
        NamedKey::Delete => 46,
        NamedKey::Super | NamedKey::Meta => 91,
        NamedKey::ContextMenu => 93,
        NamedKey::F1 => 112,
        NamedKey::F2 => 113,
        NamedKey::F3 => 114,
        NamedKey::F4 => 115,
        NamedKey::F5 => 116,
        NamedKey::F6 => 117,
        NamedKey::F7 => 118,
        NamedKey::F8 => 119,
        NamedKey::F9 => 120,
        NamedKey::F10 => 121,
        NamedKey::F11 => 122,
        NamedKey::F12 => 123,
        NamedKey::F13 => 124,
        NamedKey::F14 => 125,
        NamedKey::F15 => 126,
        NamedKey::F16 => 127,
        NamedKey::F17 => 128,
        NamedKey::F18 => 129,
        NamedKey::F19 => 130,
        NamedKey::F20 => 131,
        NamedKey::F21 => 132,
        NamedKey::F22 => 133,
        NamedKey::F23 => 134,
        NamedKey::F24 => 135,
        NamedKey::NumLock => 144,
        NamedKey::ScrollLock => 145,
        _ => return None,
    })
}

/// The numeric keypad's own codes (`VK_NUMPAD0` 96 to `VK_DIVIDE` 111).
fn keypad_key(code: KeyCode) -> Option<i32> {
    Some(match code {
        KeyCode::Numpad0 => 96,
        KeyCode::Numpad1 => 97,
        KeyCode::Numpad2 => 98,
        KeyCode::Numpad3 => 99,
        KeyCode::Numpad4 => 100,
        KeyCode::Numpad5 => 101,
        KeyCode::Numpad6 => 102,
        KeyCode::Numpad7 => 103,
        KeyCode::Numpad8 => 104,
        KeyCode::Numpad9 => 105,
        KeyCode::NumpadMultiply => 106,
        KeyCode::NumpadAdd => 107,
        KeyCode::NumpadComma => 108,
        KeyCode::NumpadSubtract => 109,
        KeyCode::NumpadDecimal => 110,
        KeyCode::NumpadDivide => 111,
        _ => return None,
    })
}

/// The code of a key by its place on a US keyboard.
fn placed_key(code: KeyCode) -> Option<i32> {
    Some(match code {
        KeyCode::Space => 32,
        KeyCode::Digit0 => 48,
        KeyCode::Digit1 => 49,
        KeyCode::Digit2 => 50,
        KeyCode::Digit3 => 51,
        KeyCode::Digit4 => 52,
        KeyCode::Digit5 => 53,
        KeyCode::Digit6 => 54,
        KeyCode::Digit7 => 55,
        KeyCode::Digit8 => 56,
        KeyCode::Digit9 => 57,
        KeyCode::KeyA => 65,
        KeyCode::KeyB => 66,
        KeyCode::KeyC => 67,
        KeyCode::KeyD => 68,
        KeyCode::KeyE => 69,
        KeyCode::KeyF => 70,
        KeyCode::KeyG => 71,
        KeyCode::KeyH => 72,
        KeyCode::KeyI => 73,
        KeyCode::KeyJ => 74,
        KeyCode::KeyK => 75,
        KeyCode::KeyL => 76,
        KeyCode::KeyM => 77,
        KeyCode::KeyN => 78,
        KeyCode::KeyO => 79,
        KeyCode::KeyP => 80,
        KeyCode::KeyQ => 81,
        KeyCode::KeyR => 82,
        KeyCode::KeyS => 83,
        KeyCode::KeyT => 84,
        KeyCode::KeyU => 85,
        KeyCode::KeyV => 86,
        KeyCode::KeyW => 87,
        KeyCode::KeyX => 88,
        KeyCode::KeyY => 89,
        KeyCode::KeyZ => 90,
        KeyCode::Semicolon => 186,
        KeyCode::Equal => 187,
        KeyCode::Comma => 188,
        KeyCode::Minus => 189,
        KeyCode::Period => 190,
        KeyCode::Slash => 191,
        KeyCode::Backquote => 192,
        KeyCode::BracketLeft => 219,
        KeyCode::Backslash => 220,
        KeyCode::BracketRight => 221,
        KeyCode::Quote => 222,
        KeyCode::IntlBackslash => 226,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn character(text: &str, place: KeyCode) -> Option<i32> {
        virtual_key(&Key::Character(text.into()), PhysicalKey::Code(place))
    }

    #[test]
    fn punctuation_keys_are_not_taken_for_editing_keys() {
        // The characters whose ASCII codes are the codes of Delete, the
        // arrows, Home, End and the page keys.
        assert_eq!(character(".", KeyCode::Period), Some(190));
        assert_eq!(character("'", KeyCode::Quote), Some(222));
        assert_eq!(character("(", KeyCode::Digit9), Some(57));
        assert_eq!(character("%", KeyCode::Digit5), Some(53));
        assert_eq!(character("&", KeyCode::Digit7), Some(55));
        assert_eq!(character("#", KeyCode::Digit3), Some(51));
        assert_eq!(character("$", KeyCode::Digit4), Some(52));
        assert_eq!(character("!", KeyCode::Digit1), Some(49));
        assert_eq!(character("\"", KeyCode::Quote), Some(222));
        assert_eq!(
            virtual_key(
                &Key::Named(NamedKey::Delete),
                PhysicalKey::Code(KeyCode::Delete)
            ),
            Some(46)
        );
    }

    #[test]
    fn letters_follow_the_layout_and_other_keys_their_place() {
        // The A of a French keyboard sits where a US keyboard has Q.
        assert_eq!(character("a", KeyCode::KeyQ), Some(65));
        assert_eq!(character("A", KeyCode::KeyQ), Some(65));
        // Its top row types `&` unshifted and is still the 1 key.
        assert_eq!(character("&", KeyCode::Digit1), Some(49));
        // A letter of another alphabet, an accented letter and a dead
        // key have the code of their place; the character comes apart.
        assert_eq!(character("ф", KeyCode::KeyA), Some(65));
        assert_eq!(character("é", KeyCode::Digit2), Some(50));
        assert_eq!(
            virtual_key(
                &Key::Dead(Some('^')),
                PhysicalKey::Code(KeyCode::BracketLeft)
            ),
            Some(219)
        );
        assert_eq!(
            virtual_key(
                &Key::Named(NamedKey::Space),
                PhysicalKey::Code(KeyCode::Space)
            ),
            Some(32)
        );
    }

    #[test]
    fn the_keypad_and_the_late_function_keys_have_codes() {
        // Num Lock on: digits, with the keypad's own codes.
        assert_eq!(character("1", KeyCode::Numpad1), Some(97));
        assert_eq!(character("0", KeyCode::Numpad0), Some(96));
        assert_eq!(character("*", KeyCode::NumpadMultiply), Some(106));
        assert_eq!(character("+", KeyCode::NumpadAdd), Some(107));
        assert_eq!(character("-", KeyCode::NumpadSubtract), Some(109));
        assert_eq!(character(".", KeyCode::NumpadDecimal), Some(110));
        assert_eq!(character("/", KeyCode::NumpadDivide), Some(111));
        // Num Lock off: the arrows they stand for.
        assert_eq!(
            virtual_key(
                &Key::Named(NamedKey::ArrowLeft),
                PhysicalKey::Code(KeyCode::Numpad4)
            ),
            Some(37)
        );
        assert_eq!(
            virtual_key(&Key::Named(NamedKey::F13), PhysicalKey::Code(KeyCode::F13)),
            Some(124)
        );
        assert_eq!(
            virtual_key(&Key::Named(NamedKey::F24), PhysicalKey::Code(KeyCode::F24)),
            Some(135)
        );
        for (named, code) in [
            (NamedKey::PrintScreen, 44),
            (NamedKey::ContextMenu, 93),
            (NamedKey::NumLock, 144),
            (NamedKey::ScrollLock, 145),
        ] {
            assert_eq!(
                virtual_key(&Key::Named(named), PhysicalKey::Code(KeyCode::Fn)),
                Some(code)
            );
        }
    }

    #[test]
    fn shift_control_and_alt_have_sided_codes() {
        assert_eq!(sided_key(16, KeyLocation::Left), Some(160));
        assert_eq!(sided_key(16, KeyLocation::Right), Some(161));
        assert_eq!(sided_key(17, KeyLocation::Right), Some(163));
        assert_eq!(sided_key(18, KeyLocation::Left), Some(164));
        assert_eq!(sided_key(18, KeyLocation::Standard), None);
        assert_eq!(sided_key(65, KeyLocation::Left), None);
    }

    #[test]
    fn with_alt_held_only_f10_reaches_the_game() {
        assert!(reaches_game(Some(65), false, false, false));
        assert!(!reaches_game(Some(65), true, false, false));
        assert!(reaches_game(Some(121), true, false, false));
        // Alt itself is a system key; AltGr (Control and Alt) types.
        assert!(!reaches_game(Some(18), false, false, false));
        assert!(reaches_game(Some(65), true, true, false));
        // Where Alt types characters, every key passes.
        assert!(reaches_game(Some(65), true, false, true));
        assert!(reaches_game(Some(18), true, false, true));
    }
}
