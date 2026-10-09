import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { validate } from './validate-output.mjs';

// Every trace in this file is synthetic. These are parser/state-machine tests,
// never evidence of Windows networking behavior or a hosted runner result.
const header = mode => ({
  type: 'header', schema: 1, profile: 'dual-stack-connect', mode,
  groups: 40, lanes: 6, fallback_ms: 300, group_ms: 700,
  work_ms: 30_000, cleanup_ms: 35_000, max_live: 12, max_starts: 480,
});
const group = (id, ms) => ({ type: 'group', id, ms });
const operation = (id, stage, ms, result = 0, error = 0) => ({ type: 'operation', id, stage, result, error, ms });
const failure = (id, stage, ms, error = 10022) => ({ type: 'failure', id, stage, error, ms });
const text = rows => `${rows.map(row => JSON.stringify(row)).join('\n')}\n`;
const clone = rows => structuredClone(rows);

// A deliberately simple counting oracle, separate from the validator's state
// machine, keeps malformed traces from failing only on stale summary counters.
function withSummary(input, elapsedMs) {
  const rows = input.filter(row => row.type !== 'summary');
  const operations = rows.filter(row => row.type === 'operation');
  const count = predicate => operations.filter(predicate).length;
  const success = stage => row => row.stage === stage && row.result === 0 && row.error === 0;
  const summary = {
    type: 'summary',
    starts: count(row => row.stage === 'connect'),
    ipv4_successes: count(row => ['connect', 'completion-consumed'].includes(row.stage) && row.id % 2 === 1 && row.result === 0 && row.error === 0),
    expected_refusals: count(row => ['connect', 'completion-consumed'].includes(row.stage) && row.id % 2 === 0 && row.result === -1 && row.error === 10061),
    closed_before_handling: count(row => row.stage === 'close-intent' && row.result === 1),
    ready_before_close: 0,
    sockets_opened: count(success('socket')),
    sockets_closed: count(success('closesocket')),
    events_opened: count(success('event-create')),
    events_closed: count(success('event-close')),
    max_live: 0,
    failures: rows.filter(row => row.type === 'failure').length,
    operation_records: operations.length,
    complete: false,
    elapsed_ms: elapsedMs ?? rows.at(-1)?.ms ?? 0,
  };
  let live = 0;
  const ready = new Set();
  for (const row of operations) {
    if (success('socket')(row)) live++;
    if (success('closesocket')(row)) live--;
    summary.max_live = Math.max(summary.max_live, live);
    if (success('event-ready')(row)) ready.add(row.id);
    if (row.stage === 'close-intent' && row.result === 1 && ready.has(row.id)) summary.ready_before_close++;
  }
  summary.complete = summary.starts === 480 && summary.ipv4_successes === 240 && summary.failures === 0 && summary.elapsed_ms <= 35_000 && summary.sockets_opened === summary.sockets_closed && summary.events_opened === summary.events_closed;
  return [...rows, summary];
}

function syntheticTrace({ mode = 'plain', zeroExposure = false, immediateV4 = false, pendingConsumer = false } = {}) {
  const rows = [header(mode)];
  const add = (...args) => rows.push(operation(...args));
  const setup = (id, ms, result = -1, error = 10035) => {
    for (const stage of ['socket', 'event-create', 'event-select', 'set-randomize']) add(id, stage, ms);
    add(id, 'get-randomize', ms, mode === 'randomized' ? 1 : 0);
    add(id, 'connect', ms, result, error);
  };
  for (let group = 0; group < 40; group++) {
    const base = group * 700;
    rows.push({ type: 'group', id: group, ms: base });
    for (let lane = 0; lane < 6; lane++) setup(group * 12 + lane * 2, base, -1, zeroExposure ? 10061 : 10035);
    if (!zeroExposure) {
      for (let lane = 0; lane < 3; lane++) {
        if (pendingConsumer && lane === 2) continue;
        const id = group * 12 + lane * 2;
        add(id, 'event-ready', base + 50);
        add(id, 'completion-api', base + 50);
        add(id, 'completion-consumed', base + 50, lane === 1 ? 0 : -1, lane === 1 ? 0 : 10061);
      }
    }
    for (let lane = 0; lane < 6; lane++) setup(group * 12 + lane * 2 + 1, base + 300, immediateV4 ? 0 : -1, immediateV4 ? 0 : 10035);
    if (!immediateV4) {
      for (let lane = 0; lane < 6; lane++) {
        const id = group * 12 + lane * 2 + 1;
        add(id, 'event-ready', base + 350);
        add(id, 'completion-api', base + 350);
        add(id, 'completion-consumed', base + 350);
      }
    }
    for (let offset = 0; offset < 12; offset++) {
      const id = group * 12 + offset;
      const deferred = offset % 2 === 0 && offset >= 6;
      const unhandled = !zeroExposure && (deferred || (pendingConsumer && offset === 4));
      if (unhandled) add(id, 'event-ready', base + 650, offset === 8 || offset === 4 ? 258 : 0);
      add(id, 'close-intent', base + 650, unhandled ? 1 : 0);
      add(id, 'closesocket', base + 650);
      add(id, 'event-close', base + 650);
    }
  }
  return withSummary(rows);
}

