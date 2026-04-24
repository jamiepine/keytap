//! Windows virtual-key code ↔ [`Key`] mapping.
//!
//! Source: <https://docs.microsoft.com/windows/win32/inputdev/virtual-key-codes>.
//! Ported from `Narsil/rdev` (MIT). Left/right modifiers are already
//! distinguished at the VK level (`VK_LCONTROL` etc.), which the LL hook
//! delivers directly — no scanCode/extended-flag disambiguation needed.

use crate::{Key, RawCode};

macro_rules! decl {
    ($($const_name:ident = $code:expr => $variant:ident);* $(;)?) => {
        $(const $const_name: u32 = $code;)*

        pub(crate) fn key_from_vk(vk: u32) -> Key {
            match vk {
                $($const_name => Key::$variant,)*
                other => Key::Unknown(RawCode(other)),
            }
        }
    };
}

decl! {
    // Modifiers (LL hook delivers L/R directly)
    VK_LSHIFT    = 0xA0 => ShiftLeft;
    VK_RSHIFT    = 0xA1 => ShiftRight;
    VK_LCONTROL  = 0xA2 => ControlLeft;
    VK_RCONTROL  = 0xA3 => ControlRight;
    VK_LMENU     = 0xA4 => AltLeft;
    VK_RMENU     = 0xA5 => AltRight;
    VK_LWIN      = 0x5B => MetaLeft;
    VK_RWIN      = 0x5C => MetaRight;
    VK_CAPITAL   = 0x14 => CapsLock;

    // Editing
    VK_BACK      = 0x08 => Backspace;
    VK_TAB       = 0x09 => Tab;
    VK_RETURN    = 0x0D => Enter;
    VK_ESCAPE    = 0x1B => Escape;
    VK_SPACE     = 0x20 => Space;

    // Arrows / navigation
    VK_LEFT      = 0x25 => ArrowLeft;
    VK_UP        = 0x26 => ArrowUp;
    VK_RIGHT     = 0x27 => ArrowRight;
    VK_DOWN      = 0x28 => ArrowDown;
    VK_HOME      = 0x24 => Home;
    VK_END       = 0x23 => End;
    VK_PRIOR     = 0x21 => PageUp;
    VK_NEXT      = 0x22 => PageDown;
    VK_INSERT    = 0x2D => Insert;
    VK_DELETE    = 0x2E => Delete;

    // Misc
    VK_SNAPSHOT  = 0x2C => PrintScreen;
    VK_SCROLL    = 0x91 => ScrollLock;
    VK_PAUSE     = 0x13 => Pause;
    VK_NUMLOCK   = 0x90 => NumLock;
    VK_APPS      = 0x5D => Menu;

    // Digits (top row)
    VK_0 = 0x30 => Digit0;
    VK_1 = 0x31 => Digit1;
    VK_2 = 0x32 => Digit2;
    VK_3 = 0x33 => Digit3;
    VK_4 = 0x34 => Digit4;
    VK_5 = 0x35 => Digit5;
    VK_6 = 0x36 => Digit6;
    VK_7 = 0x37 => Digit7;
    VK_8 = 0x38 => Digit8;
    VK_9 = 0x39 => Digit9;

    // Letters
    VK_A = 0x41 => A;
    VK_B = 0x42 => B;
    VK_C = 0x43 => C;
    VK_D = 0x44 => D;
    VK_E = 0x45 => E;
    VK_F = 0x46 => F;
    VK_G = 0x47 => G;
    VK_H = 0x48 => H;
    VK_I = 0x49 => I;
    VK_J = 0x4A => J;
    VK_K = 0x4B => K;
    VK_L = 0x4C => L;
    VK_M = 0x4D => M;
    VK_N = 0x4E => N;
    VK_O = 0x4F => O;
    VK_P = 0x50 => P;
    VK_Q = 0x51 => Q;
    VK_R = 0x52 => R;
    VK_S = 0x53 => S;
    VK_T = 0x54 => T;
    VK_U = 0x55 => U;
    VK_V = 0x56 => V;
    VK_W = 0x57 => W;
    VK_X = 0x58 => X;
    VK_Y = 0x59 => Y;
    VK_Z = 0x5A => Z;

    // Function row
    VK_F1  = 0x70 => F1;
    VK_F2  = 0x71 => F2;
    VK_F3  = 0x72 => F3;
    VK_F4  = 0x73 => F4;
    VK_F5  = 0x74 => F5;
    VK_F6  = 0x75 => F6;
    VK_F7  = 0x76 => F7;
    VK_F8  = 0x77 => F8;
    VK_F9  = 0x78 => F9;
    VK_F10 = 0x79 => F10;
    VK_F11 = 0x7A => F11;
    VK_F12 = 0x7B => F12;
    VK_F13 = 0x7C => F13;
    VK_F14 = 0x7D => F14;
    VK_F15 = 0x7E => F15;
    VK_F16 = 0x7F => F16;
    VK_F17 = 0x80 => F17;
    VK_F18 = 0x81 => F18;
    VK_F19 = 0x82 => F19;
    VK_F20 = 0x83 => F20;
    VK_F21 = 0x84 => F21;
    VK_F22 = 0x85 => F22;
    VK_F23 = 0x86 => F23;
    VK_F24 = 0x87 => F24;

    // Numpad
    VK_NUMPAD0   = 0x60 => Numpad0;
    VK_NUMPAD1   = 0x61 => Numpad1;
    VK_NUMPAD2   = 0x62 => Numpad2;
    VK_NUMPAD3   = 0x63 => Numpad3;
    VK_NUMPAD4   = 0x64 => Numpad4;
    VK_NUMPAD5   = 0x65 => Numpad5;
    VK_NUMPAD6   = 0x66 => Numpad6;
    VK_NUMPAD7   = 0x67 => Numpad7;
    VK_NUMPAD8   = 0x68 => Numpad8;
    VK_NUMPAD9   = 0x69 => Numpad9;
    VK_MULTIPLY  = 0x6A => NumpadMultiply;
    VK_ADD       = 0x6B => NumpadAdd;
    VK_SUBTRACT  = 0x6D => NumpadSubtract;
    VK_DECIMAL   = 0x6E => NumpadDecimal;
    VK_DIVIDE    = 0x6F => NumpadDivide;

    // Punctuation (OEM codes — US layout)
    VK_OEM_1      = 0xBA => Semicolon;
    VK_OEM_PLUS   = 0xBB => Equal;
    VK_OEM_COMMA  = 0xBC => Comma;
    VK_OEM_MINUS  = 0xBD => Minus;
    VK_OEM_PERIOD = 0xBE => Period;
    VK_OEM_2      = 0xBF => Slash;
    VK_OEM_3      = 0xC0 => Backtick;
    VK_OEM_4      = 0xDB => BracketLeft;
    VK_OEM_5      = 0xDC => Backslash;
    VK_OEM_6      = 0xDD => BracketRight;
    VK_OEM_7      = 0xDE => Quote;
    // ISO layout "\\|" between LShift and Z. Not present on ANSI keyboards.
    // Windows has no user-visible VK for the macOS Fn key.
    VK_OEM_102    = 0xE2 => IntlBackslash;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_known_vks() {
        let codes = [
            VK_A,
            VK_Z,
            VK_LSHIFT,
            VK_RSHIFT,
            VK_LWIN,
            VK_RWIN,
            VK_LMENU,
            VK_RMENU,
            VK_LCONTROL,
            VK_RCONTROL,
            VK_ESCAPE,
            VK_SPACE,
            VK_RETURN,
            VK_F1,
            VK_F24,
            VK_UP,
            VK_NUMPAD5,
        ];
        for c in codes {
            assert!(
                !matches!(key_from_vk(c), Key::Unknown(_)),
                "VK 0x{c:X} should map to a named Key"
            );
        }
    }

    #[test]
    fn left_right_modifiers_are_distinct() {
        assert_ne!(key_from_vk(VK_LSHIFT), key_from_vk(VK_RSHIFT));
        assert_ne!(key_from_vk(VK_LCONTROL), key_from_vk(VK_RCONTROL));
        assert_ne!(key_from_vk(VK_LMENU), key_from_vk(VK_RMENU));
        assert_ne!(key_from_vk(VK_LWIN), key_from_vk(VK_RWIN));
    }

    #[test]
    fn unknown_vk_round_trips() {
        assert_eq!(key_from_vk(0xFFFE), Key::Unknown(RawCode(0xFFFE)));
    }
}
