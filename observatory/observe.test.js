// observatory/observe.test.js — zero-dep tests; run: node --test observatory/ (or bun test observatory/)
'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');

const {
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
  renderDashboard,
} = require('./observe.js');

const Said = (handle, at, text, formats) => ({ type: 'Said', group: 'g', handle, at: String(at), text, formats: formats || [] });
const Thought = (handle, at, trace) => ({ type: 'Thought', group: 'g', handle, at: String(at), trace });

function feed(engine, events) {
  const points = [];
  for (const ev of events) points.push(...engine.ingest(ev));
  return points;
}


test('SseParser assembles frames, comments, ids, multi-line data, CRLF', () => {
  const events = [];
  const comments = [];
  const p = new SseParser((f) => events.push(f), (c) => comments.push(c));
  const wire =
    'id:7\n' +
    'name:Said\n' +
    'data:{"type":"Said","text":"line one\\nline two"}\n\n' +
    ':hb\n' +
    'retry:1000\n' +
    'id:8\r\n' +
    'name:Joined\r\n' +
    'data:{"type":"Joined"}\r\n\r\n';
  p.push(wire);
  assert.equal(events.length, 2);
  assert.equal(events[0].id, '7');
  assert.equal(events[0].name, 'Said');
  assert.equal(events[0].data, '{"type":"Said","text":"line one\\nline two"}');
  assert.equal(events[1].id, '8');
  assert.equal(events[1].name, 'Joined');
  assert.deepEqual(comments, ['hb']);
});

test('SseParser survives chunk splits mid-frame', () => {
  const events = [];
  const p = new SseParser((f) => events.push(f), () => {});
  const frame = 'id:1\nname:Said\ndata:{"a":1}\n\n';
  p.push(frame.slice(0, 7));
  p.push(frame.slice(7, 15));
  p.push(frame.slice(15));
  assert.equal(events.length, 1);
  assert.deepEqual(JSON.parse(events[0].data), { a: 1 });
});

test('SseParser keeps the last id across frames (resume state)', () => {
  const p = new SseParser(() => {}, () => {});
  p.push('id:41\ndata:{}\n\n');
  p.push('id:42\ndata:{}\n\n');
  assert.equal(p.id, '42');
});


test('nearest-rank percentile on 1..100', () => {
  const arr = Array.from({ length: 100 }, (_, i) => i + 1);
  assert.equal(pct(arr, 50), 50);
  assert.equal(pct(arr, 95), 95);
  assert.equal(pct(arr, 99), 99);
  assert.equal(pct([], 50), null);
  assert.equal(sortedPct([5, 1, 3], 50), 3);
  assert.equal(sortedPct([], 50), null);
});


test('inter-event gaps: exact samples and percentiles', () => {
  const e = new RoomMetrics('g');
  feed(e, [Said('a', 1000, 'x'), Said('a', 1500, 'y'), Said('a', 2500, 'z')]);
  assert.deepEqual(e.gaps, [500, 1000]);
  const snap = e.snapshot();
  assert.equal(snap.latency.gap.n, 2);
  assert.equal(snap.latency.gap.p50, 500);
  assert.equal(snap.latency.gap.p99, 1000);
});

test('toMs: epoch ms, numeric string, ISO instant (nanosecond precision); garbage is null, never NaN', () => {
  assert.equal(toMs(1717200100000), 1717200100000);
  assert.equal(toMs('1717200100000'), 1717200100000);
  assert.equal(toMs('2026-11-04T00:01:49.653786877Z'), Date.parse('2026-11-04T00:01:49.653Z'));
  assert.equal(toMs('not a time'), null);
  assert.equal(toMs(NaN), null);
});

test('engine derives latency from ISO instants in the envelope (the live wire shape)', () => {
  const e = new RoomMetrics('g');
  e.ingest({ type: 'Said', handle: 'a', text: 'hi', at: '2026-11-04T00:00:01.000000000Z' });
  const points = e.ingest({ type: 'Said', handle: 'b', text: 'yo', at: '2026-11-04T00:00:02.500000000Z' });
  const snap = e.snapshot();
  assert.equal(snap.latency.gap.n, 1);
  assert.equal(snap.latency.gap.p50, 1500);
  assert.equal(snap.latency.turn.p50, 1500);
  assert.deepEqual(points.filter((p) => p.metric === 'latency.turn_ms').map((p) => p.value), [1500]);
});

test('dialogue turn: different voice closes, same voice re-arms, presence does not', () => {
  const e = new RoomMetrics('g');
  const points = feed(e, [
    Said('a', 1000, 'hello'),
    Thought('a', 1200, 'thinking'), // same voice: must NOT close the turn
    Said('b', 1600, 'hi back'),
    Said('b', 2000, 'more'),
    { type: 'Joined', group: 'g', handle: 'c', at: '2100' }, // presence: must NOT close the turn
    Said('a', 3000, 'done'),
  ]);
  const turnPoints = points.filter((p) => p.metric === 'latency.turn_ms');
  assert.deepEqual(turnPoints.map((p) => p.value), [600, 1000]);
  assert.deepEqual(e.snapshot().latency.turn.stepsP50, 2); // each close is the 2nd event after its say
});


