//! The promoted properties of one Message, as routing sees them.
//!
//! Matching reads this and nothing else, which is what makes it a pure function
//! rather than a query — see the crate documentation, point 1.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use xmip_context::{ContextValue, MessageContext};

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
                ContextValue::Text(text) => set.set(key, text.clone()),
                ContextValue::Bool(flag) => set.set(key, flag.to_string()),
                ContextValue::Integer(number) => set.set(key, number.to_string()),
                ContextValue::Decimal(number) => set.set(key, number.to_string()),
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

