#![forbid(unsafe_code)]

//! Publication, subscription and dispatch — what BizTalk calls the MessageBox.
//!
//! A Message is published once. Zero or more Subscriptions match it, and each
//! match names a destination. That much Xmip inherits, because it is right.
//!
//! Subscriptions are artifacts. They are written in TOML, loaded through
//! `xmip-core-configure` and stored through `xmip-core-persist`, like every
//! other artifact. Nothing here replaces either.
//!
//! ```toml
//! [[subscription]]
//! id = "billing"
//! destination = { send-port = "Billing" }
//! filter = { equals = { property = "MessageType", value = { text = "Order" } } }
//!
//! [[subscription]]
//! id = "approval"
//! destination = { process = "Approval" }
//! filter = { greater-than = { property = "Amount", value = { integer = 1000 } } }
//! ```
//!
//! What differs from BizTalk is four things, and none of them is the storage.
//!
//! 1. **Matching is a pure function, not a query.** BizTalk evaluates
//!    subscriptions inside SQL Server, so every published Message costs a round
//!    trip to the one MessageBox every node shares. Here the match reads the
//!    promoted set and nothing else, so it runs on the node that holds the
//!    Message, and the same function answers questions at deploy time.
//! 2. **No subscriber is a disposition, not an exception.** BizTalk raises a
//!    routing failure and suspends the Message; you learn about it in
//!    production. Here it is [`Dispatch::Unroutable`], and the Message is kept
//!    for retention.
//! 3. **Every decision explains itself.** [`Routing::declines`] says why each
//!    Subscription passed on the Message. In BizTalk that answer lives in
//!    MessageBox rows you need a separate tool to read.
//! 4. **Promoted values are never guessed.** A promoted value is read as the
//!    type the Subscription states, and coercion happens there, in the open.
//!    Inferring that "0012345" is the number 12345 loses a leading zero, and an
//!    order number with it.
//!
//! Merged from the platform repository's `src/route.rs` on 2026-08-26. The
//! behaviour is that file's. `Subscriber` is this module's, and replaces a
//! `destination: String` that encoded the same three cases as text.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use xmip_context::{ContextValue, MessageContext};

// ---------------------------------------------------------------------------
// values
// ---------------------------------------------------------------------------

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
fn coerce(promoted: &str, wanted: &Value, property: &str) -> Result<Value, String> {
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

// ---------------------------------------------------------------------------
// the promoted set
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// predicates
// ---------------------------------------------------------------------------

/// A condition over the promoted set — a Subscription's filter.
///
/// This is data, not code, so it can be written in an artifact, shipped,
/// compared, and explained. The leaf conditions carry named fields so the TOML
/// reads as configuration rather than as positional arguments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Predicate {
    /// The property was promoted at all, whatever it says.
    Exists { property: String },
    Equals { property: String, value: Value },
    NotEquals { property: String, value: Value },
    GreaterThan { property: String, value: Value },
    LessThan { property: String, value: Value },
    StartsWith { property: String, prefix: String },
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

            Self::GreaterThan { property, value } => {
                compare(promoted, property, value, |actual| match order(actual, value) {
                    Some(std::cmp::Ordering::Greater) => Test::Pass,
                    Some(_) => Test::Fail(format!(
                        "{property} is {}, which is not over {}",
                        actual.show(),
                        value.show()
                    )),
                    None => Test::Fail(format!("{property} cannot be ordered against a boolean")),
                })
            }

            Self::LessThan { property, value } => {
                compare(promoted, property, value, |actual| match order(actual, value) {
                    Some(std::cmp::Ordering::Less) => Test::Pass,
                    Some(_) => Test::Fail(format!(
                        "{property} is {}, which is not under {}",
                        actual.show(),
                        value.show()
                    )),
                    None => Test::Fail(format!("{property} cannot be ordered against a boolean")),
                })
            }

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

// ---------------------------------------------------------------------------
// subscriptions
// ---------------------------------------------------------------------------

/// Who a matched Message goes to.
///
/// Three cases, named rather than encoded in a string. `"SendPort.Billing"`
/// carried the same information and made the reader parse it, which meant a
/// misspelled prefix was a runtime surprise instead of a compile error.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Subscriber {
    Process(String),
    SendPort(String),
    SendGroup(String),
}

