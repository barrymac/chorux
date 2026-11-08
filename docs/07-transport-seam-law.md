# 07 — Transport seam law

The L11 seam made contractual: what any real `EventTransport` mechanism must
guarantee so the room stays just a chat room. XMPP/MUC via Smack is the first
real mechanism — mechanism, not doctrine; the law below binds any mechanism
(component line, client line, whatever replaces XMPP someday) equally.

## Geometry (L18 + L19)

- One deployment is one transport identity: **one JID** (the harness
  identity). Room handles are session nicks; they ride *inside* the envelope,
  never on the wire identity. Group membership attaches to the harness;
  dialogue exposure stays per-handle. No role system, no permission model.
- Adoption is a login: JID credentials + host + domain are the whole
  story. A standard client library must connect to any compliant server —
  no fleet-specific protocol, no bespoke wire.
- **Rooms are group names, verbatim**: a chorux group `g` maps to the MUC
  localpart `g` on the configured conference domain. Zero mapping
  doctrine (L2), reflexive formation both ways (L5): joining an absent
  room forms it *instantly* — an unlocked room on formation. A locked
  half-created room is a formation failure, not a formation.

## The wire envelope

- Each sealed event travels as the **docs/02 field data** — a JSON object
  (`event`, `group`, `handle`, `at`, plus `text`/`formats`/`links` for
  `Said`, `trace` for `Thought`) — in the body of one ordinary MUC
  groupchat message from the deployment's nick.
- The **ledger never travels**: the SHA-256 chain formula and its head are
  room-side values (L10). The wire carries event *data*; receivers chain
  their own local records per docs/02, byte-identically for identical
  sequences. docs/02 does not move.
- `Left` is reserved (docs/02): never emitted, never ingested.
- Payloads stay opaque (L2): the adapter serializes and parses the
  envelope fields, and never reads, routes, or interprets `text`.

## Egress and ingress (L9 + L17)

- **Emission is the member's act, not the room's**: the tools surface — the
  only place an intentional act enters (L9) — records into the room *and*
  hands the event to the transport. The room itself never speaks the wire;
  `pulse` is pull-only and transports nothing.
- **Ingress is a member's act too**: a groupchat message from another nick
  is someone's intentional say. The adapter maps the sender's MUC nick to
  the room handle (verbatim — one concept, L18) and replays it through
  the room's own methods (`formOrJoin` / `say` / `think`), so the ledger
  records it exactly as a local member act would.
- **The pulse shows both presences by design** (L19): the harness nick
  (membership attaches to the harness) *and* the session handles (dialogue
  exposure stays per-session-nick). A human client's nick appears once —
  their nick IS their handle (L18: the login is the identity). No gates,
  no member list policing: an uninvited observer joining is the square
  working (L4), never an anomaly.
- **The loop guard lives at the wiring point**: the ingest path never
  re-emits *to the rail*, and the adapter drops reflections of its own
  resource. A mirrored event crosses the rail exactly once per instance.
  LOCAL taps are the other half of the same law: sinks like the stream
  and the ear hear the room's whole timeline — local member acts AND
  ingested rail acts alike — because a local tap cannot loop, and an ear
  that hears only half the room is half an ear.

## Delivery semantics (L12)

- `emit()` is a **claim**, never truth. Delivery is proven by the
  *receiver's* `pulse` showing the event — that oracle is the acceptance
  bar for every deployment and every bridge. No delivery callbacks, acks,
  or retry machinery inside core (L11); the transport owns its retry
  budget outboard (L12) and fails loudly when it cannot connect.

## Configuration (fail-loud, every missing value named at once)

Declared keys — the only way the transport activates; absent or empty
refuses startup with **all** missing values named:

```
chorux.transport.xmpp.enabled        # false/absent = honest single-node
chorux.transport.xmpp.host           # C2S endpoint
chorux.transport.xmpp.port
chorux.transport.xmpp.service-name   # e.g. the XMPP domain
chorux.transport.xmpp.username       # JID localpart
chorux.transport.xmpp.password       # env-injected, never a repo value
chorux.transport.xmpp.resource
chorux.transport.xmpp.muc-domain      # conference domain
chorux.transport.xmpp.nick           # the deployment's MUC nick (harness identity)
chorux.transport.xmpp.trust-ca-pem   # optional: PEM (CA or bundle) for private-CA rails;
                                     # absent = JVM default trust. When set, the CA pin IS
                                     # the trust boundary — chain validation runs against
                                     # the pinned material and the leaf's name is not
                                     # re-checked (private rails present unnamed leaves).
```

A transport that cannot connect **refuses startup** (L11): a deployment
configured for the rail and unable to reach it is down, not degraded — a
live-looking local room with a severed collab plane is exactly the silent
failure L12 exists to prevent.

No silent defaults. No hostnames, addresses, ports, or credentials in any
file that can reach a public tree — placeholders in `application.yml`,
values from the deployment's own environment.

## Capability map (the corpus keeps, consumed — never re-solved)

The 2b matrix's 53 keeps name what the transport must carry, as
capabilities, not mechanisms: mention/inject (say + pulse suffice),
presence/roster and room lifecycle (join forms, occupancy shows), seating
and affiliation (one identity, many handles — L19 above), bridge protocol
(the wire envelope), identity/auth (the login), dispatch/claim and focus
groups (group names as topics, L13). Anything the keeps demanded as a
*mechanism* beyond ordinary chat room behavior is a client concern (L1).

## Verification contract (the oracles)

1. **Parity**: an event emitted through the transport and parsed back is
   field-identical to the original (byte-identical `docs/02` field data).
2. **Round-trip**: a say through one deployment's tools surface appears in
   a second deployment's `pulse`, and a MUC message from an external
   client with a login appears in the local `pulse` — L18's human half.
3. **No new laws**: the tools surface, the envelope, and the chain
   formula are untouched; tests pin all three.
