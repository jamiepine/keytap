//! Linux backend — evdev via `/dev/input/event*`. Works on both X11 and
//! Wayland because it reads below the display server.
//!
//! Architecture:
//!
//! 1. [`start`] scans `/dev/input` for keyboards, sets each fd non-blocking,
//!    and spawns a worker thread.
//! 2. The worker polls each device's `fetch_events` in a loop, translates
//!    evdev `KeyCode` to [`crate::Key`], and dispatches `Event`s on the
//!    channel. Polling cadence is 10 ms — tight enough for push-to-talk,
//!    cheap enough to not warm the CPU.
//! 3. Every `linux_hotplug_interval` (default 1 s) the worker rescans
//!    `/dev/input` and adopts any new keyboards (e.g. Bluetooth or USB
//!    reconnects).
//! 4. Shutdown: flip an `AtomicBool`, worker's next tick exits, join.
//!
//! Permissions: the process must be able to `O_RDONLY` the `event*` nodes.
//! On most distributions that means membership in the `input` group.
//! [`start`] returns [`Error::NoDevices`] if no readable keyboards are
//! found.
//!
//! Device-scan + hotplug loop modeled on `martintrojer/hotkey-listener`
//! (MIT), rewritten for evdev 0.13's `EventSummary` API and keytap's
//! event model.

mod keycodes;

use std::collections::HashSet;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use evdev::{Device, EventSummary, KeyCode};
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::libc;

use crate::{Error, Event, EventKind, tap::TapBuilder};

/// Marker key we probe for to decide whether an `event*` node is a
/// keyboard. Every real keyboard advertises `KEY_A`; mice, touchpads,
/// power buttons, and most peripherals do not.
const KEYBOARD_PROBE: KeyCode = KeyCode::KEY_A;

#[derive(Debug)]
pub(crate) struct ShutdownGuard {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub(crate) fn start(tx: Sender<Event>, cfg: &TapBuilder) -> Result<ShutdownGuard, Error> {
    let keyboards = find_keyboards()?;
    if keyboards.is_empty() {
        return Err(Error::NoDevices);
    }
    let paths = keyboards
        .iter()
        .map(|(p, _)| p.clone())
        .collect::<HashSet<_>>();
    let devices: Vec<Device> = keyboards.into_iter().map(|(_, d)| d).collect();
    set_nonblocking(&devices)?;

    let running = Arc::new(AtomicBool::new(true));
    let running_worker = running.clone();
    let hotplug_interval = cfg.linux_hotplug_interval;

    let thread = thread::Builder::new()
        .name("keytap-linux-evdev".into())
        .spawn(move || {
            run_worker(devices, paths, tx, running_worker, hotplug_interval);
        })
        .map_err(|e| Error::TapFailed(format!("spawn evdev worker: {e}")))?;

    Ok(ShutdownGuard {
        running,
        thread: Some(thread),
    })
}

fn run_worker(
    mut devices: Vec<Device>,
    mut known_paths: HashSet<PathBuf>,
    tx: Sender<Event>,
    running: Arc<AtomicBool>,
    hotplug_interval: Duration,
) {
    let mut last_hotplug = Instant::now();

    while running.load(Ordering::Relaxed) {
        if last_hotplug.elapsed() >= hotplug_interval {
            adopt_new_keyboards(&mut devices, &mut known_paths);
            last_hotplug = Instant::now();
        }

        for dev in devices.iter_mut() {
            match dev.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        if let EventSummary::Key(_, code, value) = ev.destructure() {
                            let key = keycodes::key_from_code(code.0);
                            let kind = match value {
                                0 => EventKind::KeyUp(key),
                                1 => EventKind::KeyDown(key),
                                2 => EventKind::KeyRepeat(key),
                                _ => continue,
                            };
                            let _ = tx.try_send(Event {
                                time: Instant::now(),
                                kind,
                            });
                        }
                    }
                }
                Err(e) => {
                    // EAGAIN/EWOULDBLOCK is the steady-state: no events
                    // ready. Anything else we ignore here; the next
                    // hotplug pass will drop devices that have gone away.
                    let code = e.raw_os_error();
                    if code != Some(libc::EAGAIN) && code != Some(libc::EWOULDBLOCK) {
                        // Device likely disappeared. Actual removal happens
                        // on the next hotplug scan when we can't re-open it.
                    }
                }
            }
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn find_keyboards() -> Result<Vec<(PathBuf, Device)>, Error> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir("/dev/input").map_err(Error::Io)?;
    let mut saw_event_node = false;
    let mut any_permission_denied = false;

    for entry in entries.flatten() {
        let path = entry.path();
        if !is_event_node(&path) {
            continue;
        }
        saw_event_node = true;

        match Device::open(&path) {
            Ok(device) => {
                if is_keyboard(&device) {
                    out.push((path, device));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                any_permission_denied = true;
            }
            Err(_) => {
                // Other errors (bad node, device busy, etc.) — skip silently.
            }
        }
    }

    if out.is_empty() && saw_event_node && any_permission_denied {
        return Err(Error::PermissionDenied);
    }
    Ok(out)
}

fn is_event_node(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with("event"))
        .unwrap_or(false)
}

fn is_keyboard(device: &Device) -> bool {
    device
        .supported_keys()
        .map(|keys| keys.contains(KEYBOARD_PROBE))
        .unwrap_or(false)
}

fn set_nonblocking(devices: &[Device]) -> Result<(), Error> {
    for dev in devices {
        let fd = dev.as_raw_fd();
        let raw = fcntl(fd, FcntlArg::F_GETFL).map_err(io_from_nix)?;
        let flags = OFlag::from_bits_truncate(raw) | OFlag::O_NONBLOCK;
        fcntl(fd, FcntlArg::F_SETFL(flags)).map_err(io_from_nix)?;
    }
    Ok(())
}

fn io_from_nix(e: nix::errno::Errno) -> Error {
    Error::Io(std::io::Error::from_raw_os_error(e as i32))
}

fn adopt_new_keyboards(devices: &mut Vec<Device>, known: &mut HashSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return;
    };
    let mut new_devices = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_event_node(&path) || known.contains(&path) {
            continue;
        }
        if let Ok(device) = Device::open(&path) {
            if is_keyboard(&device) {
                new_devices.push((path, device));
            }
        }
    }
    if new_devices.is_empty() {
        return;
    }
    // Give newly-appeared devices a moment to fully initialize —
    // hotkey-listener found this necessary in practice for Bluetooth.
    thread::sleep(Duration::from_millis(100));
    for (path, device) in new_devices {
        if set_nonblocking(std::slice::from_ref(&device)).is_ok() {
            known.insert(path);
            devices.push(device);
        }
    }
}
