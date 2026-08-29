//! Where a Message goes, the reasoning for every Subscription that was asked,
//! and the act of publishing one Message against all of them.
//!
//! **No subscriber is a disposition, not an exception.** BizTalk raises a
//! routing failure and suspends the Message; here it is [`Dispatch::Unroutable`]
//! and the Message is kept for retention.

use crate::{Promoted, Subscriber, Subscription, Test};

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

