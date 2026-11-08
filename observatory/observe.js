#!/usr/bin/env node
// observatory/observe.js — the chorux outboard observability harness
//
// Card 661315ce. Observability attaches OUTBOARD (L16/L17): this file is a
// CLIENT. It taps the room's public SSE seam (GET /rooms/{group}/events.stream,
// the listening half in surface/RoomEventStream.kt) and derives metric
// families purely from freely-said payloads. The core diff is EMPTY — no
// metrics dialect, no required instrumentation, no named stack near the
// protocol; no agent ever had to think about being observed (L17).
//
// Laws honored here:
//   L2  — payloads stay opaque to core; outboard, the harness reads text as
//         DATA to measure, never as instructions to follow.
//   L9  — the harness observes; it does not speak into rooms. It never
//         calls say/sayTo/join. It never forms a room (rooms form on join;
//         a 404 is a loud exit, never an invitation).
//   L11 — no new daemons or components: one file, run by an operator who
//         wants to watch. No server, no port, no service.
//   L13/L14 — the only opt-in is free vocabulary: any say whose FORMATS
//         label starts with `metric#<family>` is parsed for key=value
//         tokens and becomes a series. Free labels, zero protocol change,
//         zero required instrumentation.
//   Directive 21 — optional Riemann TCP sink, behind --riemann / env.
//         Absent = zero cost: no socket, no module load, no bootup tax.
//
// Zero dependencies. Runs on Node >= 18 and Bun. No package.json, no lock,
// no install: `node observatory/observe.js --url http://<host>:8080 --group ops`.

'use strict';

function usage(exitCode, stream, message) {
  const out = stream || process.stderr;
  if (message) out.write('error: ' + message + '\n');
  out.write([
    'chorux observatory — outboard observer on the public SSE seam',
    '',
    'usage: observe.js --url <base> --group <group> [options]',
    '',
    'required:',
    '  --url <base>        room base URL, e.g. http://room-host:8080',
    '  --group <group>     the group to observe (must already exist:',
    '                      rooms form on join — the observer never forms one)',
    '',
    'options:',
    '  --jsonl <path>      append derived series samples as JSON lines',
    '  --riemann <h:p>     opt-in Riemann TCP sink (or env',
    '                      CHORUX_OBSERVATORY_RIEMANN); zero cost when absent',
    '  --no-dashboard      do not render the dashboard (jsonl/riemann only)',
    '',
    'The observer never speaks into rooms (L9) and never forms a room:',
    'a 404 on the tap is a loud exit, never a join.',
    '',
  ].join('\n'));
  if (exitCode !== undefined) process.exit(exitCode);
}

class UsageError extends Error {}

function parseArgs(argv) {
  const cfg = { url: null, group: null, jsonl: null, riemann: null, dashboard: true };
  const args = argv.slice(2);
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    const next = () => {
      if (i + 1 >= args.length) throw new UsageError('missing value for ' + a);
      return args[++i];
    };
    if (a === '--url') cfg.url = next();
    else if (a === '--group') cfg.group = next();
    else if (a === '--jsonl') cfg.jsonl = next();
    else if (a === '--riemann') cfg.riemann = next();
    else if (a === '--no-dashboard') cfg.dashboard = false;
    else if (a === '--help' || a === '-h') { usage(0, process.stdout); }
    else throw new UsageError('unknown flag: ' + a);
  }
  if (!cfg.url) throw new UsageError('--url is required');
  if (!cfg.group) throw new UsageError('--group is required');
  if (cfg.group.includes('/')) throw new UsageError('--group must not contain a slash (zero-mapping law)');
  if (!cfg.riemann && process.env.CHORUX_OBSERVATORY_RIEMANN) {
    cfg.riemann = process.env.CHORUX_OBSERVATORY_RIEMANN;
  }
  if (cfg.riemann && !/^[^:\s]+:\d+$/.test(cfg.riemann)) {
    throw new UsageError('--riemann must be host:port, got "' + cfg.riemann + '"');
  }
  if (cfg.url.endsWith('/')) cfg.url = cfg.url.slice(0, -1);
  return cfg;
}

