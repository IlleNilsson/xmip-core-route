//! The promoted properties of one Message, as routing sees them.
//!
//! Matching reads this and nothing else, which is what makes it a pure function
//! rather than a query — see the crate documentation, point 1.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use context::ContextValue;
use path::expression::Names;

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

/// A filter reads its names here, and a name nothing promoted is unknown
/// with that reason.
impl Names for Promoted {
    fn value(&self, name: &str) -> Option<&str> {
        self.get(name)
    }

    fn absent(&self, name: &str) -> String {
        format!("nothing promoted {name}")
    }
}

/// The one reading of a value a filter names, whichever spelling named it:
/// `X`, `context:X`, `header:`, `party:`, `regex:` and `content:` all read
/// through this. A value that is not there and a `Null` are absent: nothing
/// is promoted, so `exists` fails and no comparison matches, not even one
/// with empty text. A `Binary` value is refused, with the reason as the
/// error, because bytes are not text. Anything else is its one rendering,
/// `ScalarValue::text` (foundation/core): text as it is, a boolean and a
/// number as they are written. `key` is what the reason names. ADR-0046,
/// amended 2026-09-24.
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
        Some(value) => Ok(value.text().map(std::borrow::Cow::into_owned)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