const full = syntheticTrace();
const pick = (rows, id, stage) => rows.find(row => row.id === id && row.stage === stage && row.type === 'operation');
const indexOf = (rows, id, stage) => rows.findIndex(row => row.id === id && row.stage === stage && row.type === 'operation');
function changed(mutate, input = full, refresh = true) {
  const rows = clone(input);
  mutate(rows);
  return text(refresh ? withSummary(rows) : rows);
}
function rejectsMutation(name, mutate, pattern, input = full, refresh = true) {
  test(name, () => assert.throws(() => validate(changed(mutate, input, refresh)), pattern));
}
function capture(input) {
  try { validate(text(input)); } catch (error) { return error; }
  assert.fail('Expected rejection');
}

for (const mode of ['plain', 'randomized']) {
  test(`synthetic full ${mode} evidence qualifies from operations`, () => {
    const rows = syntheticTrace({ mode });
    const result = validate(text(rows));
    assert.deepEqual(result, { ...rows.at(-1), qualified: true });
    assert.equal(result.starts, 480);
    assert.equal(result.ipv4_successes, 240);
    assert.equal(result.expected_refusals, 80);
    assert.equal(result.closed_before_handling, 120);
    assert.equal(result.ready_before_close, 80);
    assert.equal(result.sockets_opened, 480);
    assert.equal(result.sockets_closed, 480);
    assert.equal(result.events_opened, 480);
    assert.equal(result.events_closed, 480);
    assert.equal(result.max_live, 12);
    assert.equal(result.failures, 0);
    assert.equal(result.elapsed_ms, 27_950);
  });
}

test('CRLF and a final line without a newline remain valid JSONL', () => {
  assert.equal(validate(text(full).replaceAll('\n', '\r\n')).qualified, true);
  assert.equal(validate(text(full).trimEnd()).qualified, true);
});
test('immediate IPv4 successes count once', () => {
  const result = validate(text(syntheticTrace({ immediateV4: true })));
  assert.equal(result.ipv4_successes, 240);
});
test('unsignaled consuming IPv6 lanes may close unhandled', () => {
  const result = validate(text(syntheticTrace({ pendingConsumer: true })));
  assert.equal(result.closed_before_handling, 160);
  assert.equal(result.ready_before_close, 80);
});
test('all immediate IPv6 refusals leave zero cancellation exposure inconclusive', () => {
  const error = capture(syntheticTrace({ zeroExposure: true }));
  assert.equal(error.code, 'inconclusive-evidence');
  assert.equal(error.summary.complete, true);
  assert.equal(error.summary.qualified, false);
  assert.equal(error.summary.expected_refusals, 240);
  assert.equal(error.summary.closed_before_handling, 0);
});

test('summary alone cannot establish completeness', () => {
  assert.throws(() => validate(text([header('plain'), full.at(-1)])), /Summary counter/);
});
rejectsMutation('forged complete flag fails independently', rows => { rows.at(-1).complete = false; }, /completeness/, full, false);
for (const key of Object.keys(full.at(-1)).filter(key => !['type', 'complete', 'elapsed_ms'].includes(key))) {
  rejectsMutation(`forged ${key} counter fails independently`, rows => { rows.at(-1)[key]++; }, /counter/, full, false);
}
rejectsMutation('missing IPv4 completion is not replaced by event readiness', rows => { rows.splice(indexOf(rows, 1, 'completion-consumed'), 1); }, /immediate completion/);
rejectsMutation('missing consuming IPv6 refusal is not replaced by readiness', rows => { rows.splice(indexOf(rows, 0, 'completion-consumed'), 1); }, /immediate completion/);
rejectsMutation('timeout cannot pretend to be a successful IPv4 completion', rows => { pick(rows, 1, 'event-ready').result = 258; }, /eligible signaled/);
rejectsMutation('completion without readiness is rejected', rows => { rows.splice(indexOf(rows, 1, 'event-ready'), 1); }, /eligible signaled/);
rejectsMutation('deferred IPv6 completion consumption is rejected', rows => {
  const at = indexOf(rows, 6, 'event-ready');
  rows.splice(at + 1, 0, operation(6, 'completion-consumed', rows[at].ms));
  pick(rows, 6, 'close-intent').result = 0;
}, /eligible signaled/);
rejectsMutation('would-block at completion is a fatal API result', rows => {
  const row = pick(rows, 1, 'completion-consumed'); row.result = -1; row.error = 10035;
}, /Missing immediate failure/);
rejectsMutation('IPv4 refusal is fatal even when IPv6 refusal is expected', rows => {
  const row = pick(rows, 1, 'completion-consumed'); row.result = -1; row.error = 10061;
}, /Missing immediate failure/);
rejectsMutation('10055 cannot qualify without a failure record', rows => {
  const row = pick(rows, 0, 'completion-consumed'); row.error = 10055;
}, /Missing immediate failure/);
rejectsMutation('an expected IPv6 refusal must not emit a failure', rows => {
  const at = indexOf(rows, 0, 'completion-consumed');
  rows.splice(at + 1, 0, failure(0, 'completion-consumed', rows[at].ms, 10061));
}, /Orphan/);
rejectsMutation('successful result with nonzero error is invalid', rows => { pick(rows, 1, 'completion-consumed').error = 10055; }, /operation result/);
rejectsMutation('failure result with zero error is invalid', rows => { pick(rows, 0, 'completion-consumed').error = 0; }, /operation result/);
rejectsMutation('ready row cannot invent a completion status', rows => { pick(rows, 6, 'event-ready').error = 10061; }, /operation result/);
rejectsMutation('extra polling timeout records are rejected', rows => {
  const at = indexOf(rows, 6, 'event-ready'); rows.splice(at, 0, operation(6, 'event-ready', rows[at].ms, 258));
}, /repeated readiness/);
rejectsMutation('synchronous connect cannot own an event-ready row', rows => { const row = pick(rows, 6, 'connect'); row.result = 0; row.error = 0; }, /readiness/);

