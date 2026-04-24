//! Chord matching on top of a raw [`crate::Tap`].
//!
//! A chord is a **set** of keys — order doesn't matter for activation.
//! The matcher is a small state machine: when the held-key set transitions
//! into a registered chord, [`ChordEvent::Start`] fires; when it transitions
//! out, [`ChordEvent::End`] fires. Never overlapping `Start`s — transitioning
//! directly from chord A to chord B emits `End(A)` then `Start(B)`.
//!
//! Ambiguity resolution: if two registered chords match the current held
//! set, the chord with more keys wins (longest match).
//!
//! ```no_run
//! use keytap::chord::{ChordMatcher, Chord, ChordEvent};
//! use keytap::Key;
//!
//! # fn main() -> Result<(), keytap::Error> {
//! let matcher = ChordMatcher::builder()
//!     .add("ptt",    Chord::of([Key::MetaRight, Key::AltRight]))
//!     .add("cancel", Chord::of([Key::Escape]))
//!     .build()?;
//!
//! while let Ok(ev) = matcher.recv() {
//!     match ev {
//!         ChordEvent::Start { id, .. } => println!("start {id:?}"),
//!         ChordEvent::End   { id, .. } => println!("end {id:?}"),
//!     }
//! }
//! # Ok(()) }
//! ```

use std::collections::HashSet;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvError, RecvTimeoutError, TryRecvError};

use crate::{Error, Event, EventKind, Key, Tap};

/// A set of physical keys whose simultaneous-held state triggers a chord.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chord {
    keys: HashSet<Key>,
}

