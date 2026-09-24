//! The promoted properties of one Message, as routing sees them.
//!
//! Matching reads this and nothing else, which is what makes it a pure function
//! rather than a query — see the crate documentation, point 1.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use context::{ContextValue, MessageContext};

/// The promoted properties of one Message, as routing sees them.
///
/// Ordered by name so an explanation reads the same way twice, and so two sets
/// with the same content are the same set.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Promoted {
    values: BTreeMap<String, String>,
}

impl Promoted {
    #[must_use]
    pub fn new() -> Self {
        Self {
            values: BTreeMap::new(),
        }
    }

    /// Add one property. A later promotion of the same name replaces an earlier
    /// one, because the last handler to speak is the one that knew most.
    #[must_use]
    pub fn set(mut self, property: impl Into<String>, value: impl Into<String>) -> Self {
        self.values.insert(property.into(), value.into());
        self
    }

    /// Take the promoted set from a Message's Context.
    ///
    /// Everything arrives as text, because text is what came off the wire and
    /// the Subscription is what states the type. Each value is read through
    /// [`routable`], so a `Null` is absent and promotes nothing. A `Binary`
    /// value is left out here, because gathering reads every key and no
    /// filter need name that one; a filter that does name it is refused by
    /// [`crate::promote`], which reads the names the filters use. ADR-0046,
    /// amended 2026-09-24.
    #[must_use]
    pub fn from_context(context: &MessageContext) -> Self {
        context.iter().fold(Self::new(), |set, (key, value)| {
            match routable(key, Some(value)) {
                Ok(Some(text)) => set.set(key, text),
                Ok(None) | Err(_) => set,
            }
        })
    }

    #[must_use]
    pub fn get(&self, property: &str) -> Option<&str> {
        self.values.get(property).map(String::as_str)
    }

    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.values.keys().map(String::as_str).collect()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }
}

/// A context value as the text a filter compares, the one rendering every
/// route technology uses (ADR-0044): text as it is, a boolean and a number as
/// they are written. `None` for Null and for Binary, which have no text that
/// would be right often enough to be worth being wrong the rest of the time.
#[must_use]
pub fn text_of(value: &ContextValue) -> Option<String> {
    match value {
        ContextValue::Text(text) => Some(text.clone()),
        ContextValue::Bool(flag) => Some(flag.to_string()),
        ContextValue::Integer(number) => Some(number.to_string()),
        ContextValue::Decimal(number) => Some(number.to_string()),
        ContextValue::Null | ContextValue::Binary(_) => None,
    }
}

/// The one reading of a value a filter names, whichever spelling named it:
/// `X`, `context:X`, `header:`, `party:`, `regex:` and `content:` all read
/// through this. A value that is not there and a `Null` are absent: nothing
/// is promoted, so `exists` fails and no comparison matches, not even one
/// with empty text. A `Binary` value is refused, with the reason as the
/// error, because bytes are not text. Anything else is [`text_of`]. `key` is
/// what the reason names. ADR-0046, amended 2026-09-24.
///
/// # Errors
/// The value is `Binary`.
pub fn routable(key: &str, value: Option<&ContextValue>) -> Result<Option<String>, String> {
    match value {
        None | Some(ContextValue::Null) => Ok(None),
        Some(ContextValue::Binary(bytes)) => Err(format!(
            "{key} holds {} bytes, and bytes are not routable as text",
            bytes.len()
        )),
        Some(value) => Ok(text_of(value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_of_renders_what_a_filter_compares_and_declines_bytes_and_null() {
        assert_eq!(
            text_of(&ContextValue::Text("Order".into())).as_deref(),
            Some("Order")
        );
        assert_eq!(text_of(&ContextValue::Bool(true)).as_deref(), Some("true"));
        assert_eq!(text_of(&ContextValue::Integer(12)).as_deref(), Some("12"));
        assert_eq!(text_of(&ContextValue::Null), None);
        assert_eq!(text_of(&ContextValue::Binary(vec![1, 2])), None);
    }

    #[test]
    fn routable_reads_missing_and_null_as_absent_and_refuses_bytes() {
        assert_eq!(routable("Region", None), Ok(None));
        assert_eq!(routable("Note", Some(&ContextValue::Null)), Ok(None));
        assert_eq!(
            routable("Amount", Some(&ContextValue::Integer(1500))),
            Ok(Some("1500".into()))
        );
        assert_eq!(
            routable("Blob", Some(&ContextValue::Binary(vec![0, 1, 2]))),
            Err("Blob holds 3 bytes, and bytes are not routable as text".into())
        );
    }

    #[test]
    fn from_context_leaves_out_null_and_bytes() {
        let context = MessageContext::new()
            .with_value("MessageType", ContextValue::Text("Order".into()))
            .with_value("Note", ContextValue::Null)
            .with_value("Blob", ContextValue::Binary(vec![0]));
        assert_eq!(
            Promoted::from_context(&context).names(),
            vec!["MessageType"]
        );
    }
}
