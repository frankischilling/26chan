import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { validateOutput, MAX_BYTES, MAX_OPERATIONS, REQUEST_BYTES, summarizeDiagnostics, readDiagnostics, DIAGNOSTIC_LIMITS } from './validate-output.mjs';

const encode = records => records.map(row => JSON.stringify(row)).join('\n') + '\n';
function recount(records) {
  const ops = records.filter(row => row.type === 'operation');
  const good = stage => ops.filter(row => row.stage === stage && row.result >= 0 && row.error === 0);
  const summary = records.at(-1);
  Object.assign(summary, {
    attempts: ops.filter(row => row.stage === 'socket').length,
    successes: good('http-complete').length,
    failures: records.filter(row => row.type === 'failure').length,
    sockets_opened: good('socket').length,
    sockets_closed: good('closesocket').length,
    events_opened: good('event-create').length + good('write-event-create').length,
    events_closed: good('event-close').length + good('write-event-close').length,
    operation_records: ops.length,
    sync_writes: good('write-complete-sync').length,
    async_writes: good('write-complete-async').length,
    pending_at_close: good('pending-at-close').reduce((total, row) => total + row.result, 0),
    post_close_signals: good('post-close-signal').length,
    retained_writes: good('write-retained').length,
    bytes_sent: [...good('write-complete-sync'), ...good('write-complete-async')].reduce((total, row) => total + row.result, 0),
    bytes_received: good('http-recv').reduce((total, row) => total + row.result, 0),
  });
  return records;
}
function sample({ batches = 1, mode = 'plain', asynchronous = false, pendingConnect = true, partial = false, blockedReads = false } = {}) {
  const records = [{ type: 'header', schema: 2, profile: 'pooled-event-overlapped', exchanges: 6, mode, batches, width: 6, interval_ms: 50, total_cap_ms: 30000, request: 'owned-readyz' }];
  let ms = 0;
  const op = (id, stage, result = 0, error = 0) => records.push({ type: 'operation', id, stage, result, error, ms });
  for (let batch = 0; batch < batches; batch++) {
    ms = batch * 50;
    for (let lane = 0; lane < 6; lane++) {
      const id = batch * 6 + lane;
      for (const stage of ['socket', 'nonblocking', 'nodelay', 'keepalive', 'event-create', 'write-event-create', 'event-select-connect', 'set-randomize', 'get-randomize']) op(id, stage);
      records.push({ type: 'option', id, value: mode === 'randomized', length: 4, ms });
      op(id, 'binding-before');
      op(id, 'connect', pendingConnect ? -1 : 0, pendingConnect ? 10035 : 0);
      op(id, 'binding-after', 1);
    }
    for (let lane = 0; lane < 6; lane++) {
      const id = batch * 6 + lane;
      if (pendingConnect) for (const stage of ['connect-wait', 'connect-enumerate', 'connect-async']) op(id, stage);
    }
    for (let exchange = 0; exchange < 6; exchange++) {
      for (let lane = 0; lane < 6; lane++) {
        const id = batch * 6 + lane;
        op(id, 'exchange-begin', exchange);
        const parts = partial ? [1, 19, 50] : [70];
        for (const part of parts) {
          op(id, 'write-reset');
          op(id, 'write-submit', asynchronous ? -1 : 0, asynchronous ? 997 : 0);
          if (asynchronous) op(id, 'write-wait');
          op(id, asynchronous ? 'write-complete-async' : 'write-complete-sync', part);
        }
        if (exchange === 0) op(id, 'event-select-read-close');
        if (blockedReads) {
          // Readiness can race; a second EWOULDBLOCK is valid only with another
          // complete wait/enumerate/FD_READ/error observation sequence.
          for (let attempt = 0; attempt < 2; attempt++) {
            op(id, 'http-recv', -1, 10035);
            op(id, 'read-wait'); op(id, 'read-enumerate'); op(id, 'read-events', 1); op(id, 'read-event-error');
          }
        }
        for (const bytes of partial ? [3, 57, 40] : [100]) op(id, 'http-recv', bytes);
        op(id, 'http-complete', exchange);
      }
    }
    for (let lane = 0; lane < 6; lane++) {
      const id = batch * 6 + lane;
      op(id, 'pending-at-close'); op(id, 'shutdown'); op(id, 'closesocket');
    }
    for (let lane = 0; lane < 6; lane++) {
      const id = batch * 6 + lane;
      op(id, 'event-close'); op(id, 'write-event-close');
    }
  }
  op(batches * 6, 'wsa-cleanup');
  records.push({ type: 'summary', complete: true, elapsed_ms: ms });
  return recount(records);
}
const row = (records, stage, id = 0) => records.find(item => item.type === 'operation' && item.stage === stage && item.id === id);
const index = (records, stage, id = 0) => records.findIndex(item => item.type === 'operation' && item.stage === stage && item.id === id);
function rejects(records) {
  try { assert.equal(validateOutput(encode(records)).passed, false); }
  catch (error) { if (error.code === 'ERR_ASSERTION') throw error; }
}
function shiftFrom(records, from, delta) {
  for (const item of records.slice(from, -1)) item.ms += delta;
  records.at(-1).elapsed_ms += delta;
}
for (const [name, options] of [
  ['synchronous writes', {}],
  ['asynchronous writes with terminal results', { asynchronous: true }],
  ['synchronous connect', { pendingConnect: false }],
  ['randomized readback', { mode: 'randomized' }],
  ['short writes continue only unsent suffix', { partial: true }],
  ['partial asynchronous writes and repeated read readiness', { partial: true, asynchronous: true, blockedReads: true }],
  ['successive complete pools on fixed schedule', { batches: 3 }],
  ['default sixteen-pool workload', { batches: 16, asynchronous: true }],
]) test(`qualifies synthetic ${name}; reproduction remains inconclusive`, () => {
  const result = validateOutput(encode(sample(options)));
  assert.equal(result.passed, true);
  assert.equal(result.reproduction, 'inconclusive');
  assert.equal(result.summary.bytes_sent, result.summary.attempts * 6 * REQUEST_BYTES);
  assert.equal(result.summary.successes, result.summary.attempts * 6);
});
test('a valid unbound WSAEINVAL getsockname result qualifies', () => {
  const records = sample(); Object.assign(row(records, 'binding-before'), { result: -1, error: 10022 });
  assert.equal(validateOutput(encode(records)).passed, true);
});

