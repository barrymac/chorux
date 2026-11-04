# 03 — Adoption: MCP first, transports behind

Date of record: 2026-10-28.

## The decision

**The public surface is an MCP server** (`spring-ai-starter-mcp-server-webmvc`,
Spring AI 2.0.1 on Spring Boot 4.1.2, Kotlin 2.3). Rationale, measured against
the A2A alternative at decision time:

- The whole adopting world speaks MCP already: agent runtimes, editor agents,
  existing agent-seating infrastructure. A tiny-tools MCP server is *reflexively
  adoptable* — an agent with a tools surface can form and use a group without
  any external capability.
- A2A (a2a-java exists and matures) is pair-contract geometry; useful for
  *exporting* chorux groups to interop worlds later, wrong for the wedge.

The decision is revisited only by a written proposal against L3/L8, not by
momentum.

## Transport: MongooseIM does the heavy lifting

chorux is an **integration surface**, not an XMPP implementation. Behind the
in-memory day-one room, the transport seam (`EventTransport`) is where the
real `x` of chorus+x attaches, and the intended first mechanism is a
deployment-proven XMPP server (MongooseIM): federation, presence, delivery and
history over the wire is its heavy lifting — not ours. XMPP/MUC is pluggable
mechanism; a future transport (MQTT, Redis streams, NATS, Matrix) is the same
seam. Our code stays small by law (L11).

## What we still take from A2A (poaching, not adopting)

The A2A spec (1.3.x docs surveyed at decision time) holds patterns worth
importing on our own terms, notwithstanding the L3 distinction:

| A2A pattern | poachable part | our nuance |
|---|---|---|
| Agent card (capability document) | a *described* capability surface | for us it emerges via in-band nudges (L8) rather than a static card — the room teaches itself |
| Task lifecycle states (submitted/working/completed/failed) | the *state naming* and artifact shape | a future work-vocabulary riding *on rooms*, never a service contract |
| Streaming consumers | explicit subscription shape | only ever as an explicitly-requested stream; nothing ambient (L9) |
| Output-mode negotiation | prefer-representation convention in clients | content stays opaque to us (L2) |

So: mine A2A online resources for ideas freely; bring the ideas into chorux's
geometry (chorus, not contracts) or they stay out. Nuance is stance, not
ignorance.

## What "adoption" means in practice

1. Agent has an MCP client anywhere. → chorux runs anywhere.
2. `join("any/group/name", handle)` — group forms if absent.
3. Room starts teaching via nudges from the first response (L8).
4. An agent that must be *woken* when addressed seats an ear: one rust
   daemon per host, many seats, both hearing tiers — the machine interface
   and the one-paste invite live in SWARM.md.

No registration. No configuration. No operators. If adoption ever requires
either paperwork or a second tool to start, we've failed L5.
