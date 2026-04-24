//! Print every keyboard event the OS sees. Exits after 15 seconds or on
//! Ctrl-C.
//!
//! Run: `cargo run --example raw`. On macOS you'll be prompted for Input
//! Monitoring permission the first time.

use std::time::{Duration, Instant};

use keytap::{EventKind, Tap};

fn main() -> Result<(), keytap::Error> {
    let tap = Tap::new()?;
    println!("tap created — press keys (15s)");

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(15) {
        match tap.recv_timeout(Duration::from_millis(500)) {
            Ok(event) => match event.kind {
                EventKind::KeyDown(k) => println!("DOWN  {k:?}"),
                EventKind::KeyUp(k) => println!("UP    {k:?}"),
                EventKind::KeyRepeat(k) => println!("REP   {k:?}"),
            },
            Err(_) => continue,
        }
    }

    println!("done");
    Ok(())
}
