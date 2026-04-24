//! Platform-specific tap implementations. The public crate does not expose
//! any of this; callers interact only with [`crate::Tap`].
//!
//! Each platform module provides:
//!
//! - `start(sender, builder) -> Result<ShutdownGuard, Error>`: spawn the
//!   listener thread, install the OS tap, and return a guard whose `Drop`
//!   tears everything down.

use crossbeam_channel::Sender;

use crate::{Error, Event, tap::TapBuilder};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as imp;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod unsupported;
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
use unsupported as imp;

pub(crate) use imp::ShutdownGuard;

pub(crate) fn start(tx: Sender<Event>, cfg: &TapBuilder) -> Result<ShutdownGuard, Error> {
    imp::start(tx, cfg)
}
