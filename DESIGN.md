# keytap — Design Document

Cross-platform, observe-only global keyboard taps with left/right modifier
fidelity and clean shutdown. Built for push-to-talk, hotkey daemons, overlay
toggles, and anything else that needs to see raw key events when the app is not
in the foreground.

## 1. Why this exists

The Rust ecosystem has no crate that satisfies all five of these at once:

| | rdev | global-hotkey | hotkey-listener | **keytap (target)** |
|---|---|---|---|---|
| macOS + Windows + Linux | ✅ | ✅ | ❌ (no Windows) | ✅ |
| Linux Wayland | ❌ (X11) | ❌ (X11) | ✅ (evdev) | ✅ (evdev) |
| Raw observe-only event stream | ✅ | ❌ (register-only) | ❌ (register-only) | ✅ |
| Left/right modifier fidelity | ✅ | ❌ (collapses) | ❌ (collapses) | ✅ |
| Clean `Drop`-based shutdown | ❌ (listen blocks forever) | ✅ | ✅ (partial: macOS stuck) | ✅ |
| Released in last 12 months | ❌ (2023) | ✅ | ✅ | ✅ |
| No Sonoma main-thread crash | ❌ by default | ✅ | ❌ (inherits rdev) | ✅ (never call the crashing API) |

Every apparent "drop-in replacement" for rdev either collapses `ShiftLeft` and
`ShiftRight` into one `SHIFT` flag (killing the Voicebox chord story) or
registers named shortcuts with the OS and can't emit the raw event stream.

## 2. Scope

### Goals

- **Observe-only global key events** — every press/release the OS sees, delivered
  to the consumer as a stream. Never swallow events.
- **Physical identity, not semantic identity.** `Key::MetaRight` is distinct from
  `Key::MetaLeft`. No character interpretation, no layout translation, no dead
  keys. The caller decides what those keys mean.
- **Clean lifecycle.** A `Tap` is created, produces events, and is dropped. When
  it drops, the platform thread shuts down and the OS tap is removed. No
  process-lifetime listener threads.
- **Thread-safe API.** The public surface is `Send + Sync` where reasonable.
  Creation can happen on any thread. The caller never has to run anything on
  the main thread.
- **Small, auditable surface.** Target: <3 kLOC Rust + FFI, one public module
  per concept. No global state, no mutexes on the hot path.
- **Optional chord matcher** on top of the raw stream — the common case for
  push-to-talk and hotkey daemons. Built in the same crate, behind the `chord`
  feature, so it uses the tap's exact key vocabulary.

### Non-goals

- **Key simulation** (synthetic input). Use `enigo`, `CGEventPost` directly, or
  platform-specific code. This is where rdev accumulates a lot of bug surface
  and we don't want it.
- **Grab / intercept** (blocking keys from reaching focused apps). On Linux this
  requires root (or uinput shenanigans). On macOS it requires elevated event tap
  permissions. Separate concerns.
- **Mouse, scroll wheel, tablet.** Keyboard only in v1. A sibling `mousetap`
  crate can come later if there's demand.
- **Character interpretation.** No `event.name: Option<String>`. This is the
  path that calls `TSMGetInputSourceProperty` on macOS and crashes on Sonoma+.
  If a caller wants characters, they can layer their own keymap on top of the
  physical events.
- **Registered shortcuts with OS filtering.** If a caller only wants "fire when
  Shift+D is pressed," they can either use `global-hotkey` or the chord matcher
  on top of keytap. The raw layer does not filter.

## 3. Public API

### 3.1 Key

A flat enum of physical key identities. Left/right modifier variants are
distinct. No `Shift` variant — only `ShiftLeft` and `ShiftRight`.

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[non_exhaustive]
pub enum Key {
    // Letters (positional / QWERTY layout, NOT layout-interpreted)
    A, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,

    // Digit row
    Digit0, Digit1, Digit2, Digit3, Digit4,
    Digit5, Digit6, Digit7, Digit8, Digit9,

    // Function row
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24,

    // Modifiers — left/right ALWAYS distinguished
    ShiftLeft,  ShiftRight,
    ControlLeft, ControlRight,
    AltLeft,    AltRight,    // AltRight == AltGr on some layouts
    MetaLeft,   MetaRight,   // Cmd on macOS, Win on Windows, Super on Linux

