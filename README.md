<h1 align="center">keytap</h1>

<p align="center">
  <strong>Cross-platform, observe-only global keyboard taps for Rust.</strong><br/>
  <span>macOS, Windows, and Linux (X11 + Wayland). Left/right modifier fidelity, clean shutdown, zero silent-failure modes.</span>
</p>

<p align="center">
  <a href="https://crates.io/crates/keytap">
    <img src="https://img.shields.io/crates/v/keytap.svg?color=DEA584" alt="crates.io" />
  </a>
  <a href="https://docs.rs/keytap">
    <img src="https://img.shields.io/docsrs/keytap?color=5865F2" alt="docs.rs" />
  </a>
  <a href="https://github.com/jamiepine/keytap/actions/workflows/ci.yml">
    <img src="https://img.shields.io/github/actions/workflow/status/jamiepine/keytap/ci.yml?branch=main&label=CI" alt="CI" />
  </a>
  <a href="https://crates.io/crates/keytap">
    <img src="https://img.shields.io/crates/d/keytap?color=000" alt="downloads" />
  </a>
  <a href="https://github.com/jamiepine/keytap/blob/main/LICENSE">
    <img src="https://img.shields.io/static/v1?label=License&message=MIT%20or%20Apache-2.0&color=000" alt="license" />
  </a>
  <a href="https://github.com/jamiepine/keytap">
    <img src="https://img.shields.io/static/v1?label=MSRV&message=1.85&color=DEA584" alt="MSRV 1.85" />
  </a>
  <a href="https://deepwiki.com/jamiepine/keytap">
    <img src="https://img.shields.io/static/v1?label=Ask&message=DeepWiki&color=5B6EF7" alt="Ask DeepWiki" />
  </a>
</p>

<p align="center">
  <a href="#quick-start">Quick Start</a> &bull;
  <a href="#chord-matching">Chord Matching</a> &bull;
  <a href="#keys-and-modifiers">Keys</a> &bull;
  <a href="#feature-flags">Features</a> &bull;
  <a href="#compared-to-alternatives">Comparison</a> &bull;
  <a href="./DESIGN.md">Design</a> &bull;
  <a href="https://docs.rs/keytap">Docs</a>
</p>

---

## Why

Every other Rust crate for global keyboard events forces a tradeoff:

- **rdev** — full raw event stream, but collapses modifiers in some paths, crashes on macOS 14+ under threaded callers (`TSMGetInputSourceProperty` on a background thread), and has no clean shutdown.
- **global-hotkey** — well-maintained, but registers named shortcuts with the OS and doesn't expose a raw event stream. No left/right modifier distinction.
- **hotkey-listener** — nice evdev backend for Linux Wayland, but no Windows support and collapses `ShiftLeft`/`ShiftRight` into one `Shift`.

keytap is a focused, observe-only keyboard tap that keeps left/right modifier identity, shuts down cleanly when you drop it, and fails fast with a typed error if the OS denies permission — instead of silently producing no events.

---

## Quick Start

```toml
[dependencies]
keytap = "0.3"
```

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
// Dropping `tap` stops the OS listener — no process-lifetime threads.
```

`Tap::new()` spawns a platform listener thread, installs the OS-level tap, and returns a handle. Events arrive on an internal channel; `recv`, `try_recv`, `recv_timeout`, and `iter` are all available. `Tap` is `Send + Sync` — share it via `Arc<Tap>` to fan events out across threads.

On macOS the first call may return `Error::PermissionDenied` if the process doesn't have Input Monitoring. This is a proactive check via `IOHIDCheckAccess`, not a silent failure.

---

## Chord Matching

The default `chord` feature adds a state machine on top of the raw stream for the common "fire when this combination is held" pattern. Two modes:

```rust
use keytap::{Key, chord::{ChordMatcher, Chord, ChordEvent}};

let matcher = ChordMatcher::builder()
    // Momentary (default): Start on activation, End the moment any
    // chord key is released. Standard push-to-talk.
    .add("ptt", Chord::of([Key::MetaRight, Key::AltRight]))

    // Toggle: Start on first complete press, End on the *next* complete
    // press. Releases between presses are ignored — stays active until
    // re-pressed. While active, other registered chords are suppressed.
    .add_toggle("hands-free",
                Chord::of([Key::MetaRight, Key::AltRight, Key::Space]))
    .build()?;

