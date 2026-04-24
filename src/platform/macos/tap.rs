//! macOS event-tap implementation.
//!
//! Architecture:
//!
//! 1. Main thread calls [`start`]. We check `IOHIDCheckAccess` for Input
//!    Monitoring permission. If denied, return early with `PermissionDenied`.
//! 2. Spawn a dedicated worker thread. The thread:
//!    - Creates a `CGEventTap` in listen-only mode for keyboard + flags-changed
//!      events, with our [`CallbackContext`] as `user_info`.
//!    - Wraps the tap in a `CFRunLoopSource`, adds it to the current thread's
//!      `CFRunLoop`, enables the tap.
//!    - Sends its `CFRunLoopRef` back to the main thread through a handoff
//!      channel.
//!    - Calls `CFRunLoop::run()` — blocks until `CFRunLoopStop` is called.
//! 3. Main thread stashes the run-loop ref and thread handle in the
//!    [`ShutdownGuard`].
//! 4. On drop, main thread calls `CFRunLoopStop(ref)` → worker exits its run
//!    loop, drops the tap, the worker thread joins.
//!
//! Key design choices:
//!
//! - **No global callback.** `user_info` carries a `*mut CallbackContext`
//!   so multiple concurrent `Tap`s in the same process don't alias.
//! - **No TSM / layout APIs.** We read only `kCGKeyboardEventKeycode` and
//!   `kCGKeyboardEventAutorepeat`; never `TSMGetInputSourceProperty` or
//!   `UCKeyTranslate`. That's the class of API that crashes off-main-thread
//!   callers on macOS 14+.
//! - **Modifier tracking.** `kCGEventFlagsChanged` events don't carry a
//!   "down/up" bit — the tap sees the *new* flags. We maintain a modifier
//!   held-set in [`CallbackContext::modifier_state`] and synthesize
//!   `KeyDown`/`KeyUp` on edges.

use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crossbeam_channel::{Sender, bounded};
use objc2_core_foundation::{CFMachPort, CFRunLoop, kCFRunLoopCommonModes};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType,
};

use super::keycodes::key_from_code;
use crate::log;
use crate::{Error, Event, EventKind, Key, tap::TapBuilder};

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOHIDCheckAccess(request: u32) -> u32;
}
const K_IOHID_REQUEST_TYPE_LISTEN: u32 = 1;
const K_IOHID_ACCESS_TYPE_GRANTED: u32 = 0;

/// CGEventFlags bits corresponding to each side of each modifier. We use
/// these to decide whether a FlagsChanged event means "key went down" or
/// "key went up" — CGEventFlags holds the *new* state; we diff against our
/// tracked state.
const CGEVENT_FLAG_SHIFT_LEFT: u64 = 0x00020002;
const CGEVENT_FLAG_SHIFT_RIGHT: u64 = 0x00020004;
const CGEVENT_FLAG_CONTROL_LEFT: u64 = 0x00040001;
const CGEVENT_FLAG_CONTROL_RIGHT: u64 = 0x00042000;
const CGEVENT_FLAG_ALT_LEFT: u64 = 0x00080020;
const CGEVENT_FLAG_ALT_RIGHT: u64 = 0x00080040;
const CGEVENT_FLAG_META_LEFT: u64 = 0x00100008;
const CGEVENT_FLAG_META_RIGHT: u64 = 0x00100010;
const CGEVENT_FLAG_CAPS_LOCK: u64 = 0x00010000;

/// Heap-allocated context pointed to by the CGEventTap's `user_info`.
/// Lives for the full lifetime of the tap. Freed in [`ShutdownGuard::drop`]
/// after the run loop has stopped and the tap thread has joined.
struct CallbackContext {
    tx: Sender<Event>,
    macos_no_repeat_detection: bool,
    /// Previous CGEventFlags value — used to detect modifier press/release
    /// edges in FlagsChanged events.
    last_flags: std::sync::atomic::AtomicU64,
}

/// Newtype wrapper so we can move a `CFRunLoop*` across threads.
/// Sending a raw pointer and calling `CFRunLoopStop` from another thread
/// is documented-safe. `Sync` is justified the same way — the only time
/// anyone touches this pointer is via `CFRunLoopStop` in `Drop`, which
/// can be called from any thread.
struct SendableRunLoopPtr(NonNull<CFRunLoop>);
unsafe impl Send for SendableRunLoopPtr {}
unsafe impl Sync for SendableRunLoopPtr {}

/// Same story for the callback-context pointer: we move the `*mut` across
/// threads but never dereference it concurrently — the worker installs it
/// as the tap's user_info and returns; the C callback is the only reader
/// while the tap is live, and by the time [`ShutdownGuard::drop`] frees the
/// box the worker has already joined.
struct SendableCtxPtr(*mut CallbackContext);
unsafe impl Send for SendableCtxPtr {}

impl SendableCtxPtr {
    /// Consume the wrapper and return the raw pointer. Consuming (not
    /// field-accessing) forces the closure capture to be the wrapper
    /// itself, not the inner raw pointer.
    fn into_raw(self) -> *mut CallbackContext {
        self.0
    }
}