// Line-oriented SSE state machine (server emits id:, name:, data:, and the
// comments :hb / :gap — surface/RoomEventStream.kt). Handles CRLF, multi-line
// data, and ignores unknown fields per the SSE spec's ignore-unknown rule.
class SseParser {
  constructor(onEvent, onComment) {
    this.onEvent = onEvent;
    this.onComment = onComment;
    this.buffer = '';
    this.id = null;
    this.name = null;
    this.dataLines = [];
  }

  push(chunk) {
    this.buffer += chunk;
    let idx;
    while ((idx = this.buffer.indexOf('\n')) !== -1) {
      const line = this.buffer.slice(0, idx).replace(/\r$/, '');
      this.buffer = this.buffer.slice(idx + 1);
      this.line(line);
    }
  }

  line(line) {
    if (line === '') {
      if (this.dataLines.length > 0) {
        this.onEvent({
          id: this.id,
          name: this.name,
          data: this.dataLines.join('\n'),
        });
      }
      this.name = null;
      this.dataLines = [];
      return;
    }
    if (line.startsWith(':')) {
      this.onComment(line.slice(1).trim());
      return;
    }
    const colon = line.indexOf(':');
    const field = colon === -1 ? line : line.slice(0, colon);
    let value = colon === -1 ? '' : line.slice(colon + 1);
    if (value.startsWith(' ')) value = value.slice(1);
    if (field === 'id') this.id = value;
    else if (field === 'name') this.name = value;
    else if (field === 'data') this.dataLines.push(value);
  }
}

// Nearest-rank percentile over an ascending-sorted array. Honest for small n.
function pct(sortedAsc, p) {
  const n = sortedAsc.length;
  if (n === 0) return null;
  const rank = Math.ceil((p / 100) * n);
  return sortedAsc[Math.min(n - 1, Math.max(0, rank - 1))];
}

function sortedPct(arr, p) {
  if (arr.length === 0) return null;
  const s = [...arr].sort((a, b) => a - b);
  return pct(s, p);
}

// The envelope's `at` reaches the tap as it was said: epoch ms or an ISO
// instant (possibly nanosecond precision). Return epoch ms or null — never
// NaN (a NaN at poisons every comparison downstream).
function toMs(a) {
  if (typeof a === 'number') return Number.isFinite(a) ? a : null;
  if (typeof a !== 'string') return null;
  if (/^-?\d+$/.test(a)) return Number(a);
  const t = Date.parse(a);
  return Number.isFinite(t) ? t : null;
}

function fmtMs(v) {
  if (v === null || v === undefined) return '—';
  if (v >= 10000) return (v / 1000).toFixed(1) + 's';
  if (v >= 1000) return (v / 1000).toFixed(2) + 's';
  return Math.round(v) + 'ms';
}

function fmtNum(v) {
  if (v === null || v === undefined) return '—';
  return typeof v === 'number' ? (Number.isInteger(v) ? String(v) : v.toFixed(2)) : String(v);
}

// Outboard reads of freely-said payloads (never core routing, L2):
const IS_QUESTION = (text) => /\?\s*$/.test((text || '').trim());
const normText = (text) =>
  (text || '').toLowerCase().replace(/[^a-z0-9\s]/g, ' ').replace(/\s+/g, ' ').trim();
const KV_TOKEN = /([A-Za-z0-9_.@/-]+)\s*=\s*(-?\d+(?:\.\d+)?)/g;
const METRIC_PREFIX = 'metric#';

const MAX_SAMPLES = 1000; // per distribution — bounded memory, honest n
const TICKER_LEN = 12;

class RoomMetrics {
  constructor(group) {
    this.group = group;
    this.seq = 0;
    this.events = 0;
    this.malformed = 0;
    this.hb = 0;
    this.gapComments = 0;

    this.prevAt = null;
    this.gaps = [];
    this.pendingTurn = null; // {handle, at, seq} — last Said, awaiting a different voice
    this.turns = [];
    this.turnSteps = [];

    this.handles = new Map(); // handle -> {says, thoughts, sinceLastSay}
    this.stepsBetween = [];

    this.windows = new Map(); // asker handle -> {norm, at, seq, preview}
    this.questions = 0;
    this.answered = 0;
    this.stepsToAnswer = [];
    this.answerLatency = [];
    this.seenQuestions = new Map();
    this.repeats = [];
    this.repeatDistances = [];

    this.series = new Map(); // seriesName -> {value, at, handle}
    this.seriesOrder = [];

    this.ticker = [];
  }