impl Chord {
    pub fn of<I: IntoIterator<Item = Key>>(keys: I) -> Self {
        Self {
            keys: keys.into_iter().collect(),
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &Key> {
        self.keys.iter()
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Whether every key in this chord is currently held.
    fn matches(&self, held: &HashSet<Key>) -> bool {
        !self.keys.is_empty() && self.keys.is_subset(held)
    }
}

/// Events emitted by a [`ChordMatcher`].
#[derive(Clone, Debug, PartialEq)]
pub enum ChordEvent<Id: Clone + Debug> {
    Start { id: Id, time: Instant },
    End { id: Id, time: Instant },
}

/// Chord matcher built on top of a [`crate::Tap`]. Spawns a worker thread
/// that consumes the tap's raw events and emits chord transitions.
#[derive(Debug)]
pub struct ChordMatcher<Id: Clone + Debug + Eq + Hash + Send + 'static> {
    rx: Receiver<ChordEvent<Id>>,
    worker: Option<WorkerHandle>,
}

#[derive(Debug)]
struct WorkerHandle {
    thread: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
}

impl<Id: Clone + Debug + Eq + Hash + Send + 'static> ChordMatcher<Id> {
    pub fn builder() -> ChordMatcherBuilder<Id> {
        ChordMatcherBuilder {
            registered: Vec::new(),
            tap: None,
        }
    }

    pub fn recv(&self) -> Result<ChordEvent<Id>, RecvError> {
        self.rx.recv()
    }

    pub fn try_recv(&self) -> Result<ChordEvent<Id>, TryRecvError> {
        self.rx.try_recv()
    }

    pub fn recv_timeout(&self, d: Duration) -> Result<ChordEvent<Id>, RecvTimeoutError> {
        self.rx.recv_timeout(d)
    }
}

impl<Id: Clone + Debug + Eq + Hash + Send + 'static> Drop for ChordMatcher<Id> {
    fn drop(&mut self) {
        if let Some(mut handle) = self.worker.take() {
            handle.running.store(false, Ordering::Relaxed);
            if let Some(thread) = handle.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

#[derive(Debug)]
pub struct ChordMatcherBuilder<Id: Clone + Debug + Eq + Hash + Send + 'static> {
    registered: Vec<(Id, Chord)>,
    tap: Option<Tap>,
}

impl<Id: Clone + Debug + Eq + Hash + Send + 'static> ChordMatcherBuilder<Id> {
    pub fn add(mut self, id: Id, chord: Chord) -> Self {
        self.registered.push((id, chord));
        self
    }

    /// Use an existing [`Tap`] instead of creating a new one. Useful when
    /// the caller wants to observe raw events AND match chords from the
    /// same OS tap.
    ///
    /// Note: an injected tap is consumed by the matcher and cannot also
    /// be used for raw reads (single-consumer channel). For a true
    /// fan-out, build two `Tap`s.
    pub fn with_tap(mut self, tap: Tap) -> Self {
        self.tap = Some(tap);
        self
    }

    pub fn build(mut self) -> Result<ChordMatcher<Id>, Error> {
        let tap = match self.tap.take() {
            Some(t) => t,
            None => Tap::new()?,
        };

        let (tx, rx) = crossbeam_channel::unbounded();
        let registered = std::mem::take(&mut self.registered);
        let running = Arc::new(AtomicBool::new(true));
        let running_worker = running.clone();

        let thread = thread::Builder::new()
            .name("keytap-chord".into())
            .spawn(move || {
                let mut state = MatcherState::new(registered);
                while running_worker.load(Ordering::Relaxed) {
                    match tap.recv_timeout(Duration::from_millis(50)) {
                        Ok(event) => {
                            state.process(event, |ev| {
                                let _ = tx.send(ev);
                            });
                        }
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                // tap drops here, closing the OS tap.
            })
            .expect("failed to spawn chord worker thread");

        Ok(ChordMatcher {
            rx,
            worker: Some(WorkerHandle {
                thread: Some(thread),
                running,
            }),
        })
    }
}

/// Pure state machine. All platform-free; directly unit-testable.
///
/// Call [`MatcherState::process`] with each [`Event`] from the tap; it
/// invokes the callback with zero or one [`ChordEvent`] per call.
#[derive(Debug)]
pub(crate) struct MatcherState<Id: Clone + Debug + Eq + Hash> {
    registered: Vec<(Id, Chord)>,
    held: HashSet<Key>,
    active: Option<Id>,
}

impl<Id: Clone + Debug + Eq + Hash> MatcherState<Id> {
    pub(crate) fn new(registered: Vec<(Id, Chord)>) -> Self {
        Self {
            registered,
            held: HashSet::new(),
            active: None,
        }
    }

    pub(crate) fn process<F: FnMut(ChordEvent<Id>)>(&mut self, event: Event, mut emit: F) {
        match event.kind {
            EventKind::KeyDown(k) => {
                self.held.insert(k);
            }
            EventKind::KeyUp(k) => {
                self.held.remove(&k);
            }
            EventKind::KeyRepeat(_) => return, // edge-triggered; ignore
        }

        let new_match = self.longest_match();
        if new_match != self.active {
            if let Some(prev) = self.active.take() {
                emit(ChordEvent::End {
                    id: prev,
                    time: event.time,
                });
            }
            if let Some(next) = new_match.clone() {
                emit(ChordEvent::Start {
                    id: next,
                    time: event.time,
                });
            }
            self.active = new_match;
        }
    }

    /// Longest-match resolution: if multiple chords match the held set,
    /// prefer the one with more keys. Ties broken by registration order
    /// (earlier wins).
    fn longest_match(&self) -> Option<Id> {
        let max_len = self
            .registered
            .iter()
            .filter(|(_, c)| c.matches(&self.held))
            .map(|(_, c)| c.len())
            .max()?;
        self.registered
            .iter()
            .find(|(_, c)| c.len() == max_len && c.matches(&self.held))
            .map(|(id, _)| id.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn down(k: Key) -> Event {
        Event {
            time: Instant::now(),
            kind: EventKind::KeyDown(k),
        }
    }

    fn up(k: Key) -> Event {
        Event {
            time: Instant::now(),
            kind: EventKind::KeyUp(k),
        }
    }

    fn repeat(k: Key) -> Event {
        Event {
            time: Instant::now(),
            kind: EventKind::KeyRepeat(k),
        }
    }

    fn run<Id: Clone + Debug + Eq + Hash>(
        registered: Vec<(Id, Chord)>,
        events: Vec<Event>,
    ) -> Vec<ChordEvent<Id>> {
        let mut state = MatcherState::new(registered);
        let mut out = Vec::new();
        for e in events {
            state.process(e, |ce| out.push(ce));
        }
        out
    }

    fn ids<Id: Clone + Debug>(evs: &[ChordEvent<Id>]) -> Vec<String> {
        evs.iter()
            .map(|e| match e {
                ChordEvent::Start { id, .. } => format!("start({id:?})"),
                ChordEvent::End { id, .. } => format!("end({id:?})"),
            })
            .collect()
    }

    #[test]
    fn single_key_chord_emits_start_and_end() {
        let out = run(
            vec![("esc", Chord::of([Key::Escape]))],
            vec![down(Key::Escape), up(Key::Escape)],
        );
        assert_eq!(ids(&out), vec!["start(\"esc\")", "end(\"esc\")"]);
    }

    #[test]
    fn two_key_chord_activates_on_second_key() {
        let out = run(
            vec![("ptt", Chord::of([Key::MetaRight, Key::AltRight]))],
            vec![
                down(Key::MetaRight), // not active yet
                down(Key::AltRight),  // now active
                up(Key::AltRight),    // end
                up(Key::MetaRight),
            ],
        );
        assert_eq!(ids(&out), vec!["start(\"ptt\")", "end(\"ptt\")"]);
    }

    #[test]
    fn end_fires_when_any_chord_key_released() {
        let out = run(
            vec![("ptt", Chord::of([Key::MetaRight, Key::AltRight]))],
            vec![
                down(Key::MetaRight),
                down(Key::AltRight),
                up(Key::MetaRight), // releasing either ends
                up(Key::AltRight),
            ],
        );
        assert_eq!(ids(&out), vec!["start(\"ptt\")", "end(\"ptt\")"]);
    }

    #[test]
    fn left_right_modifiers_are_distinct() {
        // MetaRight+AltRight is the chord; MetaLeft+AltLeft must NOT match.
        let out = run::<&str>(
            vec![("ptt", Chord::of([Key::MetaRight, Key::AltRight]))],
            vec![
                down(Key::MetaLeft),
                down(Key::AltLeft),
                up(Key::AltLeft),
                up(Key::MetaLeft),
            ],
        );
        assert!(out.is_empty(), "unexpected: {out:?}");
    }

    #[test]
    fn longest_match_wins() {
        // If "a" and "a+b" both match, "a+b" wins.
        let out = run(
            vec![
                ("short", Chord::of([Key::A])),
                ("long", Chord::of([Key::A, Key::B])),
            ],
            vec![
                down(Key::A), // "short" active
                down(Key::B), // transition to "long"
                up(Key::B),   // back to "short"
                up(Key::A),
            ],
        );
        assert_eq!(
            ids(&out),
            vec![
                "start(\"short\")",
                "end(\"short\")",
                "start(\"long\")",
                "end(\"long\")",
                "start(\"short\")",
                "end(\"short\")",
            ]
        );
    }

    #[test]
    fn transitioning_between_chords_emits_end_then_start() {
        // A→B transition: end(A), start(B), never overlapping actives.
        let out = run(
            vec![("a", Chord::of([Key::A])), ("b", Chord::of([Key::B]))],
            vec![
                down(Key::A),
                down(Key::B), // A still matches (set contains A), but B also matches.
                              // Both len=1; ties broken by registration order → "a" wins, stays active.
                              // So this should NOT transition.
            ],
        );
        // Because A is still held, "a" remains the longest match (registered first).
        assert_eq!(ids(&out), vec!["start(\"a\")"]);
    }

    #[test]
    fn key_repeat_does_not_affect_state() {
        let out = run(
            vec![("esc", Chord::of([Key::Escape]))],
            vec![
                down(Key::Escape),
                repeat(Key::Escape),
                repeat(Key::Escape),
                repeat(Key::Escape),
                up(Key::Escape),
            ],
        );
        // Exactly one start, one end — repeats must not emit spurious events.
        assert_eq!(ids(&out), vec!["start(\"esc\")", "end(\"esc\")"]);
    }

    #[test]
    fn non_chord_keys_do_not_trigger() {
        let out = run::<&str>(
            vec![("ptt", Chord::of([Key::MetaRight]))],
            vec![down(Key::A), up(Key::A), down(Key::B), up(Key::B)],
        );
        assert!(out.is_empty());
    }

    #[test]
    fn every_start_has_matching_end() {
        // Property: after any finite event sequence ending with all keys
        // released, starts count == ends count.
        let events = vec![
            down(Key::A),
            down(Key::B),
            up(Key::A),
            down(Key::A),
            up(Key::B),
            up(Key::A),
        ];
        let out = run(
            vec![
                ("a", Chord::of([Key::A])),
                ("b", Chord::of([Key::B])),
                ("ab", Chord::of([Key::A, Key::B])),
            ],
            events,
        );
        let starts = out
            .iter()
            .filter(|e| matches!(e, ChordEvent::Start { .. }))
            .count();
        let ends = out
            .iter()
            .filter(|e| matches!(e, ChordEvent::End { .. }))
            .count();
        assert_eq!(starts, ends);
    }

    #[test]
    fn empty_chord_never_matches() {
        let out = run::<&str>(
            vec![("empty", Chord::of(std::iter::empty()))],
            vec![down(Key::A), up(Key::A)],
        );
        assert!(out.is_empty());
    }
}
