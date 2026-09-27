//! Where a Subscription's filter reads from — the route technologies.
//!
//! A filter names properties. A property with no prefix is a context value,
//! read through [`crate::routable`] exactly as `context:` reads it: missing
//! and `Null` are absent, bytes are refused (ADR-0046, amended 2026-09-24).
//! A property with one —
//! `content:dot:order.total`, `header:http.content-type`,
//! `metadata:generation`, `party:sender`, `contract:name`,
//! `regex:OrderNo:<pattern>` — is read by the technology of that name, and
//! the seven technologies are the seven ways a Message can be asked.
//! ADR-0046; the eighth, `expression`, went when a filter became an
//! expression itself (ADR-0066).
//!
//! A technology compiles the name it is given once, when configuration is
//! read, and the compiled [`Reading`] answers every Message after that
//! without parsing it again (ADR-0046, amended 2026-09-27). The capability
//! owns the split and the gathering ([`crate::Gathering`]); a technology owns
//! one reading. Nothing here knows what any prefix means.

use message::Message;
use path::Content;

/// The prefix a property carries when a technology reads it.
const SEPARATOR: char = ':';

/// The technology a property with no prefix belongs to.
pub const CONTEXT: &str = "context";

/// A route technology: compiles the names a filter reads from a Message.
pub trait Source: Send + Sync {
    /// The manifest leaf, and the prefix a property carries: `content`,
    /// `header`, `metadata`, `party`, `contract`, `regex`, or
    /// `context` for the one that reads what the others do not.
    fn technology(&self) -> &'static str;

    /// `name` — the part after the prefix — compiled, once.
    ///
    /// # Errors
    /// The name is not one this technology can read, with the reason.
    fn compile(&self, name: &str) -> Result<Box<dyn Reading>, String>;
}

/// One compiled name, read from every Message.
pub trait Reading: Send + Sync {
    /// The value for `message`, or `None` when the Message has no such thing.
    /// A filter treats `None` as nothing promoted, which is a decline with a
    /// reason, not an error. `content` is the first section's, parsed at most
    /// once per form for every reading of the Message; `None` when the
    /// Message has no section.
    ///
    /// # Errors
    /// The Message's content or context cannot be read the way the name
    /// asks, with the reason.
    fn read(
        &self,
        message: &Message,
        content: Option<&Content<'_>>,
    ) -> Result<Option<String>, String>;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_property_without_a_prefix_is_context_and_with_one_is_its_technology() {
        assert_eq!(split("MessageType"), ("context", "MessageType"));
        assert_eq!(split("content:order.total"), ("content", "order.total"));
        assert_eq!(split(":odd"), ("context", ":odd"));
    }

    #[test]
    fn a_source_error_names_the_technology_and_the_property() {
        let error = SourceError::new("regex", "OrderNo:(", "does not compile");
        assert_eq!(error.to_string(), "regex:OrderNo:(: does not compile");
    }
}