for (const [name, mutate, options] of [
  ['unknown header field', r => { r[0].endpoint = 'private'; }],
  ['schema-one baseline cannot masquerade as pooled', r => { r[0].schema = 1; }],
  ['wrong profile', r => { r[0].profile = 'plain'; }],
  ['wrong exchange count', r => { r[0].exchanges = 1; }],
  ['wrong pool width', r => { r[0].width = 7; }],
  ['wrong workload cadence', r => { r[0].interval_ms = 1; }],
  ['wrong request identity', r => { r[0].request = 'other'; }],
  ['unsupported mode', r => { r[0].mode = 'retry'; }],
  ['oversized workload', r => { r[0].batches = 129; }],
  ['zero workload', r => { r[0].batches = 0; }],
  ['unknown stage', r => { r[1].stage = 'undocumented'; }],
  ['unrequested payload', r => { r[1].payload = 'secret'; }],
  ['missing summary', r => { r.pop(); }],
  ['missing cleanup', r => { r.splice(-2, 1); recount(r); }],
  ['duplicate cleanup', r => { r.splice(-2, 0, { ...r.at(-2) }); recount(r); }],
  ['cleanup before release', r => { const [cleanup] = r.splice(-2, 1); r.splice(index(r, 'event-close'), 0, cleanup); }],
  ['unowned socket close', r => { row(r, 'closesocket').id = 6; }],
  ['reused socket identity', r => { row(r, 'socket', 1).id = 0; }],
  ['socket close before shutdown', r => { const at = index(r, 'shutdown'); [r[at], r[at + 1]] = [r[at + 1], r[at]]; }],
  ['event release before other pool sockets close', r => { const [item] = r.splice(index(r, 'event-close'), 1); r.splice(index(r, 'pending-at-close', 1), 0, item); }],
  ['duplicate event creation', r => { const at = index(r, 'event-create'); r.splice(at, 0, { ...r[at] }); recount(r); }],
  ['duplicate event release', r => { const at = index(r, 'event-close'); r.splice(at, 0, { ...r[at] }); recount(r); }],
  ['missing separate write event', r => { r.splice(index(r, 'write-event-create'), 1); recount(r); }],
  ['hidden option API failure', r => { row(r, 'set-randomize').error = 10022; }],
  ['failed option API paired correctly', r => { Object.assign(row(r, 'set-randomize'), { result: -1, error: 10022 }); }],
  ['missing readback', r => { r.splice(r.findIndex(item => item.type === 'option'), 1); }],
  ['readback before getsockopt', r => { const at = r.findIndex(item => item.type === 'option'); [r[at - 1], r[at]] = [r[at], r[at - 1]]; }],
  ['wrong readback value', r => { r.find(item => item.type === 'option').value = true; }],
  ['wrong BOOL readback length', r => { r.find(item => item.type === 'option').length = 8; }],
  ['unreadable option presented as success', r => { Object.assign(r.find(item => item.type === 'option'), { value: null, length: null }); }],
  ['already bound before connect', r => { row(r, 'binding-before').result = 1; }],
  ['no implicit binding', r => { row(r, 'binding-after').result = 0; }],
  ['missing read-close event selection', r => { r.splice(index(r, 'event-select-read-close'), 1); recount(r); }],
  ['duplicate read-close selection', r => { const at = index(r, 'event-select-read-close'); r.splice(at, 0, { ...r[at] }); recount(r); }],
  ['missing exchange begin', r => { r.splice(index(r, 'exchange-begin'), 1); recount(r); }],
  ['nonsequential exchange begin', r => { row(r, 'exchange-begin').result = 1; }],
  ['wrong completion ordinal', r => { row(r, 'http-complete').result = 5; }],
  ['duplicate exchange completion', r => { const at = index(r, 'http-complete'); r.splice(at, 0, { ...r[at] }); recount(r); }],
  ['missing reset', r => { r.splice(index(r, 'write-reset'), 1); recount(r); }],
  ['missing submit', r => { r.splice(index(r, 'write-submit'), 1); recount(r); }],
  ['async submit without wait', r => { r.splice(index(r, 'write-wait'), 1); recount(r); }, { asynchronous: true }],
  ['async wait without terminal result', r => { r.splice(index(r, 'write-complete-async'), 1); recount(r); }, { asynchronous: true }],
  ['sync completion disguised as async', r => { row(r, 'write-complete-sync').stage = 'write-complete-async'; recount(r); }],
  ['async completion disguised as sync', r => { row(r, 'write-complete-async').stage = 'write-complete-sync'; recount(r); }, { asynchronous: true }],
  ['sync submit followed by unnecessary wait', r => { r.splice(index(r, 'write-complete-sync'), 0, { type: 'operation', id: 0, stage: 'write-wait', result: 0, error: 0, ms: 0 }); recount(r); }],
  ['zero-byte successful write', r => { row(r, 'write-complete-sync').result = 0; recount(r); }],
  ['oversized successful write', r => { row(r, 'write-complete-sync').result = 71; recount(r); }],
  ['short request incorrectly marked complete', r => { row(r, 'write-complete-sync').result = 69; recount(r); }],
  ['request replay after complete request', r => { const at = index(r, 'write-reset'); r.splice(at + 3, 0, ...r.slice(at, at + 3).map(item => ({ ...item }))); recount(r); }],
  ['recv before terminal write', r => { const a = index(r, 'http-recv'), b = index(r, 'write-complete-sync'); [r[a], r[b]] = [r[b], r[a]]; }],
  ['empty receive', r => { row(r, 'http-recv').result = 0; recount(r); }],
  ['undersized response', r => { row(r, 'http-recv').result = 1; recount(r); }],
  ['recv exceeds native buffer', r => { row(r, 'http-recv').result = 1025; recount(r); }],
  ['response over 8192 bytes', r => { const at = index(r, 'http-recv'); r.splice(at, 1, ...Array.from({ length: 9 }, () => ({ ...r[at], result: 1024 }))); recount(r); }],
  ['read wait removed after EWOULDBLOCK', r => { r.splice(index(r, 'read-wait'), 1); recount(r); }, { blockedReads: true }],
  ['read enumeration removed', r => { r.splice(index(r, 'read-enumerate'), 1); recount(r); }, { blockedReads: true }],
  ['read per-event error removed', r => { r.splice(index(r, 'read-event-error'), 1); recount(r); }, { blockedReads: true }],
  ['FD_READ missing', r => { row(r, 'read-events').result = 0; }, { blockedReads: true }],
  ['FD_CLOSE included', r => { row(r, 'read-events').result = 33; }, { blockedReads: true }],
  ['read event contains unexpected flags', r => { row(r, 'read-events').result = 3; }, { blockedReads: true }],
  ['read event has API error', r => { Object.assign(row(r, 'read-event-error'), { result: -1, error: 10054 }); }, { blockedReads: true }],
  ['read wait timeout hidden', r => { row(r, 'read-wait').result = 258; }, { blockedReads: true }],
  ['write wait timeout hidden', r => { row(r, 'write-wait').result = 258; }, { asynchronous: true }],
  ['pending ownership flag forged', r => { row(r, 'pending-at-close').result = 1; recount(r); }],
  ['missing pending ownership observation', r => { r.splice(index(r, 'pending-at-close'), 1); recount(r); }],
  ['hidden global cleanup failure', r => { Object.assign(row(r, 'wsa-cleanup', 6), { result: -1, error: 10022 }); }],
  ['negative timestamp', r => { r[1].ms = -1; }],
  ['decreasing timestamp', r => { r[1].ms = 1; }],
  ['summary before observations', r => { r.at(-2).ms = 1; }],
  ['connect deadline exceeded', r => { shiftFrom(r, index(r, 'connect-wait'), 1001); }],
  ['exchange deadline exceeded', r => { shiftFrom(r, index(r, 'http-recv'), 1001); }],
  ['successful global deadline exceeded', r => { r.at(-1).elapsed_ms = 30001; }],
  ['hard cleanup deadline exceeded', r => { r.at(-1).complete = false; r.at(-1).elapsed_ms = 35001; }],
  ['next pool starts before fixed offset', r => { for (const item of r.slice(1, -1)) item.ms = 0; r.at(-1).elapsed_ms = 0; }, { batches: 2 }],
  ['false completeness', r => { r[0].batches = 2; }],
]) test(`fails closed: ${name}`, () => { const records = sample(options); mutate(records); rejects(records); });