  handleStat(handle) {
    let h = this.handles.get(handle);
    if (!h) {
      h = { says: 0, thoughts: 0, sinceLastSay: 0 };
      this.handles.set(handle, h);
    }
    return h;
  }

  pushSample(arr, v) {
    arr.push(v);
    if (arr.length > MAX_SAMPLES) arr.shift();
  }

  // Pure derivation, no I/O: returns the series points the sinks consume.
  ingest(ev) {
    const points = [];
    this.seq++;
    this.events++;
    const at = toMs(ev.at); // null when the envelope's at is unusable — time metrics skip, counts stay honest
    const t = ev.type;

    if (this.prevAt !== null && at !== null && at >= this.prevAt) {
      this.pushSample(this.gaps, at - this.prevAt);
      points.push({ ts: at, metric: 'latency.gap_ms', value: at - this.prevAt, handle: ev.handle });
    }
    if (at !== null) this.prevAt = at;

    // A Said leaves a pending turn; the next Said or volunteered Thought
    // from a different voice closes it (presence events do not).
    if (t === 'Said' || t === 'Thought') {
      if (this.pendingTurn && this.pendingTurn.handle !== ev.handle) {
        const steps = this.seq - this.pendingTurn.seq;
        if (at !== null && this.pendingTurn.at !== null) {
          const ms = at - this.pendingTurn.at;
          this.pushSample(this.turns, ms);
          points.push({ ts: at, metric: 'latency.turn_ms', value: ms, handle: ev.handle });
        }
        this.pushSample(this.turnSteps, steps);
        points.push({ ts: at, metric: 'latency.turn_steps', value: steps, handle: ev.handle });
      }
      if (t === 'Said') this.pendingTurn = { handle: ev.handle, at, seq: this.seq };
    }

    // Nobody volunteered traces → no ratios; the dashboard says so honestly
    // instead of fabricating 0/0.
    const h = this.handleStat(ev.handle);
    if (t === 'Thought') {
      h.thoughts++;
      h.sinceLastSay++;
    } else if (t === 'Said') {
      if (h.sinceLastSay > 0) {
        this.pushSample(this.stepsBetween, h.sinceLastSay);
        points.push({ ts: at, metric: 'reasoning.thoughts_per_answer', value: h.sinceLastSay, handle: ev.handle });
      }
      h.says++;
      h.sinceLastSay = 0;
    }

    if (t === 'Said') {
      if (IS_QUESTION(ev.text)) {
        this.questions++;
        const norm = normText(ev.text);
        const seen = this.seenQuestions.get(norm);
        if (seen) {
          const distance = this.seq - seen.seq;
          this.repeats.push({ norm, distance });
          this.pushSample(this.repeatDistances, distance);
          points.push({ ts: at, metric: 'cep.repeat_distance_events', value: distance, handle: ev.handle });
        }
        this.seenQuestions.set(norm, { seq: this.seq, at });
        this.windows.set(ev.handle, { norm, at, seq: this.seq, preview: preview(ev.text) });
      }
      if (this.windows.size > 0) {
        for (const [asker, w] of [...this.windows]) {
          if (asker !== ev.handle) {
            const steps = this.seq - w.seq;
            this.answered++;
            this.pushSample(this.stepsToAnswer, steps);
            points.push({ ts: at, metric: 'cep.steps_to_answer', value: steps, handle: ev.handle });
            if (at !== null && w.at !== null) {
              this.pushSample(this.answerLatency, at - w.at);
              points.push({ ts: at, metric: 'cep.answer_latency_ms', value: at - w.at, handle: ev.handle });
            }
            this.windows.delete(asker);
          }
        }
      }
    }

    if (t === 'Said' && Array.isArray(ev.formats)) {
      const label = ev.formats.find((f) => typeof f === 'string' && f.startsWith(METRIC_PREFIX));
      if (label) {
        const family = label.slice(METRIC_PREFIX.length);
        KV_TOKEN.lastIndex = 0;
        let m;
        while ((m = KV_TOKEN.exec(ev.text || '')) !== null) {
          const name = 'volunteered.' + family + '.' + m[1];
          const value = Number(m[2]);
          if (!this.series.has(name)) this.seriesOrder.push(name);
          this.series.set(name, { value, at, handle: ev.handle });
          points.push({ ts: at, metric: name, value, handle: ev.handle });
        }
      }
    }

    this.ticker.push({ t, handle: ev.handle, preview: t === 'Thought' ? preview(ev.trace) : preview(ev.text) });
    if (this.ticker.length > TICKER_LEN) this.ticker.shift();

    return points;
  }

