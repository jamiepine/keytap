use std::time::Instant;

use crate::Key;

/// A keyboard event observed by a [`Tap`](crate::Tap).
#[derive(Copy, Clone, Debug)]
pub struct Event {
    /// Monotonic time the OS stamped the event. Not system time.
    pub time: Instant,
    /// What happened.
    pub kind: EventKind,
}

/// Event variants.
///
/// Auto-repeat is surfaced as a distinct [`EventKind::KeyRepeat`] so that
/// consumers don't have to maintain their own de-duplication state. Callers
/// that want rdev-style "collapse repeats into press" semantics can treat
/// `KeyRepeat` as a no-op.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum EventKind {
    KeyDown(Key),
    KeyUp(Key),
    KeyRepeat(Key),
}
