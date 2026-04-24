//! Internal diagnostics shim. Re-exports `tracing`'s logging macros when
//! the `tracing` feature is enabled; otherwise expands to no-ops so call
//! sites don't need their own `cfg` gates.

#[cfg(feature = "tracing")]
#[macro_export]
#[doc(hidden)]
macro_rules! __ktap_debug {
    ($($t:tt)*) => { ::tracing::debug!($($t)*) };
}

#[cfg(not(feature = "tracing"))]
#[macro_export]
#[doc(hidden)]
macro_rules! __ktap_debug {
    ($($t:tt)*) => {};
}

#[cfg(feature = "tracing")]
#[macro_export]
#[doc(hidden)]
macro_rules! __ktap_trace {
    ($($t:tt)*) => { ::tracing::trace!($($t)*) };
}

#[cfg(not(feature = "tracing"))]
#[macro_export]
#[doc(hidden)]
macro_rules! __ktap_trace {
    ($($t:tt)*) => {};
}

pub(crate) use __ktap_debug as debug;
pub(crate) use __ktap_trace as trace;