  onComment(comment) {
    if (comment === 'hb') this.hb++;
    else if (comment === 'gap') this.gapComments++;
  }

  snapshot() {
    const latency = {
      gap: { n: this.gaps.length, p50: sortedPct(this.gaps, 50), p95: sortedPct(this.gaps, 95), p99: sortedPct(this.gaps, 99) },
      turn: {
        n: this.turns.length,
        p50: sortedPct(this.turns, 50),
        p95: sortedPct(this.turns, 95),
        stepsP50: sortedPct(this.turnSteps, 50),
      },
    };
    const reasoning = {
      handles: [...this.handles.entries()]
        .map(([handle, h]) => ({
          handle,
          says: h.says,
          thoughts: h.thoughts,
          perSay: h.says > 0 ? h.thoughts / h.says : null,
        }))
        .sort((a, b) => b.says + b.thoughts - (a.says + a.thoughts)),
      stepsBetween: { n: this.stepsBetween.length, p50: sortedPct(this.stepsBetween, 50) },
      anyThoughts: this.stepsBetween.length > 0 || [...this.handles.values()].some((h) => h.thoughts > 0),
    };
    const cep = {
      open: [...this.windows.entries()].map(([handle, w]) => ({ handle, preview: w.preview, at: w.at })),
      questions: this.questions,
      answered: this.answered,
      stepsToAnswer: { n: this.stepsToAnswer.length, p50: sortedPct(this.stepsToAnswer, 50), p95: sortedPct(this.stepsToAnswer, 95) },
      answerLatency: { p50: sortedPct(this.answerLatency, 50) },
      repeats: this.repeats.length,
      lastRepeatDistance: this.repeats.length ? this.repeats[this.repeats.length - 1].distance : null,
    };
    const volunteered = this.seriesOrder.map((name) => ({ series: name, ...this.series.get(name) }));
    return {
      group: this.group,
      seq: this.seq,
      events: this.events,
      malformed: this.malformed,
      hb: this.hb,
      gapComments: this.gapComments,
      latency,
      reasoning,
      cep,
      volunteered,
      ticker: [...this.ticker],
    };
  }
}

function preview(text) {
  const s = (text || '').replace(/\s+/g, ' ').trim();
  return s.length > 60 ? s.slice(0, 57) + '…' : s;
}

// Hand-encoded protobuf (schema: riemann proto.proto — Event{time=1 int64,
// state=2, service=3, host=4, description=5, tags=7, ttl=8 float,
// attributes=9, time_micros=10, metric_sint64=13, metric_d=14 double,
// metric_f=15 float}; Msg{ok=2, error=3, states=4, query=5, events=6};
// Attribute{key=1, value=2}). No schema tooling, no dependency — the wire
// format is small and stable.

function varint(n) {
  const out = [];
  let v = n;
  do {
    let b = v & 0x7f;
    v = Math.floor(v / 128);
    if (v !== 0) b |= 0x80;
    out.push(b);
  } while (v !== 0);
  return out;
}

function tag(field, wireType) {
  return varint((field << 3) | wireType);
}

function encStr(field, s) {
  const bytes = Buffer.from(String(s), 'utf8');
  return [...tag(field, 2), ...varint(bytes.length), ...bytes];
}

function encVarintField(field, n) {
  return [...tag(field, 0), ...varint(n)];
}