test('thoughts-per-answer and thoughts/say per handle', () => {
  const e = new RoomMetrics('g');
  const points = feed(e, [
    Thought('a', 1000, 't1'),
    Thought('a', 1100, 't2'),
    Thought('a', 1200, 't3'),
    Said('a', 1300, 'answer'),
    Thought('b', 1400, 't1'),
    Said('b', 1500, 'other'),
  ]);
  assert.deepEqual(e.stepsBetween, [3, 1]);
  assert.deepEqual(
    points.filter((p) => p.metric === 'reasoning.thoughts_per_answer').map((p) => p.value),
    [3, 1]
  );
  const snap = e.snapshot();
  const a = snap.reasoning.handles.find((h) => h.handle === 'a');
  assert.equal(a.says, 1);
  assert.equal(a.thoughts, 3);
  assert.equal(a.perSay, 3);
  assert.equal(snap.reasoning.stepsBetween.p50, 1); // nearest-rank: of [3,1], rank ceil(.5*2)=1 → 1
  assert.equal(snap.reasoning.anyThoughts, true);
});

test('a say with no volunteered traces fabricates no ratios', () => {
  const e = new RoomMetrics('g');
  feed(e, [Said('a', 1000, 'plain'), Said('b', 2000, 'plain')]);
  const snap = e.snapshot();
  assert.equal(snap.reasoning.stepsBetween.n, 0);
  assert.equal(snap.reasoning.anyThoughts, false);
});


test('question → answer: steps, latency, open windows', () => {
  const e = new RoomMetrics('g');
  const points = feed(e, [
    Said('a', 1000, 'what is the p99?'),
    Thought('b', 1200, 'hmm'),
    Said('a', 1300, 'anyone?'), // re-arms a's window
    Thought('b', 1500, 'checking'),
    Said('b', 1700, 'it is 8.9s'), // closes a's window
  ]);
  const snap = e.snapshot();
  assert.equal(snap.cep.questions, 2);
  assert.equal(snap.cep.answered, 1);
  assert.deepEqual(e.stepsToAnswer, [2]); // 2 events after the re-armed question: Thought, then the closing Say
  assert.deepEqual(e.answerLatency, [400]);
  assert.deepEqual(
    points.filter((p) => p.metric === 'cep.steps_to_answer').map((p) => p.value),
    [2]
  );
  assert.equal(snap.cep.open.length, 0);
});

test('unanswered questions stay open; asker can hold only one window', () => {
  const e = new RoomMetrics('g');
  feed(e, [Said('a', 1000, 'q1?'), Said('b', 1200, 'q2?'), Said('a', 1300, 'q3?')]);
  const snap = e.snapshot();
  assert.equal(snap.cep.answered, 2); // b's q2 closed a's q1, then a's q3 closed b's q2
  assert.equal(snap.cep.open.length, 1);
  assert.equal(snap.cep.open[0].handle, 'a');
});

test('repeated questions: normalized text, distance in events', () => {
  const e = new RoomMetrics('g');
  const points = feed(e, [
    Said('a', 1000, 'What is the p99 today?'),
    Said('b', 1100, 'answer'),
    Said('c', 1200, 'noise'),
    Said('a', 1300, 'what is the p99  today ?'), // normalizes to the same text as the first ask
  ]);
  const snap = e.snapshot();
  assert.equal(snap.cep.repeats, 1);
  assert.equal(snap.cep.lastRepeatDistance, 3);
  assert.deepEqual(
    points.filter((p) => p.metric === 'cep.repeat_distance_events').map((p) => p.value),
    [3]
  );
});

test('normText and IS_QUESTION heuristics', () => {
  assert.equal(normText("What's the p99?"), 'what s the p99');
  assert.equal(normText('  A   B  '), 'a b');
  assert.equal(IS_QUESTION('hello?'), true);
  assert.equal(IS_QUESTION('hello ? '), true);
  assert.equal(IS_QUESTION('hello'), false);
  assert.equal(IS_QUESTION('?!'), false);
});


test('metric# formats + key=value text becomes series points', () => {
  const e = new RoomMetrics('g');
  const points = feed(e, [
    Said('a', 1000, 'recall@3=0.67 ndcg=0.81', ['metric#retrieval']),
    Said('a', 1100, 'unrelated, no metric label', ['poke#x']),
    Said('b', 1200, 'latency_p99=8.9 errors=-1', ['metric#retrieval', 'other']),
  ]);
  const snap = e.snapshot();
  const byName = Object.fromEntries(snap.volunteered.map((s) => [s.series, s]));
  assert.equal(byName['volunteered.retrieval.recall@3'].value, 0.67);
  assert.equal(byName['volunteered.retrieval.ndcg'].value, 0.81);
  assert.equal(byName['volunteered.retrieval.latency_p99'].value, 8.9);
  assert.equal(byName['volunteered.retrieval.errors'].value, -1);
  assert.equal(snap.volunteered.length, 4);
  assert.equal(
    points.filter((p) => p.metric === 'volunteered.retrieval.recall@3').length,
    1
  );
  assert.equal(byName['volunteered.retrieval.recall@3'].handle, 'a');
});


