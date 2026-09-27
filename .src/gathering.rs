//! The gathering: every property a set of filters names, each compiled once
//! by the route technology its prefix names, and read from every Message
//! into the one [`Promoted`] set routing decides over.
//!
//! What is compiled is only what the filters use. A Message's context is not
//! rendered whole; a property no filter names is never read (ADR-0046,
//! amended 2026-09-27). A property that cannot be read — a prefix no loaded
//! technology provides, a name its technology refuses — is found when it is
//! compiled and kept as that refusal, so every Message it would be read from
//! is refused at arrival with the reason, as ADR-0046 has it, without the
//! name being compiled again.

use message::Message;
use path::Content;

use crate::source::{CONTEXT, Reading, Source, SourceError, split};
use crate::{Promoted, Subscription, routable};

/// One property and its compiled reading, or why it has none.
struct Gathered {
    property: String,
    technology: String,
    name: String,
    reading: Result<Box<dyn Reading>, SourceError>,
}

/// Every property a set of filters names, compiled once.
pub struct Gathering {
    gathered: Vec<Gathered>,
}

/// A bare name, or `context:` when no technology of that name is loaded: the
/// context value itself, through [`routable`].
struct ContextKey(String);

impl Reading for ContextKey {
    fn read(&self, message: &Message, _: Option<&Content<'_>>) -> Result<Option<String>, String> {
        routable(&self.0, message.context().get(&self.0))
    }
}

impl Gathering {
    /// Compile `properties`, each through the source whose technology it
    /// carries. Context is read from the Message itself when no source
    /// claims it, so a set of sources may be empty and a filter over context
    /// alone still works.
    #[must_use]
    pub fn new(sources: &[&dyn Source], properties: &[&str]) -> Self {
        let gathered = properties
            .iter()
            .map(|property| {
                let (technology, name) = split(property);
                let source = sources
                    .iter()
                    .find(|source| source.technology() == technology);
                let reading = match source {
                    Some(source) => source
                        .compile(name)
                        .map_err(|reason| SourceError::new(technology, name, reason)),
                    None if technology == CONTEXT => {
                        Ok(Box::new(ContextKey(name.to_string())) as Box<dyn Reading>)
                    }
                    None => Err(SourceError::new(
                        technology,
                        *property,
                        "no route technology of that name is loaded",
                    )),
                };
                Gathered {
                    property: (*property).to_string(),
                    technology: technology.to_string(),
                    name: name.to_string(),
                    reading,
                }
            })
            .collect();
        Self { gathered }
    }

    /// Compile every name `subscriptions`' filters use, each once however
    /// many filters name it.
    #[must_use]
    pub fn of(sources: &[&dyn Source], subscriptions: &[Subscription]) -> Self {
        let mut names: Vec<&str> = subscriptions
            .iter()
            .flat_map(|subscription| subscription.filter.names())
            .collect();
        names.sort_unstable();
        names.dedup();
        Self::new(sources, &names)
    }

    /// The properties gathered, in the order they were compiled.
    pub fn properties(&self) -> impl Iterator<Item = &str> {
        self.gathered
            .iter()
            .map(|gathered| gathered.property.as_str())
    }