function encDoubleField(field, v) {
  const buf = Buffer.alloc(8);
  buf.writeDoubleLE(v, 0);
  return [...tag(field, 1), ...buf];
}

function encAttribute(key, value) {
  const body = [...encStr(1, key), ...(value === undefined ? [] : encStr(2, value))];
  return [...tag(9, 2), ...varint(body.length), ...body];
}

function encodeRiemannEvent(ev) {
  let out = [];
  if (ev.time !== undefined) out.push(...encVarintField(1, Math.floor(ev.time)));
  if (ev.state !== undefined) out.push(...encStr(2, ev.state));
  if (ev.service !== undefined) out.push(...encStr(3, ev.service));
  if (ev.host !== undefined) out.push(...encStr(4, ev.host));
  if (ev.description !== undefined) out.push(...encStr(5, ev.description));
  for (const t of ev.tags || []) out.push(...encStr(7, t));
  for (const a of ev.attributes || []) out.push(...encAttribute(a.key, a.value));
  if (ev.metric_d !== undefined) out.push(...encDoubleField(14, ev.metric_d));
  return out;
}

function encodeRiemannMsg(events) {
  let out = [];
  for (const e of events) {
    const body = encodeRiemannEvent(e);
    out.push(...tag(6, 2), ...varint(body.length), ...body);
  }
  return Buffer.from(out);
}

// Opt-in TCP sink. Absent from the CLI = the class is never constructed:
// no socket, no net module, zero bootup cost (directive 21).
class RiemannSink {
  constructor(hostPort, { group } = {}) {
    const i = hostPort.lastIndexOf(':');
    this.host = hostPort.slice(0, i);
    this.port = Number(hostPort.slice(i + 1));
    this.group = group;
    this.sock = null;
    this.lastLoud = 0;
    this.sent = 0;
  }

  loud(err) {
    const now = Date.now();
    if (now - this.lastLoud > 10000) {
      this.lastLoud = now;
      process.stderr.write('observatory: riemann sink unreachable (' + (err && err.code ? err.code : String(err)) + ') — observation continues\n');
    }
  }

  connect(cb) {
    const net = require('net'); // loaded only when the sink is opted in
    this.sock = net.connect(this.port, this.host);
    this.sock.on('error', (err) => {
      this.loud(err);
      try { this.sock.destroy(); } catch (_) {}
      this.sock = null;
    });
    this.sock.on('connect', () => cb && cb());
  }

  send(points) {
    if (!points || points.length === 0) return;
    const events = points.map((p) => {
      const family = p.metric.split('.')[0];
      const ev = {
        time: p.ts == null ? undefined : Math.floor(p.ts / 1000),
        time_micros: undefined,
        service: 'chorux.observatory.' + this.group + '.' + p.metric,
        host: this.group, // host = the room; real hostnames never ride
        description: 'derived from freely-said payloads (outboard, L16)',
        tags: ['chorux', 'observatory', family],
        metric_d: p.value,
      };
      if (p.handle) ev.attributes = [{ key: 'handle', value: p.handle }]; // cardinality rides the payload, never the service name
      return ev;
    });
    const payload = encodeRiemannMsg(events);
    const write = () => {
      try {
        this.sock.write(payload);
        this.sent += events.length;
      } catch (err) {
        this.loud(err);
      }
    };
    if (!this.sock) this.connect(write);
    else write();
  }
}

const C = {
  dim: (s) => (process.stdout.isTTY ? '\x1b[2m' + s + '\x1b[0m' : s),
  bold: (s) => (process.stdout.isTTY ? '\x1b[1m' + s + '\x1b[0m' : s),
};

