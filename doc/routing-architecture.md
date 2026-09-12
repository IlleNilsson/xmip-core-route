# Xmip Routing Architecture

Status: Frozen architectural baseline

## Purpose

`xmip-routing` is a key Xmip capability. It publishes Messages, manages Subscriptions, evaluates those Subscriptions, resolves Subscribers and dispatches Messages. For Composite calls, it also routes the resulting response back to the originating Receive Port and Receive Location.

## Module structure

```text
xmip-routing
    xmip-publishing
    xmip-subscriptions
    xmip-subscription-evaluation
    xmip-response-routing
```

These are submodules of the Routing Module, not independent responsibility domains outside Routing.

## xmip-publishing

`xmip-publishing` publishes Messages into Routing.

A published Message makes available the routing information carried by the Message and its context, including references such as:

```text
Message
Message context
Source Endpoint
Party
Contract
Message type
Journey
```

Publishing does not evaluate Subscriptions.

Path is used to deliver values that Subscription evaluation can use. Path does not own Publication, Subscriptions or Routing decisions.

## xmip-subscriptions

`xmip-subscriptions` owns declared Subscriptions.

A Subscription includes:

```text
Identity
Conditions
Required values
Subscriber
Version
Enabled state
```

Subscribers may be:

```text
Xmip Process
Send Port
Send Port Group
```

Subscriptions declare interest. They do not execute routing by themselves.

## xmip-subscription-evaluation

`xmip-subscription-evaluation` evaluates a published Message against applicable Subscriptions.

```text
Published Message
    -> Applicable Subscriptions
    -> Required values delivered through Path
    -> DMN evaluation
    -> Zero, one or many matching Subscribers
```

DMN provides the decision model and evaluation semantics. Routing still owns Subscriptions and their evaluation.

## Dispatch

`xmip-routing` dispatches the published Message to every matched Subscriber.

```text
Zero matches
    -> Dead Journey unless no destination is explicitly permitted

One match
    -> Dispatch once

Multiple matches
    -> Dispatch to every matching Xmip Process, Send Port or Send Port Group
```

A Send Port Group is a named collection of Send Ports. Routing dispatches to every Send Port in the matched group.

## Composite response routing

Composite calls require a durable response path.

```text
Incoming Composite call
    -> Receive Location receives request Stream
    -> Receive Port accepts the Stream and creates request Message
    -> Message is published
    -> Subscriptions are evaluated
    -> Xmip Process and/or Send Port is invoked
    -> Response Message is produced
    -> xmip-response-routing
    -> Originating Receive Port
    -> Originating Receive Location returns response Stream
```

`xmip-response-routing` owns:

```text
Correlation to the original request
Originating Receive Port reference
Originating Receive Location response reference
Response timeout
Terminal response outcome
Selection of the response Message when several actions occur
Return routing to the waiting Receive Port
```

The Receive Location performs the physical response to the caller. The Receive Port owns the response Message inside Xmip.

## Frozen principles

> Publishing publishes Messages. Subscriptions declare interest. Subscription Evaluation determines which Subscriptions match. Routing coordinates them and dispatches the Message to matching Subscribers.

> Path delivers values that Subscription evaluation may use. Path does not inspect, publish, subscribe or decide destinations.

> DMN provides decision semantics inside Xmip Routing. It does not own Routing responsibilities.

> Routing dispatches published Messages to Subscribers and, for Composite calls, routes the resulting response Message back to the originating Receive Port and Receive Location.