    /// Read every property from `message`. The first section's content is
    /// parsed at most once per form, whichever readings ask for it.
    ///
    /// # Errors
    /// A property could not be compiled, a reading refused the Message, or a
    /// value it names holds bytes.
    pub fn promote(&self, message: &Message) -> Result<Promoted, SourceError> {
        let content = message
            .sections()
            .first()
            .map(|section| Content::of(&section.stream));
        let mut promoted = Promoted::new();
        for gathered in &self.gathered {
            let reading = gathered.reading.as_ref().map_err(Clone::clone)?;
            let value = reading
                .read(message, content.as_ref())
                .map_err(|reason| SourceError::new(&gathered.technology, &gathered.name, reason))?;
            if let Some(value) = value {
                promoted = promoted.set(gathered.property.as_str(), value);
            }
        }
        Ok(promoted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Subscriber;
    use context::{ContextValue, MessageContext};
    use message::MessageTreatment;
    use path::expression::Expression;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use xcore::MessageId;

    /// Reads `metadata:generation`, counting how often it compiles a name.
    #[derive(Default)]
    struct Counting(AtomicUsize);

    struct Generation;

    struct Nothing;

    impl Source for Counting {
        fn technology(&self) -> &'static str {
            "metadata"
        }

        fn compile(&self, name: &str) -> Result<Box<dyn Reading>, String> {
            self.0.fetch_add(1, Ordering::Relaxed);
            match name {
                "generation" => Ok(Box::new(Generation)),
                "nothing" => Ok(Box::new(Nothing)),
                _ => Err("not a thing a Message has".to_string()),
            }
        }
    }

    impl Reading for Generation {
        fn read(
            &self,
            message: &Message,
            _: Option<&Content<'_>>,
        ) -> Result<Option<String>, String> {
            Ok(Some(message.generation().to_string()))
        }
    }

    impl Reading for Nothing {
        fn read(&self, _: &Message, _: Option<&Content<'_>>) -> Result<Option<String>, String> {
            Ok(None)
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

    fn promote(sources: &[&dyn Source], properties: &[&str]) -> Result<Promoted, SourceError> {
        Gathering::new(sources, properties).promote(&message())
    }

    #[test]
    fn context_is_promoted_with_no_source_and_a_technology_adds_what_it_reads() {
        let counting = Counting::default();
        let sources: [&dyn Source; 1] = [&counting];
        let promoted =
            promote(&sources, &["MessageType", "metadata:generation"]).expect("both readable");
        assert_eq!(promoted.get("MessageType"), Some("Order"));
        assert_eq!(promoted.get("metadata:generation"), Some("0"));

        let none = promote(&[], &["MessageType"]).expect("context alone");
        assert_eq!(none.get("MessageType"), Some("Order"));
    }

    #[test]
    fn only_the_names_the_filters_use_are_gathered_each_once() {
        let filter = |text: &str| Expression::parse(text).expect("compiles");
        let to = |id: &str, text: &str| {
            Subscription::new(id, Subscriber::SendPort(id.to_string()), filter(text))
        };
        let subscriptions = [
            to("a", "MessageType = 'Order' and metadata:generation = 0"),
            to("b", "MessageType = 'Invoice'"),
        ];
        let counting = Counting::default();
        let sources: [&dyn Source; 1] = [&counting];
        let gathering = Gathering::of(&sources, &subscriptions);
        assert_eq!(
            gathering.properties().collect::<Vec<_>>(),
            vec!["MessageType", "metadata:generation"]
        );

        let context = MessageContext::new()
            .with_value("MessageType", ContextValue::Text("Order".into()))
            .with_value("Unused", ContextValue::Binary(vec![0, 1]));
        let message = Message::received(
            MessageId::new(3),
            Vec::new(),
            context,
            MessageTreatment::default(),
        );
        let promoted = gathering
            .promote(&message)
            .expect("unused bytes are never read");
        assert_eq!(promoted.names(), vec!["MessageType", "metadata:generation"]);
    }

    #[test]
    fn a_name_compiles_once_and_is_read_from_every_message_in_microseconds() {
        let counting = Counting::default();
        let sources: [&dyn Source; 1] = [&counting];
        let gathering = Gathering::new(&sources, &["metadata:generation", "MessageType"]);
        let message = message();
        let started = std::time::Instant::now();
        for _ in 0..10_000 {
            let promoted = gathering.promote(&message).expect("readable");
            assert_eq!(promoted.len(), 2);
        }
        let each = started.elapsed() / 10_000;
        assert_eq!(counting.0.load(Ordering::Relaxed), 1, "compiled once");
        // Generous for a debug build under load; a millisecond is the defect.
        assert!(each.as_micros() < 200, "{each:?} per Message");
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
        let gather = |properties: &[&str]| Gathering::new(&[], properties);

        let promoted = gather(&["MessageType", "Note", "Region"])
            .promote(&message)
            .expect("readable");
        assert_eq!(promoted.get("MessageType"), Some("Order"));
        assert_eq!(promoted.get("Note"), None);
        assert_eq!(promoted.get("Region"), None);
        let holds = |text: &str| {
            Expression::parse(text)
                .expect("compiles")
                .evaluate(&promoted)
                .holds()
        };
        assert!(holds("exists MessageType"));
        assert!(!holds("exists Note"));
        assert!(!holds("exists Region"));
        assert!(!holds("Note = ''"));

        let refused = gather(&["Blob"]).promote(&message).expect_err("bytes");
        assert_eq!(refused.technology, "context");
        assert_eq!(refused.property, "Blob");
        assert!(refused.reason.contains("3 bytes"));
    }

    #[test]
    fn nothing_read_is_nothing_promoted_and_a_missing_technology_is_an_error() {
        let counting = Counting::default();
        let sources: [&dyn Source; 1] = [&counting];
        let promoted = promote(&sources, &["metadata:nothing"]).expect("readable");
        assert_eq!(promoted.get("metadata:nothing"), None);

        let missing = Gathering::new(&sources, &["party:sender"])
            .promote(&message())
            .expect_err("no party");
        assert_eq!(missing.technology, "party");
        assert!(missing.to_string().contains("no route technology"));

        let refused = Gathering::new(&sources, &["metadata:colour"])
            .promote(&message())
            .expect_err("refused");
        assert_eq!(refused.property, "colour");
    }
}
