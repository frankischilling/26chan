import test from 'node:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { validateOutput, validateFixture } from './validate-output.mjs';
import { syntheticEvidence, recount, jsonl } from './synthetic-evidence.mjs';
const run = ({ records, fixture }) => validateOutput(jsonl(records), jsonl(fixture));
for (const schedule of ['serialized', 'overlap']) for (const randomize of [false, true]) test(`full ${schedule}/${randomize} lifecycle qualifies, reproduction remains inconclusive`, () => {
  const result = run(syntheticEvidence(schedule, randomize));
  assert.equal(result.passed, true); assert.equal(result.workloadComplete, true); assert.equal(result.exposureVerified, true); assert.equal(result.reproduction, 'inconclusive');
});
function fixtureReject(name, edit) { test(name, () => { const sample = syntheticEvidence(); edit(sample); const result = run(sample); assert.equal(result.passed, false); assert.equal(result.fixtureEvidence.state, 'invalid'); }); }
function reject(name, edit) { test(name, () => { const sample = syntheticEvidence(); edit(sample); recount(sample.records); assert.throws(() => run(sample)); }); }
const find = (sample, stage, id = 0) => sample.records.find(r => r.stage === stage && r.id === id);
reject('missing terminal record', s => s.records.pop());
reject('summary-only evidence', s => s.records.splice(1, s.records.length - 2));
reject('counter tampering', s => s.records.at(-1).attempts = 120);
reject('wrong option contrast', s => s.records.find(r => r.type === 'option').value = true);
reject('option byte width', s => s.records.find(r => r.type === 'option').length = 1);
reject('socket ID duplication', s => find(s, 'socket', 1).id = 0);
reject('pool lane identity', s => find(s, 'connect', 1).lane = 2);
reject('exchange identity', s => find(s, 'body-consumed').exchange = 4);
reject('request suffix changed', s => find(s, 'write-submit-bytes').result--);
reject('write completion overrun', s => find(s, 'write-complete-sync').result++);
reject('recv exceeds buffer', s => find(s, 'http-recv').result = 1025);
reject('body exceeds contract', s => find(s, 'body-consumed').result = 4097);
reject('pending write claimed at close', s => find(s, 'pending-at-close').result = 1);
reject('HTTP complete removed', s => s.records.splice(s.records.indexOf(find(s, 'http-complete')), 1));
reject('setup stage omitted', s => s.records.splice(s.records.indexOf(find(s, 'nodelay')), 1));
reject('duplicate close', s => s.records.splice(s.records.indexOf(find(s, 'closesocket')), 0, { ...find(s, 'closesocket') }));
reject('short pool retirement gap', s => find(s, 'next-pool-start', 6).result = 99);
reject('missing pool boundary', s => s.records.splice(s.records.indexOf(find(s, 'pool-retired')), 1));
reject('missing lane0 service', s => s.records.splice(s.records.indexOf(find(s, 'receive-service')), 1));
reject('wrong lane0 service identity', s => find(s, 'receive-service').result = 5);
reject('work exceeds30s', s => s.records.at(-1).elapsed_ms = 30001);
reject('cleanup exceeds35s', s => s.records.at(-1).elapsed_ms = 35001);
reject('first failed operation overwritten', s => s.records.at(-1).first_error = 10055);
fixtureReject('fixture pool mismatch', s => s.fixture[0].pool = 1);
fixtureReject('fixture duplicate sequence', s => s.fixture[1].sequence = 1);
fixtureReject('fixture wrong bytes', s => s.fixture[1].emitted_bytes = 1023);
fixtureReject('fixture too-short server delay', s => s.fixture[2].monotonic_ns = 249999999);
fixtureReject('fixture reordered emission', s => s.fixture[1].event = 'suffix_emitted');
reject('malformed operation result/error', s => find(s, 'socket').error = 10055);
test('missing fixture is inconclusive with complete workload', () => { const s = syntheticEvidence(); const r = validateOutput(jsonl(s.records), ''); assert.equal(r.workloadComplete, true); assert.equal(r.passed, false); });
test('incomplete fixture is inconclusive', () => { const s = syntheticEvidence(); s.fixture.pop(); assert.equal(run(s).passed, false); });
test('fixture log output failure is inconclusive', () => { const s = syntheticEvidence(); s.fixture[3].output_failed = true; assert.equal(run(s).passed, false); });
test('200ms exposure boundary fails closed without shortening workload', () => {
  const s = syntheticEvidence();
  const from = s.records.indexOf(find(s, 'connect-submit', 5)), to = s.records.indexOf(find(s, 'http-complete'));
  for (const row of s.records.slice(from, to)) if ('ms' in row) { row.ms = Math.max(row.ms, 200); if ('boot_ms' in row) row.boot_ms = 100000 + row.ms; }
  const result = run(s); assert.equal(result.workloadComplete, true); assert.equal(result.exposureVerified, false); assert.equal(result.passed, false);
});
test('199ms exposure boundary qualifies', () => {
  const s = syntheticEvidence();
  const from = s.records.indexOf(find(s, 'connect-submit', 5)), to = s.records.indexOf(find(s, 'http-complete'));
  for (const row of s.records.slice(from, to)) if ('ms' in row) { row.ms = Math.max(row.ms, 199); if ('boot_ms' in row) row.boot_ms = 100000 + row.ms; }
  assert.equal(run(s).passed, true);
});
test('duplicate JSON fields and escaped aliases rejected', () => {
  const s = syntheticEvidence(); const text = jsonl(s.records);
  assert.throws(() => validateOutput(text.replace('"schema":1', '"schema":1,"schema":1'), jsonl(s.fixture)));
  assert.throws(() => validateOutput(text.replace('"schema":1', '"schema":1,"sch\\u0065ma":1'), jsonl(s.fixture)));
});
test('truncated JSONL rejected', () => { const s = syntheticEvidence(); assert.throws(() => validateOutput(jsonl(s.records).trimEnd(), jsonl(s.fixture))); });
test('unbounded input rejected', () => assert.throws(() => validateOutput('x'.repeat(8 * 1024 * 1024 + 1))));
test('fixture unknown fields rejected', () => { const s = syntheticEvidence(); s.fixture[0].network_delivery = true; assert.throws(() => validateFixture(jsonl(s.fixture))); });
function failedSocket(error = 10055) {
  const s = syntheticEvidence();
  const header = s.records[0], socket = { ...find(s, 'socket'), result: -1, error };
  const failure = { type: 'failure', id: 0, stage: 'socket', error, ms: 0 };
  const pending = { ...find(s, 'pending-at-close'), exchange: -1, ms: 0, boot_ms: 100000 };
  const cleanup = { ...find(s, 'wsa-cleanup', 120), exchange: -1, ms: 0, boot_ms: 100000 };
  const summary = Object.fromEntries(Object.keys(s.records.at(-1)).map(k => [k, 0]));
  Object.assign(summary, { type: 'summary', complete: false, failures: 1, first_error: error, operation_records: 3 });
  return { records: [header, socket, failure, pending, cleanup, summary], fixture: [] };
}
test('complete failed terminal evidence preserves actual10055 and cleanup', () => {
  const s = failedSocket(); const r = validateOutput(jsonl(s.records), '');
  assert.equal(r.passed, false); assert.equal(r.error10055, 1); assert.equal(r.outcome, 'observed-native-10055'); assert.equal(r.failureKind, 'native-operation'); assert.equal(r.firstFailure.stage, 'socket'); assert.equal(r.firstNativeOperationFailure.error, 10055);
});
test('non10055 API failure distinguished', () => {
  const s = failedSocket(10013); assert.equal(validateOutput(jsonl(s.records), '').outcome, 'observed-native-failure');
});
test('failed trace cannot hide10055', () => {
  const s = failedSocket(); s.records.splice(2, 1); Object.assign(s.records.at(-1), { failures: 0, first_error: 0 }); assert.throws(() => validateOutput(jsonl(s.records), ''));
});
test('synthetic deadline is not claimed as a Winsock return', () => {
  const s = failedSocket(); s.records.splice(1, 3);
  s.records.splice(1, 0, { type: 'failure', id: 0, stage: 'total-deadline', error: 10060, ms: 0 });
  Object.assign(s.records.at(-1), { operation_records: 1, first_error: 10060 });
  const r = validateOutput(jsonl(s.records), ''); assert.equal(r.outcome, 'harness-or-deadline-failure'); assert.equal(r.firstNativeOperationFailure, null);
});
test('unreported retained resources rejected by ownership counters', () => {
  const s = failedSocket(); s.records.at(-1).sockets_opened = 1; assert.throws(() => validateOutput(jsonl(s.records), ''));
});
test('would-block and read notifications do not count as bytes', () => {
  const s = syntheticEvidence(); const recv = find(s, 'http-recv'); const pos = s.records.indexOf(recv);
  const row = stage => ({ ...recv, stage, result: 0, error: 0 });
  s.records.splice(pos, 0, { ...row('http-recv'), result: -1, error: 10035 }, row('read-wait'), row('read-enumerate'), row('read-events'), row('read-wait'), row('read-enumerate'), { ...row('read-events'), result: 1 }, row('read-event-error'));
  recount(s.records); assert.equal(run(s).passed, true);
});
test('async write completion is accounted separately from receives', () => {
  const s = syntheticEvidence(); const submit = find(s, 'write-submit'); submit.result = -1; submit.error = 997;
  const completion = find(s, 'write-complete-sync'); completion.stage = 'write-complete-async';
  s.records.splice(s.records.indexOf(completion), 0, { ...completion, stage: 'write-wait', result: 0 });
  s.records.at(-1).sync_writes--; s.records.at(-1).async_writes++; recount(s.records);
  const r = run(s); assert.equal(r.passed, true); assert.equal(r.summary.async_writes, 1); assert.equal(r.receiveModel, 'nonblocking-recv-no-pending-kernel-receive-proof');
});
test('successful partial sends preserve unsent suffix without retry', () => {
  const s = syntheticEvidence(); const first = find(s, 'write-complete-sync'), bytes = first.result; first.result = 10;
  const mk = (stage, result) => ({ ...first, stage, result });
  s.records.splice(s.records.indexOf(first) + 1, 0, mk('write-reset', 0), mk('write-submit-bytes', bytes - 10), mk('write-submit', 0), mk('write-complete-sync', bytes - 10));
  s.records.at(-1).sync_writes++; recount(s.records); assert.equal(run(s).passed, true);
});
test('later partial send cannot reset exposure anchor', () => {
  const s = syntheticEvidence(); const first = find(s, 'write-complete-sync'), bytes = first.result; first.result = 10;
  const mk = (stage, result) => ({ ...first, stage, result });
  const pos = s.records.indexOf(first) + 1;
  s.records.splice(pos, 0, mk('write-reset', 0), mk('write-submit-bytes', bytes - 10), mk('write-submit', 0), mk('write-complete-sync', bytes - 10));
  const end = s.records.indexOf(find(s, 'http-complete'));
  for (const row of s.records.slice(pos, end)) if ('ms' in row) { row.ms = Math.max(row.ms, 200); if ('boot_ms' in row) row.boot_ms = 100000 + row.ms; }
  s.records.at(-1).sync_writes++; recount(s.records); assert.equal(run(s).passed, false);
});
test('post-connect timestamp closes a pre-call scheduling gap', () => {
  const s = syntheticEvidence(); const from = s.records.indexOf(find(s, 'connect', 5)), to = s.records.indexOf(find(s, 'http-complete'));
  for (const row of s.records.slice(from, to)) if ('ms' in row) { row.ms = Math.max(row.ms, 200); if ('boot_ms' in row) row.boot_ms = 100000 + row.ms; }
  assert.equal(find(s, 'connect-submit', 5).ms, 5); assert.equal(run(s).passed, false);
});
function pendingWriteFailure() {
  const s = syntheticEvidence(); const completion = find(s, 'write-complete-sync');
  s.records.splice(s.records.indexOf(completion));
  const submit = find(s, 'write-submit'); submit.result = -1; submit.error = 997;
  const op = (stage, result = 0, error = 0, id = 0) => ({ ...completion, id, pool: Math.floor(id / 6), lane: id % 6, exchange: id === 120 ? -1 : 0, stage, result, error });
  const fail = (stage, error, id = 0) => ({ type: 'failure', id, stage, error, ms: 0 });
  s.records.push(op('write-wait'), op('write-complete-async', -1, 996), fail('write-complete', 996), op('pending-at-close', 1), fail('pending-at-close', 996), op('shutdown'), op('closesocket'), op('post-close-wait'), op('post-close-signal'), op('write-retained', 1), fail('completion-incomplete', 996), op('event-close'), op('wsa-cleanup', 0, 0, 120), fail('ownership', 10022, 120));
  s.records.push({ type: 'summary', attempts: 1, successes: 0, completed_pools: 0, failures: 4, first_error: 996, sockets_opened: 1, sockets_closed: 1, events_opened: 2, events_closed: 1, max_live: 1, operation_records: 0, complete: false, elapsed_ms: 0, sync_writes: 0, async_writes: 0, pending_at_close: 1, post_close_signals: 1, retained_writes: 1, bytes_sent: 0, bytes_received: 0 });
  recount(s.records); s.fixture = []; return s;
}
test('pending overlapped write storage remains owned after close even when signaled', () => {
  const s = pendingWriteFailure(); const r = validateOutput(jsonl(s.records), '');
  assert.equal(r.passed, false); assert.equal(r.summary.retained_writes, 1); assert.equal(r.firstFailure.error, 996); assert.equal(r.firstNativeOperationFailure.stage, 'write-complete-async');
});
test('post-close signal cannot free pending write event', () => {
  const s = pendingWriteFailure(); const eventClose = find(s, 'event-close'); s.records.splice(s.records.indexOf(eventClose) + 1, 0, { ...eventClose, stage: 'write-event-close' }); s.records.at(-1).events_closed++; recount(s.records); assert.throws(() => validateOutput(jsonl(s.records), ''));
});
test('event release before socket close rejected even on failed run', () => {
  const s = pendingWriteFailure(); const event = find(s, 'event-close'); s.records.splice(s.records.indexOf(event), 1); s.records.splice(s.records.indexOf(find(s, 'closesocket')), 0, event); assert.throws(() => validateOutput(jsonl(s.records), ''));
});
test('pending write reset rejected', () => {
  const s = pendingWriteFailure(); const wait = find(s, 'write-wait'); s.records.splice(s.records.indexOf(wait), 0, { ...wait, stage: 'write-reset' }); recount(s.records); assert.throws(() => validateOutput(jsonl(s.records), ''));
});
test('cleanup cannot overwrite first operation failure', () => {
  const s = pendingWriteFailure(); s.records.at(-1).first_error = 10022; assert.throws(() => validateOutput(jsonl(s.records), ''));
});
test('extra body event after drop rejected', () => {
  const s = syntheticEvidence(); s.fixture[2].event = 'body_dropped'; s.fixture[2].emitted_bytes = 1024; assert.throws(() => validateFixture(jsonl(s.fixture)));
});
test('impossible prefix miss immediately after socket creation is rejected', () => {
  const s = failedSocket(); s.records[1].result = 0; s.records[1].error = 0;
  s.records[2].stage = 'prefix-not-observed'; s.records[2].error = 0;
  const pending = s.records[3]; s.records.splice(4, 0, { ...pending, stage: 'shutdown' }, { ...pending, stage: 'closesocket' });
  Object.assign(s.records.at(-1), { first_error: 0, sockets_opened: 1, sockets_closed: 1, max_live: 1 }); recount(s.records);
  assert.throws(() => validateOutput(jsonl(s.records), ''), /Exposure failure lacks/);
});
test('missing incomplete-prefix evidence is inconclusive despite full counters', () => {
  const s = syntheticEvidence(); s.records.splice(s.records.indexOf(find(s, 'prefix-observed')), 1); recount(s.records);
  const r = run(s); assert.equal(r.workloadComplete, true); assert.equal(r.exposureVerified, false); assert.equal(r.passed, false);
});
reject('pool closes early while sibling final response is outstanding', s => {
  const closeStages = ['pending-at-close', 'shutdown', 'closesocket'];
  const rows = s.records.filter(r => r.id === 0 && closeStages.includes(r.stage));
  s.records = s.records.filter(r => !rows.includes(r));
  const target = s.records.findIndex(r => r.id === 5 && r.exchange === 5 && r.stage === 'http-recv');
  s.records.splice(target, 0, ...rows);
});
test('serialized schedule rejects impossible1050-byte prefix marker', () => {
  const s = syntheticEvidence('serialized'); const prefix = find(s, 'prefix-observed'); const pos = s.records.indexOf(prefix);
  prefix.result = 1050; s.records[pos - 1].result = 1050; s.records[pos - 2].result += 26; s.records[pos + 1].result -= 26;
  recount(s.records); assert.throws(() => run(s));
});
test('synthetic10055 failure code alone is not an observed transport10055', () => {
  const s = failedSocket(); s.records.splice(1, 3);
  s.records.splice(1, 0, { type: 'failure', id: 0, stage: 'total-deadline', error: 10055, ms: 0 });
  Object.assign(s.records.at(-1), { operation_records: 1 });
  const r = validateOutput(jsonl(s.records), ''); assert.equal(r.error10055, 0); assert.equal(r.outcome, 'harness-or-deadline-failure'); assert.equal(r.firstNativeOperationFailure, null);
});
for (const [label, fixtureText, state] of [['corrupt', '{bad json}\n', 'invalid'], ['missing', '', 'missing'], ['unreadable', null, 'unavailable'], ['truncated', '{"schema_version":1}', 'invalid']]) {
  test(`native10055 survives ${label} fixture evidence`, () => {
    const s = failedSocket(); const r = validateOutput(jsonl(s.records), fixtureText);
    assert.equal(r.outcome, 'observed-native-10055'); assert.equal(r.firstNativeOperationFailure.error, 10055); assert.equal(r.firstFailure.error, 10055);
    assert.equal(r.fixtureEvidence.state, state); assert.ok(r.fixtureEvidence.problem); assert.equal(r.passed, false); assert.equal(r.exposureVerified, false);
  });
  test(`malformed native still rejects with ${label} fixture evidence`, () => {
    const s = failedSocket(); s.records.at(-1).sockets_opened = 1;
    assert.throws(() => validateOutput(jsonl(s.records), fixtureText));
  });
}
test('complete native workload remains unqualified with corrupt fixture', () => {
  const s = syntheticEvidence(); const r = validateOutput(jsonl(s.records), '{bad json}\n');
  assert.equal(r.workloadComplete, true); assert.equal(r.passed, false); assert.equal(r.exposureVerified, false); assert.equal(r.fixtureEvidence.state, 'invalid'); assert.equal(r.outcome, 'inconclusive');
});
test('failure marker must match the recorded native error', () => {
  const s = failedSocket(); s.records[2].error = 10013; s.records.at(-1).first_error = 10013;
  assert.throws(() => validateOutput(jsonl(s.records), ''), /matching native operation/);
});
test('premature-final marker without stream history is rejected', () => {
  const s = failedSocket(); s.records[2].stage = 'premature-final-consumption'; s.records[2].error = 0; s.records.at(-1).first_error = 0;
  assert.throws(() => validateOutput(jsonl(s.records), ''), /Exposure failure lacks/);
});

