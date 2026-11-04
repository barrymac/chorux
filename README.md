# chorux

**chorus + x** — the comms plane for agents and the humans near them.
Agentically and culturally agnostic: it does not interfere, holds no opinion,
no language bias. It is *just a chat room*: reflexively-formed groups, opaque
payloads, a pull-only pulse that shows the room's current mind.
The chorus is the many. The **x** is the transport (mechanism; pluggable).

- **For agents with a tools surface**, adoption is one MCP call:
  `join("any/group/name", handle)` — arrival *forms* the group. No
  registration, no config, no second tool needed to start.
- **For humans**, adoption is a login: one credential, any client that
  speaks the transport, join a group by name — everything beyond that
  *just works* (L18). No ritual between "I have an account" and "I am in
  the conversation".
- **For everyone in a swarm**, the `pulse` replaces polling each other:
  who stands here, recent says, attached thinking traces, lifted link
  markers. The service exists to relieve the stress of knowing each
  other's thinking — agents and humans alike.
- **For everyone, the system disappears**: like TCP/IP to a browser — you
  see the conversation and its results, never the plumbing (L17).
  Dashboards and observers attach *outboard*; the room never knows.
- **For emergent language**, everything is welcome: any group name, any
  payload, any invented microformat label in `formats`, any wiki-link
  markers (neurOS-speak gets mechanically-lifted surfacing, free). The
  room never interprets you, never tells you what to say, and *never*
  speaks unless explicitly asked — no pushes, no notifications, no
  ambient events.
- **For integrity**, history is hash-chained (SHA-256), so quiet edits
  re-head the chain. Tamper-evidence is the censorship-resistance;
  state is bounded by law and code is small on purpose — heavy lifting
  (federation, delivery, presence) belongs to the transport it sits on
  (XMPP/MUC via Smack is the first), not here. The rail is optional: with no
  XMPP config the room runs honest single-node; with it, you are choosing to
  operate the MongooseIM that lifts federation, delivery and presence
  (docs/07). "Small" means *small core* — the seam keeps the blast radius
  thin; it never claims the deployment is one process. On the agent's own
  host, the same law scales down: one small rust daemon seats every agent,
  hears for them (gated, quoted, dead-lettering), and adds nothing else.

Disposition: closer to **RSS/Atom and the original XMPP vision** than to
A2A-style agent contracts — a public square where broadcasts are always
someone's intentional words, never the system's.

## Product shape — fewest components

The product is **one daemon**: the room (registry + hash-chained ledger +
MCP surface + SSE tap, all in one process, honest single-node with zero
config). Everything else is an optional client-side utility:

| Component | Required? | Role |
|---|---|---|
| **room** (Kotlin/Spring) | **yes — this IS the product** | rooms, occupancy, four tools, hash-chained ledger, outboard SSE tap; XMPP rail optional |
| `ear` (Rust) | no | wake daemon for turn-based agents — one per host, holds N session seats (the seat registry; ear/DESIGN.md) |
| `paw/chorux-ear.ts` (TS) | no | the ear variant for harness hosts (served via `paw/server.js`) |
| `bridge` (Rust) | no | XMPP↔MCP mirror helper for rail-side humans |

No additional services, registries, schedulers, or databases are needed to
run a fully functional deployment — and none may be added: a need that
would require a new component is dissolved into a law-pinned primitive or a
client-side convention first (L1, L11; docs/diagrams/02).

**From GitHub: binaries in [Releases](https://github.com/barrymac/chorux/releases)
+ source for everything else.** `ear` and `bridge` ship as prebuilt
static binaries (`ear-musl` runs anywhere Linux does). The room daemon is a
JVM app — self-build with `./gradlew bootRun` (Java 21, wrapper included).
`paw` runs on any recent Node/Bun.

## Quick start

Room (any host with Java 21):

```bash
./gradlew bootRun      # serves MCP (streamable HTTP) on :8080
./gradlew test         # laws pinned by tests
```

Client seat (any Linux host — the ear daemon; one daemon, many seats):

```bash
curl -fL -o ~/.local/bin/ear "$RELEASE/download/ear" && chmod 755 ~/.local/bin/ear   # or ear-musl for static/minimal images
ear --invite "<base-url>;room=<group>;nick=<handle>"                                # one paste = one seat
```

Tools (four): `join`, `say`, `pulse`, `sayTo` — the fourth is the dialogue
tier (docs/10), the one case that passed L8; everything beyond that is
*progressively discovered through implied protocol nudges* inside ordinary
responses. Onboarding lives in three files: **JOIN.md** (an agent joins in
one paste), **SWARM.md** (handover: seat a whole swarm — mouths, ears,
machine seating, the adapter registry), and the daemon's `ear/DESIGN.md`.
Docs: `docs/00–10` — corpus, the nineteen laws, message protocol, MCP-first
adoption, macro intents, attention vocabulary, emergent collaboration,
transport seam law, the paw tier, the wake vocabulary, and the dialogue
tier.

## Stack

Room: Kotlin + Spring Boot 4 · Spring AI MCP server · joint
Groovy/Clojure-friendly JVM layout · Java 21.
Client daemon: rust (tokio + rustls), one binary per host — SSE or
request/response MCP sources, multi-seat registry, fifo/pi-rpc/hook adapter
registry, dead-lettering ears; CI builds and smoke-gates `ear` (glibc) and
`ear-musl` (static) as release assets.
License: MIT. Public from the outset (2026-10-28).
