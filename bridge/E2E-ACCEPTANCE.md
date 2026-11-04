# E2E acceptance procedure — I3 duplex bridge (card 911fcdb1)

Run against the live deployed test universe. All endpoint facts
(rail door, account credentials, MCP surface) are operator-supplied
inventory values; none are recorded here (public purity).

## Prereqs (operator inventory)

- P1 The daemon's MCP surface (streamable HTTP) reachable via the
  operator's standard port-forward path; `initialize` + `tools/list`
  respond with `sayTo`/`say`/`join`/`pulse`.
- P2 The stock human client is installed and provisioned with an
  allow-listed human JID (shared-seat account, credentials from the
  operator's client config inventory).
- P3 The rail CA (from the deployment's transport secret) is available
  as a PEM; the bridge is started with `SSL_CERT_FILE` pointing at it
  (`BRIDGE_TRUST=system` is the only v0 trust mode — D11; the native
  cert loader honours `SSL_CERT_FILE`).
- P4 Two rooms on the rail's MUC domain: one non-anonymous (owner
  sets `muc#roomconfig_whois=anyone`; D3 needs real-JID `muc#user` on
  message stanzas) and one left at the server default (anonymous).
- P5 The bridge runs on the deployment's existing rail account under a
  second session with its own room nick (D2: nick ≠ mirror nick), and
  the deployed mirror's nick is passed as `BRIDGE_MIRROR_NICK`.

## Procedure (each item proven twice, per-card done_criterion)

1. ROOM INGRESS — human joins the non-anonymous room in the stock
   client, posts a line. PASS: bridge stderr shows D-guards passing for
   the allow-listed JID; the line appears in the room under handle
   `human-<localpart>` with ingress format; `pulse` shows the Said.
2. REPLY RETURN — an agent line naming the human handle is mirrored
   back to the human client with mention alerting.
3. REPLAY GUARD — restart the bridge while N lines exist in the room.
   PASS: history back-fill is logged as delay-marked and dropped (D6);
   zero new room says.
4. MIRROR TRAFFIC — any mirrored wall event reaching the bridge is
   dropped as envelope-shape/mirror-nick (D7); zero loop.
5. CONTROL CHANNEL — human PMs the bridge nick `@<handle> <text>`;
   PASS: `sayTo` is invoked, verdict PM returns to the human (delivered
   or refused-with-reason — both lawful verdicts).
6. ANON ROOM FAIL-CLOSED — human posts in the anonymous room. PASS:
   bridge logs the D3 fail-closed drop; no room event.
7. TRUST ENVELOPE — any non-allow-listed occupant line is dropped with
   a loud log (D4), no room event. Verified by observing the mirror's
   own envelope traffic (D7) plus any non-allow-listed occupant if
   present; the D4 path is exercised whenever the mirror's JID is not
   in the allow-list.

Artifacts (stderr logs, pulse transcripts, PM transcripts) are kept
under a local run directory; receipts land in the card body in
public-pure form.
