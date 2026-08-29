//! A value a Subscription compares against.
//!
//! **Promoted values are never guessed.** A promoted value is read as the type
//! the Subscription states, and coercion happens there, in the open. Inferring
//! that "0012345" is the number 12345 loses a leading zero, and an order number
//! with it.

use serde::{Deserialize, Serialize};

/// A value a Subscription compares against.
///
/// Three variants deliberately. Decimal is absent until a Contract needs one,
/// because floating point is the wrong answer for money and a wrong decimal is
/// worse than an absent one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Value {
    Text(String),
    Integer(i64),
    Boolean(bool),
}

impl Value {
    /// The name of this value's type, for explanations.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Value::Text(_) => "text",
            Value::Integer(_) => "integer",
            Value::Boolean(_) => "boolean",
        }
    }

    /// Render for an explanation, quoted so an empty value is still visible.
    #[must_use]
    pub fn show(&self) -> String {
        match self {
            Value::Text(text) => format!("'{text}'"),
            Value::Integer(number) => number.to_string(),
            Value::Boolean(flag) => flag.to_string(),
        }
    }
}

/// Read a promoted text value as the type the Subscription asked for.
///
/// The Subscription carries the intent, so this never guesses. When the text
/// will not read as that type the failure names both, which is the point:
/// "Amount is 'about ten', which is not an integer" is a fixable sentence and
/// "no match" is not.
pub(crate) fn coerce(promoted: &str, wanted: &Value, property: &str) -> Result<Value, String> {
    match wanted {
        Value::Text(_) => Ok(Value::Text(promoted.to_string())),
        Value::Integer(_) => promoted
            .trim()
            .parse::<i64>()
            .map(Value::Integer)
            .map_err(|_| format!("{property} is '{promoted}', which is not an integer")),
        Value::Boolean(_) => match promoted.trim().to_ascii_lowercase().as_str() {
            "true" => Ok(Value::Boolean(true)),
            "false" => Ok(Value::Boolean(false)),
            _ => Err(format!(
                "{property} is '{promoted}', which is not true or false"
            )),
        },
    }
}

