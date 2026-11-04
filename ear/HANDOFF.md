# HANDOFF — the ear passes to the client lane (2026-10-29)

You own `ear/` from here. This is not a wish — a WORKING v0 exists and is
loop-proven. Build on it; delete anything below that disagrees with what you
measure, but cite what you measured.

## What exists

- `ear/DESIGN.md` — the laws (one exchange per host; fetch/pull split; wake
  rights as authorization; QUOTED pokes; no XMPP client in the ear; honest
  liveness; three tiers one binary)
- `ear/Cargo.toml` + `ear/src/main.rs` — v0 single-file rust ear
  (tokio + reqwest rustls + pico-args + serde_json; `cargo build` → done)
- Serving half (the server lane's, already on main): `RoomEventStream` —
  GET `/rooms/{group}/events.stream`, SSE with per-group `id:` sequencing,
  `Last-Event-ID` replay ring (lossless resume while the ring remembers),
  honest `:gap` comment when the past scrolled, `:hb` every 15s so tunnels
  can't strangle idle streams. The client lane's `docs/07` seam law is the
  tap point.

## Proven live tonight (local loopback room, MCP curl speaker)

1. wake on `@nick`, silence on non-mention lines (gate fidelity)
2. framed delivery: `room line from <handle>: <text>` (quoted-data law)
3. spool jsonl + `--hook` stdin delivery, dead-letter on hook failure
4. server KILLED mid-stream → backoff reconnect 1s→2s→4s… cap 30s, jitter-todo
5. 404 absent room → loud waiting-for-formation, never death (fixed mid-test:
   an ear must out-wait a server bounce, rooms form on join)
6. server restart → reconnect + honest `:gap` note in the spool
7. clean-state single delivery, last-id persisted across process restarts

## Yours next (ranked by dog-journey value)

1. **state-dir seat lock** — two ears sharing one `--state` dir double-deliver
   (measured on this host). flock the state dir; refuse a second seat loudly.
2. **`--invite "<one paste string>"`** — the dog tier from DESIGN.md:
   parse one token blob into base/room/nick. Dumb-human bar: one paste.
3. **tests** — SSE parser + gate as unit tests; e2e against a local chorux.
4. **release line** — musl static build, CI job beside the java one,
   gitea/github release binaries = the curl-install moment. (Coordinate with
   the pipeline owner on `.woodpecker.yml`; repo is public — secrets fail
   loud, named.)
5. **JOIN.md dog-tier refresh** — only after a FOREIGN dog rides it for real.
6. Later: wake-policy ACLs in config (who may poke — operator-only / roster /
   open; default tight). The gate is authorization, not just filtering.

## Not yours (stay off)

- Server stream semantics (`RoomEventStream.kt` + its tests) — the server
  lane holds that. (Outage handling on the ear side is law, not anecdote:
  backoff, watchdog, `:gap` honesty, out-wait-the-server — ear/DESIGN.md.)
- The room's own `docs/07` + XMPP transport — obviously.

## Tone note

The one bug class to fear: silent loss masquerading as delivery. When in
doubt, log loudly and dead-letter; never drop a wake quietly.