    // Arrows
    ArrowUp, ArrowDown, ArrowLeft, ArrowRight,

    // Navigation
    Home, End, PageUp, PageDown, Insert, Delete,

    // Editing
    Escape, Tab, CapsLock, Space, Enter, Backspace,

    // Punctuation (positional — by US-QWERTY physical location)
    Backtick, Minus, Equal,
    BracketLeft, BracketRight, Backslash,
    Semicolon, Quote, Comma, Period, Slash,

    // Numpad
    Numpad0, Numpad1, Numpad2, Numpad3, Numpad4,
    Numpad5, Numpad6, Numpad7, Numpad8, Numpad9,
    NumpadAdd, NumpadSubtract, NumpadMultiply, NumpadDivide,
    NumpadEnter, NumpadDecimal, NumLock,

    // Misc
    PrintScreen, ScrollLock, Pause, Menu,

    // Escape hatch: raw OS scancode for anything not mapped.
    // Exposed so consumers can support esoteric layouts without
    // waiting for a keytap release.
    Unknown(RawCode),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct RawCode(pub u32);
```

Key design choices:
- **`#[non_exhaustive]`** so we can add media keys later without a breaking
  release.
- **No `KeyCode` vs `Key` distinction** (rdev has both; it confuses people). One
  enum, physical identity.
- **`Unknown(RawCode)`** is always emitted rather than silently dropped. rdev
  drops unmapped events; we propagate them.

### 3.2 Events

```rust
#[derive(Copy, Clone, Debug)]
pub struct Event {
    /// Monotonic time the OS stamped the event. Not system time.
    pub time: Instant,

    /// What happened.
    pub kind: EventKind,
}

#[derive(Copy, Clone, Debug)]
pub enum EventKind {
    KeyDown(Key),
    KeyUp(Key),

    /// Auto-repeat keydown. Separate variant so consumers don't have
    /// to maintain their own repeat-detection state.
    KeyRepeat(Key),
}
```

Rationale for `KeyRepeat`:
- macOS auto-repeat delivers identical KeyDown events via CGEventTap, distinguishable via `kCGKeyboardEventAutorepeat`.
- Windows `LLKHF_EXTENDED` / repeat flag.
- Linux evdev: `EV_KEY` with value=2.

rdev collapses these into `KeyPress`, which forces every caller to de-dup. We
expose the distinction and let the caller collapse if they want.

### 3.3 The Tap

```rust
pub struct Tap { /* opaque */ }

impl Tap {
    /// Create with default config. Starts the platform listener immediately.
    /// Blocks on `new()` only for the handshake that confirms the OS accepted
    /// the tap (typically <10ms).
    pub fn new() -> Result<Self, Error>;

    pub fn builder() -> TapBuilder;

    /// Blocking receive.
    pub fn recv(&self) -> Result<Event, RecvError>;

    /// Non-blocking.
    pub fn try_recv(&self) -> Result<Event, TryRecvError>;

    /// Blocking with deadline.
    pub fn recv_timeout(&self, d: Duration) -> Result<Event, RecvTimeoutError>;

    /// Drain & iterate.
    pub fn iter(&self) -> TapIter<'_>;
}

impl Drop for Tap {
    fn drop(&mut self) {
        // Signals the platform thread to stop, joins it, removes the OS tap.
        // Bounded by TapConfig::shutdown_timeout (default 500ms).
    }
}
```

`Tap: Send + Sync` — the internal channel is `crossbeam-channel`.

```rust
pub struct TapBuilder { /* opaque */ }

impl TapBuilder {
    /// Channel capacity. Events beyond capacity are DROPPED (and counted).
    /// Default: 4096. Consumers can query dropped_count() to detect backpressure.
    pub fn capacity(self, n: usize) -> Self;

    /// Bounded vs unbounded. Bounded is default — we refuse to grow memory
    /// unboundedly if the consumer stalls.
    pub fn unbounded(self) -> Self;

    /// On Linux evdev, how long to wait between USB hotplug rescans.
    /// Default: 1s.
    pub fn linux_hotplug_interval(self, d: Duration) -> Self;

    /// On macOS, disable the repeat-detection path (emit every autorepeat
    /// as KeyDown instead of KeyRepeat). Default: off.
    pub fn macos_no_repeat_detection(self) -> Self;

    pub fn build(self) -> Result<Tap, Error>;
}
```

### 3.4 Error model

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("accessibility / input monitoring permission not granted")]
    PermissionDenied,

    #[error("no evdev devices found; is the user in the `input` group?")]
    NoDevices,

    #[error("platform tap creation failed: {0}")]
    TapFailed(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
```

On macOS, we **detect** missing Accessibility/Input Monitoring permission and
return `PermissionDenied` from `build()` instead of silently producing no
events (which is what rdev does — the single most-reported rdev "bug"). We use
`IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)` for this.

### 3.5 Chord matcher (optional, `chord` feature)

```rust
use keytap::chord::{ChordMatcher, Chord, ChordEvent};

