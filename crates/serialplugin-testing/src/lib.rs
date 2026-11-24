// Its snippets run as doctests of serialplugin-core, which one of them uses.
#![cfg_attr(not(doctest), doc = include_str!("../README.md"))]
#![warn(missing_docs)]
//!
//! ## Independence
//!
//! Written from the protocol descriptions, not with the codecs under test, so a codec bug
//! cannot hide behind the same bug in its test peer.

mod clock;
mod fake;
mod modem;
pub mod mux;
pub mod peer;

#[cfg(feature = "tokio")]
pub use clock::TokioClock;
pub use clock::{Clock, Job, ManualClock, SystemClock};
pub use fake::{Call, FakeDevice, FakePort, Faults, Lines};
pub use modem::{Command, Modem, ModemLog, MuxOptions};

use std::time::{Duration, Instant};

/// Bound of every wait in the tests. It only limits how long a failure takes: waits end on
/// the event they wait for.
pub const WAIT: Duration = Duration::from_secs(10);

/// Poll `done` (a state no event announces) until it holds; panics with `what` after
/// [`WAIT`].
///
/// It looks every millisecond. That is a polling interval, not a delay anything depends on.
pub fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "never: {what}");
        std::thread::park_timeout(Duration::from_millis(1));
    }
}
