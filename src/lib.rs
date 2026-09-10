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

mod never_fires;
mod predicate;
mod promoted;
mod routing;
mod source;
mod subscription;
mod value;

pub use never_fires::{NeverFires, never_satisfiable};
pub use predicate::{Predicate, Test};
pub use promoted::Promoted;
pub use routing::{Dispatch, Evaluation, Routing, publish};
pub use source::{CONTEXT, Source, SourceError, promote, split};
pub use subscription::{Subscriber, Subscription};
pub use value::Value;

// The tests below stay here rather than moving beside each file. They exercise
// the crate's public surface end to end — publish, predicates and the promoted
// set together — over one fixture, which is a different thing from a unit test
// of one file. rust-style.md section 2 is about tests rotting in a distant
// tests/ directory, and these are not distant.

#[cfg(test)]
mod tests {
    use super::*;
    use context::{ContextValue, MessageContext};

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
        assert!(
            Predicate::greater_than("Amount", Value::Integer(900))
                .test(&orders())
                .passed()
        );

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
        assert!(
            Predicate::equals("OrderNo", Value::Text("0012345".into()))
                .test(&promoted)
                .passed()
        );
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
        assert!(
            Predicate::equals("Urgent", Value::Boolean(true))
                .test(&orders())
                .passed()
        );
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
        assert!(
            Predicate::starts_with("Customer", "ACME-")
                .test(&orders())
                .passed()
        );
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