function renderDashboard(snap, extras) {
  const L = [];
  L.push(C.bold('chorux observatory — group "' + snap.group + '"') + C.dim('  (outboard: the room never knows)'));
  L.push(
    C.dim('events ' + snap.events +
      ' · malformed ' + snap.malformed +
      ' · heartbeats ' + snap.hb +
      ' · replay gaps ' + snap.gapComments +
      ' · reconnects ' + (extras.reconnects || 0) +
      ' · lastEventId ' + (extras.lastEventId || '—'))
  );

  L.push('');
  L.push(C.bold('── latency feel (zero instrumentation) ' + C.dim('— inter-event tempo + dialogue turns')));
  const g = snap.latency.gap;
  L.push('  inter-event gap   p50 ' + fmtMs(g.p50) + '   p95 ' + fmtMs(g.p95) + '   p99 ' + fmtMs(g.p99) + C.dim('  (n=' + g.n + ')'));
  const t = snap.latency.turn;
  L.push('  dialogue turn     p50 ' + fmtMs(t.p50) + '   p95 ' + fmtMs(t.p95) + '   steps p50 ' + fmtNum(t.stepsP50) + C.dim('  (n=' + t.n + ')'));

  L.push('');
  L.push(C.bold('── reasoning efficiency (volunteered traces only)'));
  if (snap.reasoning.handles.length === 0) {
    L.push('  no says observed yet');
  } else {
    for (const h of snap.reasoning.handles.slice(0, 8)) {
      L.push('  ' + pad(h.handle, 16) + ' says ' + pad(h.says, 4) + ' thoughts ' + pad(h.thoughts, 4) +
        '  thoughts/say ' + fmtNum(h.perSay));
    }
    const sb = snap.reasoning.stepsBetween;
    L.push(C.dim('  thoughts-between-says p50 ' + fmtNum(sb.p50) + ' (n=' + sb.n + ')' +
      (snap.reasoning.anyThoughts ? '' : ' — nobody volunteered traces this window; no ratios fabricated')));
  }

  L.push('');
  L.push(C.bold('── cross-stream correlation (tickless CEP — fires on arrival)'));
  const c = snap.cep;
  L.push('  questions ' + c.questions + ' · answered ' + c.answered +
    ' · steps-to-answer p50 ' + fmtNum(c.stepsToAnswer.p50) + '  p95 ' + fmtNum(c.stepsToAnswer.p95) +
    ' · answer latency p50 ' + fmtMs(c.answerLatency.p50));
  L.push('  repeat questions ' + c.repeats + (c.repeats ? ' (last distance ' + c.lastRepeatDistance + ' events — memory-miss proxy)' : ''));
  for (const w of c.open) {
    L.push(C.dim('  open window @' + w.handle + ': "' + w.preview + '"'));
  }

  L.push('');
  L.push(C.bold('── volunteered series (metric# free labels — zero requirement)'));
  if (snap.volunteered.length === 0) {
    L.push(C.dim('  none yet — agents may volunteer via formats ["metric#<family>"] + key=value text'));
  } else {
    for (const s of snap.volunteered.slice(-12)) {
      L.push('  ' + pad(s.series, 42) + ' ' + pad(fmtNum(s.value), 10) + ' by ' + s.handle);
    }
  }

  L.push('');
  L.push(C.bold('── ticker'));
  if (snap.ticker.length === 0) L.push(C.dim('  (silent room)'));
  for (const e of snap.ticker) {
    L.push('  [' + e.t + '] ' + e.handle + ': ' + e.preview);
  }
  return L.join('\n');
}

function pad(s, n) {
  s = String(s === null || s === undefined ? '—' : s);
  return s.length >= n ? s.slice(0, n) : s + ' '.repeat(n - s.length);
}

class LoudExit extends Error {}

async function connectOnce(cfg, lastEventId) {
  const url = cfg.url + '/rooms/' + encodeURIComponent(cfg.group) + '/events.stream';
  const headers = { Accept: 'text/event-stream' };
  if (lastEventId) headers['Last-Event-ID'] = lastEventId;
  const res = await fetch(url, { headers });
  if (res.status === 404) {
    throw new LoudExit('no such group "' + cfg.group + '" — rooms form on join; the observer never forms one (L18). Join the room first, then observe.');
  }
  if (!res.ok) {
    throw new LoudExit('tap refused: HTTP ' + res.status + ' from ' + url);
  }
  return res;
}

