# xmip-core-route

Publication, subscription and dispatch — what BizTalk called the MessageBox.
A Message is published once; zero or more Subscriptions match it, matching is
a pure function over context rather than a query, and every match names a
destination and opens one Journey.

Routing creates no new Message. Zero matches means no Journey and the Message
goes to the DMQ; a re-publication is a new Publication, so a Journey stays a
line. Subscriptions are artifacts written in TOML in an Xmip Application
(ADR-0064), loaded by
`xmip-core-configure` and stored by `xmip-core-persist`; this crate replaces
neither. Decision logic is not routing (ADR-0043).

A Subscription's filter is one line of Xmip's expression language
(`xmip-core-path`'s `expression`, ADR-0066) — `filter = "MessageType = 'Order' and
not Amount > 1000 and header:http.x-channel = 'web'"` — compiled as the
Application is read and decided from the compiled tree for every Message;
this crate never parses it. The literal's spelling is the kind a promoted
value is read as, and a value that is not there is *unknown* with its reason,
never a silent false: only a filter that is true matches, and every decline
says why.

The route technologies are `content`, `context`, `contract`, `header`,
`metadata`, `party` and `regex`, each reading the properties its prefix names
(ADR-0046). `expression` is retired: a filter is itself the expression
(ADR-0046, amended 2026-09-26).

**Compiled once, read per Message.** A `Gathering` is built once from the
Subscriptions and the route technologies loaded (`Gathering::of`): it takes
the names every filter uses, each once, and has each `Source` compile its
name into a `Reading` — a pattern, a path through the path engine, a context
key built. `Gathering::promote` then reads each compiled name from a Message
into the `Promoted` set routing decides over, with the first section's
content parsed at most once per form for all of them; nothing else of the
Message is read, and no context value no filter names is rendered. A name
that cannot be compiled — a prefix no loaded technology provides, a name its
technology refuses — is kept as that refusal: `Gathering::refusals` says each
before any Message is read, and a node refuses to start while there is one
(ADR-0066 clause 1); a gathering used without asking refuses each Message at
arrival with the same reason (ADR-0046, amended 2026-09-27). The tests hold it: a name
compiles once and ten thousand Messages are read from it, each well under a
millisecond on a debug build.

A filter reads every value through one function, `routable`: a property the
Message does not hold and a `Null` are absent, so `exists` fails and no
comparison matches, not even with empty text; bytes are refused. `X` and
`context:X` read the same, and so do the other route technologies (ADR-0046,
amended 2026-09-24).

`doc/architecture/runtime-model.md` sections 9 and 11 and ADR-0013 govern it;
`architecture.toml` carries the maturity.
