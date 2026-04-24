//! Fallback for platforms keytap doesn't support. Compiles but always
//! errors from `start()`.

use crossbeam_channel::Sender;

use crate::{Error, Event, tap::TapBuilder};

#[derive(Debug)]
pub(crate) struct ShutdownGuard {}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {}
}

pub(crate) fn start(_tx: Sender<Event>, _cfg: &TapBuilder) -> Result<ShutdownGuard, Error> {
    Err(Error::TapFailed(
        "keytap does not support this platform".into(),
    ))
}