pub(crate) struct ShutdownGuard {
    run_loop: Option<SendableRunLoopPtr>,
    thread: Option<JoinHandle<()>>,
    // Box<CallbackContext> leaked as raw for user_info. Reclaimed after
    // the worker joins.
    ctx_ptr: AtomicPtr<CallbackContext>,
}

impl std::fmt::Debug for ShutdownGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShutdownGuard").finish_non_exhaustive()
    }
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        log::debug!("keytap: stopping macOS CGEventTap");
        if let Some(rl) = self.run_loop.take() {
            // Safe: CFRunLoopStop is documented as thread-safe with respect
            // to the target run loop.
            unsafe {
                let run_loop_ref: &CFRunLoop = rl.0.as_ref();
                run_loop_ref.stop();
            }
        }
        if let Some(t) = self.thread.take() {
            // If the worker doesn't exit within a reasonable window we don't
            // block the user's program indefinitely — but we do log.
            // thread::join has no timeout variant in std, so we just join.
            let _ = t.join();
        }
        // Now it's safe to free the callback context.
        let ptr = self.ctx_ptr.swap(ptr::null_mut(), Ordering::AcqRel);
        if !ptr.is_null() {
            unsafe { drop(Box::from_raw(ptr)) };
        }
    }
}

pub(crate) fn start(tx: Sender<Event>, cfg: &TapBuilder) -> Result<ShutdownGuard, Error> {
    log::debug!("keytap: starting macOS CGEventTap");
    // Proactive permission check. On denial, fail fast with a typed error
    // instead of silently producing no events (the #1 rdev complaint).
    let access = unsafe { IOHIDCheckAccess(K_IOHID_REQUEST_TYPE_LISTEN) };
    if access != K_IOHID_ACCESS_TYPE_GRANTED {
        log::debug!("keytap: IOHIDCheckAccess denied (access={})", access);
        return Err(Error::PermissionDenied);
    }

    let ctx = Box::new(CallbackContext {
        tx,
        macos_no_repeat_detection: cfg.macos_no_repeat_detection,
        last_flags: std::sync::atomic::AtomicU64::new(0),
    });
    let ctx_ptr = Box::into_raw(ctx);

    // Handoff channel for the worker to send its CFRunLoop pointer back.
    let (rl_tx, rl_rx) = bounded::<Result<SendableRunLoopPtr, Error>>(1);
    let ready_flag = Arc::new(AtomicBool::new(false));
    let ready_flag_worker = ready_flag.clone();

    let ctx_send = SendableCtxPtr(ctx_ptr);
    let thread = thread::Builder::new()
        .name("keytap-macos-tap".into())
        .spawn(move || {
            run_tap_thread(ctx_send.into_raw(), rl_tx, ready_flag_worker);
        })
        .map_err(|e| Error::TapFailed(format!("spawn tap thread: {e}")))?;

    // Wait for the worker to either succeed in creating the tap and send its
    // run loop, or fail. Wait up to 2s.
    let run_loop = match rl_rx.recv_timeout(std::time::Duration::from_secs(2)) {
        Ok(Ok(rl)) => rl,
        Ok(Err(e)) => {
            let _ = thread.join();
            unsafe { drop(Box::from_raw(ctx_ptr)) };
            return Err(e);
        }
        Err(_) => {
            // Timed out: either tap_create is hanging (shouldn't) or the
            // thread panicked before sending. Best effort cleanup.
            unsafe { drop(Box::from_raw(ctx_ptr)) };
            return Err(Error::TapFailed("tap creation handshake timed out".into()));
        }
    };

    while !ready_flag.load(Ordering::Acquire) {
        std::thread::yield_now();
    }

    Ok(ShutdownGuard {
        run_loop: Some(run_loop),
        thread: Some(thread),
        ctx_ptr: AtomicPtr::new(ctx_ptr),
    })
}