for (const missing of [false, true]) test(`CLI preserves native failure with ${missing ? 'missing' : 'corrupt'} fixture file`, () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'receive-connect-evidence-'));
  try {
    const native = path.join(directory, 'native.jsonl'), fixture = path.join(directory, 'fixture.jsonl');
    fs.writeFileSync(native, jsonl(failedSocket().records));
    if (!missing) fs.writeFileSync(fixture, '{bad json}\n');
    const cli = fileURLToPath(new URL('./validate-output.mjs', import.meta.url));
    const result = spawnSync(process.execPath, [cli, native, fixture], { encoding: 'utf8', timeout: 10000 });
    assert.equal(result.status, 1); const report = JSON.parse(result.stdout);
    assert.equal(report.outcome, 'observed-native-10055'); assert.equal(report.fixtureEvidence.state, missing ? 'unavailable' : 'invalid'); assert.equal(report.passed, false);
    fs.writeFileSync(native, '{bad native}\n');
    const malformed = spawnSync(process.execPath, [cli, native, fixture], { encoding: 'utf8', timeout: 10000 });
    assert.equal(malformed.status, 2); assert.equal(malformed.stdout, '');
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});
function validPrefixMiss() {
  const s = syntheticEvidence(); const terminal = s.records.indexOf(find(s, 'http-complete'));
  const records = s.records.slice(0, terminal + 1).filter(row => row.type === 'header' || (row.id === 0 && !['receive-service', 'siblings-submitted', 'prefix-observed'].includes(row.stage)));
  records.push({ type: 'failure', id: 0, stage: 'prefix-not-observed', error: 0, ms: 250 });
  for (const stage of ['pending-at-close', 'shutdown', 'closesocket', 'event-close', 'write-event-close']) records.push({ ...find(s, stage), exchange: 0, ms: 250, boot_ms: 100250 });
  records.push({ ...find(s, 'wsa-cleanup', 120), exchange: -1, ms: 250, boot_ms: 100250 });
  records.push({ ...s.records.at(-1), attempts: 1, successes: 1, completed_pools: 0, failures: 1, first_error: 0, sockets_opened: 1, sockets_closed: 1, events_opened: 2, events_closed: 2, max_live: 1, complete: false, elapsed_ms: 250, sync_writes: 1 });
  recount(records); return records;
}
test('actual complete streaming response with unobserved prefix remains inconclusive', () => {
  const result = validateOutput(jsonl(validPrefixMiss()), '');
  assert.equal(result.outcome, 'inconclusive'); assert.equal(result.firstFailure.stage, 'prefix-not-observed'); assert.equal(result.firstNativeOperationFailure, null); assert.equal(result.passed, false);
});
test('prefix miss cannot contradict an observed prefix', () => {
  const records = validPrefixMiss(); const bodyIndex = records.findIndex(row => row.stage === 'body-consumed' && row.result === 1024);
  records.splice(bodyIndex + 1, 0, { ...records[bodyIndex], stage: 'prefix-observed' }); recount(records);
  assert.throws(() => validateOutput(jsonl(records), ''), /Exposure failure prefix state/);
});
for (const mode of ['sync', 'async']) for (const delay of [999, 1000]) test(`${mode} connect terminal success at${delay}ms respects pre-call strict deadline`, () => {
  const s = syntheticEvidence();
  if (mode === 'sync') {
    const connect = find(s, 'connect'); connect.result = 0; connect.error = 0;
    s.records = s.records.filter(row => row.id !== 0 || !['connect-wait', 'connect-enumerate', 'connect-async'].includes(row.stage));
  }
  const index = s.records.indexOf(find(s, mode === 'sync' ? 'connect' : 'connect-async'));
  for (const row of s.records.slice(index)) { if ('ms' in row) row.ms += delay; if ('boot_ms' in row) row.boot_ms += delay; }
  s.records.at(-1).elapsed_ms += delay; recount(s.records);
  if (delay === 999) assert.equal(run(s).passed, true);
  else assert.throws(() => run(s), /[Cc]onnect deadline/);
});
