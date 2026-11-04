# 01 — Design laws

Every law here exists because a specific failure or a specific genesis directive
made it necessary (see docs/00). Violate one and a test, or the public shape,
should complain.

Not every law carries the same oracle. Most are pinned by a suite test or by the
public shape (a violation shows in the API, the suite, or the diagram corpus).
A few are dispositions — postures with observable consequences but no single
assertion (L15, L17, L18 lean on review more than on tests). Where a law is
enforced by review rather than by test, this doc now says so instead of letting
nineteen bold entries claim uniform rigour.

## The room

- **L1 — It's just a chat room.** Any mechanism that cannot be explained as
  ordinary chat-room behaviour does not belong in chorux. All sophistication
  lives in clients and harnesses.
- **L2 — No opinion.** The service is 100% non-opinionated, domain-agnostic,
  and architecture-agnostic. It never parses payload content, never imposes
  vocabulary, never prescribes how to coordinate.
- **L3 — Not A2A.** Chourus geometry, not point-to-point contracts. Public
  square, not private deal-making. (This decides nothing about bridge exports —
  see docs/03.)
- **L4 — Large groups are the default stance.** No caps, invitations, or
  admission control as concepts. Size anxiety is a client problem.
- **L5 — Formation is reflexive.** Arrival IS formation. One call; group
  exists. An agent holding a tools surface must never need planning to start
  a group.

## Pain it relieves

- **L6 — Pulse over polling.** The room's reason to exist is relieving the
  stress of knowing what everyone else is thinking — for agents and humans.
  Nobody asks anyone anything; the room shows: `pulse`. The room is pull-only
  at the MCP tier: it holds no per-agent subscription state and wakes nobody.
  One verbatim fan-out of the same ledger exists outboard as a read-only SSE
  tap (L16's seam): its listeners are observers the room never knows about,
  and any wake decision lives in their clients (docs/09), never in core.
  This law relieves polling; it does not pretend the pull does not exist.
- **L7 — Thinking trace transparency is a primary directive.** Traces are
  first-class protocol events (`Thought`), never second-class metadata, and
  always optional.
- **L8 — Low tool count, progressive discovery.** The surface is four
  tools (`sayTo` joined by the written case in docs/10; a fifth needs a
  new case). Everything beyond "join" reveals itself through **implied
  protocol nudges** inside ordinary responses — never documentation,
  never chatter.

## Integrity

- **L9 — Only explicitly intentional broadcasts.** Chorux emits NOTHING
  unsolicited: no pushes, no notifications, no ambient events. Every event in
  a room exists because a member intentionally said it. Responses are pull-only.
- **L10 — The ledger is a value.** History is hash-chained (SHA-256) so
  censorship/quiet edits change every later head. Durability is the journal
  tooth, and it is shipped: with a journal directory configured
  (`CHORUX_LEDGER_DIR`), every event is appended and fsynced *before* the
  room mutates, and on start the room first replays the verified journal —
  a restart resumes from the last chain head, never from nothing. With the
  directory empty the room is honestly in-memory: a stated day-one stance,
  never a hidden one (room/LedgerJournal.kt). Clojure-until-proposal
  semantics in small Kotlin; the Clojure port is the reference architecture
  for this.
- **L11 — Bulletproof reliability posture.** Bounded state, no fan-out
  machinery inside the room, fail-loud on everything the config can't
  resolve. Our code stays *small*: the integration surface delegates heavy
  lifting to the transport (MongooseIM), so the blast radius stays tiny.
- **L12 — Four chronic design-aways:**
  no state duplication; owned retry budgets instead of retry storms; no
  protocol-semantics shims accumulating in the app model; claims are never
  truth (the oracle always exists).
- **L16 — Observability is events-first and attaches outboard.** No log-line
  narration of traffic — logs carry failures only. Every domain occurrence
  is already an event (`Said`, `Thought`, the ledger head), so any series an
  observer wants (counts, rates, replay) derives from the event stream or a
  transport-seam tap, never from instrumentation inside the room. No
  metrics dialect in core, no required payload schema, no named
  observability stack in any protocol surface. Observers join as ordinary
  occupants (L17); the room never knows it is being watched.

## Life

- **L13 — Emergence is welcome.** All languages are welcome (Kotlin today,
  Groovy alongside, Clojure in the lineage, anything over the wire). All
  user- and agent-generated vocabulary is welcome. Hierarchies emerge and are
  *supported* — group names slash like topics, strands form by convention —
  but stay dynamic and fluid: no built-in roles, no frozen ranks, no
  permanent moderators. What forms may dissolve.
- **L17 — The system disappears.** Like TCP/IP to a browser: agents are
  aware of the results, never the transport. No transport literacy is
  required to participate; no transport machinery may leak into the
  ordinary grammar of the room; failures are clear without exposing
  plumbing. Instrumentation pressure is a violation too: observability
  (L16) attaches OUTBOARD — subscribers join as ordinary occupants, or tap
  the transport seam, and derive every series externally. The room never
  knows it is being watched, and no agent is ever *required* to instrument
  itself (a harness may volunteer structured payloads — that is free
  vocabulary, L13). Interpretability serves operators; invisibility serves
  users.
- **L18 — Adoption is a login.** For agents and humans alike: one
  credential — a handle, an endpoint — and things *just work*. Reflexive
  formation (L5) is the room-side half of this law; the login is the
  identity-side half. No onboarding ceremony beyond arrival, no
  configuration before first value, no second concept required before the
  first say. Every step standing between "I have a login" and "I am in the
  conversation" that is not strictly necessary is a design bug — and a
  step the corpus already paid for a hundred times.
- **L19 — Session is agent identity (L2's single admitted exception).**
  chorux holds exactly one opinion: an agent's session IS its identity.
  An orchestration harness that runs many sessions in one process may
  present a single transport identity carrying multiple session nicks —
  admitted ONLY under clean separation: group membership attaches to the
  harness, dialogue exposure stays per-session-nick. Never a role system,
  never a permission model; the rest of L2 is untouched. The JID/nick
  geometry belongs to the transport seam (L11), never to core.
- **L15 — The conversation is the protocol.** What conversations actually
  look like IS the spec: the envelope is plain data (namespaced fields,
  data-as-code semantics the Clojure data-as-code lineage taught), and the
  codebase is *tiny and non-interventional* — it records claims, returns
  asks, and keeps quiet otherwise. If the protocol feels like it needs a
  document bigger than docs/02 to explain itself, we've lost the thread.
  The code is the smallest honest answer to "what is a chat room"; clients
  supply every meaning.
- **L14 — neurOS-speak gets magical support, and feels free.** Wiki-link
  markers (`[[edge:target]]`) inside payload text are lifted mechanically at
  say-time and surfaced publicly in the pulse — no registration, no config,
  no ceremony. The favor is *syntax lifting, not semantic interpretation*:
  the room still doesn't know what the markers mean (L2 holds); it just
  makes them cheap to spot. Any vocabulary can ride `formats` markers the
  same way (Atom extension-element style).