let matcher = ChordMatcher::<&'static str>::builder()
    .add("ptt", Chord::of([Key::MetaRight, Key::AltRight]))
    .add("cancel", Chord::of([Key::Escape]))
    .build()?;

while let Ok(ev) = matcher.recv() {
    match ev {
        ChordEvent::Start { id: "ptt", .. } => start_recording(),
        ChordEvent::End   { id: "ptt", .. } => stop_recording(),
        ChordEvent::Start { id: "cancel", .. } => cancel(),
        _ => {}
    }
}
```

Semantics:
- A chord is a **set** of keys. Order doesn't matter for activation.
- `Start` fires when all chord keys are held AND no other non-chord keys are
  held. (Configurable — see `ChordBuilder::allow_extra`.)
- `End` fires when any chord key is released.
- If the user transitions directly from chord A to chord B (partially
  overlapping), End(A) fires before Start(B). Never overlapping `Start` events.
- Ambiguity resolution: if two registered chords match the current key set,
  the one with more keys wins (longest match).

Internal state machine:
```
held_keys: HashSet<Key>
active_chord: Option<ChordId>

on Event::KeyDown(k):
    held_keys.insert(k)
    new_match = longest_chord_matching(held_keys)
    if new_match != active_chord:
        if let Some(prev) = active_chord: emit End(prev)
        if let Some(next) = new_match:    emit Start(next)
        active_chord = new_match

on Event::KeyUp(k):
    held_keys.remove(k)
    (same matching logic as KeyDown)