for (const stage of ['socket', 'event-create', 'event-select', 'set-randomize', 'get-randomize', 'connect']) {
  rejectsMutation(`missing ${stage} setup row is rejected`, rows => { rows.splice(indexOf(rows, 0, stage), 1); }, /ownership|setup|readiness|eligible signaled/);
}
rejectsMutation('setup stage reordering is rejected', rows => {
  const a = indexOf(rows, 0, 'set-randomize'); const b = indexOf(rows, 0, 'get-randomize');
  [rows[a], rows[b]] = [rows[b], rows[a]];
}, /setup/);
rejectsMutation('plain option readback must equal zero', rows => { pick(rows, 0, 'get-randomize').result = 1; }, /readback/);
rejectsMutation('randomized option readback must equal one', rows => { pick(rows, 0, 'get-randomize').result = 0; }, /readback/, syntheticTrace({ mode: 'randomized' }));
rejectsMutation('non-boolean option readback is invalid', rows => { pick(rows, 0, 'get-randomize').result = 2; }, /operation result/);
rejectsMutation('duplicate socket open is rejected', rows => { rows.splice(3, 0, clone(rows[2])); }, /Duplicate socket/);
rejectsMutation('duplicate close intent is rejected', rows => {
  const at = indexOf(rows, 0, 'close-intent'); rows.splice(at + 1, 0, clone(rows[at]));
}, /after close intent/);
rejectsMutation('duplicate socket close is rejected', rows => {
  const at = indexOf(rows, 0, 'closesocket'); rows.splice(at + 1, 0, clone(rows[at]));
}, /after close attempt/);
rejectsMutation('duplicate event close is rejected', rows => {
  const at = indexOf(rows, 0, 'event-close'); rows.splice(at + 1, 0, clone(rows[at]));
}, /after event close/);
rejectsMutation('missing socket close cannot be hidden by summary', rows => { rows.splice(indexOf(rows, 0, 'closesocket'), 1); }, /released socket/);
rejectsMutation('missing event close cannot be hidden by summary', rows => { rows.splice(indexOf(rows, 478, 'event-close'), 1); }, /no close attempt/);
rejectsMutation('missing close intent cannot be hidden by summary', rows => { rows.splice(indexOf(rows, 0, 'close-intent'), 1); }, /socket close/);
rejectsMutation('event must close after its socket', rows => {
  const a = indexOf(rows, 0, 'closesocket'); const b = indexOf(rows, 0, 'event-close');
  [rows[a], rows[b]] = [rows[b], rows[a]];
}, /released socket/);
rejectsMutation('completion cannot be consumed after close', rows => {
  const at = indexOf(rows, 0, 'closesocket'); rows.splice(at + 1, 0, operation(0, 'completion-consumed', rows[at].ms));
}, /after close attempt/);
rejectsMutation('ready unhandled closure must use intent one', rows => { pick(rows, 6, 'close-intent').result = 0; }, /intent does not match/);
rejectsMutation('consumed closure must use intent zero', rows => { pick(rows, 0, 'close-intent').result = 1; }, /intent does not match/);
rejectsMutation('pending closure requires a final readiness observation', rows => { rows.splice(indexOf(rows, 8, 'event-ready'), 1); }, /readiness observation/);
rejectsMutation('event readiness cannot be queried after close intent', rows => {
  const at = indexOf(rows, 6, 'close-intent'); rows.splice(at + 1, 0, operation(6, 'event-ready', rows[at].ms));
}, /after close intent/);

