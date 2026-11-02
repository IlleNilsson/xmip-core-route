//! The module's timing program (`doc/governance/rust-style.md` section
//! 5c): routing held to a time. A plain `cargo test` leaves it out; the
//! landing runs it after every other test of the module, alone, one test
//! at a time (`cargo test --test measured --all-features --
//! --test-threads=1`), so what is measured is Xmip's work and not its
//! neighbors':
//!
//! - a promoted name is read from every Message in microseconds, over the
//!   stand-in source `gathering.rs`'s tests use (`counting.rs`, shared).

#[allow(dead_code)]
mod counting;

use context::MessageContext;
use message::{Message, MessageTreatment};
use path::Content;
use xcore::{MessageId, ScalarValue};
use xmip_core_route::{Gathering, Reading, Source};

use counting::{Counting, message};

#[test]
fn a_name_is_read_from_every_message_in_microseconds() {
    let counting = Counting::default();
    let sources: [&dyn Source; 1] = [&counting];
    let gathering = Gathering::new(&sources, &["metadata:generation", "MessageType"]);
    let message = message();
    let started = std::time::Instant::now();
    for _ in 0..10_000 {
        let promoted = gathering.promote(&message).expect("readable");
        assert_eq!(promoted.len(), 2);
    }
    let each = started.elapsed() / 10_000;
    // Generous for a debug build under load; a millisecond is the defect.
    assert!(each.as_micros() < 200, "{each:?} per Message");
}
