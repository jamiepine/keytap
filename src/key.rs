/// A physical key identity, layout-independent.
///
/// Left and right modifier variants are always distinguished; there is no
/// generic `Shift` / `Control` / `Alt` / `Meta` variant. `Meta` maps to ⌘ on
/// macOS, the Windows key on Windows, and Super on Linux.
///
/// Letter, digit, and punctuation variants are keyed to their **physical
/// US-QWERTY location**, not the glyph the user sees on a non-US layout.
/// Use [`Key::Unknown`] for scancodes that keytap doesn't have a named
/// variant for yet.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Key {
    // Letters (positional, QWERTY)
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,

    // Digits
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,

    // Function row
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,

    // Modifiers — left/right ALWAYS distinguished
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    MetaLeft,
    MetaRight,

    // Arrows
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,

    // Navigation
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,

    // Editing
    Escape,
    Tab,
    CapsLock,
    Space,
    Enter,
    Backspace,

    // Punctuation (positional)
    Backtick,
    Minus,
    Equal,
    BracketLeft,
    BracketRight,
    Backslash,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,

    // Numpad
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    NumpadEnter,
    NumpadDecimal,
    NumLock,

    // Misc
    PrintScreen,
    ScrollLock,
    Pause,
    Menu,

    /// ISO-layout key between Left Shift and Z. Absent on ANSI (US)
    /// keyboards but present on most European / Japanese layouts.
    IntlBackslash,

    /// macOS Fn key. The key is firmware-level on Windows and doesn't
    /// surface to the OS; on Linux it appears as `KEY_FN` (0x1D0 = 464).
    Function,

    /// Escape hatch for any scancode keytap doesn't recognize. Always
    /// emitted, never silently dropped.
    Unknown(RawCode),
}

/// A raw, platform-specific scancode. On macOS this is a `kVK_*` virtual
/// keycode; on Windows a virtual-key code; on Linux an evdev `KEY_*` code.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RawCode(pub u32);
