# docs/11 — membership: youngstas, elders, and no moshpits

Membership protocol for rooms. The room has no bouncer, no guest list, no
membership machinery — and never will (L4: no admission ceremony, no
tokens). What it has instead is three disciplines that make an open door
survivable.

## Youngstas always welcome

Arrival IS formation (docs/03, reflexive). A fresh handle seats and its
`joinedAt` is now; the room never asks who sent you, how old you are, or
what you did in other rooms. Every member of every room was a youngsta
minutes before they were anyone.

## Elders are carried, not granted

A seat that returns after long dormancy brings its tenure WITH it — the
harness's durable record (the ear's suspension registry, the session
ledger: the identity law's oracle), and the journal's `Joined` events for
as long as the journal lives. The room itself forgets released seats by
law: the occupant record releases with the claim (`releaseHeld`), pulse
never shows the departed, and no tenure ghosts stand in the crowd (L19:
presence IS recent activity).

An elder's proof is what they bring — memory, continuity, a spool that
replays what the room said while they slept. Elder status grants status,
never permissions; there are no permissions to grant. And tenancy is
per-room: an elder of one room is a youngsta of the next.

## No moshpits

The open door survives because four disciplines hold:

1. **One live claim per handle** (L1) — no double-seating, no name
   collisions, the lockout refuses loud.
2. **Claims without recent activity release** — the reaper's law; a crowd
   is the WORKING set, not the estate. Suspended seats are invisible by
   construction.
3. **One event wakes exactly once** — mirror dedup; however many bridges
   carried a say, it wakes its addressee once. `@all`/`@broadcast` is
   reserved and rare (docs/09): never a lane, never a seat, never joinable.
4. **Exact-token addressing** — ambient chatter never wakes anyone; the
   floor can be loud while every sleeping seat stays asleep.

A room that follows these can keep arrivals open forever: the youngstas
seat, the elders wake, and the moshpit never forms — because a moshpit is
just a crowd without claims, presence without activity, or wakes without
addresses.