for (const field of ['attempts', 'successes', 'failures', 'sockets_opened', 'sockets_closed', 'events_opened', 'events_closed', 'operation_records', 'sync_writes', 'async_writes', 'pending_at_close', 'post_close_signals', 'retained_writes', 'bytes_sent', 'bytes_received']) {
  test(`recomputes ${field} instead of trusting summary`, () => { const records = sample(); records.at(-1)[field]++; assert.throws(() => validateOutput(encode(records)), /Counter|cap|complete/); });
}

// Remove the first socket's HTTP work after its initial submit. Other sockets
// continue their own sequential exchanges; no failed request is retried.
function failedWrite({ pending = false, signal = false } = {}) {
  let records = sample({ asynchronous: pending });
  const submitAt = index(records, 'write-submit');
  const retained = records.slice(0, submitAt + 1);
  const submit = retained.at(-1);
  Object.assign(submit, { result: -1, error: pending ? 997 : 10055 });
  const failure = (stage, error) => ({ type: 'failure', id: 0, stage, error, ms: 0 });
  if (pending) {
    retained.push({ type: 'operation', id: 0, stage: 'write-wait', result: 258, error: 0, ms: 0 });
    retained.push(failure('write-wait', 10060));
  } else retained.push(failure('write-submit', 10055));
  let closing = false;
  for (const item of records.slice(submitAt + 1)) {
    if (item.id === 0 && item.stage === 'pending-at-close') closing = true;
    if (item.id === 0 && !closing) continue;
    if (pending && item.id === 0 && item.stage === 'pending-at-close') {
      item.result = 1; retained.push(item, failure('pending-at-close', 996)); continue;
    }
    if (pending && item.id === 0 && item.stage === 'event-close') {
      retained.push({ type: 'operation', id: 0, stage: 'post-close-wait', result: signal ? 0 : 258, error: 0, ms: 0 });
      if (signal) retained.push({ type: 'operation', id: 0, stage: 'post-close-signal', result: 0, error: 0, ms: 0 });
      retained.push({ type: 'operation', id: 0, stage: 'write-retained', result: 1, error: 0, ms: 0 }, failure('completion-incomplete', 996));
    }
    if (pending && item.id === 0 && item.stage === 'write-event-close') continue;
    retained.push(item);
  }
  records = retained;
  if (pending) records.at(-1).complete = false;
  return recount(records);
}
test('reported 10055 stays an unqualified diagnostic result', () => {
  const result = validateOutput(encode(failedWrite()));
  assert.equal(result.error10055, 1); assert.equal(result.passed, false);
});
test('unreported 10055 cannot be hidden by a plausible summary', () => {
  const records = failedWrite().filter(item => item.type !== 'failure'); recount(records);
  assert.throws(() => validateOutput(encode(records)), /Unreported/);
});
for (const signal of [false, true]) test(`pending write retained after ${signal ? 'notification' : 'timeout'} is always unqualified`, () => {
  const result = validateOutput(encode(failedWrite({ pending: true, signal })));
  assert.equal(result.passed, false); assert.equal(result.summary.pending_at_close, 1);
  assert.equal(result.summary.retained_writes, 1); assert.equal(result.summary.events_closed, 11);
  assert.equal(result.summary.post_close_signals, Number(signal));
});
test('post-close notification cannot release write storage/event', () => {
  const records = failedWrite({ pending: true, signal: true });
  records.splice(index(records, 'event-close'), 0, { type: 'operation', id: 0, stage: 'write-event-close', result: 0, error: 0, ms: 0 }); recount(records);
  assert.throws(() => validateOutput(encode(records)), /Pending\/retained/);
});
test('post-close notification cannot stand in for terminal completion bytes', () => {
  const records = failedWrite({ pending: true, signal: true });
  Object.assign(row(records, 'post-close-signal'), { stage: 'write-complete-async', result: 70 }); recount(records);
  assert.throws(() => validateOutput(encode(records)), /I\/O after close/);
});
test('false complete cannot rehabilitate retained writes', () => {
  const records = failedWrite({ pending: true, signal: true }); records.at(-1).complete = true;
  assert.equal(validateOutput(encode(records)).passed, false);
});
test('incomplete schedule cannot qualify', () => {
  const records = sample(); records[0].batches = 2; records.at(-1).complete = false;
  assert.equal(validateOutput(encode(records)).passed, false);
});
test('output requires final newline, no blank tail, and valid JSON objects', () => {
  for (const text of [encode(sample()).trimEnd(), encode(sample()) + '\n', 'null\nnull\n', '[]\n[]\n', '{}\n']) assert.throws(() => validateOutput(text));
});
test('duplicate and escaped duplicate JSON keys are rejected', () => {
  const text = encode(sample());
  for (const key of ['schema', '\\u0073chema']) assert.throws(() => validateOutput(text.replace('"schema":2', `"schema":1,"${key}":2`)), /Duplicate/);
});
test('4 MiB byte cap includes multibyte text', () => {
  assert.throws(() => validateOutput('x'.repeat(MAX_BYTES) + '\n'), /oversized/);
  assert.throws(() => validateOutput('é'.repeat(MAX_BYTES / 2) + '\n'), /oversized/);
});
test('individual record size capped independently', () => {
  const records = sample(); records[1].payload = 'x'.repeat(1024);
  assert.throws(() => validateOutput(encode(records)), /line length/);
});
test('operation cap is enforced even for an explicitly failed trace', () => {
  const records = sample(), at = index(records, 'http-recv');
  records.splice(at, 0, ...Array.from({ length: MAX_OPERATIONS }, () => ({ ...records[at], result: 1 })));
  records.at(-1).complete = false; recount(records);
  assert.throws(() => validateOutput(encode(records)), /Operation output cap/);
});
test('every single operation omission fails closed even with honest counters', () => {
  const original = sample({ asynchronous: true, blockedReads: true });
  for (let at = 1; at < original.length - 1; at++) {
    if (original[at].type !== 'operation') continue;
    const records = structuredClone(original); records.splice(at, 1); recount(records);
    rejects(records);
  }
});
test('every non-repeatable operation duplication fails closed with honest counters', () => {
  const original = sample({ asynchronous: true });
  for (let at = 1; at < original.length - 1; at++) {
    // Repeated positive recv rows can be separate chunks with the same length
    // and rounded timestamp, so their duplication is not independently provable.
    if (original[at].type !== 'operation' || original[at].stage === 'http-recv') continue;
    const records = structuredClone(original); records.splice(at, 0, { ...records[at] }); recount(records);
    rejects(records);
  }
});
test('adjacent distinct operation reordering fails closed', () => {
  const original = sample({ asynchronous: true, blockedReads: true });
  for (let at = 1; at < original.length - 2; at++) {
    const first = original[at], next = original[at + 1];
    if (first.type !== 'operation' || next.type !== 'operation' || first.stage === next.stage) continue;
    const records = structuredClone(original); [records[at], records[at + 1]] = [records[at + 1], records[at]];
    rejects(records);
  }
});
test('read selection before first request is rejected', () => {
  const records = sample();
  const [selection] = records.splice(index(records, 'event-select-read-close'), 1);
  records.splice(index(records, 'exchange-begin'), 0, selection);
  assert.throws(() => validateOutput(encode(records)), /Expected/);
});
test('next pool cannot begin while prior sockets or events are still owned', () => {
  const records = sample({ batches: 2 });
  const [socket] = records.splice(index(records, 'socket', 6), 1); socket.ms = 0;
  records.splice(index(records, 'pending-at-close'), 0, socket);
  assert.throws(() => validateOutput(encode(records)), /Creation order\/schedule|Prior pool/);
});
test('reset before pending completion cannot reuse overlapped storage', () => {
  const records = sample({ asynchronous: true });
  records.splice(index(records, 'write-wait'), 0, { ...row(records, 'write-reset') }); recount(records);
  assert.throws(() => validateOutput(encode(records)), /Reset\/reuse/);
});
test('millisecond rounding at one second does not add an unlogged tolerance', () => {
  const records = sample(); shiftFrom(records, index(records, 'http-recv'), 1000);
  assert.equal(validateOutput(encode(records)).passed, true);
});
test('CLI distinguishes qualifying, unqualified, malformed and missing evidence', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'pooled-validator-'));
  const file = path.join(directory, 'output.jsonl');
  const script = new URL('./validate-output.mjs', import.meta.url);
  const run = (...args) => spawnSync(process.execPath, [fileURLToPath(script), ...args], { encoding: 'utf8' });
  try {
    fs.writeFileSync(file, encode(sample()));
    const good = run(file); assert.equal(good.status, 0, good.stderr);
    assert.equal(JSON.parse(good.stdout).reproduction, 'inconclusive');
    fs.writeFileSync(file, encode(failedWrite())); assert.equal(run(file).status, 1);
    fs.writeFileSync(file, '{}\n'); assert.equal(run(file).status, 2);
    assert.equal(run(path.join(directory, 'missing.jsonl')).status, 2);
    assert.equal(run().status, 2);
    fs.writeFileSync(file, 'x'.repeat(MAX_BYTES + 1)); assert.equal(run(file).status, 2);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

// Static contracts complement synthetic evidence tests. They cannot establish
// native Windows runtime behavior or substitute for a hosted execution.
const source = fs.readFileSync(new URL('./winsock-probe.cpp', import.meta.url), 'utf8');
test('source request bytes and workload match the independent validator', () => {
  const request = source.match(/const char kRequest\[\] = ("(?:[^"\\]|\\.)*");/);
  assert.ok(request); assert.equal(Buffer.byteLength(JSON.parse(request[1])), REQUEST_BYTES);
  assert.match(source, /kExchanges = 6/); assert.match(source, /kWidth = 6/);
  assert.match(source, /kOperationMs = 1000, kTotalMs = 30000/); assert.match(source, /kCleanupMs = 35000/);
  assert.match(source, /kMaxOperationRecords = 20000/);
});
test('source asks terminal status only on a live socket in HTTP', () => {
  assert.equal((source.match(/WSAGetOverlappedResult\s*\(/g) ?? []).length, 1);
  const http = source.slice(source.indexOf('bool http('), source.indexOf('\nint run('));
  assert.match(http, /WSAGetOverlappedResult\(item\.socket,/);
  const release = source.slice(source.indexOf('void closeSocket()'), source.indexOf('~Owned()'));
  assert.doesNotMatch(release, /(?:WSA)?GetOverlappedResult\s*\(|\.Internal(?:High)?\b/);
  assert.match(release, /\(void\)write\.release\(\)/);
  assert.match(release, /writeEvent = WSA_INVALID_EVENT/);
});
test('source sends overlapped writes with null byte-count pointer and owned storage', () => {
  assert.match(source, /WSASend\(item\.socket, &write\.descriptor, 1, nullptr, 0, &write\.overlapped, nullptr\)/);
  assert.match(source, /std::array<char, sizeof\(kRequest\) - 1> buffer/);
  assert.match(source, /write\.overlapped\.hEvent = item\.writeEvent/);
  assert.match(source, /sent \+= transferred/);
  assert.match(source, /if \(write\.pending\).*write-ownership/);
});
test('source group close precedes every release and does not hide errors', () => {
  assert.match(source, /for \(auto& item : active\) if \(item\) item->closeSocket\(\);\s*for \(auto& item : active\) if \(item\) item->releaseResources\(\);/);
  assert.doesNotMatch(source, /10055\s*(?:\)|&&|\|\||==)|WSAENOBUFS\s*(?:\)|&&|\|\||==)/);
  assert.doesNotMatch(source, /for\s*\([^)]*retry|while\s*\([^)]*retry|tolerat/i);
});

const launcher = fs.readFileSync(new URL('./run-probe.ps1', import.meta.url), 'utf8');
function assertTimeoutCleanupContract(text) {
  const timeout = text.match(/if \(-not \$compilerProcess\.WaitForExit\(120000\)\) \{([\s\S]*?)\n  \}/)?.[1];
  assert.ok(timeout, 'Compiler timeout branch must be explicit');
  assert.match(timeout, /\$status\.compiler_timeout=\$true/);
  assert.match(timeout, /\$status\.child_cleanup_complete=\$false/);
  assert.match(timeout, /\$status\.cleanup_unverified_reason='compiler-timeout-descendants-unverified'/);
  assert.match(timeout, /\$status\.passed=\$false/);
  assert.match(timeout, /Save-EvidenceState\s*throw 'Compiler timeout\.'/);
  const cleanup = text.slice(text.indexOf('} finally {'));
  assert.match(cleanup, /\$entry\.process\.Kill\(\$true\)/);
  assert.doesNotMatch(cleanup, /\.Kill\(\)/);
  const kill = cleanup.indexOf('$entry.process.Kill($true)');
  const beforeKill = cleanup.slice(0, kill);
  assert.match(beforeKill, /\$status\.child_cleanup_complete=\$false/);
  assert.match(beforeKill, /\$status\.process_tree_kill_attempted=\$true/);
  assert.match(cleanup, /WaitForExit\(5000\)/);
  // Root exit must never restore a claim about descendant cleanup.
  assert.doesNotMatch(text, /\$status\.child_cleanup_complete\s*=\s*\$true/);
  assert.match(cleanup, /-not \$status\.child_cleanup_complete\) \{ \$qualificationExit=1 \}/);
  assert.match(cleanup, /exit \$finalExit/);
}
test('compiler timeout remains unverified even if the root exits before cleanup', () => {
  assertTimeoutCleanupContract(launcher);
});
test('cleanup contract rejects root-only kill and false descendant certification', () => {
  for (const altered of [
    launcher.replace('.Kill($true)', '.Kill()'),
    launcher.replace("$status.cleanup_unverified_reason='compiler-timeout-descendants-unverified'", ''),
    launcher.replace('$status.compiler_timeout=$true', '$status.compiler_timeout=$false'),
    launcher.replace('$status.child_cleanup_complete=$false', '$status.child_cleanup_complete=$true'),
    launcher.replace('$entry.process.Dispose()', '$entry.process.Dispose(); $status.child_cleanup_complete=$true'),
    launcher.replace('$status.process_tree_kill_attempted=$true', '$status.process_tree_kill_attempted=$false'),
  ]) assert.throws(() => assertTimeoutCleanupContract(altered));
});
test('hosted wrapper rejects unverified launcher cleanup', () => {
  const hosted = fs.readFileSync(new URL('./run-hosted.ps1', import.meta.url), 'utf8');
  assert.match(hosted, /\$status\.child_cleanup_complete -ne \$true/);
});

function diagnosticStatus(overrides = {}) {
  return { schema: 1, phase: 'finished', compiler_exit: 0, native_exit: 0, validator_exit: 0, compiler_timeout: false, native_timeout: false, validator_timeout: false, child_cleanup_complete: true, process_tree_kill_attempted: false, cleanup_unverified_reason: null, failure_class: null, complete: true, passed: true, ...overrides };
}
function diagnosticInputs(overrides = {}) {
  return { status: JSON.stringify(diagnosticStatus()), native: encode(sample()), compiler_stdout: '', compiler_stderr: '', ...overrides };
}
test('diagnostic mode exposes allowlisted status and actual completion counts', () => {
  const report = summarizeDiagnostics(diagnosticInputs({ native: encode(sample({ asynchronous: true })) }));
  assert.equal(report.status_state, 'valid'); assert.equal(report.native_state, 'validated');
  assert.equal(report.native_passed, true); assert.equal(report.native_summary.async_writes, 36);
  assert.equal(report.native_summary.sync_writes, 0); assert.deepEqual(report.native_failures, []);
  assert.equal(report.native_summary.pending_at_close, 0);
  assert.ok(Buffer.byteLength(JSON.stringify(report)) < 8192);
});
test('absent or rejected launcher status cannot imply native success', () => {
  for (const status of [null, '{}', '{"schema":1,"schema":1}', 'arbitrary-secret', false, 'x'.repeat(DIAGNOSTIC_LIMITS.status + 1)]) {
    const report = summarizeDiagnostics(diagnosticInputs({ status }));
    assert.notEqual(report.status_state, 'valid'); assert.equal(report.status, null);
    assert.equal(report.native_passed, null); assert.equal(report.native_summary, null);
    assert.equal(report.compiler_state, 'unavailable');
  }
});
test('launcher status rejects unknown fields and untrusted scalar types', () => {
  for (const override of [{ phase: 'secret-path' }, { compiler_exit: '0' }, { native_exit: 2147483648 }, { passed: 'true' }, { complete: 1 }, { cleanup_unverified_reason: 'secret' }, { failure_class: 'secret' }, { secret: 'never print' }]) {
    const report = summarizeDiagnostics(diagnosticInputs({ status: JSON.stringify(diagnosticStatus(override)) }));
    assert.equal(report.status_state, 'rejected'); assert.equal(report.status, null);
    assert.doesNotMatch(JSON.stringify(report), /secret|never print/);
  }
});
test('compiler errors export only fixed code and source line, never paths or messages', () => {
  const report = summarizeDiagnostics(diagnosticInputs({
    compiler_stdout: 'C:\\private\\secret\\winsock-probe.cpp(123,4): error C2065: sensitive contents\nLINK : fatal error LNK1120: sensitive symbol\nuntrusted arbitrary text\n',
    compiler_stderr: 'C:\\private\\different.cpp(17): error C9999: not our source\n',
  }));
  assert.equal(report.compiler_state, 'scanned');
  assert.deepEqual(report.compiler_errors, [{ code: 'C2065', line: 123 }, { code: 'LNK1120', line: null }]);
  assert.doesNotMatch(JSON.stringify(report), /private|secret|sensitive|symbol|arbitrary|different/);
});
test('oversized or nontext compiler evidence is rejected and error counts are bounded', () => {
  for (const text of ['x'.repeat(DIAGNOSTIC_LIMITS.compiler + 1), false, { message: 'secret' }]) {
    const report = summarizeDiagnostics(diagnosticInputs({ compiler_stdout: text }));
    assert.equal(report.compiler_state, 'rejected'); assert.deepEqual(report.compiler_errors, []);
  }
  const report = summarizeDiagnostics(diagnosticInputs({ compiler_stdout: 'winsock-probe.cpp(1): error C2065: secret\n'.repeat(30) }));
  assert.equal(report.compiler_errors.length, 16); assert.equal(report.compiler_errors_truncated, true);
});
test('native diagnostics reject malformed, oversized or untrusted rows without echoing them', () => {
  const good = sample();
  const unknown = structuredClone(good); unknown[1].secret = 'do-not-print';
  const stage = structuredClone(good); stage[1].stage = 'do-not-print';
  const badHeader = structuredClone(good); badHeader[0].profile = 'do-not-print';
  const badSummary = structuredClone(good); badSummary.at(-1).bytes_sent = 'do-not-print';
  for (const native of ['bad json\n', 'x'.repeat(MAX_BYTES + 1), false, encode(unknown), encode(stage), encode(badHeader), encode(badSummary)]) {
    const report = summarizeDiagnostics(diagnosticInputs({ native }));
    assert.equal(report.native_state, 'rejected'); assert.equal(report.native_summary, null); assert.equal(report.native_passed, null);
    assert.doesNotMatch(JSON.stringify(report), /do-not-print|bad json/);
  }
});
test('native failures stay failures and expose only bounded ordinal stage error', () => {
  const records = failedWrite();
  const report = summarizeDiagnostics(diagnosticInputs({ native: encode(records), status: JSON.stringify(diagnosticStatus({ native_exit: 1, validator_exit: 1, passed: false })) }));
  assert.equal(report.native_passed, false); assert.equal(report.status.passed, false);
  assert.ok(report.native_failures.length > 0);
  for (const failure of report.native_failures) assert.deepEqual(Object.keys(failure).sort(), ['error', 'id', 'stage']);
  const many = structuredClone(records);
  const failure = many.find(row => row.type === 'failure');
  many.splice(-1, 0, ...Array.from({ length: 30 }, () => ({ ...failure })));
  const bounded = summarizeDiagnostics(diagnosticInputs({ native: encode(many) }));
  assert.equal(bounded.native_failures.length, 16); assert.equal(bounded.failures_truncated, true);
  assert.equal(bounded.native_passed, null); assert.equal(bounded.native_state, 'shape-valid');
});
test('diagnostic validator explanations are fixed codes with bounded fields', () => {
  const missing = sample(); missing.splice(index(missing, 'event-select-read-close'), 1); recount(missing);
  const report = summarizeDiagnostics(diagnosticInputs({ native: encode(missing) }));
  assert.equal(report.native_passed, null); assert.equal(report.native_state, 'shape-valid');
  assert.deepEqual(report.validator_problem, { code: 'expected-stage', stage: 'event-select-read-close', id: 0 });
  const counter = sample(); counter.at(-1).bytes_sent++;
  assert.deepEqual(summarizeDiagnostics(diagnosticInputs({ native: encode(counter) })).validator_problem, { code: 'counter-mismatch', counter: 'bytes_sent' });
});
test('diagnostics reads only bounded regular evidence files', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'pooled-diagnostics-'));
  try {
    assert.equal(readDiagnostics(directory).status_state, 'unavailable');
    fs.writeFileSync(path.join(directory, 'status.json'), JSON.stringify(diagnosticStatus()));
    fs.writeFileSync(path.join(directory, 'output.jsonl'), encode(sample()));
    assert.equal(readDiagnostics(directory).native_passed, true);
    fs.writeFileSync(path.join(directory, 'compiler.stdout.txt'), 'x'.repeat(DIAGNOSTIC_LIMITS.compiler + 1));
    assert.equal(readDiagnostics(directory).compiler_state, 'rejected');
    fs.writeFileSync(path.join(directory, 'status.json'), 'x'.repeat(DIAGNOSTIC_LIMITS.status + 1));
    assert.equal(readDiagnostics(directory).status_state, 'rejected');
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});
test('diagnostic CLI never weakens ordinary qualification mode or echoes missing paths', () => {
  const script = fileURLToPath(new URL('./validate-output.mjs', import.meta.url));
  const absent = path.join(os.tmpdir(), 'pooled-secret-missing-directory');
  const report = spawnSync(process.execPath, [script, '--diagnostics', absent], { encoding: 'utf8' });
  assert.equal(report.status, 0); assert.equal(report.stderr, ''); assert.doesNotMatch(report.stdout, /secret|missing-directory/);
  assert.equal(JSON.parse(report.stdout).native_passed, null);
  assert.equal(spawnSync(process.execPath, [script, absent], { encoding: 'utf8' }).status, 2);
  const malformedArguments = spawnSync(process.execPath, [script, '--diagnostics'], { encoding: 'utf8' });
  assert.deepEqual(JSON.parse(malformedArguments.stdout), { type: 'pooled-diagnostics-unavailable', schema: 1, code: 'report-error' });
});
test('reporter is bounded and owned without changing the captured qualification exit', () => {
  const report = launcher.slice(launcher.indexOf('# Reporting is separate'));
  assert.match(report, /\$qualificationExit=0\s*if \(-not \$status\.complete -or -not \$status\.passed -or -not \$status\.child_cleanup_complete\) \{ \$qualificationExit=1 \}/);
  assert.equal((report.match(/\$qualificationExit=/g) ?? []).length, 2);
  assert.match(report, /\$reporter=Start-Process/); assert.match(report, /WaitForExit\(10000\)/);
  assert.match(report, /\$reporter\.Kill\(\$true\)/); assert.match(report, /WaitForExit\(5000\)/);
  assert.match(report, /\$reporter\.Dispose\(\)/); assert.match(report, /exit \$finalExit/);
  assert.doesNotMatch(report, /Get-Content|Write-Output\s+\$_|throw \$_/);
  assert.match(report, /RedirectStandardError/);
});

