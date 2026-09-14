# xmip-core-route

Publication, subscription and dispatch — what BizTalk called the MessageBox.
A Message is published once; zero or more Subscriptions match it, matching is
a pure function over context rather than a query, and every match names a
destination and opens one Journey.

Routing creates no new Message. Zero matches means no Journey and the Message
goes to the DMQ; a re-publication is a new Publication, so a Journey stays a
line. Subscriptions are artifacts written in TOML, loaded by
`xmip-core-configure` and stored by `xmip-core-persist`; this crate replaces
neither. Decision logic is not routing (ADR-0043).

`doc/architecture/runtime-model.md` sections 9 and 11 and ADR-0013 govern it;
`architecture.toml` carries the maturity.