fn run_tap_thread(
    ctx_ptr: *mut CallbackContext,
    rl_tx: Sender<Result<SendableRunLoopPtr, Error>>,
    ready: Arc<AtomicBool>,
) {
    // Keyboard events only: KeyDown | KeyUp | FlagsChanged.
    // (We start with kCGEventMaskForAllEvents to match rdev's behavior and
    // let the callback do its own filtering — simpler and avoids mis-masking
    // events that come through as FlagsChanged for modifier-only combinations.)
    let mask: u64 = (1u64 << CGEventType::KeyDown.0)
        | (1u64 << CGEventType::KeyUp.0)
        | (1u64 << CGEventType::FlagsChanged.0);

    let tap = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::HIDEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            mask,
            Some(raw_callback),
            ctx_ptr as *mut c_void,
        )
    };
    let tap = match tap {
        Some(t) => t,
        None => {
            let _ = rl_tx.send(Err(Error::TapFailed(
                "CGEventTapCreate returned null — \
                 most likely Input Monitoring permission was revoked \
                 between the permission check and tap creation"
                    .into(),
            )));
            return;
        }
    };

    let source = match CFMachPort::new_run_loop_source(None, Some(&tap), 0) {
        Some(s) => s,
        None => {
            let _ = rl_tx.send(Err(Error::TapFailed(
                "CFMachPortCreateRunLoopSource returned null".into(),
            )));
            return;
        }
    };

    let current_loop = match CFRunLoop::current() {
        Some(rl) => rl,
        None => {
            let _ = rl_tx.send(Err(Error::TapFailed(
                "CFRunLoop::current() returned None".into(),
            )));
            return;
        }
    };
    current_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
    CGEvent::tap_enable(&tap, true);

    // Hand the run loop back to the main thread for shutdown signaling.
    // CFRunLoop is a !Send CFRetained, but the raw pointer is fine to move.
    let rl_ptr = SendableRunLoopPtr(NonNull::from(&*current_loop));
    if rl_tx.send(Ok(rl_ptr)).is_err() {
        return;
    }
    ready.store(true, Ordering::Release);

    // Block until CFRunLoopStop is called against this loop.
    CFRunLoop::run();
    // Run loop has stopped. Everything in scope here drops cleanly:
    // source, tap, current_loop all release their Core Foundation refs.
}

unsafe extern "C-unwind" fn raw_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    cg_event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: user_info was set to a valid Box<CallbackContext> in start()
    // and the pointer is valid for the entire life of the tap. The context
    // is only freed after this thread has joined.
    let ctx: &CallbackContext = unsafe { &*(user_info as *const CallbackContext) };

    // Decode keycode once; all three event types need it.
    let cg_event_ref: &CGEvent = unsafe { cg_event.as_ref() };
    let keycode =
        CGEvent::integer_value_field(Some(cg_event_ref), CGEventField::KeyboardEventKeycode) as u32;
    let key = key_from_code(keycode);

    let now = Instant::now();

    let maybe_event = match event_type {
        CGEventType::KeyDown => {
            let auto_repeat = CGEvent::integer_value_field(
                Some(cg_event_ref),
                CGEventField::KeyboardEventAutorepeat,
            ) != 0;
            let kind = if auto_repeat && !ctx.macos_no_repeat_detection {
                EventKind::KeyRepeat(key)
            } else {
                EventKind::KeyDown(key)
            };
            Some(Event { time: now, kind })
        }
        CGEventType::KeyUp => Some(Event {
            time: now,
            kind: EventKind::KeyUp(key),
        }),
        CGEventType::FlagsChanged => {
            // Distinguish press/release by diffing against the previous flags.
            // CGEvent's "flags" field (field 0x81) is the modifier state
            // AFTER this event. A modifier bit going 0→1 is a KeyDown; 1→0
            // is a KeyUp.
            let flags = cg_event_flags(cg_event_ref);
            let prev = ctx.last_flags.swap(flags, Ordering::Relaxed);
            let bit = flag_bit_for_key(key);
            if bit == 0 {
                // Unknown modifier — synthesize a KeyDown+KeyUp would be
                // wrong; we emit nothing until we can classify it.
                None
            } else {
                let was_down = (prev & bit) != 0;
                let is_down = (flags & bit) != 0;
                match (was_down, is_down) {
                    (false, true) => Some(Event {
                        time: now,
                        kind: EventKind::KeyDown(key),
                    }),
                    (true, false) => Some(Event {
                        time: now,
                        kind: EventKind::KeyUp(key),
                    }),
                    _ => None,
                }
            }
        }
        _ => None,
    };

    if let Some(event) = maybe_event {
        // Non-blocking: if the consumer can't keep up, drop the event.
        if ctx.tx.try_send(event).is_err() {
            log::trace!("keytap: channel full — dropping event");
        }
    }

    // ListenOnly — return value is ignored, but must be valid.
    cg_event.as_ptr()
}

/// Read `CGEventFlags` from a CGEvent. `CGEventGetFlags` is the only path —
/// flags do not have a `CGEventField` id, so `CGEventGetIntegerValueField`
/// silently returns 0 for them.
fn cg_event_flags(event: &CGEvent) -> u64 {
    CGEvent::flags(Some(event)).0
}

/// Map a modifier [`Key`] to the CGEventFlags bit that represents its
/// held state.
fn flag_bit_for_key(key: Key) -> u64 {
    match key {
        Key::ShiftLeft => CGEVENT_FLAG_SHIFT_LEFT,
        Key::ShiftRight => CGEVENT_FLAG_SHIFT_RIGHT,
        Key::ControlLeft => CGEVENT_FLAG_CONTROL_LEFT,
        Key::ControlRight => CGEVENT_FLAG_CONTROL_RIGHT,
        Key::AltLeft => CGEVENT_FLAG_ALT_LEFT,
        Key::AltRight => CGEVENT_FLAG_ALT_RIGHT,
        Key::MetaLeft => CGEVENT_FLAG_META_LEFT,
        Key::MetaRight => CGEVENT_FLAG_META_RIGHT,
        Key::CapsLock => CGEVENT_FLAG_CAPS_LOCK,
        _ => 0,
    }
}
