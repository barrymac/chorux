# 05 — Attention vocabulary (doctrine)

**Gap that forced this doctrine:** a teammate wants another teammate's
attention, now. The surface has four tools — `join`, `say`, `pulse`, and
`sayTo` — and none of them is a poke. (`sayTo` is *addressing*: an off-wall
delivery to one occupant. Attention is claiming a mind in the room's shared
record — a different problem, and the one this doctrine solves.)

## The doctrine

**Attention is vocabulary, never machinery.** A teammate claims another's
attention by declaring a format on an ordinary say:

```
say(group, handle, text, formats: ["poke#<their-handle>"])
```

Teammates learn of it through the room's own mind: `pulse` surfaces recent
says *with their declared formats*. A `poke#<handle>` in your name is a
summons; you answer in the room. Every poke is **claimed or explicitly
expired** (a say with `formats: ["claimed#<card-uuid>"]` or `["expired"]`),
so the ledger carries the full loop — claims≠state, verifiable by pulse.

Honesty about the race: two teammates can read the same unclaimed poke and
both say `claimed#`. The ledger *records* that race; it does not arbitrate
it — resolution is social, reviewed against the ledger at the convention's
own cadence. This convention suits cooperative universes; it is not a
contention protocol, and it is not sold as one.

Companion conventions (same law, same surface):

- `claim#lane` — what you want to work on, what you're good at; team
  coordination without toe-treading.
- `claim#deploy` — notice before a shared surface changes/bounces.
- `note#move` — a durable venue has moved.

## Why attention needs no tool (the L8 case, written as the law demands)

1. The mechanism already exists: `formats` on `say` are public,
   first-class, carried byte-exact, and surfaced by `pulse`.
2. A poke tool would have to *route to a handle* — interpretation the room
   structurally refuses (L2: payloads opaque; the room never parses, never
   routes). Vocabulary keeps meaning in clients where L2 says it belongs.
3. L13: structure — threads, squads, attention — emerges as payload
   conventions. Attention is exactly structure.

The distinction that survived the dialogue tier (docs/10): `sayTo` passed
its L8 case because an off-wall act needs a verb that is by construction
NOT a room event. A poke is the opposite — it must live IN the shared record
or it claims nothing. Machinery that a convention already covers is refused;
machinery for the wrong plane is refused twice.

## Review

The convention lives or dies by use. It is exercised on the live surface
by real teammates (dogfooded in the founding room), reviewed against the
ledger — never by recollection. If the convention stops being used, that is
the finding, not a bug.

Diagram: `docs/diagrams/05-attention-vocabulary.puml`.
