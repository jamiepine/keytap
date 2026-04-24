# Testing Plan

End-to-end automation for the OS→tap round-trip, per platform.

Today the test suite is 23 unit tests — the chord state machine and the
scancode tables. Those are good, but they prove nothing about the actual
FFI paths: a silent scancode-table drift, a regression in macOS
`FlagsChanged` diffing, or a Windows LL-hook shutdown hang would all
pass `cargo test` green. This document plans an integration layer that
closes that gap.

## 1. Goals

1. **Verify the OS→tap path end-to-end** on macOS, Linux, and Windows —
   from synthetic OS-level input through the platform FFI into a
   received [`Event`].
2. **Catch regressions in the paths most likely to silently break**:
   scancode tables, modifier diff logic (macOS `FlagsChanged`), repeat
   synthesis (per-platform), left/right fidelity, clean shutdown.
3. **Run in public CI** on at least Linux + Windows. Maintainer-local
   on macOS (see §7).
4. **Fast**: the full integration suite should finish in under 30 seconds
   per OS so it gates every PR without slowing merges.

## 2. Non-goals

- Testing `Error::PermissionDenied` — requires revoking TCC on macOS or
  the `input` group on Linux, which CI can't do reversibly.
- Testing Bluetooth / USB hotplug, display-server switches, layout
  switching — these need real hardware.
- Fuzzing the chord state machine — separate effort, orthogonal.
- Performance benchmarks — no perf targets exist yet.
- Character-level / keymap-aware tests — keytap doesn't do character
  interpretation, so there's nothing to test here.

## 3. Architecture

A private, dev-only `test-synthesis` feature exposes a `Synth` trait
with per-platform implementations. Integration tests live in
`tests/roundtrip.rs` and use the trait to inject known sequences,
then read from a live `Tap` and assert.

```text
 ┌──────────────────────────────────────────┐
 │            tests/roundtrip.rs            │
 │  "assert key_a_round_trips(&mut s, &t)"  │
 └───────────────┬──────────────────────────┘
                 │ Synth trait
 ┌───────────────▼──────────────────────────┐
 │     src/test_synthesis/mod.rs            │
 │  pub(crate) trait Synth {                │
 │      fn down(&mut self, code: RawCode);  │
 │      fn up(&mut self, code: RawCode);    │
 │  }                                       │
 └─┬────────────────┬────────────────┬──────┘
   │ cfg(macos)     │ cfg(linux)     │ cfg(windows)
   ▼                ▼                ▼
 CGEventPost     uinput device    SendInput
 HIDEventTap    /dev/uinput        INPUT_KEYBOARD
```

**Feature shape in `Cargo.toml`:**

```toml
[features]
test-synthesis = []  # dev-only; NOT a public API surface
```

The module is `#[cfg(feature = "test-synthesis")] pub mod test_synthesis`
in `lib.rs`. We publish it so integration tests in `tests/` can reach
it, but document it as unstable and exclude from docs.rs.

**Why a trait instead of free functions?** RAII cleanup differs
per platform — Linux needs to destroy the uinput device, macOS and
Windows don't need anything. A `Synth` value held by the test owns
the teardown.

## 4. Per-platform synthesis

### 4.1 macOS — `CGEventPost`

```rust
use objc2_core_graphics::{CGEvent, CGEventTapLocation};

pub struct MacSynth;

impl Synth for MacSynth {
    fn down(&mut self, code: RawCode) {
        let ev = unsafe {
            CGEvent::new_keyboard_event(None, code.0 as u16, true).unwrap()
        };
        unsafe { CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&ev)) };
    }
    fn up(&mut self, code: RawCode) {
        let ev = unsafe {
            CGEvent::new_keyboard_event(None, code.0 as u16, false).unwrap()
        };
        unsafe { CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&ev)) };
    }
}
```

**Delivery**: events posted at `kCGHIDEventTap` are delivered to
session event taps (where our `Tap` listens), so the round-trip works.

**Permission**: the process doing the posting needs Accessibility
permission, **and** the same process's CGEventTap needs Input
Monitoring permission to observe. Both are grantable once per binary
via TCC; non-issue locally, blocker in public CI (see §7).

**Gotcha — `FlagsChanged`**: for modifier keys, `CGEvent::new_keyboard_event`
posts a regular `KeyDown`/`KeyUp`, not a `FlagsChanged`. Our tap handles
both code paths, so the test still validates the decode, but the
`FlagsChanged` diff logic isn't exercised by this path. To exercise
it, post a real `CGEvent` with flags set directly — out of scope for
phase 1, tracked as a phase 2 follow-up.

### 4.2 Linux — uinput