function assertReporterFailureGate(text) {
  assert.match(text, /\$reporterHealthy=\(\$reporterLaunched -and -not \$reporterTimeout -and \$reporterCleanup -and \$reporterExit -eq 0\)/);
  assert.match(text, /\$finalExit=\$qualificationExit\s*if \(-not \$reporterHealthy\) \{ \$finalExit=1 \}/);
  assert.match(text, /qualification_exit=\$qualificationExit/);
  assert.match(text, /final_exit=\$finalExit/);
  assert.match(text, /exit \$finalExit\s*$/);
}
test('reporter failure adds failure without clearing the original native result', () => {
  assertReporterFailureGate(launcher);
  for (const altered of [
    launcher.replace('exit $finalExit', 'exit $qualificationExit'),
    launcher.replace('if (-not $reporterHealthy) { $finalExit=1 }', ''),
    launcher.replace('-and -not $reporterTimeout ', ''),
    launcher.replace('-and $reporterCleanup ', ''),
    launcher.replace('$reporterLaunched -and ', ''),
    launcher.replace('-and $reporterExit -eq 0', ''),
  ]) assert.throws(() => assertReporterFailureGate(altered));
});
test('native pass cannot qualify with reporter timeout or unverified cleanup', () => {
  assertReporterFailureGate(launcher);
  const finalExit = (qualification, { launched = true, timeout = false, cleanup = true, exit = 0 } = {}) => {
    const healthy = launched && !timeout && cleanup && exit === 0;
    return healthy ? qualification : 1;
  };
  assert.equal(finalExit(0), 0); assert.equal(finalExit(1), 1);
  for (const bad of [{ timeout: true }, { cleanup: false }, { launched: false }, { exit: null }, { exit: 1 }]) {
    assert.equal(finalExit(0, bad), 1); assert.equal(finalExit(1, bad), 1);
  }
});
