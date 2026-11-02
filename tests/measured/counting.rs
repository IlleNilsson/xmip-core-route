//! The stand-in source a gathering is tested with, and the Message it
//! reads: what `gathering.rs`'s tests and the module's timing program both
//! promote. Shared, not copied: `.src/gathering.rs`'s tests take it with
//! `#[path]`, and the includer brings the route's and the Message's names
//! into scope for it (`super::*`).

use std::sync::atomic::{AtomicUsize, Ordering};

use super::{
    Content, Message, MessageContext, MessageId, MessageTreatment, Reading, ScalarValue, Source,
};

/// Reads `metadata:generation`, counting how often it compiles a name.
#[derive(Default)]
pub struct Counting(pub AtomicUsize);

/// The Message's generation, read.
pub struct Generation;

/// Nothing, read.
pub struct Nothing;

impl Source for Counting {
    fn technology(&self) -> &'static str {
        "metadata"
    }

    fn compile(&self, name: &str) -> Result<Box<dyn Reading>, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        match name {
            "generation" => Ok(Box::new(Generation)),
            "nothing" => Ok(Box::new(Nothing)),
            _ => Err("not a thing a Message has".to_string()),
        }
    }
}

impl Reading for Generation {
    fn read(&self, message: &Message, _: Option<&Content<'_>>) -> Result<Option<String>, String> {
        Ok(Some(message.generation().to_string()))
    }
}

impl Reading for Nothing {
    fn read(&self, _: &Message, _: Option<&Content<'_>>) -> Result<Option<String>, String> {
        Ok(None)
    }
}

/// A Message of type `Order`.
pub fn message() -> Message {
    let context =
        MessageContext::new().with_value("MessageType", ScalarValue::Text("Order".into()));
    Message::received(
        MessageId::new(1),
        Vec::new(),
        context,
        MessageTreatment::default(),
    )
}