rejectsMutation('IPv4 fallback cannot start at 299 ms', rows => {
  for (const row of rows) if (row.type === 'operation' && row.ms === 300) row.ms = 299;
}, /fallback interval/);
rejectsMutation('IPv4 fallback interval belongs to its own lane', rows => {
  for (const row of rows) if (row.type === 'operation' && row.ms === 0 && row.id === 10) row.ms = 1;
}, /fallback interval/);
rejectsMutation('connect at the group deadline is too late', rows => {
  const at = indexOf(rows, 11, 'connect');
  for (let index = at; index < rows.length; index++) if (rows[index].type === 'operation') rows[index].ms += 400;
}, /group deadline/);
rejectsMutation('connect at the work deadline is too late', rows => {
  for (const row of rows) if ((row.type === 'operation' || row.type === 'group')) row.ms += 3000;
}, /work deadline/);
rejectsMutation('timestamps may not decrease', rows => { pick(rows, 0, 'completion-consumed').ms = 49; }, /timestamp order/);
rejectsMutation('operation timestamps may not exceed 40-second diagnostic bound', rows => { pick(rows, 479, 'event-close').ms = 40_001; }, /summary status|timestamp order/);
rejectsMutation('summary elapsed time cannot precede final operation', rows => { rows.at(-1).elapsed_ms--; }, /precedes/, full, false);
rejectsMutation('summary elapsed time cannot exceed 40-second diagnostic bound', rows => { rows.at(-1).elapsed_ms = 40_001; }, /summary status/, full, false);
rejectsMutation('negative operation time is invalid', rows => { rows[1].ms = -1; }, /timestamp order/);
rejectsMutation('fractional operation time is invalid', rows => { rows[1].ms = 0.5; }, /timestamp order/);
rejectsMutation('groups must begin with group zero', rows => { rows[1].id = 1; }, /Groups/);
rejectsMutation('next group cannot acquire ownership before all prior resources close', rows => {
  const at = rows.findIndex(row => row.type === 'group' && row.id === 1);
  const row = rows.splice(at, 1)[0]; row.ms = 650;
  rows.splice(indexOf(rows, 10, 'close-intent'), 0, row);
}, /Groups/);
rejectsMutation('a group number cannot be skipped', rows => { rows.find(row => row.type === 'group' && row.id === 1).id = 2; }, /Groups/);

// These abbreviated failures preserve every acquired resource's cleanup history.
function syntheticAbort(stage, { error = 10055, closeFailure = false, eventCloseFailure = false } = {}) {
  const stages = ['socket', 'event-create', 'event-select', 'set-randomize', 'get-randomize', 'connect'];
  const rows = [header('plain'), group(0, 0)];
  let socket = false;
  let event = false;
  for (const current of stages) {
    const failed = current === stage;
    rows.push(operation(0, current, 0, failed ? -1 : 0, failed ? error : 0));
    if (failed) { rows.push(failure(0, current, 0, error)); break; }
    if (current === 'socket') socket = true;
    if (current === 'event-create') event = true;
  }
  if (socket) {
    rows.push(operation(0, 'close-intent', 1));
    rows.push(operation(0, 'closesocket', 1, closeFailure ? -1 : 0, closeFailure ? 10055 : 0));
    if (closeFailure) rows.push(failure(0, 'closesocket', 1, 10055));
    else if (event) {
      rows.push(operation(0, 'event-close', 1, eventCloseFailure ? -1 : 0, eventCloseFailure ? 10055 : 0));
      if (eventCloseFailure) rows.push(failure(0, 'event-close', 1, 10055));
    }
  }
  return withSummary(rows);
}
for (const stage of ['socket', 'event-create', 'event-select', 'set-randomize', 'get-randomize', 'connect']) {
  test(`10055 at ${stage} is a structurally checked early abort, never qualifying`, () => {
    const error = capture(syntheticAbort(stage));
    assert.equal(error.code, 'nonqualifying-evidence');
    assert.equal(error.summary.failures, 1);
    assert.equal(error.summary.qualified, false);
    assert.equal(error.summary.sockets_closed, error.summary.sockets_opened);
    assert.equal(error.summary.events_closed, error.summary.events_opened);
  });
}
test('failed socket close retains its event until process exit', () => {
  const error = capture(syntheticAbort('get-randomize', { closeFailure: true }));
  assert.equal(error.code, 'nonqualifying-evidence');
  assert.equal(error.summary.sockets_opened, 1);
  assert.equal(error.summary.sockets_closed, 0);
  assert.equal(error.summary.events_opened, 1);
  assert.equal(error.summary.events_closed, 0);
  assert.equal(error.summary.failures, 2);
});
test('failed event close is counted once and fails qualification', () => {
  const error = capture(syntheticAbort('get-randomize', { eventCloseFailure: true }));
  assert.equal(error.code, 'nonqualifying-evidence');
  assert.equal(error.summary.sockets_closed, 1);
  assert.equal(error.summary.events_closed, 0);
  assert.equal(error.summary.failures, 2);
});
rejectsMutation('event cannot be released after failed socket close', rows => {
  rows.splice(rows.length - 1, 0, operation(0, 'event-close', 1));
}, /released socket/, syntheticAbort('get-randomize', { closeFailure: true }));
rejectsMutation('close failure cannot be retried on the same socket', rows => {
  rows.splice(rows.length - 1, 0, operation(0, 'closesocket', 1));
}, /after close attempt/, syntheticAbort('get-randomize', { closeFailure: true }));
rejectsMutation('failure must immediately follow the failed API', rows => {
  const at = rows.findIndex(row => row.type === 'failure'); [rows[at], rows[at + 1]] = [rows[at + 1], rows[at]];
}, /Missing immediate failure/, syntheticAbort('get-randomize'));
for (const field of ['id', 'error', 'stage']) {
  rejectsMutation(`failure ${field} must match the operation`, rows => {
    const row = rows.find(row => row.type === 'failure'); row[field] = field === 'stage' ? 'connect' : row[field] + 1;
  }, /Missing immediate failure/, syntheticAbort('get-randomize'));
}
rejectsMutation('opened socket cleanup is still required after early abort', rows => {
  rows.splice(indexOf(rows, 0, 'close-intent'), 3);
}, /no close attempt/, syntheticAbort('get-randomize'));
rejectsMutation('created event cleanup is still required after early abort', rows => {
  rows.splice(indexOf(rows, 0, 'event-close'), 1);
}, /no close attempt/, syntheticAbort('get-randomize'));
rejectsMutation('fatal failure stops new socket work', rows => {
  rows.splice(rows.length - 1, 0, operation(2, 'socket', 1));
}, /New work after fatal/, syntheticAbort('get-randomize'));

