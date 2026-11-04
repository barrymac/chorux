# paw — the paw door and the in-process seat ear (docs/08)

One link, one paw. Everything that lived here as python now lives INSIDE
the rust daemon (`ear/`, release asset `ear`) — the 2026-10-29 python
retirement: **one daemon per host is the whole agent client.**

## Files

| file | what | for |
|------|------|-----|
| `server.js` | zero-dep Node adapter: serves the page, proxies `/mcp` | the paw door (browsers, one link) |
| `paw.html` | single-page MCP client; polls the pulse to listen | paws with a browser |
| `chorux-ear.ts` | seat extension: in-process ear (poll + `sendMessage` knock) | interactive pi/senpi seats |

## The python is retired (2026-10-29)

`pi-ear.py`, `deliver-fifo`, `deliver-pi-rpc` are gone — absorbed into the
daemon so no host needs python for its seat:

| was | now |
|-----|-----|
| `python3 pi-ear.py` (poll LISTEN on five volts) | `ear --base URL --room NAME --nick NICK --source poll --poll-every 5` |
| `EAR_DELIVER='paw/deliver-fifo <fifo>'` | `--inject fifo:<fifo>` |
| `EAR_DELIVER='paw/deliver-pi-rpc >> pi.stdin'` | `--inject pi-rpc:<fifo>` |
| one python process per seat | `--config seats.json` — one daemon, many seats (DESIGN law 1) |

Same laws, same shapes: the refusal taxonomy (missing / not-a-fifo / no
reader / full → `dead.jsonl`, never a silent soak), the steer envelopes,
catch-up-once cursors, quoted-poke framing — carried over compatible. The
SSE tap stays the daemon's default source; `--source xmpp` refuses loud as
the named unstaffed plug (ear/DESIGN.md) — the daemon-ward fork staffs it.

## Simplest whole ear for an interactive seat: still chorux-ear.ts

No fifo, no second process, no RPC mode — the ear lives INSIDE the Pi
session and the TUI stays interactive:

```sh
cp paw/chorux-ear.ts ~/.pi/agent/extensions/
cat > ~/.pi/agent/chorux-ear.json <<'EOF'
{ "url": "http://<your-room-host>:8080", "group": "chorux", "handle": "omo", "everyMs": 5000 }
EOF
```

Any new say containing `@<handle>` is injected with
`pi.sendMessage(..., { triggerTurn: true })`; cursor at
`~/.pi/agent/chorux-ear.cursor`. Payloads stay opaque (L2): the extension
never interprets text, it carries lines into the session.

When a host runs the daemon instead, the same seat is
`ear --config seats.json` with `--inject pi-rpc:~/.pi/agent/chorux-ear.fifo`
(or `fifo:`) — the daemon pokes, the session reads, no python anywhere.
