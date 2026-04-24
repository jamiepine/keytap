# keytap

Cross-platform, observe-only global keyboard taps for Rust. macOS, Windows,
Linux (X11 and Wayland).

```rust
use keytap::{Tap, EventKind, Key};

let tap = Tap::new()?;

for event in tap.iter() {
    match event.kind {
        EventKind::KeyDown(Key::MetaRight) => println!("Right-⌘ down"),
        EventKind::KeyUp(Key::MetaRight)   => println!("Right-⌘ up"),
        _ => {}
    }
}
// Dropping `tap` stops the platform listener cleanly.
```

With the `chord` feature (default):

```rust
use keytap::chord::{ChordMatcher, Chord, ChordEvent};
use keytap::Key;

let matcher = ChordMatcher::builder()
    // Momentary (default): End fires when any chord key is released.
    .add("ptt", Chord::of([Key::MetaRight, Key::AltRight]))
    // Toggle: End fires on the NEXT complete press. Stays active
    // between presses; other chords are suppressed while it's active.
    .add_toggle("hands-free",
                Chord::of([Key::MetaRight, Key::AltRight, Key::Space]))
    .build()?;

while let Ok(event) = matcher.recv() {
    match event {
        ChordEvent::Start { id, .. } => start_recording(id),
        ChordEvent::End   { id, .. } => stop_recording(id),
    }
}
```

## What it does

- Sees every keyboard press/release the OS sees, including when your app is
  not in the foreground.
- Distinguishes left vs right modifiers (`ShiftLeft` ≠ `ShiftRight`,
  `MetaRight` ≠ `MetaLeft`, etc.).
- Never swallows keys — observation only.
- Shuts down cleanly when the `Tap` is dropped.
- Works on Linux Wayland by reading evdev directly.
- Never calls the macOS APIs that crash on Sonoma+ under threaded callers.

## What it doesn't do

- Simulate keys (use `enigo` or call the OS directly).
- Intercept / grab keys (would need root on Linux).
- Mouse events.
- Character interpretation / layout translation.

## Status

v0.1: macOS backend is implemented and live-tested. Linux (evdev) and
Windows (`WH_KEYBOARD_LL`) backends are implemented and compile-verified
across targets, but not yet run on real hardware — first-run bug reports
welcome.

Architecture and platform internals are documented in [DESIGN.md](./DESIGN.md).

## License

MIT OR Apache-2.0