test('pending event-ready API failure still cleans up socket and event', () => {
  const rows = [header('plain'), group(0, 0)];
  for (const stage of ['socket', 'event-create', 'event-select', 'set-randomize', 'get-randomize']) rows.push(operation(0, stage, 0));
  rows.push(operation(0, 'connect', 0, -1, 10035));
  rows.push(operation(0, 'event-ready', 1, -1, 10055), failure(0, 'event-ready', 1, 10055));
  rows.push(operation(0, 'close-intent', 1, 1), operation(0, 'closesocket', 1), operation(0, 'event-close', 1));
  const error = capture(withSummary(rows));
  assert.equal(error.code, 'nonqualifying-evidence');
  assert.equal(error.summary.closed_before_handling, 1);
  assert.equal(error.summary.ready_before_close, 0);
  assert.equal(error.summary.events_closed, 1);
});
test('async 10055 completion is fatal with a paired failure and cleanup', () => {
  const rows = [header('plain'), group(0, 0)];
  for (const stage of ['socket', 'event-create', 'event-select', 'set-randomize', 'get-randomize']) rows.push(operation(0, stage, 0));
  rows.push(operation(0, 'connect', 0, -1, 10035), operation(0, 'event-ready', 1));
  rows.push(operation(0, 'completion-api', 1));
  rows.push(operation(0, 'completion-consumed', 1, -1, 10055), failure(0, 'completion-consumed', 1, 10055));
  rows.push(operation(0, 'close-intent', 1), operation(0, 'closesocket', 1), operation(0, 'event-close', 1));
  const error = capture(withSummary(rows));
  assert.equal(error.code, 'nonqualifying-evidence');
  assert.equal(error.summary.closed_before_handling, 0);
  assert.equal(error.summary.failures, 1);
});
test('startup failure preserves a final summary', () => {
  const error = capture(withSummary([header('plain'), failure(0, 'startup', 0, 10055)]));
  assert.equal(error.code, 'nonqualifying-evidence');
  assert.equal(error.summary.failures, 1);
});
for (const [stage, ms] of [['work-deadline', 30_000], ['cleanup-deadline', 35_001], ['wsa-cleanup', 27_950]]) {
  test(`${stage} failure prevents qualification after otherwise complete work`, () => {
    const rows = clone(full); rows.splice(rows.length - 1, 0, failure(0, stage, ms, 10060));
    const error = capture(withSummary(rows));
    assert.equal(error.code, 'nonqualifying-evidence');
    assert.equal(error.summary.failures, 1);
  });
}
test('group deadline failure is checked against its explicit group row', () => {
  const rows = [header('plain'), group(0, 0), operation(0, 'socket', 0), failure(0, 'group-deadline', 700, 10060), operation(0, 'close-intent', 700), operation(0, 'closesocket', 700)];
  assert.equal(capture(withSummary(rows)).code, 'nonqualifying-evidence');
  rows[3].ms = 699;
  assert.equal(capture(withSummary(rows)).code, 'invalid-evidence');
});
for (const [stage, ms] of [['work-deadline', 29_999], ['cleanup-deadline', 34_999]]) {
  test(`premature ${stage} is invalid`, () => {
    assert.equal(capture(withSummary([header('plain'), failure(0, stage, ms)])).code, 'invalid-evidence');
  });
}