```rust
use evdev::{uinput::VirtualDeviceBuilder, AttributeSet, KeyCode, InputEvent, EventType};

pub struct LinuxSynth {
    dev: evdev::uinput::VirtualDevice,
}

impl LinuxSynth {
    pub fn new() -> io::Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for k in 1..=255 { keys.insert(KeyCode::new(k)); }
        let dev = VirtualDeviceBuilder::new()?
            .name("keytap-test-synth")
            .with_keys(&keys)?
            .build()?;
        // Sleep briefly so the main evdev enumeration picks up the new node.
        std::thread::sleep(Duration::from_millis(100));
        Ok(Self { dev })
    }
}

impl Synth for LinuxSynth {
    fn down(&mut self, code: RawCode) { self.write(code, 1); }
    fn up  (&mut self, code: RawCode) { self.write(code, 0); }
}

impl LinuxSynth {
    fn write(&mut self, code: RawCode, value: i32) {
        let ev = InputEvent::new(EventType::KEY, code.0 as u16, value);
        self.dev.emit(&[ev]).unwrap();
    }
}
// Drop on VirtualDevice calls UI_DEV_DESTROY automatically — good RAII.
```

**Permission**: needs `/dev/uinput` read/write. On GitHub's
`ubuntu-latest`, uinput is built-in as a module; the CI step runs:

```bash
sudo modprobe uinput || true
sudo chmod 666 /dev/uinput
```

The Linux `Tap` enumerates `/dev/input/event*` at `start()`. The
virtual device appears there as `eventN` and is picked up by the same
`KEY_A`-presence probe real keyboards use. The `.with_keys()`
declaration in the builder ensures the probe sees KEY_A.

**Sequencing**: `evdev` auto-appends `SYN_REPORT` events, so
consecutive `down`/`up` calls are atomic from the kernel's POV.

### 4.3 Windows — `SendInput`

```rust
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE,
};

pub struct WinSynth;

impl Synth for WinSynth {
    fn down(&mut self, code: RawCode) { send(code, 0); }
    fn up  (&mut self, code: RawCode) { send(code, KEYEVENTF_KEYUP); }
}

fn send(code: RawCode, flags: u32) {
    let mut input: INPUT = unsafe { std::mem::zeroed() };
    input.r#type = INPUT_KEYBOARD;
    input.Anonymous.ki = KEYBDINPUT {
        wVk: code.0 as u16,
        wScan: 0,
        dwFlags: flags,
        time: 0,
        dwExtraInfo: 0,
    };
    unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
}
```

**Delivery**: LL hook observes injected events with `LLKHF_INJECTED`
set. We don't filter that, so the round-trip works.

**Permission**: none required. Simplest of the three.

## 5. Test matrix

Each test creates a fresh `Tap`, synthesizes a scripted sequence,
drains events with a 500 ms timeout, and asserts the received stream.

| # | Scenario | Asserts |
|---|---|---|
| 1 | `KeyA` down, up | `[KeyDown(A), KeyUp(A)]` |
| 2 | `ShiftLeft` + `A` + release both | `[KeyDown(ShiftLeft), KeyDown(A), KeyUp(A), KeyUp(ShiftLeft)]`, exact order |
| 3 | `ShiftLeft` vs `ShiftRight` — synth each separately | Received keys differ (left/right fidelity) |
| 4 | `MetaRight` + `AltRight` chord | On macOS exercises the `FlagsChanged` diff logic |
| 5 | Hold `A` for 300 ms without releasing, then release | `[KeyDown(A), KeyRepeat(A)*, KeyUp(A)]` — repeat count platform-dependent, assert ≥ 1 |
| 6 | Synth an out-of-range scancode | Received as `Key::Unknown(RawCode(...))` |
| 7 | Create tap, synth 100 events, drop tap within 50 ms | No panic, no thread leak (checked via `join` timeout) |
| 8 | Drop tap **before** any events | Shutdown path works with an idle OS tap |

All tests run with `--test-threads=1`. Synthesized events are
globally visible (you can literally see them in the foreground app),
so parallelism would cause cross-contamination.

### 5.1 Test layout

```text
tests/
  roundtrip.rs            # hosts all tests; uses Synth trait
  mod_common.rs           # drain-with-timeout helpers, etc.
```

Each test looks roughly like:

```rust
#[test]
fn left_right_shift_are_distinct() {
    let tap = Tap::new().expect("tap open");
    let mut synth = Synth::new().expect("synth init");

    synth.down(scancode_for(Key::ShiftLeft));
    synth.up(scancode_for(Key::ShiftLeft));
    synth.down(scancode_for(Key::ShiftRight));
    synth.up(scancode_for(Key::ShiftRight));

    let events = drain(&tap, Duration::from_millis(500));
    let keys: Vec<Key> = events.into_iter().map(|e| match e.kind {
        EventKind::KeyDown(k) | EventKind::KeyUp(k) => k,
        _ => unreachable!(),
    }).collect();

    assert!(keys.contains(&Key::ShiftLeft));
    assert!(keys.contains(&Key::ShiftRight));
    assert_ne!(keys[0], keys[2]); // same position, different side
}
```

`scancode_for(Key)` is a helper that returns the current platform's raw
scancode for a given `Key` — essentially the inverse of the
`key_from_code` function. We need this for the synth side regardless;
adding a public (feature-gated) inverse is a natural side-effect.

## 6. CI integration

New job in `.github/workflows/ci.yml`:

```yaml
integration:
  name: integration (${{ matrix.os }})
  runs-on: ${{ matrix.os }}
  strategy:
    fail-fast: false
    matrix:
      os: [ubuntu-latest, windows-latest]
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - uses: Swatinem/rust-cache@v2

    - name: linux: set up uinput
      if: matrix.os == 'ubuntu-latest'
      run: |
        sudo modprobe uinput || true
        sudo chmod 666 /dev/uinput
        test -c /dev/uinput

    - name: cargo test (integration)
      run: cargo test --features test-synthesis --test roundtrip -- --test-threads=1
```

**macOS not in matrix** — see §7.

Run-time budget: 30s per OS. We're well under that — each test is
sub-second (synthesis is immediate, drain timeout is the lower bound).

## 7. The macOS gap

GitHub's `macos-latest` runners do not grant Input Monitoring to
arbitrary binaries. There is no public API for granting it without a
codesigned binary + manual TCC.db manipulation. Options, ranked:

1. **Document as "maintainer-local"** — add a `MAINTAINERS.md` note
   saying macOS integration tests run via `cargo test --features
   test-synthesis` on a machine that has granted Terminal (or the
   specific test binary) Input Monitoring. Cost: zero. Coverage cost:
   a macOS-specific regression can land on `main` and only be caught
   on next release-tag verification.
2. **Self-hosted macOS runner** — dedicated Mac mini (or similar) with
   Input Monitoring pre-granted to the runner binary. Cost: hardware
   + maintenance, but one-time setup. Coverage: full.
3. **Codesign + TCC override** — script to pre-add the test binary's
   code-signing identity to the TCC database during CI setup.
   Technically possible on self-hosted macOS runners with SIP modified
   or full-disk-access granted to the runner. Not possible on
   GitHub-hosted `macos-latest`. Cost: fragile; every macOS version
   may break the override path.

**Phase 1 ships option 1.** Phase 2 (if macOS regressions start
costing real time) upgrades to option 2.

Even without CI, the local `cargo test --features test-synthesis` on
macOS is a real improvement over today: it runs on every push the
maintainer makes locally, and catches the 90% of platform bugs that
would otherwise survive until a user reports them.

## 8. Implementation plan

Files to create or modify:

| File | Purpose | Est. LOC |
|---|---|---|
| `Cargo.toml` | Add `test-synthesis` feature | +1 |
| `src/lib.rs` | `#[cfg(feature = "test-synthesis")] pub mod test_synthesis` | +2 |
| `src/test_synthesis/mod.rs` | `Synth` trait, factory, shared `scancode_for` table | 50 |
| `src/test_synthesis/macos.rs` | `CGEventPost`-based synth | 100 |
| `src/test_synthesis/linux.rs` | uinput-based synth with RAII | 180 |
| `src/test_synthesis/windows.rs` | `SendInput`-based synth | 80 |
| `tests/roundtrip.rs` | Eight test scenarios from §5 | 200 |
| `.github/workflows/ci.yml` | New `integration` job | +25 |
| `MAINTAINERS.md` | macOS-local instructions | 30 |
| **Total** | | **~670** |

**Estimated effort**: 1-2 days of focused work. Linux uinput is the
bulk of it (RAII, permission handling, device enumeration timing);
Windows is half a day; macOS synth code is small but needs
platform-specific verification.

## 9. Known limitations of the harness

- **Injected events are globally visible.** On the maintainer's
  machine during local testing, the simulated key presses land in
  whatever app had focus. Tests should prefer keys that don't type
  visible characters (`F20`, `MetaRight`), and/or the maintainer
  should run tests in a scratch Terminal window.
- **Repeat timing is platform-dependent.** macOS repeat rate is tied
  to the user's keyboard settings; Linux comes from the virtual
  device's `auto_repeat` config; Windows follows the global keyboard
  repeat rate. Test #5 asserts `≥ 1` `KeyRepeat`, not an exact count.
- **Shutdown race tests are timing-dependent.** Test #7 uses a 50 ms
  drop window as a heuristic. A slower CI runner could false-positive
  under load; we'll raise the window if flakes happen.

## 10. Open questions

1. **Should `test_synthesis` be feature-gated or cfg-gated?** Feature
   lets `cargo test --features` work naturally; cfg requires
   `--cfg test_synthesis`. Feature is simpler; concern is that
   crates.io users could technically enable it and post synthetic
   input. Mitigated by naming (`test-synthesis`), docs, and the fact
   that no public type is exposed — it's only reachable from the
   crate's own test harness.

2. **Do we publish `scancode_for(Key)` as public API?** It's useful to
   consumers who want to display "press Shift+A" instructions. Could
   ship alongside the main crate without the test-synthesis gate.
   Proposal: yes, as `Key::macos_scancode() / windows_vk() / evdev_code()`
   — decide in phase 1 or punt to later.

3. **Should integration tests run on every PR or only on push to
   main?** Every PR is the usual default, but uinput setup in CI
   costs ~10s. Proposal: every PR; optimize later if PR latency
   becomes a complaint.
