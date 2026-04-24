use std::time::Duration;

use crossbeam_channel::{Receiver, RecvError, RecvTimeoutError, TryRecvError};

use crate::{Error, Event};

/// Default channel capacity if the caller doesn't override it.
const DEFAULT_CAPACITY: usize = 4096;

/// Default time a `Tap::drop` will wait for the platform thread to exit
/// before logging a warning and returning.
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(500);

/// A live global keyboard tap. Drop it to stop listening.
///
/// Each `Tap` owns one platform-specific listener thread. Multiple `Tap`s
/// in the same process are allowed; each creates an independent OS tap.
#[derive(Debug)]
pub struct Tap {
    rx: Receiver<Event>,
    // When this field drops, the platform thread is signaled to stop and
    // joined. See `platform::ShutdownGuard`.
    _shutdown: crate::platform::ShutdownGuard,
}

impl Tap {
    /// Create a tap with default configuration.
    pub fn new() -> Result<Self, Error> {
        Self::builder().build()
    }

    pub fn builder() -> TapBuilder {
        TapBuilder::default()
    }

    pub fn recv(&self) -> Result<Event, RecvError> {
        self.rx.recv()
    }

    pub fn try_recv(&self) -> Result<Event, TryRecvError> {
        self.rx.try_recv()
    }

    pub fn recv_timeout(&self, d: Duration) -> Result<Event, RecvTimeoutError> {
        self.rx.recv_timeout(d)
    }

    pub fn iter(&self) -> TapIter<'_> {
        TapIter {
            inner: self.rx.iter(),
        }
    }
}

/// Iterator over a [`Tap`]. Blocks on each `next()`. Ends when the tap is
/// dropped.
#[derive(Debug)]
pub struct TapIter<'a> {
    inner: crossbeam_channel::Iter<'a, Event>,
}

impl Iterator for TapIter<'_> {
    type Item = Event;
    fn next(&mut self) -> Option<Event> {
        self.inner.next()
    }
}

/// Configurable [`Tap`] construction.
#[derive(Debug, Clone)]
pub struct TapBuilder {
    pub(crate) capacity: usize,
    pub(crate) unbounded: bool,
    pub(crate) linux_hotplug_interval: Duration,
    pub(crate) macos_no_repeat_detection: bool,
    pub(crate) shutdown_timeout: Duration,
}

impl Default for TapBuilder {
    fn default() -> Self {
        Self {
            capacity: DEFAULT_CAPACITY,
            unbounded: false,
            linux_hotplug_interval: Duration::from_secs(1),
            macos_no_repeat_detection: false,
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
        }
    }
}

impl TapBuilder {
    pub fn capacity(mut self, n: usize) -> Self {
        self.capacity = n;
        self
    }

    pub fn unbounded(mut self) -> Self {
        self.unbounded = true;
        self
    }

    pub fn linux_hotplug_interval(mut self, d: Duration) -> Self {
        self.linux_hotplug_interval = d;
        self
    }

    pub fn macos_no_repeat_detection(mut self) -> Self {
        self.macos_no_repeat_detection = true;
        self
    }

    pub fn shutdown_timeout(mut self, d: Duration) -> Self {
        self.shutdown_timeout = d;
        self
    }

    pub fn build(self) -> Result<Tap, Error> {
        let (tx, rx) = if self.unbounded {
            crossbeam_channel::unbounded()
        } else {
            crossbeam_channel::bounded(self.capacity)
        };
        let shutdown = crate::platform::start(tx, &self)?;
        Ok(Tap {
            rx,
            _shutdown: shutdown,
        })
    }
}
