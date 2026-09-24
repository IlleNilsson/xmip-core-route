//! Where a Subscription's filter reads from — the route technologies.
//!
//! A filter names properties. A property with no prefix is a context value,
//! read through [`crate::routable`] exactly as `context:` reads it: missing
//! and `Null` are absent, bytes are refused (ADR-0046, amended 2026-09-24).
//! A property with one —
//! `content:order.total`, `header:content-type`, `metadata:generation`,
//! `party:sender`, `contract:name`, `expression:over-limit`,
//! `regex:invoice-number` — is read by the technology of that name, and the
//! eight technologies are the eight ways a Message can be asked. ADR-0046.
//!
//! The capability owns the split and the gathering; a technology owns one
//! reading. Nothing here knows what any prefix means.

use message::Message;

use crate::{Promoted, routable};

/// The prefix a property carries when a technology reads it.
const SEPARATOR: char = ':';

/// The technology a property with no prefix belongs to.
pub const CONTEXT: &str = "context";

/// A route technology: reads named values from a Message for the filter.
pub trait Source: Send + Sync {
    /// The manifest leaf, and the prefix a property carries: `content`,
    /// `header`, `metadata`, `party`, `contract`, `expression`, `regex`, or
    /// `context` for the one that reads what the others do not.
    fn technology(&self) -> &'static str;

    /// The value of `name` for this Message — the part after the prefix —
    /// or `None` when the Message has no such thing. A filter treats `None`
    /// as nothing promoted, which is a decline with a reason, not an error.
    ///
    /// # Errors
    /// The name is not one this technology can read, or the Message's content
    /// cannot be read the way the name asks.
    fn read(&self, message: &Message, name: &str) -> Result<Option<String>, SourceError>;
}

/// Why a source could not answer a property.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceError {
    pub technology: String,
    pub property: String,
    pub reason: String,
}

impl SourceError {
    #[must_use]
    pub fn new(
        technology: impl Into<String>,
        property: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            technology: technology.into(),
            property: property.into(),
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.technology, self.property, self.reason)
    }
}

impl std::error::Error for SourceError {}

/// A property split into the technology that reads it and the name it reads.
/// No prefix is context, and the name is the whole property.
#[must_use]
pub fn split(property: &str) -> (&str, &str) {
    match property.split_once(SEPARATOR) {
        Some((technology, name)) if !technology.is_empty() => (technology, name),
        _ => (CONTEXT, property),
    }
}

/// Promote every property the filters name, each from the source whose
/// technology it carries. Context is read from the Message itself, through
/// [`routable`], when no source claims it, so a set of sources may be empty
/// and a filter over context alone still works.
///
/// # Errors
/// A property names a technology no source provides, a source refused it, or
/// it names a context value that holds bytes.
pub fn promote(
    message: &Message,
    sources: &[&dyn Source],
    properties: &[&str],
) -> Result<Promoted, SourceError> {
    let mut promoted = Promoted::from_context(message.context());

    for property in properties {
        let (technology, name) = split(property);
        let source = sources
            .iter()
            .find(|source| source.technology() == technology);

        let value = match source {
            Some(source) => source.read(message, name)?,
            None if technology == CONTEXT => routable(name, message.context().get(name))
                .map_err(|reason| SourceError::new(CONTEXT, name, reason))?,
            None => {
                return Err(SourceError::new(
                    technology,
                    *property,
                    "no route technology of that name is loaded",
                ));
            }
        };

        if let Some(value) = value {
            promoted = promoted.set(*property, value);
        }
    }

    Ok(promoted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Predicate, Value};
    use context::{ContextValue, MessageContext};
    use message::MessageTreatment;
    use xcore::MessageId;

    struct Counting;

    impl Source for Counting {
        fn technology(&self) -> &'static str {
            "metadata"
        }

        fn read(&self, message: &Message, name: &str) -> Result<Option<String>, SourceError> {
            match name {
                "generation" => Ok(Some(message.generation().to_string())),
                "nothing" => Ok(None),
                other => Err(SourceError::new(
                    "metadata",
                    other,
                    "not a thing a Message has",
                )),
            }
        }
    }

    fn message() -> Message {
        let context =
            MessageContext::new().with_value("MessageType", ContextValue::Text("Order".into()));
        Message::received(
            MessageId::new(1),
            Vec::new(),
            context,
            MessageTreatment::default(),
        )
    }

    #[test]
    fn a_property_without_a_prefix_is_context_and_with_one_is_its_technology() {
        assert_eq!(split("MessageType"), ("context", "MessageType"));
        assert_eq!(split("content:order.total"), ("content", "order.total"));
        assert_eq!(split(":odd"), ("context", ":odd"));
    }

    #[test]
    fn context_is_promoted_with_no_source_and_a_technology_adds_what_it_reads() {
        let sources: [&dyn Source; 1] = [&Counting];
        let promoted = promote(
            &message(),
            &sources,
            &["MessageType", "metadata:generation"],
        )
        .expect("both readable");
        assert_eq!(promoted.get("MessageType"), Some("Order"));
        assert_eq!(promoted.get("metadata:generation"), Some("0"));

        let none = promote(&message(), &[], &["MessageType"]).expect("context alone");
        assert_eq!(none.get("MessageType"), Some("Order"));
    }

    #[test]
    fn a_bare_property_reads_missing_and_null_as_absent_and_refuses_bytes() {
        let context = MessageContext::new()
            .with_value("MessageType", ContextValue::Text("Order".into()))
            .with_value("Note", ContextValue::Null)
            .with_value("Blob", ContextValue::Binary(vec![0, 1, 2]));
        let message = Message::received(
            MessageId::new(2),
            Vec::new(),
            context,
            MessageTreatment::default(),
        );

        let promoted =
            promote(&message, &[], &["MessageType", "Note", "Region"]).expect("readable");
        assert_eq!(promoted.get("MessageType"), Some("Order"));
        assert_eq!(promoted.get("Note"), None);
        assert_eq!(promoted.get("Region"), None);
        assert!(Predicate::exists("MessageType").test(&promoted).passed());
        assert!(!Predicate::exists("Note").test(&promoted).passed());
        assert!(!Predicate::exists("Region").test(&promoted).passed());
        assert!(
            !Predicate::equals("Note", Value::Text(String::new()))
                .test(&promoted)
                .passed()
        );

        let refused = promote(&message, &[], &["Blob"]).expect_err("bytes");
        assert_eq!(refused.technology, "context");
        assert_eq!(refused.property, "Blob");
        assert!(refused.reason.contains("3 bytes"));
    }

    #[test]
    fn nothing_read_is_nothing_promoted_and_a_missing_technology_is_an_error() {
        let sources: [&dyn Source; 1] = [&Counting];
        let promoted = promote(&message(), &sources, &["metadata:nothing"]).expect("readable");
        assert_eq!(promoted.get("metadata:nothing"), None);

        let missing = promote(&message(), &sources, &["party:sender"]).expect_err("no party");
        assert_eq!(missing.technology, "party");
        assert!(missing.to_string().contains("no route technology"));

        let refused = promote(&message(), &sources, &["metadata:colour"]).expect_err("refused");
        assert_eq!(refused.property, "colour");
    }
}