while let Ok(event) = matcher.recv() {
    match event {
        ChordEvent::Start { id, .. } => start(id),
        ChordEvent::End   { id, .. } => stop(id),
    }
}
```

**Semantics:**

- A chord is a **set** of keys — order doesn't matter for activation.
- **Longest match wins.** If `A` and `A+B` are both registered, pressing `A` then `B` transitions from `A` to `A+B`. Ties broken by registration order (earlier wins).
- **Non-overlapping Start events.** Transitioning directly from chord `X` to chord `Y` emits `End(X)` then `Start(Y)` — never two simultaneous actives.
- **Auto-repeat is ignored.** Chord activation is edge-triggered; holding a chord doesn't spam events.
- **Toggle suppresses others.** While a toggle chord is active, other registered chords won't fire — the session can't be hijacked by an overlapping chord.

---

## Keys and Modifiers

Left and right modifiers are always distinct — no generic `Shift` / `Control` / `Alt` / `Meta` variant. `Meta` is ⌘ on macOS, the Windows key on Windows, and Super on Linux.

The `Key` enum covers the standard 104-key layout plus:

- **F1–F24** on all three platforms
- **Full numpad** — digits, operators, decimal, `NumpadEnter`, `NumLock`
- **`IntlBackslash`** — ISO layout key between Left Shift and `Z` (absent on ANSI)
- **`Function`** — macOS Fn key
- **`Unknown(RawCode)`** — any scancode keytap doesn't name yet is still emitted, never dropped

Letter, digit, and punctuation variants are keyed to their **physical US-QWERTY location**, not the glyph the user sees on a non-US layout. No character interpretation, no layout translation — that's the path that crashes rdev on macOS 14+.

---

## Feature Flags

| Flag | Default | Effect |
|---|---|---|
| `chord` | ✅ | `keytap::chord::{ChordMatcher, Chord, ChordEvent, ChordMode}` |
| `serde` | ❌ | `Serialize` / `Deserialize` on `Key`, `RawCode`, `Chord`, `ChordMode` — for storing hotkey configs on disk |
| `tracing` | ❌ | `debug!` at tap start/stop, `trace!` on channel-full backpressure, `debug!` on Linux hotplug adoption |

---

## Compared to Alternatives

|   | keytap | [rdev] | [global-hotkey] | [hotkey-listener] |
|---|:---:|:---:|:---:|:---:|
| macOS / Windows / Linux | ✅ | ✅ | ✅ | macOS + Linux only |
| Linux Wayland | ✅ (evdev) | ❌ (X11) | ❌ (X11) | ✅ (evdev) |
| Raw observe-only event stream | ✅ | ✅ | ❌ (register-only) | ❌ (register-only) |
| Left/right modifier fidelity | ✅ | ✅ | ❌ | ❌ |
| Clean `Drop`-based shutdown | ✅ | ❌ (`listen()` blocks forever) | ✅ | partial |
| macOS permission detected at startup | ✅ | ❌ (silent no-events) | N/A (uses Carbon) | ❌ |
| Multiple taps per process | ✅ | ❌ (global callback) | ✅ | ❌ |
| No Sonoma main-thread crash | ✅ (API path doesn't exist) | ❌ | ✅ | ❌ (inherits rdev) |

[rdev]: https://github.com/Narsil/rdev
[global-hotkey]: https://github.com/tauri-apps/global-hotkey
[hotkey-listener]: https://github.com/martintrojer/hotkey-listener

---

## What it doesn't do

- **Key simulation** — use [`enigo`](https://github.com/enigo-rs/enigo) or call the OS directly.
- **Grab / intercept** — requires root on Linux; distinct concern.
- **Mouse events** — keyboard-only in v1. A sibling `mousetap` crate may come later.
- **Character interpretation** — no `event.name: Option<String>`. Keytap emits physical keycodes; consumers that want characters layer their own keymap.

---

## Status

v0.3. macOS backend is live-tested end-to-end. Linux (evdev) and Windows (`WH_KEYBOARD_LL`) backends are implemented and compile-verified across targets; first-run bug reports on real hardware are welcome. Architecture and platform internals are documented in [DESIGN.md](./DESIGN.md).

---

## License

MIT OR Apache-2.0.
