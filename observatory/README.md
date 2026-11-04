# observatory — outboard room metrics for chorux

A single-file, zero-dependency Node/Bun process that watches a chorux room
and derives metrics from what the room says in public — nothing else. It is
the reference occupant of law **L16** (observability attaches outboard): it
consumes the same public SSE seam any listener consumes and the same pulse
any tool can call, and it asks **nothing** of the room that a human client
would not already produce by talking.

```
node observatory/observe.js --url http://<your-room-host>:8080 --group <group> [flags]
```

or `bun observatory/observe.js …`. Node ≥ 18 (a global `fetch`); Bun works
as-is.

## What it never does

- **Never speaks into the room.** It joins no tools, calls no `say`, sends
  nothing on any surface (L9: the harness observes; it does not speak).
- **Never requires instrumentation.** No agent had to think about being
  observed (L17). Everything derives from freely-said payloads: the public
  event stream and volunteered `formats` labels.
- **Is not a component of chorux.** No daemon, no deployment, nothing in
  the core build (L11: a need that would require a new component dissolves
  into a client convention — this is that client). The core diff for the
  whole observability exercise is EMPTY.

## Flags

| flag | meaning |
|---|---|
| `--url` | room base URL, e.g. `http://<your-room-host>:8080` (required) |
| `--group` | group to watch (required; must already have a room — join it first) |
| `--jsonl <file>` | append derived series samples as JSONL |
| `--riemann <host:port>` | opt-in TCP sink of the derived series, hand-encoded Riemann protobuf |
| `--no-dashboard` | suppress the terminal render |
| `--help` | usage |

`--riemann` may also come from `CHORUX_OBSERVATORY_RIEMANN=host:port`.
Absent, the sink is never constructed — observation costs nothing extra,
and if the sink is unreachable the harness says so loudly and keeps
observing. Sink failure never kills observation.

## The metric families (all derived outboard)

**Latency feel** — inter-event gaps and dialogue turn latency, from the
public `at` timestamps: gap p50/p95/p99, turn ms p50/p95/p99 (a Said to the
next different-voice Said), turn step counts. Not server timing; the tempo
a participant would feel.

**Reasoning efficiency** — from volunteered `thinkingTrace` only: thoughts
per answer, thoughts-per-say per handle, steps-between-says. Rooms where
nobody volunteers traces yield no ratios — the dashboard says so instead
of fabricating 0/0.

**Cross-stream correlation** — tickless CEP over the stream itself:
question→answer step counts and latencies, open (unanswered) windows,
repeated-question detection on normalized text.

**Volunteered series** — any `Said` carrying a `metric#<family>` format
label with `key=value` pairs in the text becomes live series, e.g.
`say("recall@3=0.67 ndcg=0.81", formats: ["metric#retrieval"])` → series
`volunteered.retrieval.recall@3` and `…ndcg`. This is a **client
convention**, not a protocol requirement: any `metric#` label works,
unknown ones are ignored, and a room that never volunteers one simply has
an empty section. The names inside `metric#` are the sayer's own
vocabulary — nothing is sanctioned or required.

**Health** — reconnects, replay gaps, heartbeat and event counts, the
watched `Last-Event-ID`.

## Output

- Terminal dashboard (ANSI when on a TTY, plain otherwise), re-rendered on
  every event and every heartbeat — no timer of its own.
- `--jsonl`: one JSON object per derived sample
  `{series, metric, value, at, handle, family, group}` — the derived-series
  artifact, ready for any downstream plotter.
- `--riemann`: the same samples as Riemann `Event`s (state `"ok"` for
  observations, `"warn"` for malformed-envelope notices). The room name
  rides `host`; real hostnames never do.

## Tests

```
node --test observatory/observe.test.js
# or: bun test observatory/
```

19 tests: SSE parsing (chunk splits, CRLF, comments, resume ids),
nearest-rank percentiles, every metric family, golden-byte protobuf
encoding (pinned to the canonical Riemann schema), and fail-loud CLI
argument checking.

## Why outboard, in one paragraph

A metrics dialect inside the room protocol would make every client an
instrument first and a speaker second; instrumentation requirements are
a tax on adoption and a lie about causality (the room's job is talk, not
telemetry). The public seam already carries everything an honest observer
needs: timestamps, voices, volunteered traces, and free-label formats.
So the observer lives outside, pays its own costs, fails its own failures,
and the protocol never learns it exists.
