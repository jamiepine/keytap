//! macOS Core Graphics keycode ↔ [`Key`] mapping.
//!
//! Source: Apple's `HIToolbox/Events.h` (`kVK_*` constants) and
//! `IOHIDFamily/IOHIDSystem/IOKit/hidsystem/ev_keymap.h` for NX special keys.
//!
//! Ported from `Narsil/rdev` (MIT) with left/right modifier coverage
//! already present in the source.

use crate::{Key, RawCode};

// Letters
const KEY_A: u32 = 0;
const KEY_S: u32 = 1;
const KEY_D: u32 = 2;
const KEY_F: u32 = 3;
const KEY_H: u32 = 4;
const KEY_G: u32 = 5;
const KEY_Z: u32 = 6;
const KEY_X: u32 = 7;
const KEY_C: u32 = 8;
const KEY_V: u32 = 9;
const KEY_B: u32 = 11;
const KEY_Q: u32 = 12;
const KEY_W: u32 = 13;
const KEY_E: u32 = 14;
const KEY_R: u32 = 15;
const KEY_Y: u32 = 16;
const KEY_T: u32 = 17;
const KEY_U: u32 = 32;
const KEY_I: u32 = 34;
const KEY_O: u32 = 31;
const KEY_P: u32 = 35;
const KEY_J: u32 = 38;
const KEY_K: u32 = 40;
const KEY_L: u32 = 37;
const KEY_N: u32 = 45;
const KEY_M: u32 = 46;

// Digits (top row)
const DIGIT_0: u32 = 29;
const DIGIT_1: u32 = 18;
const DIGIT_2: u32 = 19;
const DIGIT_3: u32 = 20;
const DIGIT_4: u32 = 21;
const DIGIT_5: u32 = 23;
const DIGIT_6: u32 = 22;
const DIGIT_7: u32 = 26;
const DIGIT_8: u32 = 28;
const DIGIT_9: u32 = 25;

// Punctuation
const MINUS: u32 = 27;
const EQUAL: u32 = 24;
const BRACKET_LEFT: u32 = 33;
const BRACKET_RIGHT: u32 = 30;
const BACKSLASH: u32 = 42;
const SEMICOLON: u32 = 41;
const QUOTE: u32 = 39;
const COMMA: u32 = 43;
const PERIOD: u32 = 47;
const SLASH: u32 = 44;
const BACKTICK: u32 = 50;

// Modifiers (left/right distinguished)
const SHIFT_LEFT: u32 = 56;
const SHIFT_RIGHT: u32 = 60;
const CONTROL_LEFT: u32 = 59;
const CONTROL_RIGHT: u32 = 62;
const ALT_LEFT: u32 = 58;
const ALT_RIGHT: u32 = 61;
const META_LEFT: u32 = 55;
const META_RIGHT: u32 = 54;
const CAPS_LOCK: u32 = 57;

// Editing / navigation
const RETURN: u32 = 36;
const TAB: u32 = 48;
const SPACE: u32 = 49;
const BACKSPACE: u32 = 51;
const ESCAPE: u32 = 53;

// Arrows
const ARROW_LEFT: u32 = 123;
const ARROW_RIGHT: u32 = 124;
const ARROW_DOWN: u32 = 125;
const ARROW_UP: u32 = 126;

// Navigation
const HOME: u32 = 115;
const PAGE_UP: u32 = 116;
const DELETE: u32 = 117;
const END: u32 = 119;
const PAGE_DOWN: u32 = 121;

// Function row
const F1: u32 = 122;
const F2: u32 = 120;
const F3: u32 = 99;
const F4: u32 = 118;
const F5: u32 = 96;
const F6: u32 = 97;
const F7: u32 = 98;
const F8: u32 = 100;
const F9: u32 = 101;
const F10: u32 = 109;
const F11: u32 = 103;
const F12: u32 = 111;
const F13: u32 = 105;
const F14: u32 = 107;
const F15: u32 = 113;
const F16: u32 = 106;
const F17: u32 = 64;
const F18: u32 = 79;
const F19: u32 = 80;
const F20: u32 = 90;
// F21–F24 have no kVK_* constants on Apple keyboards.

// ISO / layout extras
const ISO_SECTION: u32 = 10; // kVK_ISO_Section — physical key between LShift and Z on ISO
const FUNCTION: u32 = 63; // kVK_Function — macOS Fn

// Numpad
const NUMPAD_DECIMAL: u32 = 65;
const NUMPAD_MULTIPLY: u32 = 67;
const NUMPAD_ADD: u32 = 69;
const NUM_LOCK: u32 = 71; // kVK_ANSI_KeypadClear — maps to NumLock
const NUMPAD_DIVIDE: u32 = 75;
const NUMPAD_ENTER: u32 = 76;
const NUMPAD_SUBTRACT: u32 = 78;
const NUMPAD_0: u32 = 82;
const NUMPAD_1: u32 = 83;
const NUMPAD_2: u32 = 84;
const NUMPAD_3: u32 = 85;
const NUMPAD_4: u32 = 86;
const NUMPAD_5: u32 = 87;
const NUMPAD_6: u32 = 88;
const NUMPAD_7: u32 = 89;
const NUMPAD_8: u32 = 91;
const NUMPAD_9: u32 = 92;

