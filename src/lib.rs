//! keytap — cross-platform, observe-only global keyboard taps.
//!
//! See `DESIGN.md` in the repository root for the full architecture.
//!
//! ```no_run
//! use keytap::{Tap, EventKind, Key};
//!
//! # fn main() -> Result<(), keytap::Error> {
//! let tap = Tap::new()?;
//! for event in tap.iter() {
//!     if let EventKind::KeyDown(Key::MetaRight) = event.kind {
//!         println!("Right-⌘ pressed");
//!     }
//! }
//! # Ok(()) }
//! ```

#![warn(missing_debug_implementations)]
#![allow(dead_code)] // scaffolding — impls land in subsequent commits

pub use crossbeam_channel::{RecvError, RecvTimeoutError, TryRecvError};

mod error;
mod event;
mod key;
mod log;
mod platform;
mod tap;

#[cfg(feature = "chord")]
pub mod chord;

pub use error::Error;
pub use event::{Event, EventKind};
pub use key::{Key, RawCode};
pub use tap::{Tap, TapBuilder, TapIter};

// Compile-time guarantees that the public types used across threads are
// actually thread-safe. If someone ever changes `Tap` / `ChordMatcher` so
// they're no longer `Send + Sync`, the build breaks here instead of at
// some distant user site.
#[allow(dead_code)]
const fn assert_send_sync<T: Send + Sync>() {}

const _ASSERT_TAP_SEND_SYNC: () = assert_send_sync::<Tap>();
const _ASSERT_EVENT_SEND: () = assert_send_sync::<Event>();
const _ASSERT_KEY_SEND: () = assert_send_sync::<Key>();
const _ASSERT_ERROR_SEND: () = assert_send_sync::<Error>();

#[cfg(feature = "chord")]
const _ASSERT_CHORD_MATCHER_SEND_SYNC: () = assert_send_sync::<chord::ChordMatcher<&'static str>>();
#[cfg(feature = "chord")]
const _ASSERT_CHORD_SEND: () = assert_send_sync::<chord::Chord>();
#[cfg(feature = "chord")]
const _ASSERT_CHORD_MODE_SEND: () = assert_send_sync::<chord::ChordMode>();
