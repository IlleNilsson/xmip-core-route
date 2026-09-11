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
    /// the Subscription is what states the type. Binary values are left out:
    /// there is no reading of arbitrary bytes as text that is right often
    /// enough to be worth being wrong the rest of the time.
    #[must_use]
    pub fn from_context(context: &MessageContext) -> Self {
        context
            .iter()
            .fold(Self::new(), |set, (key, value)| match value {
                ContextValue::Binary(_) => set,
                ContextValue::Null => set.set(key, ""),
                other => set.set(key, text_of(other).unwrap_or_default()),
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
}
