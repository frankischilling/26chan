import fs from 'node:fs';
import { pathToFileURL } from 'node:url';
export const MAX_BYTES = 8 * 1024 * 1024;
export const MAX_OPERATIONS = 30000;
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const natural = (n, max = Number.MAX_SAFE_INTEGER) => Number.isSafeInteger(n) && n >= 0 && n <= max;
function exact(row, keys) {
  assert(row && typeof row === 'object' && !Array.isArray(row), 'Expected record object');
  assert(Object.keys(row).sort().join() === [...keys].sort().join(), 'Unexpected fields');
}
function parse(text, cap = MAX_BYTES, linesCap = 40000) {
  assert(typeof text === 'string' && Buffer.byteLength(text) <= cap && text.endsWith('\n'), 'Incomplete/oversized output');
  const lines = text.slice(0, -1).split('\n');
  assert(lines.length <= linesCap, 'Record cap');
  return lines.map(line => {
    assert(line.length && line.length <= 4096, 'Invalid line length');
    const keys = [...line.matchAll(/"(?:[^"\\]|\\.)*"\s*:/g)].map(m => JSON.parse(m[0].replace(/\s*:$/, '')));
    assert(new Set(keys).size === keys.length, 'Duplicate JSON field');
    return JSON.parse(line);
  });
}
const fixtureKeys = ['schema_version', 'scope', 'sequence', 'pool', 'request_id', 'lane', 'event', 'monotonic_ns', 'unix_ms', 'emitted_bytes', 'output_failed', 'emission_boundary'];
// These are body-frame polls, not TCP sends or client delivery. Independent
// process timing is never compared directly with native elapsed timestamps.
export function validateFixture(text) {
  if (text === '') return { pools: new Map(), complete: false };
  const records = parse(text, 512 * 1024, 128), pools = new Map();
  let sequence = 0, previous = 0;
  for (const row of records) {
    exact(row, fixtureKeys);
    assert(row.schema_version === 1 && row.scope === 'fixture-transport-overlap' && row.emission_boundary === 'http_body_poll', 'Fixture identity');
    assert(row.sequence === ++sequence && natural(row.pool, 19) && row.request_id === row.pool + 1 && row.lane === 0, 'Fixture sequence/identity');
    assert(natural(row.monotonic_ns) && row.monotonic_ns >= previous && natural(row.unix_ms) && typeof row.output_failed === 'boolean', 'Fixture timing');
    previous = row.monotonic_ns;
    const events = pools.get(row.pool) ?? [];
    assert(events.at(-1)?.event !== 'body_dropped', 'Event after dropped body');
    if (events.length === 0) assert(row.pool === pools.size, 'Fixture pool acceptance order');
    const expected = ['accepted', 'prefix_emitted', 'suffix_emitted', 'body_complete'][events.length];
    assert(row.event === expected || (row.event === 'body_dropped' && events.length > 0 && events.length < 4), 'Fixture event order');
    assert(row.emitted_bytes === (row.event === 'accepted' ? 0 : row.event === 'prefix_emitted' ? 1024 : row.event === 'body_dropped' ? events.at(-1).emitted_bytes : 4096), 'Fixture byte count');
    events.push(row); pools.set(row.pool, events);
    if (row.event === 'suffix_emitted') assert(row.monotonic_ns - events[1].monotonic_ns >= 250000000, 'Fixture suffix delay');
  }
  return { pools, complete: pools.size === 20 && [...pools.values()].every(events => events.length === 4 && events[3].event === 'body_complete' && events.every(row => !row.output_failed)) };
}
const setup = ['socket', 'nonblocking', 'nodelay', 'keepalive', 'event-create', 'write-event-create', 'event-select-connect', 'set-randomize', 'get-randomize', 'binding-before', 'connect-submit', 'connect', 'binding-after'];
const cleanupStages = new Set(['pending-at-close', 'shutdown', 'closesocket', 'post-close-wait', 'post-close-signal', 'write-retained', 'event-close', 'write-event-close']);
const stages = new Set([...setup, ...cleanupStages, 'connect-wait', 'connect-enumerate', 'connect-async', 'event-select-read-close', 'exchange-begin', 'write-reset', 'write-submit-bytes', 'write-submit', 'write-wait', 'write-complete-sync', 'write-complete-async', 'read-wait', 'read-enumerate', 'read-events', 'read-event-error', 'close-event-error', 'http-recv', 'body-consumed', 'prefix-observed', 'http-complete', 'receive-service', 'siblings-submitted', 'pool-retired', 'next-pool-start', 'wsa-cleanup']);
const failureStages = new Set([...stages, 'verify-randomize', 'connect-sync', 'connect-deadline', 'connect-event-missing', 'exchange-ownership', 'request-cap', 'write-complete', 'write-length', 'read-event-unexpected', 'unexpected-peer-close', 'response-cap', 'fixture-response', 'http-deadline', 'prefix-not-observed', 'premature-final-consumption', 'completion-incomplete', 'total-deadline', 'output-cap', 'ownership']);
const nativeFailureStages = new Set(['socket', 'nonblocking', 'nodelay', 'keepalive', 'event-create', 'write-event-create', 'event-select-connect', 'set-randomize', 'get-randomize', 'connect', 'connect-wait', 'connect-enumerate', 'connect-async', 'event-select-read-close', 'write-reset', 'write-submit', 'write-wait', 'write-complete-sync', 'write-complete-async', 'read-wait', 'read-enumerate', 'read-event-error', 'close-event-error', 'http-recv', 'shutdown', 'closesocket', 'event-close', 'write-event-close', 'wsa-cleanup']);
const summaryKeys = ['type', 'attempts', 'successes', 'completed_pools', 'failures', 'first_error', 'sockets_opened', 'sockets_closed', 'events_opened', 'events_closed', 'max_live', 'operation_records', 'complete', 'elapsed_ms', 'sync_writes', 'async_writes', 'pending_at_close', 'post_close_signals', 'retained_writes', 'bytes_sent', 'bytes_received'];
export const requestBytes = (id, exchange) => Buffer.byteLength(`GET ${id < 120 && id % 6 === 0 && exchange === 0 ? `/__transport_overlap?pool=${Math.floor(id / 6)}` : '/readyz'} HTTP/1.1\r\nHost: 127.0.0.1:3000\r\nConnection: keep-alive\r\n\r\n`);
const bodyLength = (id, exchange) => id < 120 && id % 6 === 0 && exchange === 0 ? 4096 : 26;
function qualifySocket(rows, id, header) {
  let i = 0;
  const peek = () => rows[i];
  const take = stage => { const row = rows[i++]; assert(row?.stage === stage, `Expected ${stage} for socket ${id}`); return row; };
  const zero = stage => { const row = take(stage); assert(row.result === 0 && row.error === 0, `Unsuccessful ${stage}`); return row; };
  for (const stage of setup) {
    const row = take(stage);
    assert(row.exchange === -1, 'Setup exchange identity');
    if (stage === 'binding-before') assert((row.result === 0 && row.error === 0) || (row.result === -1 && row.error === 10022), 'Initially bound socket');
    else if (stage === 'binding-after') assert(row.result === 1 && row.error === 0, 'No implicit bind');
    else if (stage === 'connect') assert((row.result === 0 && row.error === 0) || (row.result === -1 && row.error === 10035), 'Connect failure');
    else assert(row.result === 0 && row.error === 0, `Setup failure ${stage}`);
  }
  const connect = rows.find(r => r.stage === 'connect'), submitted = rows.find(r => r.stage === 'connect-submit');
  assert(connect.ms - submitted.ms < 1000, 'Synchronous connect deadline');
  if (connect.result === -1) for (const stage of ['connect-wait', 'connect-enumerate', 'connect-async']) {
    const row = zero(stage); assert(row.ms - submitted.ms < 1000 && row.exchange === -1, 'Connect deadline/identity');
  }
  for (let exchange = 0; exchange < (id === 120 ? 1 : 6); ++exchange) {
    const begin = take('exchange-begin');
    assert(begin.result === exchange && begin.error === 0 && begin.exchange === exchange, 'Exchange ordinal');
    if (exchange === 0) zero('event-select-read-close');
    let sent = 0, received = 0, consumed = 0, prefixSeen = false;
    while (sent < requestBytes(id, exchange)) {
      zero('write-reset');
      const bytes = take('write-submit-bytes');
      assert(bytes.result === requestBytes(id, exchange) - sent && bytes.error === 0, 'Unsent request suffix');
      const submit = take('write-submit'), async = submit.result === -1 && submit.error === 997;
      assert(async || (submit.result === 0 && submit.error === 0), 'Write submission');
      if (async) zero('write-wait');
      const completion = take(async ? 'write-complete-async' : 'write-complete-sync');
      assert(completion.result > 0 && completion.result <= bytes.result && completion.error === 0, 'Write completion length');
      sent += completion.result;
    }
    while (peek()?.stage !== 'http-complete') {
      if (peek()?.stage === 'read-wait') {
        zero('read-wait'); zero('read-enumerate');
        const event = take('read-events');
        assert([0, 1].includes(event.result) && event.error === 0, 'Unexpected read event/peer close');
        if (event.result === 1) zero('read-event-error');
      }
      if (peek()?.stage === 'read-wait') continue;
      const recv = take('http-recv');
      if (recv.result === -1 && recv.error === 10035) continue;
      assert(recv.result > 0 && recv.result <= Math.min(1024, 8192 - received) && recv.error === 0, 'Receive count');
      received += recv.result;
      const body = take('body-consumed');
      assert(body.error === 0 && body.result >= consumed && body.result <= bodyLength(id, exchange) && body.result <= received && body.result - consumed <= recv.result, 'Body accounting');
      if (consumed > 0) assert(body.result - consumed === recv.result, 'Body/header boundary changed');
      consumed = body.result;
      if (peek()?.stage === 'prefix-observed') {
        const prefix = take('prefix-observed');
        assert(!prefixSeen && bodyLength(id, exchange) === 4096 && prefix.result === consumed && consumed === 1024 && prefix.error === 0, 'Invalid incomplete prefix');
        prefixSeen = true;
      }
    }
    const complete = take('http-complete');
    assert(complete.result === exchange && complete.error === 0 && consumed === bodyLength(id, exchange) && received > consumed && received <= 8192, 'Incomplete HTTP response');
    assert(complete.ms - begin.ms < 1000, 'HTTP deadline');
    for (const row of rows.filter(r => r.ms >= begin.ms && r.ms <= complete.ms && r.exchange === exchange)) assert(row.ms <= 30000, 'Work deadline');
  }
  for (const stage of ['pending-at-close', 'shutdown', 'closesocket', 'event-close', 'write-event-close']) zero(stage);
  assert(i === rows.length, 'Extra socket operations');
}
export function validateOutput(text, fixtureText = '') {
  const records = parse(text), header = records.shift(), summary = records.pop();
  exact(header, ['type', 'schema', 'profile', 'schedule', 'randomize', 'pools', 'width', 'exchanges', 'interval_ms', 'total_cap_ms', 'cleanup_cap_ms', 'response_cap', 'operation_cap', 'start_boot_ms']);
  assert(header.type === 'header' && header.schema === 1 && header.profile === 'receive-connect-history' && ['serialized', 'overlap'].includes(header.schedule) && typeof header.randomize === 'boolean' && header.pools === 20 && header.width === 6 && header.exchanges === 6 && header.interval_ms === 100 && header.total_cap_ms === 30000 && header.cleanup_cap_ms === 35000 && header.response_cap === 8192 && header.operation_cap === 30000 && natural(header.start_boot_ms), 'Header invalid');
  exact(summary, summaryKeys);
  assert(summary.type === 'summary' && typeof summary.complete === 'boolean', 'Summary invalid');
  for (const key of summaryKeys.filter(k => !['type', 'complete'].includes(k))) assert(natural(summary[key]), 'Summary number invalid');
  assert(summary.elapsed_ms <= 35000, 'Cleanup deadline');
  const counted = Object.fromEntries(summaryKeys.filter(k => !['type', 'complete', 'elapsed_ms', 'first_error'].includes(k)).map(k => [k, 0]));
  const owners = new Map(), operations = [], failures = [], retired = [], starts = [];
  let previousMs = 0, previousBoot = 0, live = 0, events = 0, cleanup = false, previous = null;
  for (const row of records) {
    exact(row, row.type === 'operation' ? ['type', 'id', 'pool', 'lane', 'exchange', 'stage', 'result', 'error', 'ms', 'boot_ms'] : row.type === 'option' ? ['type', 'id', 'value', 'length', 'ms'] : ['type', 'id', 'stage', 'error', 'ms']);
    assert(natural(row.id, 120) && natural(row.ms, 35000) && row.ms >= previousMs, 'Identity/time invalid'); previousMs = row.ms;
    if (row.type === 'failure') {
      assert(failureStages.has(row.stage) && natural(row.error, 65535) && (row.error > 0 || ['prefix-not-observed', 'premature-final-consumption'].includes(row.stage)), 'Failure invalid');
      assert(owners.has(row.id) || ['total-deadline', 'output-cap', 'ownership', 'wsa-cleanup'].includes(row.stage), 'Unowned failure');
      const history = operations.filter(operation => operation.id === row.id);
      const matchingApiFailure = stage => history.some(operation => operation.stage === stage && ((operation.result === -1 && operation.error === row.error) || (operation.error === 0 && ((stage === 'http-recv' && operation.result === 0) || (stage.endsWith('-wait') && operation.result !== 0)))));
      if (nativeFailureStages.has(row.stage)) assert(matchingApiFailure(row.stage), 'Failure lacks matching native operation');
      if (row.stage === 'connect-sync') assert(matchingApiFailure('connect'), 'Connect failure lacks operation');
      if (row.stage === 'write-complete') assert(matchingApiFailure('write-complete-sync') || matchingApiFailure('write-complete-async'), 'Write failure lacks completion');
      if (['prefix-not-observed', 'premature-final-consumption'].includes(row.stage)) {
        assert(row.id < 120 && row.id % 6 === 0 && history.some(operation => operation.stage === 'connect-submit') && history.some(operation => ['connect', 'connect-async'].includes(operation.stage) && operation.result === 0 && operation.error === 0) && history.filter(operation => ['write-complete-sync', 'write-complete-async'].includes(operation.stage) && operation.exchange === 0 && operation.error === 0).reduce((total, operation) => total + operation.result, 0) === requestBytes(row.id, 0) && history.some(operation => operation.stage === 'exchange-begin' && operation.exchange === 0) && history.some(operation => operation.stage === 'http-recv' && operation.exchange === 0 && operation.result > 0) && history.some(operation => operation.stage === 'body-consumed' && operation.exchange === 0 && operation.result === 4096) && history.some(operation => operation.stage === 'http-complete' && operation.exchange === 0), 'Exposure failure lacks completed streaming response');
        const prefix = history.some(operation => operation.stage === 'prefix-observed' && operation.exchange === 0);
        assert(prefix === (row.stage === 'premature-final-consumption'), 'Exposure failure prefix state');
        if (prefix) {
          const siblings = operations.filter(operation => operation.pool === Math.floor(row.id / 6) && operation.lane > 0 && operation.stage === 'connect-submit');
          assert(siblings.length > 0 && siblings.length < 5, 'Premature final lacks partial sibling submissions');
        }
      }
      if (cleanup) assert(['ownership', 'wsa-cleanup', 'total-deadline'].includes(row.stage), 'Failure after global cleanup');
      failures.push(row); counted.failures++; previous = row; continue;
    }
    if (row.type === 'option') {
      const owner = owners.get(row.id);
      assert(owner && !owner.option && previous?.stage === 'get-randomize' && previous.id === row.id && !owner.closeAttempted, 'Option order');
      assert(previous.result === 0 ? typeof row.value === 'boolean' && Number.isInteger(row.length) && row.length >= -1 && row.length <= 65535 : row.value === null && row.length === null, 'Option shape');
      owner.option = row; previous = row; continue;
    }
    assert(row.type === 'operation' && stages.has(row.stage) && Number.isSafeInteger(row.result) && row.result >= -2147483648 && row.result <= 2147483647 && natural(row.error, 65535), 'Operation invalid');
    assert(row.pool === Math.floor(row.id / 6) && row.lane === row.id % 6 && Number.isInteger(row.exchange) && row.exchange >= -1 && row.exchange <= 5 && natural(row.boot_ms) && row.boot_ms >= previousBoot, 'Operation identity/clock'); previousBoot = row.boot_ms;
    assert((row.result >= 0 && row.error === 0) || (row.result === -1 && row.error > 0), 'Result/error disagreement');
    assert(++counted.operation_records <= MAX_OPERATIONS, 'Operation cap'); operations.push(row);
    const success = row.result >= 0 && row.error === 0;
    if (row.stage === 'wsa-cleanup') { assert(!cleanup && row.id === 120, 'Global cleanup identity'); cleanup = true; previous = row; continue; }
    assert(!cleanup, 'Operation after global cleanup');
    if (failures.length) assert(cleanupStages.has(row.stage), 'New work after first failure');
    if (row.stage === 'next-pool-start') {
      assert(row.id > 0 && row.id % 6 === 0 && !owners.has(row.id) && live === 0 && events === 0 && row.exchange === -1, 'Next pool ownership');
      starts.push(row); previous = row; continue;
    }
    if (row.stage === 'pool-retired') {
      assert(row.id % 6 === 0 && row.result === row.pool && live === 0 && events === 0 && !retired.some(r => r.pool === row.pool), 'Pool retirement');
      retired.push(row); if (row.pool < 20) counted.completed_pools++; previous = row; continue;
    }
    if (row.stage === 'socket') {
      assert(row.id === owners.size, 'Socket creation order');
      if (row.lane === 0) assert(live === 0 && events === 0, 'Prior pool still owned');
      owners.set(row.id, { socket: success, closed: false, closeAttempted: false, event: false, writeEvent: false, pending: false, retained: false, once: new Set(), exchange: -1, last: null });
      if (success) { ++live; ++counted.sockets_opened; counted.max_live = Math.max(counted.max_live, live); }
    }
    const owner = owners.get(row.id); assert(owner, 'Operation before ownership');
    if (!cleanupStages.has(row.stage)) assert(!owner.closeAttempted, 'I/O after close');
    if (setup.includes(row.stage) || cleanupStages.has(row.stage) || ['connect-wait', 'connect-enumerate', 'connect-async', 'event-select-read-close', 'siblings-submitted'].includes(row.stage)) {
      assert(!owner.once.has(row.stage), 'Repeated lifecycle stage'); owner.once.add(row.stage);
    }
    if (row.stage === 'exchange-begin') { assert(row.result === owner.exchange + 1 && row.exchange === row.result, 'Exchange identity'); owner.exchange = row.exchange; }
    assert(row.exchange === owner.exchange, 'Mislabeled exchange');
    if (row.stage === 'event-create' || row.stage === 'write-event-create') {
      assert(owner.socket && !owner.closed, 'Event owner missing');
      if (success) { owner[row.stage === 'event-create' ? 'event' : 'writeEvent'] = true; ++events; ++counted.events_opened; }
    }
    if (row.stage.startsWith('write-') && !['write-event-create', 'write-retained'].includes(row.stage)) assert(owner.writeEvent, 'Write event owner missing');
    if (row.stage === 'connect-submit') ++counted.attempts;
    if (row.stage === 'write-reset') assert(!owner.pending, 'Pending write storage reused');
    if (row.stage === 'write-submit') { assert(!owner.pending, 'Concurrent write storage'); owner.pending = row.result === -1 && row.error === 997; }
    if (['write-complete-sync', 'write-complete-async'].includes(row.stage)) {
      if (success) { ++counted[row.stage === 'write-complete-sync' ? 'sync_writes' : 'async_writes']; counted.bytes_sent += row.result; }
      owner.pending = !success && row.error === 996;
    }
    if (row.stage === 'http-recv' && success) counted.bytes_received += row.result;
    if (row.stage === 'body-consumed') assert(previous?.type === 'operation' && previous.id === row.id && previous.exchange === row.exchange && previous.stage === 'http-recv' && previous.result > 0 && previous.error === 0, 'Body observation detached from receive');
    if (row.stage === 'prefix-observed' || row.stage === 'http-complete') assert(previous?.type === 'operation' && previous.id === row.id && previous.exchange === row.exchange && previous.stage === 'body-consumed', 'Completion/prefix detached from consumed bytes');
    if (row.stage === 'http-complete') ++counted.successes;
    if (row.stage === 'pending-at-close') { assert(row.result === Number(owner.pending), 'Pending ownership mismatch'); owner.closeAttempted = true; counted.pending_at_close += row.result; }
    if (row.stage === 'shutdown') assert(owner.socket && !owner.closed && owner.closeAttempted, 'Shutdown ownership');
    if (row.stage === 'closesocket') {
      assert(owner.socket && !owner.closed && owner.once.has('shutdown'), 'Close ownership');
      if (success) { owner.closed = true; --live; ++counted.sockets_closed; }
    }
    if (['post-close-wait', 'post-close-signal', 'write-retained'].includes(row.stage)) {
      assert(owner.pending && owner.closed && live === 0 && owner.writeEvent, 'Pending post-close ownership');
      if (row.stage === 'post-close-signal') { assert(previous?.stage === 'post-close-wait' && previous.id === row.id && previous.result === 0, 'Unobserved post-close signal'); ++counted.post_close_signals; }
      if (row.stage === 'write-retained') { assert(row.result === 1, 'Retained count'); owner.retained = true; ++counted.retained_writes; }
    }
    if (row.stage === 'event-close' || row.stage === 'write-event-close') {
      const key = row.stage === 'event-close' ? 'event' : 'writeEvent';
      assert(owner.closeAttempted && (!owner.socket || owner.closed) && live === 0 && owner[key], 'Event released before socket group close');
      assert(key !== 'writeEvent' || (!owner.pending && !owner.retained), 'Pending write released');
      if (success) { owner[key] = false; --events; ++counted.events_closed; }
    }
    assert(live >= 0 && live <= 6 && events >= 0 && events <= 12, 'Resource cap'); owner.last = row; previous = row;
  }
  assert(cleanup && summary.elapsed_ms >= previousMs, 'Missing terminal cleanup/time');
  for (const [key, value] of Object.entries(counted)) assert(summary[key] === value, `Counter mismatch: ${key}`);
  assert(summary.first_error === (failures[0]?.error ?? 0), 'First failure overwritten');
  const error10055 = operations.filter(row => nativeFailureStages.has(row.stage) && row.error === 10055).length;
  if (error10055) assert(failures.some(row => row.error === 10055), 'Unreported 10055');
  const workloadComplete = summary.complete && !failures.length && summary.attempts === 121 && summary.successes === 721 && summary.completed_pools === 20 && summary.sockets_opened === 121 && summary.sockets_closed === 121 && summary.events_opened === 242 && summary.events_closed === 242 && summary.max_live === 6 && summary.pending_at_close === 0 && summary.retained_writes === 0 && summary.elapsed_ms <= 30000;
  assert(!summary.complete || workloadComplete, 'False workload completeness');
  // Fixture evidence gates exposure, never whether an independently validated
  // native failure happened. Native grammar/counter errors still throw below.
  let fixture = { complete: false }, fixtureEvidence;
  if (fixtureText === null) fixtureEvidence = { state: 'unavailable', problem: 'fixture-file-unreadable-or-rejected' };
  else if (fixtureText === '') fixtureEvidence = { state: 'missing', problem: 'fixture-records-missing' };
  else {
    try {
      fixture = validateFixture(fixtureText);
      fixtureEvidence = { state: fixture.complete ? 'complete' : 'incomplete', problem: fixture.complete ? null : 'fixture-exposure-incomplete' };
    } catch {
      fixtureEvidence = { state: 'invalid', problem: 'fixture-records-malformed-or-inconsistent' };
    }
  }
  let exposureVerified = workloadComplete && fixture.complete;
  if (workloadComplete) {
    const extra = new Set(['receive-service', 'siblings-submitted', 'pool-retired', 'next-pool-start', 'wsa-cleanup']);
    for (let id = 0; id <= 120; ++id) {
      const owner = owners.get(id);
      assert(owner?.option?.value === header.randomize && owner.option.length === 4, 'Explicit randomization readback');
      qualifySocket(operations.filter(row => row.id === id && !extra.has(row.stage)), id, header);
    }
    assert(retired.length === 21 && starts.length === 20, 'Pool boundary count');
    for (let pool = 0; pool <= 20; ++pool) {
      const group = operations.filter(row => row.pool === pool), rows = stage => group.filter(row => row.stage === stage);
      assert(retired[pool]?.pool === pool, 'Pool order');
      const close = rows('closesocket').at(-1), creation = rows('socket')[0];
      assert(close && creation, 'Pool lifetime absent');
      if (pool > 0) {
        const priorClose = operations.filter(row => row.pool === pool - 1 && row.stage === 'closesocket').at(-1);
        assert(starts[pool - 1]?.pool === pool && starts[pool - 1].result >= 100 && starts[pool - 1].ms - priorClose.ms >= 100 && creation.ms - priorClose.ms >= 100, 'Retirement gap');
      }
      assert(Math.max(...rows('http-complete').map(row => operations.indexOf(row))) < Math.min(...rows('pending-at-close').map(row => operations.indexOf(row))), 'Pool closed before all exchanges completed');
      if (pool === 20) continue;
      const begins = rows('exchange-begin'), completes = rows('http-complete');
      for (let ex = 1; ex < 6; ++ex) assert(operations.indexOf(begins.find(row => row.exchange === ex)) > Math.max(...completes.filter(row => row.exchange === ex - 1).map(row => operations.indexOf(row))), 'Round barrier');
      if (header.schedule === 'serialized') {
        const connected = group.filter(row => row.stage === 'connect-async' || (row.stage === 'connect' && row.result === 0));
        assert(connected.length === 6 && Math.max(...connected.map(row => operations.indexOf(row))) < operations.indexOf(begins[0]), 'Serialized connects precede HTTP');
        for (let i = 1; i < begins.length; ++i) assert(operations.indexOf(begins[i]) > operations.indexOf(completes[i - 1]) && begins[i].id === pool * 6 + i % 6 && begins[i].exchange === Math.floor(i / 6), 'Serialized HTTP order');
      } else {
        const prefix = rows('prefix-observed'), siblings = rows('connect').filter(row => row.lane > 0), final = completes.find(row => row.lane === 0 && row.exchange === 0), send = rows('write-submit-bytes').find(row => row.lane === 0 && row.exchange === 0);
        const service = rows('receive-service');
        assert(prefix.length <= 1 && siblings.length === 5 && final && send, 'Invalid overlap trace');
        if (prefix.length === 0) { exposureVerified = false; continue; }
        const prefixIndex = operations.indexOf(prefix[0]), finalBody = rows('body-consumed').find(row => row.lane === 0 && row.exchange === 0 && row.result === 4096), finalIndex = operations.indexOf(finalBody);
        const submits = rows('connect-submit').filter(row => row.lane > 0);
        assert(submits.length === 5, 'Sibling connect submission count');
        if (!(finalBody && prefix[0].result === 1024 && submits.every(row => operations.indexOf(row) > prefixIndex) && siblings.every(row => operations.indexOf(row) > prefixIndex && operations.indexOf(row) < finalIndex && row.ms - send.ms < 200))) exposureVerified = false;
        assert(rows('siblings-submitted').length === 1 && rows('siblings-submitted')[0].result === 5, 'Sibling submission marker');
        assert(service.length === 5 && service.every((row, i) => row.id === pool * 6 && row.exchange === 0 && row.result === i + 1 && operations.indexOf(row) > operations.indexOf(siblings[i]) && (i === 4 || operations.indexOf(row) < operations.indexOf(siblings[i + 1]))), 'Lane0 service between siblings');
      }
    }
  }
  const expectedError = row => (row.stage === 'connect' && row.error === 10035) || (row.stage === 'write-submit' && row.error === 997) || (row.stage === 'http-recv' && row.error === 10035);
  const firstNativeOperationFailure = operations.find(row => (row.stage === 'http-recv' && row.result === 0 && row.error === 0) || row.stage === 'close-event-error' || (nativeFailureStages.has(row.stage) && row.error > 0 && !expectedError(row) && !(row.stage === 'shutdown' && row.error === 10057))) ?? null;
  if (firstNativeOperationFailure?.error > 0) assert(failures.some(row => row.id === firstNativeOperationFailure.id && row.error === firstNativeOperationFailure.error && row.ms >= firstNativeOperationFailure.ms), 'Uncorroborated native operation failure');
  const passed = workloadComplete && exposureVerified;
  const failureKind = firstNativeOperationFailure ? 'native-operation' : failures.length && failures.some(row => row.error !== 0) ? 'harness-or-deadline' : !passed ? 'exposure-inconclusive' : null;
  const outcome = error10055 ? 'observed-native-10055' : firstNativeOperationFailure ? 'observed-native-failure' : failureKind === 'harness-or-deadline' ? 'harness-or-deadline-failure' : passed ? 'qualified-bounded-success' : 'inconclusive';
  return { header, summary, passed, workloadComplete, exposureVerified, fixtureEvidence, outcome, failureKind, error10055, firstFailure: failures[0] ?? null, firstNativeOperationFailure, reproduction: error10055 ? 'native-10055-observed-no-os-causation-established' : 'inconclusive', receiveModel: 'nonblocking-recv-no-pending-kernel-receive-proof', emissionBoundary: 'http_body_poll', timingProof: { anchor: 'first-pre-WSASend-write-submit-bytes', siblingBound: 'post-connect-return', nativeClock: 'steady-clock-elapsed-integer-milliseconds', fixtureClock: 'independent-Instant-elapsed-nanoseconds', strictWindowMs: 200, minimumServerHoldMs: 250, nominalMarginMs: 50, crossProcessClockComparison: false } };
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    assert(process.argv.length === 4, 'Usage: node validate-output.mjs output.jsonl fixture.jsonl');
    const read = (path, cap) => { const stat = fs.lstatSync(path); assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= cap, 'Invalid evidence file'); return fs.readFileSync(path, 'utf8'); };
    const nativeText = read(process.argv[2], MAX_BYTES);
    let fixtureText = null;
    try { fixtureText = read(process.argv[3], 512 * 1024); } catch { /* Report separately; never discard native evidence. */ }
    const result = validateOutput(nativeText, fixtureText);
    console.log(JSON.stringify(result)); process.exitCode = result.passed ? 0 : 1;
  } catch (error) { console.error(error.message); process.exitCode = 2; }
}
