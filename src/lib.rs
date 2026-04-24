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
mod platform;
mod tap;

#[cfg(feature = "chord")]
pub mod chord;

pub use error::Error;
pub use event::{Event, EventKind};
pub use key::{Key, RawCode};
pub use tap::{Tap, TapBuilder, TapIter};
