# chorux-bridge — HANDOFF

Status as of 2026-10-31 (card 911fcdb1, "I3 duplex bridge").

## What exists

- `DESIGN.md` — the twelve laws (D1–D12), the config table, the refusal
  taxonomy, the deployment prerequisites, the deferred list. Read it
  before touching code; every guard in the code names its law.
- `src/map.rs` — the pure half. Every decision the bridge makes that can
  be tested without a wire is a function here, each with tests:
  intake (D3 muc#user JID extraction, fail-closed), history detection
  (D6 XEP-0203 + legacy XEP-0091), mirror-traffic detection (D7 envelope
  shape), control grammar (D8), handle minting (D5), verdict texts (D9).
- `src/mcp.rs` — the room half: poll.rs's proven MCP shape verbatim
  (initialize → session-id echo → notifications/initialized → id-matched
  replies from SSE-framed or raw bodies). Exactly three tools are ever
  called: join, say, sayTo (D12). `isError` results surface the room's
  refusal text verbatim as Err — the caller forwards it, never launders
  it (D9).
- `src/main.rs` — wiring and the retry budget: env-only config with
  every missing value named in one startup error (D10, refuses with
  rc 2 + usage banner, identical smoke contract to the ear), one
  StartTLS client per universe (D1), rooms re-joined on every Online
  event (MUC back-fill is dropped by D6 automatically, and counted),
  egress PM sends only for the control channel and ingress
  failure-transition notices (D9). 5s redial loop on stream loss.

## What does not exist yet (all named in DESIGN.md §Deliberately-not)

- Pinned-CA custom ServerConnector. Designed under D11, UNWRITTEN by
  choice: in v0 BRIDGE_TRUST accepts exactly `system`; anything else
  refuses at startup saying why. Private-CA rails enter via the
  container's trust store, never via an insecure mode.
- Nightly backfill/diff against rail history (the room's ledger is the
  oracle).
- XEP-0184 receipts on verdict PMs (garnish).
- Multi-universe processes (D1: N universes = N processes).

## E2E acceptance checklist (maps to card 911fcdb1's done criteria)

The five-part done criterion from the card, and how each is proven:

1. **A stock client (no plugins) posts into a watched MUC room → a
   `human-*` say lands in the chorux room.** Prove: manually send one
   line from a stock client; `pulse` the room (or read the ledger) and
   see `human-<localpart>` as the handle, with `formats` containing
   `i3#bridge-ingress` — the provenance marker (D5).
2. **MUC back-fill never re-enters.** Prove: restart the bridge in a
   room with history; the log shows history-dropped lines and no second
   batch of says (D6).
3. **Mirror emissions never re-ingress.** Prove: send a room line as a
   chorux occupant (the mirror publishes it onto the rail); the bridge
   must drop it (envelope shape, D7a — or the mirror-nick check, D7b).
   The log names which.
4. **PM `@handle text` → only the target receives; a verdict PM comes
   back with deliveredVia.** Prove: PM the bridge's nick (D2: its nick
   MUST differ from the mirror's; config refuses otherwise); watch the
   target seat's stream; the sender gets "delivered to @<handle> in
   <room> (deliveredVia …)" or the verbatim UNDELIVERABLE reason
   (D8/D9).
5. **Sender JID comes from muc#user, fail-closed.** Prove: run the
   bridge against an anonymous room — every line drops fail-closed with
   the D3 log naming the missing item (D3). Deployment prerequisite, not
   code: bridge-served rooms must be configured non-anonymous.

## Operational notes

- There is no Cargo.lock committed deliberately: the CI build is the
  compile oracle (house law: no local cargo builds). First CI run pins
  the effective versions; if tokio-xmpp 6's API drifted from what
  DESIGN/main assumed, the fix-forward reads the real job log.
- Deployment prerequisites live in DESIGN.md §Prerequisites (all three
  are configuration, not code): non-anonymous rooms, a provisioned rail
  account, private-CA trust-store mounts when the rail runs its own CA.
- The bridge sits NEXT to the daemon, never in it (D12): same host,
  same MCP port, ordinary external-client posture.
