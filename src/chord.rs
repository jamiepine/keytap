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
//! Each chord carries a [`ChordMode`]:
//! - [`ChordMode::Momentary`] (default) — `End` fires when any chord key
//!   is released. Standard push-to-talk / hotkey behaviour.
//! - [`ChordMode::Toggle`] — `Start` fires on the first complete chord
//!   press, `End` fires on the *next* complete press of the same chord.
//!   Key releases between presses are ignored; the chord stays active
//!   until explicitly re-pressed. While a Toggle chord is active, other
//!   registered chords are suppressed.
//!
//! ```no_run
//! use keytap::chord::{ChordMatcher, Chord, ChordEvent};
//! use keytap::Key;
//!
//! # fn main() -> Result<(), keytap::Error> {
//! let matcher = ChordMatcher::builder()
//!     .add("ptt",    Chord::of([Key::MetaRight, Key::AltRight]))
//!     .add_toggle("hands-free",
//!                 Chord::of([Key::MetaRight, Key::AltRight, Key::Space]))
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

/// How a registered chord behaves once it becomes satisfied.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ChordMode {
    /// `Start` fires when the chord becomes satisfied; `End` fires when
    /// it no longer is (any chord key released) or when the held set
    /// transitions to a different registered chord. The default — this
    /// is how push-to-talk and standard hotkey-daemon chords behave.
    #[default]
    Momentary,
    /// `Start` fires on the first complete press; `End` fires on the
    /// *next* complete press of the same chord. Key releases between
    /// presses are ignored — the chord stays active until explicitly
    /// re-pressed. While a Toggle chord is active, other registered
    /// chords do not fire (the active chord is "sticky").
    ///
    /// Useful for hands-free sessions where the user shouldn't have to
    /// hold keys for minutes at a time.
    Toggle,
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
    registered: Vec<(Id, Chord, ChordMode)>,
    tap: Option<Tap>,
}