async function main() {
  let cfg;
  try {
    cfg = parseArgs(process.argv);
  } catch (err) {
    if (err instanceof UsageError) usage(2, process.stderr, err.message);
    throw err;
  }

  const engine = new RoomMetrics(cfg.group);
  let riemann = null;
  if (cfg.riemann) riemann = new RiemannSink(cfg.riemann, { group: cfg.group });

  const extras = { reconnects: 0, lastEventId: null };
  let jsonlFd = null;
  if (cfg.jsonl) {
    const fs = require('fs');
    jsonlFd = fs.openSync(cfg.jsonl, 'a');
  }

  let stopped = false;
  const finish = (code) => {
    stopped = true;
    if (jsonlFd) require('fs').closeSync(jsonlFd);
    process.exit(code);
  };
  process.on('SIGINT', () => {
    process.stderr.write('\n-- interrupted; final snapshot --\n');
    process.stdout.write(renderDashboard(engine.snapshot(), extras) + '\n');
    finish(0);
  });

  const renderTick = () => {
    if (!cfg.dashboard) return;
    const snap = engine.snapshot();
    if (process.stdout.isTTY) process.stdout.write('\x1b[2J\x1b[H' + renderDashboard(snap, extras) + '\n');
    else process.stdout.write(renderDashboard(snap, extras) + '\n');
  };

  const onPoints = (points) => {
    if (jsonlFd && points.length) {
      const fs = require('fs');
      for (const p of points) {
        const family = p.metric.split('.')[0];
        const sample = {
          series: p.metric,
          metric: p.metric.slice(family.length + 1),
          value: p.value,
          at: p.ts,
          handle: p.handle,
          family,
          group: engine.group,
        };
        fs.writeSync(jsonlFd, JSON.stringify(sample) + '\n');
      }
    }
    if (riemann) riemann.send(points);
  };

  const parser = new SseParser(
    (frame) => {
      if (frame.id) extras.lastEventId = frame.id;
      let ev;
      try {
        ev = JSON.parse(frame.data);
      } catch (err) {
        engine.malformed++;
        process.stderr.write('observatory: malformed event JSON (id ' + frame.id + '): ' + err.message + '\n');
        return;
      }
      const points = engine.ingest(ev);
      onPoints(points);
      renderTick();
    },
    (comment) => {
      engine.onComment(comment);
      renderTick(); // heartbeats drive refreshes — tickless, no own timer
    }
  );

  // Asymmetric by design: the first connect fails loud (never guess a
  // server that is not there); later drops reconnect forever with resume.
  let res;
  try {
    res = await connectOnce(cfg, null);
  } catch (err) {
    if (err instanceof LoudExit) {
      process.stderr.write('observatory: ' + err.message + '\n');
      finish(1);
    }
    process.stderr.write('observatory: cannot reach ' + cfg.url + ' — ' + (err.cause ? String(err.cause) : err.message) + '\n');
    finish(1);
  }

  let backoff = 1000;
  while (!stopped) {
    try {
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        parser.push(decoder.decode(value, { stream: true }));
      }
    } catch (err) {
      if (stopped) break;
      process.stderr.write('observatory: stream dropped — ' + err.message + '\n');
    }
    if (stopped) break;
    await new Promise((r) => setTimeout(r, backoff));
    backoff = Math.min(backoff * 2, 10000);
    try {
      res = await connectOnce(cfg, extras.lastEventId);
      extras.reconnects++;
      backoff = 1000;
      process.stderr.write('observatory: resumed' + (extras.lastEventId ? ' from Last-Event-ID ' + extras.lastEventId : '') + '\n');
    } catch (err) {
      if (err instanceof LoudExit) {
        process.stderr.write('observatory: ' + err.message + '\n');
        finish(1);
      }
      process.stderr.write('observatory: reconnect failed (' + (err.cause ? String(err.cause) : err.message) + '), retrying\n');
    }
  }
}

if (require.main === module) {
  main().catch((err) => {
    process.stderr.write('observatory: fatal — ' + (err && err.stack ? err.stack : String(err)) + '\n');
    process.exit(1);
  });
}

module.exports = {
  SseParser,
  RoomMetrics,
  pct,
  sortedPct,
  toMs,
  normText,
  IS_QUESTION,
  parseArgs,
  UsageError,
  encodeRiemannMsg,
  encodeRiemannEvent,
  RiemannSink,
  renderDashboard,
  preview,
};