impl Subscriber {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Process(name) | Self::SendPort(name) | Self::SendGroup(name) => name,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Process(_) => "Process",
            Self::SendPort(_) => "SendPort",
            Self::SendGroup(_) => "SendGroup",
        }
    }
}

impl std::fmt::Display for Subscriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.kind(), self.name())
    }
}

/// One standing interest in published Messages.
///
/// The destination is an artifact — a Send Port, a Send Group or an Xmip
/// Process. Routing decides *that* a Message goes there, never *how* it gets
/// there.
///
/// `required_contract` and `transformation` name artifacts by id rather than
/// carrying them. A Subscription is configuration, and configuration that
/// embeds the thing it refers to cannot be edited without the thing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub destination: Subscriber,
    pub filter: Predicate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_contract: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transformation: Option<String>,
}

impl Subscription {
    pub fn new(id: impl Into<String>, destination: Subscriber, filter: Predicate) -> Self {
        Self {
            id: id.into(),
            destination,
            filter,
            required_contract: None,
            transformation: None,
        }
    }

    #[must_use]
    pub fn requiring(mut self, contract: impl Into<String>) -> Self {
        self.required_contract = Some(contract.into());
        self
    }

    #[must_use]
    pub fn transforming(mut self, transformation: impl Into<String>) -> Self {
        self.transformation = Some(transformation.into());
        self
    }
}

/// What one Subscription decided about one Message, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evaluation {
    pub subscription_id: String,
    pub destination: Subscriber,
    pub outcome: Test,
}

impl Evaluation {
    #[must_use]
    pub const fn matched(&self) -> bool {
        self.outcome.passed()
    }
}

/// Where a Message goes, and the reasoning for every Subscription that was
/// asked. The reasoning for the ones that declined is the valuable half.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Routing {
    pub evaluations: Vec<Evaluation>,
}

impl Routing {
    /// The destinations this Message is bound for, in subscription order.
    #[must_use]
    pub fn destinations(&self) -> Vec<&Subscriber> {
        self.evaluations
            .iter()
            .filter(|evaluation| evaluation.matched())
            .map(|evaluation| &evaluation.destination)
            .collect()
    }

    /// What routing decided, as a disposition.
    #[must_use]
    pub fn dispatch(&self) -> Dispatch {
        match self.destinations().len() {
            0 => Dispatch::Unroutable,
            matched => Dispatch::Routed(matched),
        }
    }

    /// Why each Subscription declined, in the order they were asked.
    #[must_use]
    pub fn declines(&self) -> Vec<(&str, &str)> {
        self.evaluations
            .iter()
            .filter(|evaluation| !evaluation.matched())
            .filter_map(|evaluation| {
                evaluation
                    .outcome
                    .reason()
                    .map(|why| (evaluation.subscription_id.as_str(), why))
            })
            .collect()
    }
}

/// The routing outcome for one published Message.
///
/// Unroutable is a disposition, not a failure. The Message was valid, it passed
/// its Contract, and nobody wanted it — a statement about configuration, not
/// about the Message. It is kept under retention so the question can be
/// answered later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dispatch {
    Routed(usize),
    Unroutable,
}

impl Dispatch {
    /// Whether the Message must be kept because nothing took it.
    #[must_use]
    pub const fn retains(&self) -> bool {
        matches!(self, Self::Unroutable)
    }
}

