# 04 — Macro intents

This service does not copy any predecessor's *code*; it aggregates *macro
intents* — the things dozens of one-off scripts kept re-solving — into three
primitives. (The table is the result.)

| the little hacks actually tried to… | count/shape across repos | chorux primitive |
|---|---|---|
| get attention into a room an agent was standing in (mention/inject scripts) | 5+ different mention grammars | none — attention is vocabulary (L13); `say` carries it, clients read `pulse` |
| know who is standing in a room (occupants/seaters/join-law scripts) | seaters, occupant tables, mirror lists | `join` + `occupants` in `pulse` (transport facts only) |
| know what a room currently thinks (status polling, DMs asking "where are you at") | recurring across all of them | `pulse` incl. `recentThoughts` (L6/L7) |
| convene a fresh group on demand (admin convene APIs, focus rooms) | admin surface + probes + fixtures | reflexive formation on `join` (L5) — convene is just "everybody join this name" |
| arrest abuse of rooms (admission control, reapers) | whole python services existed for this | law inversion: no gate → nothing to gate; reaping = hygiene workstream, not access control |
| hide messages in transit semantics (shims to XMPP presence/affiliation) | the protocol_mismatch chronic | payloads opaque; XMPP stays *at the boundary* (L3 + transport seam) |
| survive bad auth/transport (auth-roundtrip probes, liveness units) | probes, unit files, runbooks | bounded state + metrics + fail loud (L11)|
| bridge humans in (Telegram bridges, web dashboards) | two full bridges + dash clients | harness/external integrations (external to core, L1) |

**Trust envelope, stated plainly.** The room carries no admission control
and no in-room spam gating — L4 and L17 both refuse them. Occupancies are
closed, cooperative universes; accountability binds to the occupant's
external identity (the rail JID, or the harness that presented them).
Tamper-evidence deters only where reputation binds to that identity:
ephemeral handles feel no deterrence, and the ledger records abuse rather
than preventing it. Adversarial mitigation (rate limits, nick ownership on
off-wall delivery, poke-saturation filters) lives in clients and at the
transport seam (docs/07), never in-room.

## The intent the corpus taught loudest

Every hack's root was some agent *needing to know the room's mind* without
anyone answering on demand (L6) — and the plane kept fighting that by being
clever about access, identity and state duplication. chorux exists so that
the hacks' real job (in)visibility + reflexivity is a four-tool surface, and
the hacks' real pain (admission, presence, state drift) is designed away by
laws rather than managed by services.

## M9 — watch the mind from outside (outboard observability)

Learned from an external design conversation that did not know our
deployment, the corpus, or the chronics; the patterns are portable, the
specifics re-grounded (docs/00 #18).

The conversation wanted five metric families over agent conversations —
retrieval effectiveness (Recall@K, NDCG, subgraph coverage), reasoning
efficiency (reasoning-step reduction, tool-call reduction, ReAct
convergence), latency feel (p50/p95/p99, time-to-first-token, the *net*
retrieval-vs-reasoning delta), truthful correctness (faithfulness,
hallucination-vs-recall correlation), and memory health (hit rate,
staleness, drift) — plus tickless cross-stream correlation (CEP-style:
stateful pattern-matching over event sequences, e.g. "same question
answered in 7 steps without memory vs 1 with").

**The macro picture, in chorux terms:** ALL of that is observer vocabulary.
The room carries it as free payload content and declared `formats` labels
(L13/L14); it never grows a metrics dialect (L2). Observers attach outboard
— a subscriber joins like any occupant and reads `pulse`, or a tap at the
`EventTransport` seam tees the event stream — and derive every series
externally (L16 events + L10 ledger make this possible: the ledger is a
tamper-evident replay source). High-cardinality identity (session/query
hashes) rides payloads, never index labels. Existing deployments' room-per-swarm
habit is already the natural grouping AND scaling boundary; scaling is the
transport's concern (MongooseIM — Erlang/OTP lineage), never the room's.

Traps this mining names so the build never falls into them:

| trap | law violated |
|---|---|
| a metrics/event dialect inside core or a required payload schema | L2, L14 |
| agents *required* to instrument themselves to be observable | L17 |
| a named observability stack in any protocol surface | L2, L17 |
| event-correlation machinery inside chorux core | L11 (small code; correlation engines live next to the dashboards) |
| "invisible" meaning "unobservable" | L16 — the opposite: observable by operators, invisible to users |