on Event::KeyRepeat: ignore (chord activation is edge-triggered)
```

This is roughly what `tauri/src-tauri/src/hotkey_monitor.rs` in Voicebox
implements today, extracted and generalized.

## 4. Platform backends

All backends live behind a single `PlatformTap` trait and are selected via
`cfg`. The trait is internal; consumers see only `Tap`.

```rust
trait PlatformTap: Send {
    fn start(sender: Sender<Event>, config: &TapConfig) -> Result<Self, Error>;
    fn shutdown(self) -> Result<(), Error>;
}
```

### 4.1 macOS (`cfg(target_os = "macos")`)

- **API**: CGEventTap (`CGEventTapCreate` with
  `kCGSessionEventTap` + `kCGHeadInsertEventTap` + `kCGEventTapOptionListenOnly`).
- **Thread**: dedicated `std::thread`. Creates a `CFRunLoopSource` from the
  tap, adds it to the thread's own `CFRunLoop`, runs `CFRunLoopRun`.
- **Shutdown**: main thread calls `CFRunLoopStop` on the tap thread's run loop,
  joins the thread.
- **Repeat detection**: read `kCGKeyboardEventAutorepeat` field from the event.
- **Modifier left/right**: from the `keyCode` field — macOS already gives
  distinct virtual keycodes for left vs right modifiers
  (`kVK_Shift=56` vs `kVK_RightShift=60`, etc.).
- **Permission**: check `IOHIDCheckAccess` before creating the tap.

**Crucially: we never call `TSMGetInputSourceProperty`, `UCKeyTranslate`, or
any layout-dependent API.** That's the source of the Sonoma main-thread crash
in rdev. Keytap emits only physical keycodes, so we don't need layout info.

**Source to port from rdev**: `src/macos/keycodes.rs` (scancode → Key enum
table) is the only thing worth lifting. The listen loop needs to be rewritten
anyway because we're ditching the global callback/mutex design.

### 4.2 Windows (`cfg(target_os = "windows")`)

- **API**: `SetWindowsHookEx(WH_KEYBOARD_LL, hook_proc, ...)`.
- **Thread**: dedicated `std::thread` with its own Win32 message pump (`GetMessage`
  loop). Low-level hooks require a message pump on the hook-owning thread.
- **Shutdown**: `PostThreadMessage(WM_QUIT)` to the pump thread, join.
- **Repeat detection**: check the repeat count in `KBDLLHOOKSTRUCT` (not
  exposed directly; we track via a `last_key_down` HashMap).

  Actually — reconsider. Windows LL hook doesn't carry a repeat flag; we track
  state: if a KeyDown arrives for a key already down, it's a repeat. Same
  approach works on Linux evdev for consistency.
- **Modifier left/right**: from the `scanCode` and `flags` (`LLKHF_EXTENDED`)
  in `KBDLLHOOKSTRUCT`. Left/right Shift use different scancodes (0x2A vs
  0x36). Left/right Ctrl/Alt use the same scancode but the LLKHF_EXTENDED flag
  disambiguates.
- **Permission**: none required; low-level hooks are allowed by default.
  UIPI / integrity level matters for some cases (won't see events from
  higher-integrity processes) — documented caveat.

**Source to port from rdev**: `src/windows/keycodes.rs` table. Hook setup is
small enough to just rewrite cleanly.

### 4.3 Linux (`cfg(target_os = "linux")`)

- **API**: evdev directly, via `/dev/input/event*`.
- **Thread**: dedicated `std::thread` with `epoll` over all keyboard devices.
- **Device discovery**: scan `/dev/input/event*`, open each, check
  `EVIOCGBIT(EV_KEY)` for `KEY_A` (or similar) to filter to keyboards. Re-scan
  on a timer (default 1s) to pick up hotplug.
- **Shutdown**: close an internal eventfd that's in the epoll set; thread sees
  it and exits.
- **Keymap**: evdev uses Linux input-event-codes (`KEY_A` = 30, etc.). Direct
  mapping to our `Key` enum.
- **Left/right modifiers**: evdev already has `KEY_LEFTSHIFT` vs
  `KEY_RIGHTSHIFT`, etc. Clean.
- **Permission**: user must be in the `input` group (or grant `CAP_DAC_READ_SEARCH`).
  `build()` returns `Error::NoDevices` with actionable help text if no readable
  keyboard devices are found.

**Wayland works for free** because we're reading at the kernel input-device
level, below any display server. This is the approach
`martintrojer/hotkey-listener` uses on Linux and it's the right one.

**No X11 fallback.** If the user isn't in `input`, they get an error telling
them how to fix it. Adding an X11/XRecord path later is straightforward but
costs ~800 LOC and isn't worth it in v1.

**Source to port from hotkey-listener**: device discovery and epoll loop ideas
are worth studying, but licensed MIT — we can copy verbatim with attribution.
The crate is ~500 LOC total, so reimplementing is also cheap.

## 5. Concurrency model

```
┌──────────────┐           ┌─────────────────────┐
│ user thread  │           │ platform thread     │
│              │           │                     │
│   Tap::recv  │◀──events──│ OS callback/epoll   │
│              │           │                     │
│   Tap drops  │──shutdown▶│ exits, OS tap gone  │
└──────────────┘           └─────────────────────┘
         │                          │
         └────crossbeam channel─────┘
```

- One OS tap per `Tap`. Multiple `Tap`s in the same process = multiple OS taps.
- The channel is `crossbeam-channel` (bounded by default, unbounded optional).
- Shutdown is RAII: `Tap::drop` signals the platform thread, joins with a
  timeout (default 500ms). If the platform thread doesn't exit cleanly, the
  drop logs a warning (via `tracing` if the feature is on) and returns; we
  never block indefinitely.
- Explicit `close()` method is NOT provided. `Drop` is the API. If the user
  wants `close`, they can `drop(tap)` — same thing.

## 6. Dependencies

```toml
[dependencies]
crossbeam-channel = "0.5"
thiserror = "2"
tracing = { version = "0.1", optional = true }