for (const [name, mutate] of [
  ['unknown header field', rows => { rows[0].hostname = 'private.example'; }],
  ['unknown operation field', rows => { rows[2].address = '2001:db8::1'; }],
  ['unknown summary field', rows => { rows.at(-1).message = 'C:\\Users\\private'; }],
  ['unknown failure field', rows => { rows.splice(1, 0, { ...failure(0, 'startup', 0), details: 'private.example' }); }],
  ['unknown record type', rows => { rows[1].type = 'private.example'; }],
  ['unknown operation stage', rows => { rows[2].stage = 'private.example'; }],
  ['unknown profile', rows => { rows[0].profile = 'private.example'; }],
  ['unknown mode', rows => { rows[0].mode = 'private.example'; }],
  ['arbitrary string in result', rows => { rows[2].result = '10055 at private.example'; }],
  ['arbitrary string in error', rows => { rows[2].error = 'private.example'; }],
  ['numeric type disguised as string', rows => { rows[1].id = '0'; }],
  ['out-of-range operation ID', rows => { rows[1].id = 480; }],
  ['negative operation ID', rows => { rows[1].id = -1; }],
  ['fractional operation ID', rows => { rows[1].id = 0.5; }],
  ['unsafe integer', rows => { rows[1].ms = Number.MAX_SAFE_INTEGER + 1; }],
  ['null field', rows => { rows[2].error = null; }],
  ['nested object', rows => { rows[2].error = { address: 'private.example' }; }],
  ['nested array', rows => { rows[2].error = ['private.example']; }],
  ['extra summary before the end', rows => { rows.splice(1, 0, clone(rows.at(-1))); }],
  ['extra header inside trace', rows => { rows.splice(1, 0, clone(rows[0])); }],
  ['missing summary', rows => { rows.pop(); }],
  ['missing header', rows => { rows.shift(); }],
]) {
  rejectsMutation(name, mutate, undefined, full, false);
}
for (const key of Object.keys(header('plain')).filter(key => !['type', 'profile', 'mode'].includes(key))) {
  rejectsMutation(`fixed header ${key} is enforced`, rows => { rows[0][key]++; }, /header/, full, false);
}
for (const [name, transform] of [
  ['duplicate literal key', input => input.replace('"schema":1', '"schema":1,"schema":1')],
  ['duplicate escaped key', input => input.replace('"schema":1', '"schema":1,"\\u0073chema":1')],
  ['duplicate operation key', input => input.replace('"result":0', '"result":0,"result":0')],
  ['duplicate summary key', input => input.replace('"starts":480', '"starts":480,"starts":480')],
  ['duplicate prototype key', input => input.replace('"schema":1', '"__proto__":1,"__proto__":1')],
  ['blank line', input => input.replace('\n', '\n\n')],
  ['trailing blank line', input => `${input}\n`],
  ['two JSON objects on one line', input => input.replace('\n', '')],
  ['trailing source text', input => `${input}private.example`],
  ['trailing comma', input => input.replace('"schema":1,', '"schema":1,,')],
  ['non-JSON number', input => input.replace('"schema":1', '"schema":NaN')],
  ['infinite parsed number', input => input.replace('"ms":0', '"ms":1e999')],
  ['JSON array record', input => input.replace(/^.*\n/, '[]\n')],
  ['byte-order mark', input => `\uFEFF${input}`],
  ['unescaped control character', input => input.replace('dual-stack-connect', 'dual\tstack-connect')],
  ['unterminated string', input => input.replace('"plain"', '"plain')],
]) {
  test(name, () => assert.throws(() => validate(transform(text(full)))));
}
test('record count is bounded before parsing', () => assert.throws(() => validate('{}\n'.repeat(12_001)), /record count/));
test('byte size is bounded before parsing', () => assert.throws(() => validate(' '.repeat(2 * 1024 * 1024 + 1)), /size limit/));
test('UTF-8 byte size is used instead of string length', () => assert.throws(() => validate('é'.repeat(1024 * 1024 + 1)), /size limit/));
test('non-string input is rejected', () => assert.throws(() => validate(Buffer.from(text(full))), /must be text/));

