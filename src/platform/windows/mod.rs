//! Windows backend — `SetWindowsHookExW(WH_KEYBOARD_LL)` on a dedicated
//! message-pump thread.
//!
//! Architecture:
//!
//! 1. [`start`] spawns a worker thread. The worker thread:
//!    - Installs a thread-local [`ThreadCtx`] (holds the event `Sender`,
//!      the repeat-tracker HashSet, and the hook handle). The hook proc
//!      is a plain `unsafe extern "system" fn`, so TLS is how we route
//!      each hook callback back to its `Tap`. (rdev uses a single global
//!      callback, which limits you to one tap per process; we don't.)
//!    - Calls `SetWindowsHookExW(WH_KEYBOARD_LL, raw_callback, NULL, 0)`.
//!      `hmod` is NULL because the proc lives in our own module; the
//!      thread-id arg of 0 means "global hook."
//!    - Runs a `GetMessageW` loop until `WM_QUIT` arrives.
//!    - On exit: `UnhookWindowsHookEx`, clear TLS, thread returns.
//! 2. Shutdown: main thread calls `PostThreadMessageW(tid, WM_QUIT)`, then
//!    joins. Drop-driven.
//!
//! Left/right modifier disambiguation: low-level hooks usually deliver the
//! specific VK (`VK_LSHIFT` / `VK_RSHIFT` etc.), but some layouts / input
//! methods deliver the generic (`VK_SHIFT`). We defensively disambiguate
//! generic VKs using `scanCode` (for Shift) or `LLKHF_EXTENDED` (for Ctrl
//! and Alt).
//!
//! Repeat detection: the LL hook does not flag auto-repeat. We synthesize
//! it by tracking which VKs are currently down; a `WM_KEYDOWN` for a VK
//! already in the set is a [`EventKind::KeyRepeat`].

mod keycodes;

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_MENU, VK_SHIFT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, MSG,
    PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use self::keycodes::key_from_vk;
use crate::{Error, Event, EventKind, Key, tap::TapBuilder};

/// US-layout physical scancodes that disambiguate left vs right Shift when
/// the hook delivers the generic `VK_SHIFT`.
const SCAN_LEFT_SHIFT: u32 = 0x2A;
const SCAN_RIGHT_SHIFT: u32 = 0x36;

thread_local! {
    static THREAD_CTX: RefCell<Option<ThreadCtx>> = const { RefCell::new(None) };
}

struct ThreadCtx {
    tx: Sender<Event>,
    hook: HHOOK,
    held: HashSet<u32>,
}

#[derive(Debug)]
pub(crate) struct ShutdownGuard {
    thread_id: Arc<AtomicU32>,
    thread: Option<JoinHandle<()>>,
    // A guard to prevent accidental double-shutdown. Not strictly needed
    // but nice for paranoid callers.
    _signaled: AtomicBool,
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        let tid = self.thread_id.load(Ordering::Acquire);
        if tid != 0 {
            // Wake the worker's GetMessageW. Safe: PostThreadMessageW is
            // documented as thread-safe against any thread.
            unsafe {
                PostThreadMessageW(tid, WM_QUIT, 0, 0);
            }
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub(crate) fn start(tx: Sender<Event>, _cfg: &TapBuilder) -> Result<ShutdownGuard, Error> {
    let thread_id = Arc::new(AtomicU32::new(0));
    let thread_id_worker = thread_id.clone();

    // Handshake: the worker reports success/failure of `SetWindowsHookExW`.
    let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<(), Error>>(1);

    let thread = thread::Builder::new()
        .name("keytap-windows-ll-hook".into())
        .spawn(move || {
            // Publish our thread id so the main side can PostThreadMessageW.
            let tid = unsafe { GetCurrentThreadId() };
            thread_id_worker.store(tid, Ordering::Release);

            // Install hook with `hmod=NULL, dwThreadId=0` for a global
            // low-level keyboard hook owned by this thread.
            let hook = unsafe {
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(raw_callback), std::ptr::null_mut(), 0)
            };
            if hook.is_null() {
                let _ = ready_tx.send(Err(Error::TapFailed(
                    "SetWindowsHookExW returned NULL".into(),
                )));
                return;
            }

            THREAD_CTX.with(|cell| {
                *cell.borrow_mut() = Some(ThreadCtx {
                    tx,
                    hook,
                    held: HashSet::new(),
                });
            });
            let _ = ready_tx.send(Ok(()));

            // Pump messages until WM_QUIT.
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            loop {
                let r = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
                if r == 0 || r == -1 {
                    break;
                }
                // Low-level hooks don't require TranslateMessage/DispatchMessage
                // for our purposes — the hook proc fires independently of the
                // message loop. We just need the loop to keep the thread alive
                // and responsive to WM_QUIT.
            }

            unsafe {
                UnhookWindowsHookEx(hook);
            }
            THREAD_CTX.with(|cell| {
                cell.borrow_mut().take();
            });
        })
        .map_err(|e| Error::TapFailed(format!("spawn LL hook thread: {e}")))?;

    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => Ok(ShutdownGuard {
            thread_id,
            thread: Some(thread),
            _signaled: AtomicBool::new(false),
        }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => {
            // Best-effort: try to kill the thread via WM_QUIT in case it's stuck.
            let tid = thread_id.load(Ordering::Acquire);
            if tid != 0 {
                unsafe {
                    PostThreadMessageW(tid, WM_QUIT, 0, 0);
                }
            }
            let _ = thread.join();
            Err(Error::TapFailed(
                "LL hook install handshake timed out".into(),
            ))
        }
    }
}

unsafe extern "system" fn raw_callback(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // lparam is a pointer to KBDLLHOOKSTRUCT.
        let raw: &KBDLLHOOKSTRUCT = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let vk = raw.vkCode;
        let scan = raw.scanCode;
        let extended = (raw.flags & LLKHF_EXTENDED) != 0;
        let key = resolve_key(vk, scan, extended);

        let msg = wparam as u32;
        let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

        THREAD_CTX.with(|cell| {
            if let Some(ctx) = cell.borrow_mut().as_mut() {
                let kind = if is_down {
                    if !ctx.held.insert(vk) {
                        // Was already in held set → repeat.
                        Some(EventKind::KeyRepeat(key))
                    } else {
                        Some(EventKind::KeyDown(key))
                    }
                } else if is_up {
                    ctx.held.remove(&vk);
                    Some(EventKind::KeyUp(key))
                } else {
                    None
                };
                if let Some(kind) = kind {
                    let _ = ctx.tx.try_send(Event {
                        time: Instant::now(),
                        kind,
                    });
                }
            }
        });
    }
    // Always forward; we're observe-only.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Map a raw low-level-hook (`vkCode`, `scanCode`, `extended`) to a [`Key`].
/// Disambiguates the generic modifier VKs that occasionally appear even
/// though the LL hook usually delivers L/R-specific codes.
fn resolve_key(vk: u32, scan: u32, extended: bool) -> Key {
    if vk == VK_SHIFT as u32 {
        return match scan {
            SCAN_RIGHT_SHIFT => Key::ShiftRight,
            _ => Key::ShiftLeft,
        };
    }
    if vk == VK_CONTROL as u32 {
        return if extended {
            Key::ControlRight
        } else {
            Key::ControlLeft
        };
    }
    if vk == VK_MENU as u32 {
        return if extended {
            Key::AltRight
        } else {
            Key::AltLeft
        };
    }
    key_from_vk(vk)
}