/// Map a CGEventTap keycode to a [`Key`]. Unknown codes round-trip via
/// [`Key::Unknown`].
pub(crate) fn key_from_code(code: u32) -> Key {
    match code {
        KEY_A => Key::A,
        KEY_B => Key::B,
        KEY_C => Key::C,
        KEY_D => Key::D,
        KEY_E => Key::E,
        KEY_F => Key::F,
        KEY_G => Key::G,
        KEY_H => Key::H,
        KEY_I => Key::I,
        KEY_J => Key::J,
        KEY_K => Key::K,
        KEY_L => Key::L,
        KEY_M => Key::M,
        KEY_N => Key::N,
        KEY_O => Key::O,
        KEY_P => Key::P,
        KEY_Q => Key::Q,
        KEY_R => Key::R,
        KEY_S => Key::S,
        KEY_T => Key::T,
        KEY_U => Key::U,
        KEY_V => Key::V,
        KEY_W => Key::W,
        KEY_X => Key::X,
        KEY_Y => Key::Y,
        KEY_Z => Key::Z,

        DIGIT_0 => Key::Digit0,
        DIGIT_1 => Key::Digit1,
        DIGIT_2 => Key::Digit2,
        DIGIT_3 => Key::Digit3,
        DIGIT_4 => Key::Digit4,
        DIGIT_5 => Key::Digit5,
        DIGIT_6 => Key::Digit6,
        DIGIT_7 => Key::Digit7,
        DIGIT_8 => Key::Digit8,
        DIGIT_9 => Key::Digit9,

        MINUS => Key::Minus,
        EQUAL => Key::Equal,
        BRACKET_LEFT => Key::BracketLeft,
        BRACKET_RIGHT => Key::BracketRight,
        BACKSLASH => Key::Backslash,
        SEMICOLON => Key::Semicolon,
        QUOTE => Key::Quote,
        COMMA => Key::Comma,
        PERIOD => Key::Period,
        SLASH => Key::Slash,
        BACKTICK => Key::Backtick,

        SHIFT_LEFT => Key::ShiftLeft,
        SHIFT_RIGHT => Key::ShiftRight,
        CONTROL_LEFT => Key::ControlLeft,
        CONTROL_RIGHT => Key::ControlRight,
        ALT_LEFT => Key::AltLeft,
        ALT_RIGHT => Key::AltRight,
        META_LEFT => Key::MetaLeft,
        META_RIGHT => Key::MetaRight,
        CAPS_LOCK => Key::CapsLock,

        RETURN => Key::Enter,
        TAB => Key::Tab,
        SPACE => Key::Space,
        BACKSPACE => Key::Backspace,
        ESCAPE => Key::Escape,

        ARROW_LEFT => Key::ArrowLeft,
        ARROW_RIGHT => Key::ArrowRight,
        ARROW_DOWN => Key::ArrowDown,
        ARROW_UP => Key::ArrowUp,

        HOME => Key::Home,
        END => Key::End,
        PAGE_UP => Key::PageUp,
        PAGE_DOWN => Key::PageDown,
        DELETE => Key::Delete,

        F1 => Key::F1,
        F2 => Key::F2,
        F3 => Key::F3,
        F4 => Key::F4,
        F5 => Key::F5,
        F6 => Key::F6,
        F7 => Key::F7,
        F8 => Key::F8,
        F9 => Key::F9,
        F10 => Key::F10,
        F11 => Key::F11,
        F12 => Key::F12,
        F13 => Key::F13,
        F14 => Key::F14,
        F15 => Key::F15,
        F16 => Key::F16,
        F17 => Key::F17,
        F18 => Key::F18,
        F19 => Key::F19,
        F20 => Key::F20,

        NUMPAD_0 => Key::Numpad0,
        NUMPAD_1 => Key::Numpad1,
        NUMPAD_2 => Key::Numpad2,
        NUMPAD_3 => Key::Numpad3,
        NUMPAD_4 => Key::Numpad4,
        NUMPAD_5 => Key::Numpad5,
        NUMPAD_6 => Key::Numpad6,
        NUMPAD_7 => Key::Numpad7,
        NUMPAD_8 => Key::Numpad8,
        NUMPAD_9 => Key::Numpad9,
        NUMPAD_ADD => Key::NumpadAdd,
        NUMPAD_SUBTRACT => Key::NumpadSubtract,
        NUMPAD_MULTIPLY => Key::NumpadMultiply,
        NUMPAD_DIVIDE => Key::NumpadDivide,
        NUMPAD_ENTER => Key::NumpadEnter,
        NUMPAD_DECIMAL => Key::NumpadDecimal,
        NUM_LOCK => Key::NumLock,

        ISO_SECTION => Key::IntlBackslash,
        FUNCTION => Key::Function,

        other => Key::Unknown(RawCode(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_known_codes() {
        // Every mapped code must produce a non-Unknown Key.
        let codes = [
            KEY_A,
            KEY_Z,
            SHIFT_LEFT,
            SHIFT_RIGHT,
            META_LEFT,
            META_RIGHT,
            ALT_LEFT,
            ALT_RIGHT,
            CONTROL_LEFT,
            CONTROL_RIGHT,
            ESCAPE,
            SPACE,
            RETURN,
            F1,
            F20,
            ARROW_UP,
            NUMPAD_5,
        ];
        for c in codes {
            assert!(
                !matches!(key_from_code(c), Key::Unknown(_)),
                "code {c} should map to a named Key"
            );
        }
    }

    #[test]
    fn left_right_modifiers_are_distinct() {
        assert_ne!(key_from_code(SHIFT_LEFT), key_from_code(SHIFT_RIGHT));
        assert_ne!(key_from_code(META_LEFT), key_from_code(META_RIGHT));
        assert_ne!(key_from_code(ALT_LEFT), key_from_code(ALT_RIGHT));
        assert_ne!(key_from_code(CONTROL_LEFT), key_from_code(CONTROL_RIGHT));
    }

    #[test]
    fn unknown_code_round_trips() {
        // Pick a scancode that isn't in the table.
        assert_eq!(key_from_code(9999), Key::Unknown(RawCode(9999)));
    }
}