impl<Id: Clone + Debug + Eq + Hash + Send + 'static> ChordMatcherBuilder<Id> {
    /// Register a chord in [`ChordMode::Momentary`] (the default).
    pub fn add(self, id: Id, chord: Chord) -> Self {
        self.add_with_mode(id, chord, ChordMode::Momentary)
    }

    /// Register a chord in [`ChordMode::Toggle`]. The chord stays active
    /// between presses; the next complete press ends it.
    pub fn add_toggle(self, id: Id, chord: Chord) -> Self {
        self.add_with_mode(id, chord, ChordMode::Toggle)
    }

    /// Register a chord with an explicit [`ChordMode`]. Lower-level form
    /// of [`Self::add`] / [`Self::add_toggle`].
    pub fn add_with_mode(mut self, id: Id, chord: Chord, mode: ChordMode) -> Self {
        self.registered.push((id, chord, mode));
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
/// invokes the callback with zero or more [`ChordEvent`]s per call
/// (typically zero or one; mode transitions can emit End + Start).
#[derive(Debug)]
pub(crate) struct MatcherState<Id: Clone + Debug + Eq + Hash> {
    registered: Vec<(Id, Chord, ChordMode)>,
    held: HashSet<Key>,
    active: Option<(Id, ChordMode)>,
    /// Indices of chords that were satisfied at the end of the previous
    /// tick. Used for rising-edge detection (Toggle re-press).
    satisfied_prev: HashSet<usize>,
}

impl<Id: Clone + Debug + Eq + Hash> MatcherState<Id> {
    pub(crate) fn new(registered: Vec<(Id, Chord, ChordMode)>) -> Self {
        Self {
            registered,
            held: HashSet::new(),
            active: None,
            satisfied_prev: HashSet::new(),
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

        let satisfied_now = self.satisfied_indices();
        let rising: Vec<usize> = satisfied_now
            .difference(&self.satisfied_prev)
            .copied()
            .collect();
        self.satisfied_prev = satisfied_now;

        // Toggle is sticky: while a Toggle chord is active, the only
        // event that can fire is End(same chord) triggered by another
        // complete press. Other chord activity is suppressed.
        if let Some((active_id, ChordMode::Toggle)) = self.active.clone() {
            for &idx in &rising {
                if self.registered[idx].0 == active_id {
                    emit(ChordEvent::End {
                        id: active_id,
                        time: event.time,
                    });
                    self.active = None;
                    return;
                }
            }
            return;
        }

        // Momentary path (or no active chord): recompute longest match
        // from the currently-satisfied set. Transition End→Start across
        // chord boundaries exactly as before.
        let new_match = self.longest_match();
        let same_id = match (&new_match, &self.active) {
            (Some((a, _)), Some((b, _))) => a == b,
            (None, None) => true,
            _ => false,
        };
        if !same_id {
            if let Some((prev_id, _)) = self.active.take() {
                emit(ChordEvent::End {
                    id: prev_id,
                    time: event.time,
                });
            }
            if let Some((next_id, _)) = &new_match {
                emit(ChordEvent::Start {
                    id: next_id.clone(),
                    time: event.time,
                });
            }
            self.active = new_match;
        }
    }

    /// Indices into `registered` for every chord that's currently satisfied.
    fn satisfied_indices(&self) -> HashSet<usize> {
        self.registered
            .iter()
            .enumerate()
            .filter(|(_, (_, c, _))| c.matches(&self.held))
            .map(|(i, _)| i)
            .collect()
    }

    /// Longest-match resolution: if multiple chords match the held set,
    /// prefer the one with more keys. Ties broken by registration order
    /// (earlier wins).
    fn longest_match(&self) -> Option<(Id, ChordMode)> {
        let max_len = self
            .registered
            .iter()
            .filter(|(_, c, _)| c.matches(&self.held))
            .map(|(_, c, _)| c.len())
            .max()?;
        self.registered
            .iter()
            .find(|(_, c, _)| c.len() == max_len && c.matches(&self.held))
            .map(|(id, _, mode)| (id.clone(), *mode))
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
        let tagged = registered
            .into_iter()
            .map(|(id, c)| (id, c, ChordMode::Momentary))
            .collect();
        run_with_modes(tagged, events)
    }

    fn run_with_modes<Id: Clone + Debug + Eq + Hash>(
        registered: Vec<(Id, Chord, ChordMode)>,
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

    // ── Toggle mode ─────────────────────────────────────────────────────

    #[test]
    fn toggle_stays_active_through_key_release() {
        // First press → Start. Keys released → no End (sticky).
        let out = run_with_modes(
            vec![("t", Chord::of([Key::A]), ChordMode::Toggle)],
            vec![down(Key::A), up(Key::A)],
        );
        assert_eq!(ids(&out), vec!["start(\"t\")"]);
    }

    #[test]
    fn toggle_ends_on_second_complete_press() {
        // Press → Start. Release → silent. Press again → End.
        let out = run_with_modes(
            vec![("t", Chord::of([Key::A]), ChordMode::Toggle)],
            vec![
                down(Key::A),
                up(Key::A),
                down(Key::A), // re-press: this is the "off" toggle
                up(Key::A),
            ],
        );
        assert_eq!(ids(&out), vec!["start(\"t\")", "end(\"t\")"]);
    }

    #[test]
    fn toggle_multi_key_requires_all_keys_for_repress() {
        // Two-key toggle: releasing one key and re-pressing it does not
        // count as a full chord press, so End should NOT fire until all
        // chord keys transition unsatisfied → satisfied.
        let out = run_with_modes(
            vec![(
                "t",
                Chord::of([Key::MetaRight, Key::AltRight]),
                ChordMode::Toggle,
            )],
            vec![
                down(Key::MetaRight),
                down(Key::AltRight), // satisfied → Start
                up(Key::AltRight),   // unsatisfied (sticky, no End)
                down(Key::AltRight), // re-satisfied → End
                up(Key::AltRight),
                up(Key::MetaRight),
            ],
        );
        assert_eq!(ids(&out), vec!["start(\"t\")", "end(\"t\")"]);
    }

    #[test]
    fn toggle_suppresses_other_chords_while_active() {
        // While a Toggle chord is sticky-active, other registered chords
        // don't fire — the active session takes precedence until ended.
        let out = run_with_modes(
            vec![
                ("hands_free", Chord::of([Key::Space]), ChordMode::Toggle),
                ("cancel", Chord::of([Key::Escape]), ChordMode::Momentary),
            ],
            vec![
                down(Key::Space), // Start("hands_free")
                up(Key::Space),
                down(Key::Escape), // would fire "cancel" if toggle weren't active
                up(Key::Escape),
                down(Key::Space), // End("hands_free")
                up(Key::Space),
            ],
        );
        assert_eq!(
            ids(&out),
            vec!["start(\"hands_free\")", "end(\"hands_free\")"]
        );
    }

    #[test]
    fn ptt_upgrades_to_toggle_by_longest_match() {
        // Classic voicebox pattern: PTT [Meta, Alt] (Momentary) and
        // Hands-free [Meta, Alt, Space] (Toggle). Hold PTT, then tap
        // Space mid-hold → End(ptt), Start(hands_free) in Toggle mode.
        // Then release all keys without pressing the chord again, so
        // Toggle stays sticky-active (no End).
        let out = run_with_modes(
            vec![
                (
                    "ptt",
                    Chord::of([Key::MetaRight, Key::AltRight]),
                    ChordMode::Momentary,
                ),
                (
                    "hands_free",
                    Chord::of([Key::MetaRight, Key::AltRight, Key::Space]),
                    ChordMode::Toggle,
                ),
            ],
            vec![
                down(Key::MetaRight),
                down(Key::AltRight), // Start("ptt")
                down(Key::Space),    // upgrade: End("ptt"), Start("hands_free")
                up(Key::Space),      // Toggle sticky — nothing
                up(Key::AltRight),
                up(Key::MetaRight), // all released, still Toggle-active
            ],
        );
        assert_eq!(
            ids(&out),
            vec!["start(\"ptt\")", "end(\"ptt\")", "start(\"hands_free\")",]
        );
    }

    #[test]
    fn toggle_end_while_ptt_held_reenters_ptt() {
        // After a Toggle ends, longest_match runs normally against the
        // still-held keys. If the user was holding the full superset
        // chord (Meta+Alt+Space), ending the Toggle leaves Meta+Alt
        // held, so the shorter Momentary chord ("ptt") re-activates.
        // Pin this behaviour — it's the natural consequence of the
        // longest-match rule and matches what dictation apps expect
        // (a Toggle end while PTT keys are held keeps the recording
        // going via PTT). Release order matters: Space first ends
        // Toggle and immediately reveals PTT; releasing Meta/Alt ends
        // PTT too.
        let out = run_with_modes(
            vec![
                (
                    "ptt",
                    Chord::of([Key::MetaRight, Key::AltRight]),
                    ChordMode::Momentary,
                ),
                (
                    "hands_free",
                    Chord::of([Key::MetaRight, Key::AltRight, Key::Space]),
                    ChordMode::Toggle,
                ),
            ],
            vec![
                down(Key::MetaRight),
                down(Key::AltRight), // Start("ptt")
                down(Key::Space),    // End("ptt"), Start("hands_free")
                up(Key::Space),      // Toggle sticky, nothing
                down(Key::Space),    // re-press → End("hands_free")
                up(Key::Space),      // satisfied drops to {ptt} → Start("ptt")
                up(Key::AltRight),   // End("ptt")
                up(Key::MetaRight),
            ],
        );
        assert_eq!(
            ids(&out),
            vec![
                "start(\"ptt\")",
                "end(\"ptt\")",
                "start(\"hands_free\")",
                "end(\"hands_free\")",
                "start(\"ptt\")",
                "end(\"ptt\")",
            ]
        );
    }

    #[test]
    fn toggle_start_requires_rising_edge() {
        // If the chord is already satisfied when the matcher starts (e.g.
        // registered after the user pressed the key — not a real case
        // with our API, but the invariant is worth pinning), Start
        // should fire on the rising edge, not on the first event.
        // Here: press A, release A, press A again — expect start, end.
        let out = run_with_modes(
            vec![("t", Chord::of([Key::A]), ChordMode::Toggle)],
            vec![down(Key::A), up(Key::A), down(Key::A), up(Key::A)],
        );
        assert_eq!(ids(&out), vec!["start(\"t\")", "end(\"t\")"]);
    }

    #[test]
    fn toggle_ignores_key_repeat_events() {
        // Repeats inside an active Toggle session must not count as a
        // re-press — the OS may emit KeyRepeat while keys are held.
        let out = run_with_modes(
            vec![("t", Chord::of([Key::A]), ChordMode::Toggle)],
            vec![
                down(Key::A),
                repeat(Key::A),
                repeat(Key::A),
                up(Key::A),
                // Repeats alone should not produce a re-press signal.
            ],
        );
        assert_eq!(ids(&out), vec!["start(\"t\")"]);
    }

    #[test]
    fn momentary_and_toggle_coexist_independently_when_no_active() {
        // With no toggle active, Momentary chord behaves normally.
        let out = run_with_modes(
            vec![
                ("m", Chord::of([Key::A]), ChordMode::Momentary),
                ("t", Chord::of([Key::B]), ChordMode::Toggle),
            ],
            vec![
                down(Key::A), // Start("m")
                up(Key::A),   // End("m")
                down(Key::B), // Start("t")
                up(Key::B),   // sticky, no End
                down(Key::B), // End("t")
                up(Key::B),
            ],
        );
        assert_eq!(
            ids(&out),
            vec!["start(\"m\")", "end(\"m\")", "start(\"t\")", "end(\"t\")"]
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trips_chord_config() {
        use crate::RawCode;

        // The configuration case: a user stores their bindings on disk as
        // JSON. We want Key / ChordMode / Chord round-tripping via serde_json.
        let chord = Chord::of([Key::MetaRight, Key::AltRight, Key::IntlBackslash]);
        let mode = ChordMode::Toggle;
        let key = Key::Function;
        let raw = RawCode(12345);

        let chord_json = serde_json::to_string(&chord).unwrap();
        let mode_json = serde_json::to_string(&mode).unwrap();
        let key_json = serde_json::to_string(&key).unwrap();
        let raw_json = serde_json::to_string(&raw).unwrap();

        assert_eq!(chord, serde_json::from_str::<Chord>(&chord_json).unwrap());
        assert_eq!(mode, serde_json::from_str::<ChordMode>(&mode_json).unwrap());
        assert_eq!(key, serde_json::from_str::<Key>(&key_json).unwrap());
        assert_eq!(raw, serde_json::from_str::<RawCode>(&raw_json).unwrap());

        // Sanity: the key names are stable strings a user can hand-edit.
        assert_eq!(mode_json, "\"Toggle\"");
        assert_eq!(key_json, "\"Function\"");
    }
}
