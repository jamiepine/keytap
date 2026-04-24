//! Mirrors Voicebox's push-to-talk chord usage: MetaRight+AltRight fires
//! a "recording started / stopped" log.

use std::time::{Duration, Instant};

use keytap::{
    Key,
    chord::{Chord, ChordEvent, ChordMatcher},
};

fn main() -> Result<(), keytap::Error> {
    let matcher: ChordMatcher<&'static str> = ChordMatcher::builder()
        .add("ptt", Chord::of([Key::MetaRight, Key::AltRight]))
        .add("cancel", Chord::of([Key::Escape]))
        .build()?;

    println!("hold right-⌘ + right-⌥ for push-to-talk. Esc cancels. (30s)");

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        match matcher.recv_timeout(Duration::from_millis(500)) {
            Ok(ChordEvent::Start { id, .. }) => println!(">> start {id}"),
            Ok(ChordEvent::End { id, .. }) => println!("<< end   {id}"),
            Err(_) => continue,
        }
    }
    Ok(())
}
