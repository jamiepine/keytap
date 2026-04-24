//! macOS backend — CGEventTap on a dedicated CFRunLoop thread.

mod keycodes;

use crossbeam_channel::Sender;

use crate::{Error, Event, tap::TapBuilder};

pub(crate) use self::tap::ShutdownGuard;

mod tap;

pub(crate) fn start(tx: Sender<Event>, cfg: &TapBuilder) -> Result<ShutdownGuard, Error> {
    tap::start(tx, cfg)
}
