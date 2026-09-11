//! A condition over the promoted set — a Subscription's filter.
//!
//! Every decision explains itself: a [`Test`] carries the reason when it fails,
//! so [`crate::Routing::declines`] can say why each Subscription passed on a
//! Message rather than leaving it to be inferred from its absence.

use serde::{Deserialize, Serialize};

use crate::value::coerce;
use crate::{Promoted, Value};

/// A condition over the promoted set — a Subscription's filter.
///
/// This is data, not code, so it can be written in an artifact, shipped,
/// compared, and explained. The leaf conditions carry named fields so the TOML
/// reads as configuration rather than as positional arguments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Predicate {
    /// The property was promoted at all, whatever it says.
    Exists {
        property: String,
    },
    Equals {
        property: String,
        value: Value,
    },
    NotEquals {
        property: String,
        value: Value,
    },
    GreaterThan {
        property: String,
        value: Value,
    },
    LessThan {
        property: String,
        value: Value,
    },
    StartsWith {
        property: String,
        prefix: String,
    },
    /// Every condition holds. An empty all holds, which is how a Subscription
    /// says "everything published here".
    All(Vec<Predicate>),
    /// At least one condition holds. An empty any never holds.
    Any(Vec<Predicate>),
    Not(Box<Predicate>),
}

impl Predicate {
    pub fn exists(property: impl Into<String>) -> Self {
        Self::Exists {
            property: property.into(),
        }
    }

    pub fn equals(property: impl Into<String>, value: Value) -> Self {
        Self::Equals {
            property: property.into(),
            value,
        }
    }

    pub fn not_equals(property: impl Into<String>, value: Value) -> Self {
        Self::NotEquals {
            property: property.into(),
            value,
        }
    }

    pub fn greater_than(property: impl Into<String>, value: Value) -> Self {
        Self::GreaterThan {
            property: property.into(),
            value,
        }
    }

    pub fn less_than(property: impl Into<String>, value: Value) -> Self {
        Self::LessThan {
            property: property.into(),
            value,
        }
    }

    pub fn starts_with(property: impl Into<String>, prefix: impl Into<String>) -> Self {
        Self::StartsWith {
            property: property.into(),
            prefix: prefix.into(),
        }
    }

    #[must_use]
    pub const fn all(parts: Vec<Predicate>) -> Self {
        Self::All(parts)
    }

    #[must_use]
    pub const fn any(parts: Vec<Predicate>) -> Self {
        Self::Any(parts)
    }

    /// Everything published here, with no condition at all.
    #[must_use]
    pub const fn everything() -> Self {
        Self::All(Vec::new())
    }

    /// Every property name this condition reads.
    ///
    /// Used by [`never_satisfiable`] to find a filter naming something no
    /// Contract will ever promote.
    #[must_use]
    pub fn referenced_names(&self) -> Vec<&str> {
        let mut found = Vec::new();
        self.collect_names(&mut found);
        found.sort_unstable();
        found.dedup();
        found
    }

    fn collect_names<'a>(&'a self, into: &mut Vec<&'a str>) {
        match self {
            Self::Exists { property }
            | Self::Equals { property, .. }
            | Self::NotEquals { property, .. }
            | Self::GreaterThan { property, .. }
            | Self::LessThan { property, .. }
            | Self::StartsWith { property, .. } => into.push(property.as_str()),
            Self::All(parts) | Self::Any(parts) => {
                for part in parts {
                    part.collect_names(into);
                }
            }
            Self::Not(inner) => inner.collect_names(into),
        }
    }

    /// Test this condition against one Message's promoted set.
    #[must_use]
    pub fn test(&self, promoted: &Promoted) -> Test {
        match self {
            Self::Exists { property } => match promoted.get(property) {
                Some(_) => Test::Pass,
                None => Test::Fail(nothing_promoted(property)),
            },

            Self::Equals { property, value } => compare(promoted, property, value, |actual| {
                if actual == value {
                    Test::Pass
                } else {
                    Test::Fail(format!(
                        "{property} is {}, not {}",
                        actual.show(),
                        value.show()
                    ))
                }
            }),

            Self::NotEquals { property, value } => compare(promoted, property, value, |actual| {
                if actual == value {
                    Test::Fail(format!("{property} is {}", value.show()))
                } else {
                    Test::Pass
                }
            }),

            Self::GreaterThan { property, value } => compare(promoted, property, value, |actual| {
                match order(actual, value) {
                    Some(std::cmp::Ordering::Greater) => Test::Pass,
                    Some(_) => Test::Fail(format!(
                        "{property} is {}, which is not over {}",
                        actual.show(),
                        value.show()
                    )),
                    None => Test::Fail(format!("{property} cannot be ordered against a boolean")),
                }
            }),

            Self::LessThan { property, value } => compare(promoted, property, value, |actual| {
                match order(actual, value) {
                    Some(std::cmp::Ordering::Less) => Test::Pass,
                    Some(_) => Test::Fail(format!(
                        "{property} is {}, which is not under {}",
                        actual.show(),
                        value.show()
                    )),
                    None => Test::Fail(format!("{property} cannot be ordered against a boolean")),
                }
            }),

            Self::StartsWith { property, prefix } => match promoted.get(property) {
                None => Test::Fail(nothing_promoted(property)),
                Some(actual) if actual.starts_with(prefix.as_str()) => Test::Pass,
                Some(actual) => Test::Fail(format!(
                    "{property} is '{actual}', which does not start with '{prefix}'"
                )),
            },

            Self::All(parts) => {
                for part in parts {
                    let outcome = part.test(promoted);
                    if !outcome.passed() {
                        return outcome;
                    }
                }
                Test::Pass
            }

            Self::Any(parts) => {
                if parts.is_empty() {
                    return Test::Fail("no condition was given".to_string());
                }
                let mut reasons = Vec::new();
                for part in parts {
                    let outcome = part.test(promoted);
                    if outcome.passed() {
                        return Test::Pass;
                    }
                    if let Test::Fail(why) = outcome {
                        reasons.push(why);
                    }
                }
                Test::Fail(reasons.join("; and "))
            }

            Self::Not(inner) => {
                if inner.test(promoted).passed() {
                    Test::Fail("the excluded condition held".to_string())
                } else {
                    Test::Pass
                }
            }
        }
    }
}

/// `!predicate`. Written as the standard operator rather than an inherent
/// constructor, which would shadow `std::ops::Not::not` and read worse at every
/// call site.
impl std::ops::Not for Predicate {
    type Output = Self;

    fn not(self) -> Self {
        Self::Not(Box::new(self))
    }
}

fn nothing_promoted(property: &str) -> String {
    format!("nothing promoted {property}")
}

/// Look the property up, read it as the type the Subscription meant, and hand
/// it to the comparison.
fn compare(
    promoted: &Promoted,
    property: &str,
    wanted: &Value,
    decide: impl Fn(&Value) -> Test,
) -> Test {
    let Some(raw) = promoted.get(property) else {
        return Test::Fail(nothing_promoted(property));
    };

    match coerce(raw, wanted, property) {
        Ok(actual) => decide(&actual),
        Err(why) => Test::Fail(why),
    }
}

/// Order two values of the same type. Booleans do not order.
fn order(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (Value::Integer(a), Value::Integer(b)) => Some(a.cmp(b)),
        (Value::Text(a), Value::Text(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

/// The result of testing one condition, carrying the reason when it failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Test {
    Pass,
    Fail(String),
}

impl Test {
    #[must_use]
    pub const fn passed(&self) -> bool {
        matches!(self, Self::Pass)
    }

    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Pass => None,
            Self::Fail(why) => Some(why),
        }
    }
}