[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6"
objc2-foundation = "0.3"
core-foundation-sys = "0.8"

[target.'cfg(target_os = "windows")'.dependencies.windows-sys]
version = "0.59"
features = [
    "Win32_UI_WindowsAndMessaging",
    "Win32_Foundation",
    "Win32_System_Threading",
]

[target.'cfg(target_os = "linux")'.dependencies]
evdev = "0.13"
nix = { version = "0.29", features = ["poll", "event"] }

[features]
default = ["chord"]
chord = []           # enables keytap::chord::{ChordMatcher, ...}
tracing = ["dep:tracing"]
serde = ["dep:serde"]
```

No `once_cell`, no `lazy_static`, no global mutex. Every `Tap` is independent.

## 7. What we lift from rdev (MIT)

Concrete list of files/tables to copy with attribution:

1. **`src/macos/keycodes.rs`** — the scancode table (`kVK_ANSI_A` → `Key::A`, etc.).
   ~150 LOC of pure data. No logic.
2. **`src/windows/keycodes.rs`** — VK code table.
3. **Nothing else.** rdev's listen loops, callback dispatch, and global state
   all need rewriting anyway for the new architecture.

From `hotkey-listener` (MIT):
1. **The evdev device scan pattern** — ~30 LOC of inspiration, not
   copy-paste. Cleaner to rewrite against the current `evdev` crate API.

From `global-hotkey` (MIT/Apache):
1. **Nothing directly**, but the `keyboard-types::Code` enum is worth
   studying for Key enum naming. We'll use our own enum because
   `keyboard-types::Code` collapses some things we care about.

## 8. Testing

- **Unit tests** for `Key` scancode round-trips on each platform.
- **Chord state-machine tests** are pure logic — runnable on any host.
- **Platform integration tests** run under a feature flag and are opt-in
  (require real input devices / permissions). CI matrix only runs them on
  self-hosted runners; GitHub-hosted runners skip.
- **Fuzz target** for chord matcher: random event streams → assert no
  overlapping `Start` events, assert every `Start` is eventually paired with an
  `End`.
- **Manual test**: the `examples/raw.rs` and `examples/chord.rs` binaries mirror
  Voicebox's usage.

## 9. Roadmap

### v0.1 — "it works for Voicebox"

- [ ] `Key` enum, full coverage for standard 104-key layouts
- [ ] macOS backend (CGEventTap) with clean shutdown
- [ ] Linux evdev backend with hotplug
- [ ] Windows WH_KEYBOARD_LL backend
- [ ] `Tap::new()` / `recv()` / `Drop` working on all three
- [ ] `chord` feature with `ChordMatcher`
- [ ] Voicebox migrated from rdev → keytap; ships a release on it
- [ ] README, docs.rs landing page

### v0.2 — community-ready

- [ ] `async` feature: `tokio::sync::mpsc` variant of `Tap`
- [ ] `serde` feature: serialize `Key` and `Chord` for config storage
- [ ] macOS permission-prompt helper (`keytap::macos::request_input_monitoring()`)
- [ ] Published on crates.io

### v0.3+ — discretionary

- [ ] Media keys (`MediaPlay`, `MediaNext`, brightness, volume)
- [ ] X11/XRecord Linux fallback for users who can't join `input` group
- [ ] Windows: filter by target-process integrity level (UIPI)
- [ ] Sibling crate: `mousetap`

## 10. Open questions

1. **`Tap` single-instance vs multi-instance per process?** macOS allows
   multiple CGEventTaps cleanly. Windows allows multiple low-level hooks.
   Linux evdev: multiple readers are fine. Proposal: allow multiple `Tap`s,
   document that each owns its own thread.
2. **Should `KeyRepeat` be opt-in or opt-out?** Leaning opt-out (emit by
   default, let callers ignore). rdev's "always collapse to KeyPress" is the
   main ergonomic complaint about rdev.
3. **Does the chord matcher belong in the same crate?** Arguments for:
   uses the exact Key vocabulary, keeps the "everything you need for
   push-to-talk in one dep" story. Arguments against: scope creep. Leaning
   "same crate, behind a feature flag."
4. **Error::PermissionDenied on macOS — should `new()` offer to prompt?** Apple's
   `IOHIDRequestAccess` can trigger the system prompt. Probably yes, behind a
   helper function, not the default `new()` behavior.
5. **`Key::Function(u8)` as a catch-all for F13-F24?** Or just enumerate them?
   Leaning enumerate (F13-F24 are real keys on real keyboards and should be
   first-class).

## 11. Naming

`keytap` — short, memorable, describes the mechanism (the OS-level keyboard
tap). Crate name available on crates.io (verified).

Alternative candidates considered: `chord-tap`, `raw-key`, `peek` (taken),
`keywatch` (taken), `keyhook` (Windows-biased). `keytap` wins.
