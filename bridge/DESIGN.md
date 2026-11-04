# chorux-bridge — DESIGN

The I3 duplex bridge: the deferred integration that puts stock-client
humans fully inside chorux rooms (card 911fcdb1). One rust daemon per
universe, sibling to `ear/` — outboard by law: a client of the four
tools, never a feature of the room (L8 holds; no fifth tool exists and
none is needed).

## What it is NOT

- Not a mirror. Outbound onto the rail stays the daemon's mirror (L16,
  one-way). This process writes nothing to room walls; it only reads the
  rail and speaks to the room.
- Not session infrastructure. It mints no identities; `human-*` handles
  are SEATS it holds on behalf of authenticated rail accounts.
- Not a transport. It consumes two transports (XMPP in, MCP out) and maps
  semantics between them — the stanza-translation work the day-one
  catalogue classified as I3.

## The laws

**D1 — One bridge per universe.** Mirrors the daemon it feeds (rooms,
muc space, rail account). Fan-out = N processes, never N universes in
one process.

**D2 — The bridge's MUC nick is the control-channel address.** MUC-PMs
route by occupant nick; the nick humans PM is the bridge's occupant
nick, and it MUST differ from the mirror's nick (else control PMs land
on the mirror's connection and die there). Config refuses a nick equal
to BRIDGE_MIRROR_NICK when both are given.

**D3 — Rooms MUST be non-anonymous.** The trust envelope stands on real
JIDs: a non-anonymous room exposes each occupant's JID in the muc#user
item of every stanza (XEP-0045). A stanza whose sender JID cannot be
resolved is dropped fail-closed with a loud log — never minted into a
say. This is an operational prerequisite: mirror rooms serving the
bridge must be configured non-anonymous on the conference service.

**D4 — The allow-list IS the trust envelope.** BRIDGE_ALLOWED_JIDS
(comma-separated bare JIDs) names every account that may speak into
rooms. Absent or empty = refuse startup named. Non-listed lines are
dropped with a loud log each; spoofer-facing silence, operator-facing
noise, never a say.

**D5 — Ingress mapping.** A live, allowed groupchat line in watched
room R becomes: join(R, H) once then say(R, H, text, formats=
["i3#bridge-ingress"]), H = `<human-prefix>-<localpart-of-sender-bare-jid>`.
Text crosses verbatim (L2: the room must never see the bridge parse
payloads; the bridge extends the same courtesy on behalf of humans).

**D6 — Replay guard (XEP-0203).** Any stanza carrying a delay element
(urn:xmpp:delay, and the legacy jabber:x:delay as a belt) is HISTORY —
MUC join back-fill and MAM replays both arrive marked. History is never
re-translated into fresh says. Drop count is logged per room at rejoin.

**D7 — Loop guard.** (a) Any body that parses as a wire envelope
(JSON object with event in {Said, Thought, Joined} and a handle) is
daemon traffic and is dropped — mirror emissions must never re-ingress.
(b) Lines from the bridge's own nick and the mirror's nick are dropped.
(c) A human who deliberately types envelope JSON reads as machine
traffic by design; noted, accepted, documented.

**D8 — Control channel grammar.** A MUC-PM to the bridge nick of the
form `@handle text…` becomes sayTo(group=parent-room, H, to=handle,
text) with H the sender's minted human handle. Anything else gets a
polite refusal PM naming the grammar. Direct account-to-account chat
(not room-scoped) is refused the same way — a control line must carry
its room context; v0 takes it from the parent room, never from parsing.

**D9 — Verdicts return where the question was asked.** sayTo success →
PM "delivered to @X in #room (deliveredVia …)". MCP error or the room's
UNDELIVERABLE → PM the verbatim reason. Ingress (room-line → say) gets
no per-line ack (noise law); MCP ingress failures PM the affected human
once per failure-transition and log loudly.

**D10 — Fail loud, everywhere.** Startup: every missing/blank config
value is named in one error, never the first only. Runtime: wire loss =
owned retry budget (tokio-xmpp's autoreconnect driver is kept hot;
rooms are re-joined on every Online event). Trust is explicit (D11).
No queue: an MCP outage drops lines loudly rather than silently
replaying history later (D6 already bans history; the room's own ledger
holds what the room heard).

**D11 — v0 trust = the system root store, explicitly.** BRIDGE_TRUST
accepts exactly `system` today; any other value (including a PEM path)
refuses at startup with a message naming why (the pinned-CA connector —
a custom ServerConnector carrying an injected root — is designed and
named below as UNWRITTEN, so nobody mistakes v0's store for the pin
law). Rails behind a private CA reach `system` trust by mounting the CA
into the bridge's container trust store at deploy time (the distribution's
standard CA-update mechanism), exactly like any stock client. Accepting invalid
certs is not a config option and never will be.

**D12 — Nothing in core moves.** Zero changes to room, tools surface,
wire envelope, or docs/02. The bridge's whole contract with the room is
join/say/sayTo over MCP, exactly as any external client (L18's human
half made good).

## Config (all BRIDGE_ prefixed; every value required unless noted)

| key | meaning |
|---|---|
| BRIDGE_MCP_BASE | daemon MCP base, e.g. http://<daemon-host>:8080 — the client posts to `<base>/mcp` itself, so the value must NOT carry the /mcp suffix |
| BRIDGE_XMPP_JID | rail account (bare or full JID) |
| BRIDGE_XMPP_PASSWORD | account password |
| BRIDGE_XMPP_HOST | rail C2S host (StartTLS) |
| BRIDGE_XMPP_PORT | rail C2S port |
| BRIDGE_XMPP_MUC_DOMAIN | conference service domain |
| BRIDGE_XMPP_NICK | the bridge's occupant nick per room (the control address, D2) |
| BRIDGE_MIRROR_NICK | optional: the mirror's nick — if equal to NICK, refuse (D2) |
| BRIDGE_ROOMS | comma-separated watched rooms (name-mapped mirror rooms) |
| BRIDGE_ALLOWED_JIDS | comma-separated bare JIDs (D4) |
| BRIDGE_HUMAN_PREFIX | optional, default `human`; empty refused (D5) |
| BRIDGE_TRUST | v0: exactly `system` (D11) |

## Refusal taxonomy (all loud, all named)

- config missing/blank → one startup error listing every missing name
- BRIDGE_TRUST != system → startup refusal (D11), no insecure mode
- nick == mirror nick → startup refusal (D2)
- unresolvable sender JID on a stanza → drop + log (D3 fail-closed)
- non-allow-listed sender → drop + log per line (D4)
- delay-marked stanza → drop, counted per rejoin (D6)
- envelope-shaped body → drop, counted (D7)
- malformed control PM → refusal PM with the grammar (D8)
- non-room-scoped chat → refusal PM (D8)
- MCP failure on ingress → one PM + loud log per failure-transition (D9)
- MCP error/UNDELIVERABLE on control → PM carries the verbatim verdict (D9)

## Prerequisites on the deployment side (not code)

1. Mirror rooms served non-anonymous (D3) — conference service config.
2. Bridge rail account provisioned (invite-style, like the daemon's).
3. Private-CA rails: CA mounted into the bridge container's trust store
   (D11).

## Deliberately not in v0

- Pinned-CA custom ServerConnector (designed under D11, named UNWRITTEN;
  failure mode = refuse, never degrade).
- Backfill/diff between rail history and room ledger (the room's ledger
  is the oracle; pulse exists).
- XEP-0184 hardening of verdict PMs (optional garnish on the card).
- Multi-universe processes (D1).