test('encodeRiemannMsg golden bytes', () => {
  const msg = encodeRiemannMsg([
    {
      time: 1,
      state: 'ok',
      service: 's',
      host: 'h',
      description: 'd',
      tags: ['t'],
      attributes: [{ key: 'k', value: 'v' }],
      metric_d: 0.25,
    },
  ]);
  const expected = [
    '32', '23', // Msg.events (field 6), length 0x23
    '08', '01', // Event.time (1, varint) = 1
    '12', '02', '6f', '6b', // Event.state (2) = "ok"
    '1a', '01', '73', // Event.service (3) = "s"
    '22', '01', '68', // Event.host (4) = "h"
    '2a', '01', '64', // Event.description (5) = "d"
    '3a', '01', '74', // Event.tags (7) = "t"
    '4a', '06', // Event.attributes (9), length 6
    '0a', '01', '6b', // Attribute.key (1) = "k"
    '12', '01', '76', // Attribute.value (2) = "v"
    '71', '00', '00', '00', '00', '00', '00', 'd0', '3f', // Event.metric_d (14, double LE) = 0.25
  ].join('');
  assert.equal(msg.toString('hex'), expected);
});

test('encodeRiemannMsg handles multi-byte UTF-8 and empty messages', () => {
  assert.equal(encodeRiemannMsg([]).toString('hex'), '');
  const msg = encodeRiemannMsg([{ service: 'é' }]);
  // 0xC3 0xA9 is UTF-8 for é
  assert.equal(msg.toString('hex'), '32041a02c3a9');
});


test('parseArgs: missing required flags and bad values fail loud', () => {
  assert.throws(() => parseArgs(['node', 'x']), UsageError);
  assert.throws(() => parseArgs(['node', 'x', '--group', 'g']), /--url is required/);
  assert.throws(() => parseArgs(['node', 'x', '--url', 'http://h']), /--group is required/);
  assert.throws(() => parseArgs(['node', 'x', '--url', 'http://h', '--group', 'a/b']), /slash/);
  assert.throws(() => parseArgs(['node', 'x', '--url', 'http://h', '--group', 'g', '--wat']), /unknown flag/);
  assert.throws(() => parseArgs(['node', 'x', '--url', 'http://h', '--group', 'g', '--riemann', 'nope']), /host:port/);
  assert.throws(() => parseArgs(['node', 'x', '--url', 'http://h', '--group', 'g', '--jsonl']), /missing value/);
});

test('parseArgs: full parse, trailing-slash trim, env fallback', () => {
  const cfg = parseArgs(['node', 'x', '--url', 'http://h:8080/', '--group', 'g', '--jsonl', '/tmp/s.jsonl', '--no-dashboard']);
  assert.equal(cfg.url, 'http://h:8080');
  assert.equal(cfg.group, 'g');
  assert.equal(cfg.jsonl, '/tmp/s.jsonl');
  assert.equal(cfg.dashboard, false);
  process.env.CHORUX_OBSERVATORY_RIEMANN = 'r.example:5555';
  const cfg2 = parseArgs(['node', 'x', '--url', 'http://h', '--group', 'g']);
  assert.equal(cfg2.riemann, 'r.example:5555');
  delete process.env.CHORUX_OBSERVATORY_RIEMANN;
});


test('onComment counts heartbeats and replay gaps; snapshot shape holds', () => {
  const e = new RoomMetrics('g');
  e.onComment('hb');
  e.onComment('hb');
  e.onComment('gap');
  e.onComment('noise');
  const snap = e.snapshot();
  assert.equal(snap.hb, 2);
  assert.equal(snap.gapComments, 1);
  assert.equal(snap.group, 'g');
});

test('renderDashboard renders plain text away from a TTY', () => {
  const e = new RoomMetrics('ops');
  feed(e, [Said('a', 1000, 'what is the p99?'), Said('b', 2000, 'it is fine')]);
  const out = renderDashboard(e.snapshot(), { reconnects: 0, lastEventId: '9' });
  for (const marker of [
    'chorux observatory — group "ops"',
    'latency feel',
    'reasoning efficiency',
    'cross-stream correlation',
    'volunteered series',
    'what is the p99?',
  ]) {
    assert.ok(out.includes(marker), 'missing: ' + marker);
  }
  assert.ok(!out.includes('\x1b['), 'ANSI codes leaked into non-TTY render');
});
