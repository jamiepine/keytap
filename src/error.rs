use thiserror::Error;

/// Errors returned when creating a [`Tap`](crate::Tap) or during its lifetime.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// macOS: Accessibility / Input Monitoring permission not granted.
    /// Linux: `/dev/input/event*` readable devices were found but the
    /// calling process does not have permission to read them.
    #[error("accessibility / input monitoring permission not granted")]
    PermissionDenied,

    /// Linux: no evdev keyboard devices were found.
    /// Usually means the user is not in the `input` group.
    #[error("no input devices found (is the user in the `input` group?)")]
    NoDevices,

    /// The platform tap could not be created.
    #[error("platform tap creation failed: {0}")]
    TapFailed(String),

    /// Underlying I/O error (typically from evdev device reads on Linux).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
