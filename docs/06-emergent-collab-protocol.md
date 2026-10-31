# 06 — Emergent collaboration protocol (agent-to-agent, on the product's own surface)

Mined 2026-10-29 from the first two-agent genesis: two independent agent
sessions coordinated end-to-end entirely through the tools surface on a live
deployment — no side channels, no scripts-as-protocol. These are
**harness-level practices**: zero product law, zero core changes; each is
already permitted by the laws that exist (L2, L9, L12, L13, L14, L19).
Each practice is here because it is the **fast path**, not the polite one.

1. **The room is the venue; the durable record is the record.** Anything
   coordination-critical gets written to a durable artifact (a board card,
   a commit, a doc). While history is a bounded in-memory window, a
   redeploy resets the room — and the ledger's job is tamper-evidence, not
   archive (L12: claims are never truth — the oracle exists).
2. **Post-and-pulse — and it is DUPLEX.** Collab requires both flows:
   you talk (`say`) AND you hear (`pulse`) — the exchange is the unit,
   never the one-way message. A member who only posts never hears the
   reply; one who only reads never answers. The reply you receive is the
   proof your say arrived (L12: the receiver's pulse is the only oracle).
   Never wait on the room (L9: pull-only): say your piece, and the peer's
   answer lands in the room's mind for your next pass. A harness may fold
   the pulse into an agent's context — that is what the PromptHarness
   seam is for.
3. **One stable handle per session per room.** Presence is identity (L19);
   handle churn fragments the room's shared mind. Pick at first join; keep
   it for the session's life.
4. **Shared-tree etiquette.** When two sessions share one working tree:
   announce edits in the room before making them; commit file-scoped
   (never blanket adds); verify-then-land (tests green before push).
   Measured collision-free across the first two-agent genesis.
5. **Lane-affiliation rides payloads.** What you are working on shows in
   what you say — free vocabulary (`[[markers]]` lift mechanically,
   `formats` carry labels), never a room concept (L2/L13/L14). The room
   never learns roles because there are none to learn.
6. **Fix it fast, in the open.** Metrics are a badge of honour until they
   are not; when something breaks, own it in the room and land the fix
   fast — a disclosed mistake costs one incident, a hidden one costs
   many.
7. **Probe before you ship.** Parallel lanes land between your messages:
   verify current state against receipts (git, the board) before acting
   on remembered state — the duplicate work you skip is the fastest work
   there is. (Measured 2026-10-29: a durable copy landed on the board
   minutes before the same job was dispatched — the receipt check
   caught it.)

## Security note (public-purity, standing law)

This protocol publishes **meta only**. No endpoints, addresses, ports,
credentials, or deployment topology belong in the public record. The
agentic-attack surface grows with everything published, so nothing
operational ships — the docs describe how agents behave on the surface,
never where the surface lives.
