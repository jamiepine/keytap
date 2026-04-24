//! Linux evdev `KEY_*` ↔ [`Key`] mapping.
//!
//! Source: `<linux/input-event-codes.h>`. evdev exposes these as
//! `evdev::Key::KEY_*` constants. Left/right modifiers are distinct at the
//! kernel level (`KEY_LEFTSHIFT` vs `KEY_RIGHTSHIFT`, etc.), so no
//! additional disambiguation is needed.

use crate::{Key, RawCode};

macro_rules! decl {
    ($($code:expr => $variant:ident),* $(,)?) => {
        pub(crate) fn key_from_code(code: u16) -> Key {
            match code {
                $($code => Key::$variant,)*
                other => Key::Unknown(RawCode(other as u32)),
            }
        }
    };
}

decl! {
    1  => Escape,

    // Numbers row
    2  => Digit1, 3  => Digit2, 4  => Digit3, 5  => Digit4, 6  => Digit5,
    7  => Digit6, 8  => Digit7, 9  => Digit8, 10 => Digit9, 11 => Digit0,

    12 => Minus, 13 => Equal,
    14 => Backspace, 15 => Tab,

    // QWERTY row
    16 => Q, 17 => W, 18 => E, 19 => R, 20 => T,
    21 => Y, 22 => U, 23 => I, 24 => O, 25 => P,
    26 => BracketLeft, 27 => BracketRight, 28 => Enter,

    29 => ControlLeft,

    // Home row
    30 => A, 31 => S, 32 => D, 33 => F, 34 => G,
    35 => H, 36 => J, 37 => K, 38 => L,
    39 => Semicolon, 40 => Quote,
    41 => Backtick,
    42 => ShiftLeft,
    43 => Backslash,

    // Bottom letter row
    44 => Z, 45 => X, 46 => C, 47 => V, 48 => B,
    49 => N, 50 => M,
    51 => Comma, 52 => Period, 53 => Slash,
    54 => ShiftRight,
    55 => NumpadMultiply,
    56 => AltLeft,
    57 => Space,
    58 => CapsLock,

    // Function row
    59 => F1, 60 => F2, 61 => F3, 62 => F4, 63 => F5,
    64 => F6, 65 => F7, 66 => F8, 67 => F9, 68 => F10,

    69 => NumLock,
    70 => ScrollLock,

    // Numpad
    71 => Numpad7, 72 => Numpad8, 73 => Numpad9, 74 => NumpadSubtract,
    75 => Numpad4, 76 => Numpad5, 77 => Numpad6, 78 => NumpadAdd,
    79 => Numpad1, 80 => Numpad2, 81 => Numpad3,
    82 => Numpad0, 83 => NumpadDecimal,

    87 => F11, 88 => F12,

    96  => NumpadEnter,
    97  => ControlRight,
    98  => NumpadDivide,
    99  => PrintScreen,
    100 => AltRight,

    102 => Home,
    103 => ArrowUp,
    104 => PageUp,
    105 => ArrowLeft,
    106 => ArrowRight,
    107 => End,
    108 => ArrowDown,
    109 => PageDown,
    110 => Insert,
    111 => Delete,

    119 => Pause,

    125 => MetaLeft,
    126 => MetaRight,
    127 => Menu,

    // Extended F keys
    183 => F13, 184 => F14, 185 => F15, 186 => F16,
    187 => F17, 188 => F18, 189 => F19, 190 => F20,
    191 => F21, 192 => F22, 193 => F23, 194 => F24,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_known_codes() {
        // Sampling; exhaustive testing is implicit in the macro.
        let codes: [u16; 20] = [
            30, 48, 1, 42, 54, 29, 97, 56, 100, 125, 126, 57, 28, 103, 108, 76, 59, 88, 183, 194,
        ];
        for c in codes {
            assert!(
                !matches!(key_from_code(c), Key::Unknown(_)),
                "evdev code {c} should map to a named Key"
            );
        }
    }

    #[test]
    fn left_right_modifiers_are_distinct() {
        assert_ne!(key_from_code(42), key_from_code(54)); // shift
        assert_ne!(key_from_code(29), key_from_code(97)); // control
        assert_ne!(key_from_code(56), key_from_code(100)); // alt
        assert_ne!(key_from_code(125), key_from_code(126)); // meta
    }

    #[test]
    fn unknown_code_round_trips() {
        assert_eq!(key_from_code(9999), Key::Unknown(RawCode(9999)));
    }
}