/// Publish one Message's promoted set against every Subscription.
///
/// Every Subscription is asked, including the ones that will decline, because
/// the declines are the diagnosis.
#[must_use]
pub fn publish(promoted: &Promoted, subscriptions: &[Subscription]) -> Routing {
    Routing {
        evaluations: subscriptions
            .iter()
            .map(|subscription| Evaluation {
                subscription_id: subscription.id.clone(),
                destination: subscription.destination.clone(),
                outcome: subscription.filter.test(promoted),
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// the deploy-time check
// ---------------------------------------------------------------------------

/// A Subscription that cannot fire, and the property name that dooms it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeverFires {
    pub subscription_id: String,
    pub unknown_property: String,
}

/// Find Subscriptions that read a property nothing will ever promote.
///
/// `promotable` is every property name the deployed Contracts can produce. A
/// filter naming anything outside that set is a typo with a deployment date. In
/// BizTalk it is invisible: the Subscription is accepted, never matches, and
/// the first symptom is a Message going nowhere months later.
///
/// A static check. It needs no traffic.
#[must_use]
pub fn never_satisfiable(promotable: &[&str], subscriptions: &[Subscription]) -> Vec<NeverFires> {
    let mut doomed = Vec::new();

    for subscription in subscriptions {
        for name in subscription.filter.referenced_names() {
            if !promotable.contains(&name) {
                doomed.push(NeverFires {
                    subscription_id: subscription.id.clone(),
                    unknown_property: name.to_string(),
                });
            }
        }
    }

    doomed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orders() -> Promoted {
        Promoted::new()
            .set("MessageType", "Order")
            .set("Amount", "1500")
            .set("Customer", "ACME-0042")
            .set("Urgent", "true")
    }

    fn to(id: &str, destination: Subscriber, filter: Predicate) -> Subscription {
        Subscription::new(id, destination, filter)
    }

    fn send_port(name: &str) -> Subscriber {
        Subscriber::SendPort(name.to_string())
    }

    #[test]
    fn a_matching_subscription_names_its_destination() {
        let subscriptions = vec![to(
            "billing",
            send_port("Billing"),
            Predicate::equals("MessageType", Value::Text("Order".into())),
        )];
        let routing = publish(&orders(), &subscriptions);

        assert_eq!(routing.destinations(), vec![&send_port("Billing")]);
        assert_eq!(routing.dispatch(), Dispatch::Routed(1));
    }

    #[test]
    fn one_message_can_reach_several_destinations() {
        let subscriptions = vec![
            to(
                "billing",
                send_port("Billing"),
                Predicate::equals("MessageType", Value::Text("Order".into())),
            ),
            to("archive", send_port("Archive"), Predicate::everything()),
            to(
                "approval",
                Subscriber::Process("Approval".to_string()),
                Predicate::greater_than("Amount", Value::Integer(1000)),
            ),
        ];
        let routing = publish(&orders(), &subscriptions);

        assert_eq!(routing.destinations().len(), 3);
        assert_eq!(routing.dispatch(), Dispatch::Routed(3));
        assert_eq!(routing.destinations()[2].to_string(), "Process.Approval");
    }

    #[test]
    fn nothing_wanting_it_is_a_disposition_and_keeps_the_message() {
        let subscriptions = vec![to(
            "invoices",
            send_port("Invoices"),
            Predicate::equals("MessageType", Value::Text("Invoice".into())),
        )];
        let routing = publish(&orders(), &subscriptions);

        assert!(routing.destinations().is_empty());
        assert_eq!(routing.dispatch(), Dispatch::Unroutable);
        assert!(routing.dispatch().retains());
    }

    #[test]
    fn a_decline_says_why() {
        let subscriptions = vec![to(
            "invoices",
            send_port("Invoices"),
            Predicate::equals("MessageType", Value::Text("Invoice".into())),
        )];
        let routing = publish(&orders(), &subscriptions);

        assert_eq!(
            routing.declines(),
            vec![("invoices", "MessageType is 'Order', not 'Invoice'")]
        );
    }

    #[test]
    fn a_missing_property_is_named_not_shrugged_at() {
        let subscriptions = vec![to(
            "nordics",
            send_port("Nordics"),
            Predicate::equals("Region", Value::Text("SE".into())),
        )];
        let routing = publish(&orders(), &subscriptions);

        assert_eq!(
            routing.declines(),
            vec![("nordics", "nothing promoted Region")]
        );
    }

    #[test]
    fn the_subscription_states_the_type_and_the_text_is_read_as_it() {
        // Amount was promoted as the text "1500". The filter says integer, so
        // the comparison is numeric.
        assert!(Predicate::greater_than("Amount", Value::Integer(900))
            .test(&orders())
            .passed());

        // Lexicographically "1500" is less than "900", which is the wrong
        // answer, and the reason the type belongs on the Subscription.
        assert!(
            !Predicate::greater_than("Amount", Value::Text("900".into()))
                .test(&orders())
                .passed()
        );
    }

    #[test]
    fn a_leading_zero_survives_because_nothing_is_inferred() {
        let promoted = Promoted::new().set("OrderNo", "0012345");

        assert_eq!(promoted.get("OrderNo"), Some("0012345"));
        assert!(Predicate::equals("OrderNo", Value::Text("0012345".into()))
            .test(&promoted)
            .passed());
    }

    #[test]
    fn text_that_is_not_a_number_fails_with_a_fixable_sentence() {
        let promoted = Promoted::new().set("Amount", "about ten");

        assert_eq!(
            Predicate::greater_than("Amount", Value::Integer(5)).test(&promoted),
            Test::Fail("Amount is 'about ten', which is not an integer".to_string())
        );
    }

    #[test]
    fn booleans_read_as_written() {
        assert!(Predicate::equals("Urgent", Value::Boolean(true))
            .test(&orders())
            .passed());
    }

    #[test]
    fn an_empty_all_takes_everything_and_an_empty_any_takes_nothing() {
        assert!(Predicate::everything().test(&orders()).passed());
        assert!(!Predicate::any(vec![]).test(&orders()).passed());
    }

    #[test]
    fn any_reports_every_reason_it_declined() {
        let filter = Predicate::any(vec![
            Predicate::equals("MessageType", Value::Text("Invoice".into())),
            Predicate::exists("Region"),
        ]);

        assert_eq!(
            filter.test(&orders()),
            Test::Fail(
                "MessageType is 'Order', not 'Invoice'; and nothing promoted Region".to_string()
            )
        );
    }

    #[test]
    fn not_excludes() {
        let filter = Predicate::all(vec![
            Predicate::exists("MessageType"),
            !Predicate::equals("Customer", Value::Text("ACME-0042".into())),
        ]);

        assert_eq!(
            filter.test(&orders()),
            Test::Fail("the excluded condition held".to_string())
        );
    }

    #[test]
    fn starts_with_reads_the_text_as_text() {
        assert!(Predicate::starts_with("Customer", "ACME-")
            .test(&orders())
            .passed());
    }

    #[test]
    fn a_filter_naming_a_property_nothing_promotes_is_found_before_deployment() {
        let promotable = ["MessageType", "Amount", "Customer", "Urgent"];
        let subscriptions = vec![
            to(
                "good",
                send_port("Billing"),
                Predicate::equals("MessageType", Value::Text("Order".into())),
            ),
            // Regoin, not Region. Neither spelling is promoted.
            to(
                "typo",
                send_port("Nordics"),
                Predicate::equals("Regoin", Value::Text("SE".into())),
            ),
        ];

        let doomed = never_satisfiable(&promotable, &subscriptions);

        assert_eq!(doomed.len(), 1);
        assert_eq!(doomed[0].subscription_id, "typo");
        assert_eq!(doomed[0].unknown_property, "Regoin");
    }

    #[test]
    fn a_nested_filter_still_gives_up_every_name_it_reads() {
        let filter = Predicate::all(vec![
            Predicate::any(vec![
                Predicate::exists("A"),
                !Predicate::equals("B", Value::Integer(1)),
            ]),
            Predicate::starts_with("C", "x"),
        ]);

        assert_eq!(filter.referenced_names(), vec!["A", "B", "C"]);
    }

    #[test]
    fn the_promoted_set_is_ordered_and_the_last_promotion_wins() {
        let promoted = Promoted::new()
            .set("B", "2")
            .set("A", "1")
            .set("A", "1-corrected");

        assert_eq!(promoted.names(), vec!["A", "B"]);
        assert_eq!(promoted.get("A"), Some("1-corrected"));
        assert_eq!(promoted.len(), 2);
    }

    #[test]
    fn promotion_arrives_from_context_as_text() {
        let context = MessageContext::new()
            .with_value("OrderNo", ContextValue::Text("0012345".into()))
            .with_value("Amount", ContextValue::Integer(1500))
            .with_value("Urgent", ContextValue::Bool(true))
            .with_value("Blob", ContextValue::Binary(vec![0, 1, 2]));

        let promoted = Promoted::from_context(&context);

        assert_eq!(promoted.get("OrderNo"), Some("0012345"));
        assert_eq!(promoted.get("Amount"), Some("1500"));
        assert_eq!(promoted.get("Urgent"), Some("true"));
        assert_eq!(promoted.get("Blob"), None, "bytes are not routable as text");
    }

    #[test]
    fn a_subscription_round_trips_through_toml() {
        let subscription = to(
            "approval",
            Subscriber::Process("Approval".to_string()),
            Predicate::greater_than("Amount", Value::Integer(1000)),
        )
        .requiring("Order.v2")
        .transforming("OrderToInvoice");

        let text = toml::to_string(&subscription).expect("writing toml");
        let back: Subscription = toml::from_str(&text).expect("reading toml");

        assert_eq!(back, subscription);
    }
}
