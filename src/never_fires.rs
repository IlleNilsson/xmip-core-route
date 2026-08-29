//! The deploy-time check: a Subscription that cannot fire.
//!
//! The same function that matches at run time answers this at deploy time,
//! which is the point of matching being pure. A filter naming a property
//! nothing will ever promote is found before it is deployed rather than by its
//! silence in production.

use crate::Subscription;

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