test('CLI emits fixed summary JSON and never echoes rejected source or file paths', () => {
  const directory = mkdtempSync(join(tmpdir(), 'synthetic-dual-stack-'));
  const script = fileURLToPath(new URL('./validate-output.mjs', import.meta.url));
  const run = (name, content, expectedStatus) => {
    const path = join(directory, name);
    if (content !== undefined) writeFileSync(path, content);
    const child = spawnSync(process.execPath, [script, path], { encoding: 'utf8' });
    assert.equal(child.status, expectedStatus, child.stderr);
    assert.equal(child.stderr, '');
    assert.equal(child.stdout.trim().split('\n').length, 1);
    assert.doesNotMatch(child.stdout, /private\.example|2001:db8|Users|synthetic-dual-stack/);
    const parsed = JSON.parse(child.stdout);
    assert.deepEqual(Object.keys(parsed), [...Object.keys(full.at(-1)), 'qualified']);
    return parsed;
  };
  try {
    assert.equal(run('qualified.jsonl', text(full), 0).qualified, true);
    const inconclusive = run('inconclusive.jsonl', text(syntheticTrace({ zeroExposure: true })), 1);
    assert.equal(inconclusive.complete, true);
    assert.equal(inconclusive.closed_before_handling, 0);
    assert.equal(inconclusive.qualified, false);
    assert.equal(run('failed.jsonl', text(syntheticAbort('get-randomize')), 1).failures, 1);
    assert.equal(run('private.example.jsonl', text(full).replace('"error":0', '"error":"2001:db8::1 C:\\\\Users\\\\private.example"'), 1).qualified, false);
    assert.equal(run('missing-private.example.jsonl', undefined, 1).qualified, false);
    assert.equal(run('bom.jsonl', `\uFEFF${text(full)}`, 1).qualified, false);
    assert.equal(run('invalid-utf8.jsonl', Buffer.from([0xff, 0xfe]), 1).qualified, false);
    assert.equal(run('oversized.jsonl', ' '.repeat(2 * 1024 * 1024 + 1), 1).qualified, false);
    const noArgument = spawnSync(process.execPath, [script], { encoding: 'utf8' });
    assert.equal(noArgument.status, 1);
    assert.equal(noArgument.stderr, '');
    assert.equal(JSON.parse(noArgument.stdout).qualified, false);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test('native-style per-lane fallback and pair cleanup interleaving qualifies', () => {
  const rows = [header('plain')];
  for (let groupId = 0; groupId < 40; groupId++) {
    const base = groupId * 700;
    const source = full.filter(row => row.type === 'operation' && Math.floor(row.id / 12) === groupId);
    rows.push(group(groupId, base));
    rows.push(...source.filter(row => row.id % 2 === 0 && row.ms < base + 300).map(row => ({ ...row })));
    for (let lane = 0; lane < 6; lane++) {
      const id = groupId * 12 + lane * 2;
      for (const row of source.filter(row => row.id === id + 1 && row.ms === base + 300)) rows.push({ ...row, ms: base + 300 + lane * 10 });
      for (const row of source.filter(row => row.id === id + 1 && row.ms === base + 350)) rows.push({ ...row, ms: base + 301 + lane * 10 });
      for (const row of source.filter(row => [id, id + 1].includes(row.id) && row.ms === base + 650)) rows.push({ ...row, ms: base + 302 + lane * 10 });
    }
  }
  const result = validate(text(withSummary(rows)));
  assert.equal(result.qualified, true);
  assert.equal(result.max_live, 7);
  assert.equal(result.starts, 480);
});
rejectsMutation('every attempted group requires its explicit record', rows => { rows.splice(1, 1); }, /active group/);
rejectsMutation('group records cannot repeat', rows => { rows.splice(2, 0, clone(rows[1])); }, /Groups/);
rejectsMutation('unknown group fields are rejected', rows => { rows[1].address = 'private.example'; }, /fields/, full, false);
rejectsMutation('connect timing uses group timestamp rather than delayed socket creation', rows => {
  rows[1].ms = 0;
  for (let at = 2; at < rows.length; at++) if (rows[at].type === 'operation' && rows[at].id < 12) rows[at].ms += 400;
}, /group deadline/);
rejectsMutation('completion consumed requires successful completion API row', rows => {
  rows.splice(indexOf(rows, 1, 'completion-api'), 1);
}, /eligible signaled/);
rejectsMutation('completion API success must be immediately followed by its result', rows => {
  const at = indexOf(rows, 1, 'completion-api');
  const consumed = rows.splice(at + 1, 1)[0];
  rows.splice(at + 2, 0, consumed);
}, /immediate completion/);
rejectsMutation('completion API cannot succeed twice', rows => {
  const at = indexOf(rows, 1, 'completion-api'); rows.splice(at + 1, 0, clone(rows[at]));
}, /immediate completion/);
rejectsMutation('completion API cannot run on deferred lane', rows => {
  const at = indexOf(rows, 6, 'event-ready'); rows.splice(at + 1, 0, operation(6, 'completion-api', rows[at].ms));
}, /eligible signaled/);

function syntheticCompletionApiFailure(errorCode) {
  const rows = [header('plain'), group(0, 0)];
  for (const stage of ['socket', 'event-create', 'event-select', 'set-randomize', 'get-randomize']) rows.push(operation(0, stage, 0));
  rows.push(operation(0, 'connect', 0, -1, 10035), operation(0, 'event-ready', 1));
  rows.push(operation(0, 'completion-api', 1, -1, errorCode), failure(0, 'completion-api', 1, errorCode));
  rows.push(operation(0, 'close-intent', 1, 1), operation(0, 'closesocket', 1), operation(0, 'event-close', 1));
  return withSummary(rows);
}
for (const errorCode of [10055, 10061, 10035, 10022]) {
  test(`completion API error ${errorCode} is fatal and leaves completion unhandled`, () => {
    const error = capture(syntheticCompletionApiFailure(errorCode));
    assert.equal(error.code, 'nonqualifying-evidence');
    assert.equal(error.summary.expected_refusals, 0);
    assert.equal(error.summary.failures, 1);
    assert.equal(error.summary.closed_before_handling, 1);
    assert.equal(error.summary.ready_before_close, 1);
    assert.equal(error.summary.sockets_closed, 1);
    assert.equal(error.summary.events_closed, 1);
  });
}
rejectsMutation('failed completion API cannot have a consumed result', rows => {
  rows.splice(indexOf(rows, 0, 'close-intent'), 0, operation(0, 'completion-consumed', 1, -1, 10061));
}, /eligible signaled/, syntheticCompletionApiFailure(10061));
rejectsMutation('failed completion API cannot be retried', rows => {
  rows.splice(indexOf(rows, 0, 'close-intent'), 0, operation(0, 'completion-api', 1));
}, /eligible signaled/, syntheticCompletionApiFailure(10061));
rejectsMutation('failed completion API cannot claim handled close intent', rows => {
  pick(rows, 0, 'close-intent').result = 0;
}, /intent does not match/, syntheticCompletionApiFailure(10055));
rejectsMutation('10061 completion API failure still needs paired failure record', rows => {
  rows.splice(rows.findIndex(row => row.type === 'failure'), 1);
}, /Missing immediate failure/, syntheticCompletionApiFailure(10061));

test('deadline before the first socket preserves an empty attempted group', () => {
  const error = capture(withSummary([header('plain'), group(0, 10), failure(0, 'group-deadline', 710, 10060)]));
  assert.equal(error.code, 'nonqualifying-evidence');
  assert.equal(error.summary.operation_records, 0);
});
test('deadline between successful setup and connect preserves cleanup', () => {
  const rows = [header('plain'), group(0, 0)];
  for (const stage of ['socket', 'event-create', 'event-select', 'set-randomize', 'get-randomize']) rows.push(operation(0, stage, 0));
  rows.push(failure(0, 'group-deadline', 700, 10060));
  rows.push(operation(0, 'close-intent', 700), operation(0, 'closesocket', 700), operation(0, 'event-close', 700));
  const error = capture(withSummary(rows));
  assert.equal(error.code, 'nonqualifying-evidence');
  assert.equal(error.summary.starts, 0);
  assert.equal(error.summary.events_closed, 1);
});
test('35-second cleanup limit still controls qualification inside diagnostic bound', () => {
  const error = capture(withSummary(full, 35_001));
  assert.equal(error.code, 'nonqualifying-evidence');
  assert.equal(error.summary.complete, false);
  assert.equal(error.summary.elapsed_ms, 35_001);
});
test('summary at exactly the 35-second cleanup limit may qualify', () => {
  assert.equal(validate(text(withSummary(full, 35_000))).qualified, true);
});
test('cleanup-deadline at exactly 35 seconds is premature', () => {
  assert.equal(capture(withSummary([header('plain'), failure(0, 'cleanup-deadline', 35_000)])).code, 'invalid-evidence');
});
test('WSACleanup failure may be followed by cleanup deadline failure', () => {
  const rows = clone(full);
  rows.splice(rows.length - 1, 0, failure(0, 'wsa-cleanup', 35_001, 10055), failure(0, 'cleanup-deadline', 35_001, 10060));
  const error = capture(withSummary(rows));
  assert.equal(error.code, 'nonqualifying-evidence');
  assert.equal(error.summary.failures, 2);
});
test('watchdog-bound failed trace still derives its summary at 40 seconds', () => {
  const rows = clone(full);
  rows.splice(rows.length - 1, 0, failure(0, 'cleanup-deadline', 40_000, 10060));
  const error = capture(withSummary(rows));
  assert.equal(error.code, 'nonqualifying-evidence');
  assert.equal(error.summary.elapsed_ms, 40_000);
});

rejectsMutation('omitting API and result cannot convert a signaled consuming lane to exposure', rows => {
  rows.splice(indexOf(rows, 0, 'completion-api'), 2);
  pick(rows, 0, 'close-intent').result = 1;
}, /no completion/);
rejectsMutation('process cleanup cannot precede owned resource cleanup', rows => {
  rows.splice(indexOf(rows, 0, 'close-intent'), 0, failure(0, 'wsa-cleanup', 1, 10055));
}, /process cleanup/, syntheticAbort('get-randomize'));
rejectsMutation('negative error numbers are invalid', rows => { pick(rows, 0, 'socket').error = -1; }, /error number/);
rejectsMutation('fractional result values are invalid', rows => { pick(rows, 0, 'socket').result = 0.5; }, /stage or result/);
rejectsMutation('summary complete must be a boolean', rows => { rows.at(-1).complete = 1; }, /summary status/, full, false);
rejectsMutation('summary counts must be nonnegative safe integers', rows => { rows.at(-1).failures = -1; }, /summary counter/, full, false);
