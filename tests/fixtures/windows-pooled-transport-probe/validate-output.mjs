import fs from 'node:fs';
import { pathToFileURL } from 'node:url';

export const MAX_BYTES = 4 * 1024 * 1024;
export const MAX_OPERATIONS = 20000;
export const REQUEST_BYTES = 70;
const MAX_ELAPSED = 35000;
const MIN_RESPONSE_BYTES = Buffer.byteLength('HTTP/1.1 200 \r\nContent-Length: 25\r\n\r\nsynthetic fixture renderer');
const setup = ['socket', 'nonblocking', 'nodelay', 'keepalive', 'event-create', 'write-event-create', 'event-select-connect', 'set-randomize', 'get-randomize'];
const stages = new Set([...setup, 'binding-before', 'binding-after', 'connect', 'connect-wait', 'connect-enumerate', 'connect-async', 'event-select-read-close', 'exchange-begin', 'write-reset', 'write-submit', 'write-wait', 'write-complete-sync', 'write-complete-async', 'http-recv', 'read-wait', 'read-enumerate', 'read-events', 'read-event-error', 'close-event-error', 'http-complete', 'pending-at-close', 'shutdown', 'closesocket', 'post-close-wait', 'post-close-signal', 'write-retained', 'event-close', 'write-event-close', 'wsa-cleanup']);
const failureStages = new Set([...stages, 'verify-randomize', 'connect-sync', 'connect-event-missing', 'http-deadline', 'fixture-response', 'response-cap', 'total-deadline', 'ownership', 'output-cap', 'write-ownership', 'write-complete', 'write-length', 'completion-incomplete', 'unexpected-peer-close', 'read-event-missing']);
const waits = new Set(['connect-wait', 'write-wait', 'read-wait', 'post-close-wait']);
const completions = new Set(['write-complete-sync', 'write-complete-async']);
const cleanupStages = new Set(['pending-at-close', 'shutdown', 'closesocket', 'post-close-wait', 'post-close-signal', 'write-retained', 'event-close', 'write-event-close']);
const summaryKeys = ['type', 'attempts', 'successes', 'failures', 'sockets_opened', 'sockets_closed', 'events_opened', 'events_closed', 'operation_records', 'complete', 'elapsed_ms', 'sync_writes', 'async_writes', 'pending_at_close', 'post_close_signals', 'retained_writes', 'bytes_sent', 'bytes_received'];
function check(condition, message) { if (!condition) throw new Error(message); }
function natural(value, cap = 0xffffffff) { return Number.isSafeInteger(value) && value >= 0 && value <= cap; }
function exact(record, keys) {
  check(record !== null && typeof record === 'object' && !Array.isArray(record), 'Expected record object');
  check(Object.keys(record).sort().join() === [...keys].sort().join(), 'Unexpected fields');
}
function parseLine(line) {
  check(line.length > 0 && line.length <= 1024, 'Invalid line length');
  // JSON.parse otherwise silently accepts duplicate keys, including escaped aliases.
  const keys = [...line.matchAll(/"(?:[^"\\]|\\.)*"\s*:/g)].map(match => JSON.parse(match[0].replace(/\s*:$/, '')));
  check(new Set(keys).size === keys.length, 'Duplicate JSON field');
  return JSON.parse(line);
}
function validResult(row) {
  const zero = row.result === 0 && row.error === 0;
  const failure = row.result === -1 && row.error > 0;
  if (['exchange-begin', 'http-complete'].includes(row.stage)) return natural(row.result, 5) && row.error === 0;
  if (row.stage === 'pending-at-close') return natural(row.result, 1) && row.error === 0;
  if (row.stage === 'write-retained') return row.result === 1 && row.error === 0;
  if (row.stage === 'read-events') return natural(row.result, 1023) && row.error === 0;
  if (waits.has(row.stage)) return zero || (row.result === 258 && row.error === 0) || failure;
  if (['binding-before', 'binding-after'].includes(row.stage)) return (natural(row.result, 1) && row.error === 0) || failure;
  if (row.stage === 'http-recv') return (natural(row.result, 1024) && row.error === 0) || failure;
  if (completions.has(row.stage)) return (natural(row.result, 0x7fffffff) && row.error === 0) || failure;
  return zero || failure;
}

// This is an independent grammar for a qualifying trace, not a relaxation based
// on the producer's complete flag. Failed runs still have schemas and observed
// ownership/counters checked below, but can never qualify by skipping this grammar.
function qualify(records, header, summary) {
  let index = 0;
  const peek = () => records[index];
  const take = (id, stage) => {
    const row = records[index++];
    check(row?.type === 'operation' && row.id === id && row.stage === stage, `Expected ${stage} for socket ${id}`);
    return row;
  };
  const zero = (id, stage) => {
    const row = take(id, stage);
    check(row.result === 0 && row.error === 0, `Unsuccessful ${stage}`);
    return row;
  };
  for (let batch = 0; batch < header.batches; batch++) {
    const connected = new Map();
    // All six starts precede connect completion and every HTTP exchange.
    for (let lane = 0; lane < 6; lane++) {
      const id = batch * 6 + lane;
      for (const stage of setup) zero(id, stage);
      const option = records[index++];
      check(option?.type === 'option' && option.id === id && option.value === (header.mode === 'randomized') && option.length === 4, 'Variant readback does not prove requested mode');
      const before = take(id, 'binding-before');
      check((before.result === 0 && before.error === 0) || (before.result === -1 && before.error === 10022), 'Socket not initially unbound');
      const connect = take(id, 'connect');
      check((connect.result === 0 && connect.error === 0) || (connect.result === -1 && connect.error === 10035), 'Connect failure cannot qualify');
      connected.set(id, connect);
      const after = take(id, 'binding-after');
      check(after.result === 1 && after.error === 0, 'No implicit binding observed');
    }
    for (let lane = 0; lane < 6; lane++) {
      const id = batch * 6 + lane, connect = connected.get(id);
      if (connect.result === -1) {
        for (const stage of ['connect-wait', 'connect-enumerate', 'connect-async']) {
          const row = zero(id, stage);
          check(row.ms - connect.ms <= 1000, 'Connect deadline exceeded');
        }
      }
    }
    for (let exchange = 0; exchange < 6; exchange++) {
      for (let lane = 0; lane < 6; lane++) {
        const id = batch * 6 + lane;
        const begin = take(id, 'exchange-begin');
        check(begin.result === exchange && begin.error === 0, 'Exchange ordinal mismatch');
        let sent = 0, received = 0;
        while (sent < REQUEST_BYTES) {
          zero(id, 'write-reset');
          const submit = take(id, 'write-submit');
          const asynchronous = submit.result === -1 && submit.error === 997;
          check(asynchronous || (submit.result === 0 && submit.error === 0), 'Write submission failed');
          if (asynchronous) zero(id, 'write-wait');
          const complete = take(id, asynchronous ? 'write-complete-async' : 'write-complete-sync');
          check(complete.result > 0 && complete.result <= REQUEST_BYTES - sent && complete.error === 0, 'Invalid completed request byte count');
          sent += complete.result;
        }
        if (exchange === 0) zero(id, 'event-select-read-close');
        // A read notification is not a response; only positive recv bytes count.
        while (peek()?.stage !== 'http-complete') {
          const recv = take(id, 'http-recv');
          if (recv.result === -1 && recv.error === 10035) {
            zero(id, 'read-wait');
            zero(id, 'read-enumerate');
            const events = take(id, 'read-events');
            check(events.result === 1 && events.error === 0, 'Read readiness missing or unexpected peer close');
            zero(id, 'read-event-error');
          } else {
            check(recv.result > 0 && recv.error === 0 && recv.result <= Math.min(1024, 8192 - received), 'Invalid response byte count');
            received += recv.result;
          }
        }
        const complete = take(id, 'http-complete');
        check(complete.result === exchange && complete.error === 0 && received >= MIN_RESPONSE_BYTES && received <= 8192, 'Incomplete exchange response');
        check(complete.ms - begin.ms <= 1000, 'Exchange deadline exceeded');
      }
    }
    // No event/buffer release until all six owned sockets have been closed.
    for (let lane = 0; lane < 6; lane++) {
      const id = batch * 6 + lane;
      zero(id, 'pending-at-close');
      zero(id, 'shutdown');
      zero(id, 'closesocket');
    }
    for (let lane = 0; lane < 6; lane++) {
      const id = batch * 6 + lane;
      zero(id, 'event-close');
      zero(id, 'write-event-close');
    }
  }
  zero(summary.attempts, 'wsa-cleanup');
  check(index === records.length, 'Extra records after complete lifecycle');
}

export function validateOutput(text) {
  check(typeof text === 'string' && Buffer.byteLength(text) <= MAX_BYTES && text.endsWith('\n'), 'Incomplete/oversized output');
  const lines = text.slice(0, -1).split('\n').map(line => line.endsWith('\r') ? line.slice(0, -1) : line);
  check(lines.length >= 2 && lines.length <= 100000, 'Record count');
  const records = lines.map(parseLine);
  const header = records.shift(), summary = records.pop();
  exact(header, ['type', 'schema', 'profile', 'exchanges', 'mode', 'batches', 'width', 'interval_ms', 'total_cap_ms', 'request']);
  check(header.type === 'header' && header.schema === 2 && header.profile === 'pooled-event-overlapped' && header.exchanges === 6 && ['plain', 'randomized'].includes(header.mode) && natural(header.batches, 128) && header.batches > 0 && header.width === 6 && header.interval_ms === 50 && header.total_cap_ms === 30000 && header.request === 'owned-readyz', 'Header invalid');
  exact(summary, summaryKeys);
  check(summary.type === 'summary' && typeof summary.complete === 'boolean', 'Summary invalid');
  for (const key of summaryKeys.filter(key => !['type', 'complete'].includes(key))) check(natural(summary[key]), 'Summary number invalid');
  check(summary.attempts <= header.batches * 6 && summary.elapsed_ms <= MAX_ELAPSED, 'Attempt/cleanup deadline cap');
  if (summary.complete) check(summary.attempts === header.batches * 6 && summary.elapsed_ms <= 30000, 'False completeness');

  const observed = Object.fromEntries(summaryKeys.filter(key => !['type', 'complete', 'elapsed_ms'].includes(key)).map(key => [key, 0]));
  const owners = new Map();
  let liveSockets = 0, liveEvents = 0, previousMs = 0, previous = null, cleanupSeen = false, error10055 = 0;
  for (const row of records) {
    exact(row, row?.type === 'operation' ? ['type', 'id', 'stage', 'result', 'error', 'ms'] : row?.type === 'option' ? ['type', 'id', 'value', 'length', 'ms'] : ['type', 'id', 'stage', 'error', 'ms']);
    check(['operation', 'failure', 'option'].includes(row.type) && natural(row.id, summary.attempts) && natural(row.ms, MAX_ELAPSED) && row.ms >= previousMs, 'Record identity/time invalid');
    previousMs = row.ms;
    if (row.type === 'failure') {
      check(failureStages.has(row.stage) && natural(row.error, 65535) && row.error > 0, 'Failure invalid');
      check((row.id < summary.attempts && owners.has(row.id)) || (row.id === summary.attempts && ['total-deadline', 'output-cap', 'ownership', 'wsa-cleanup'].includes(row.stage)), 'Unowned failure');
      if (cleanupSeen) check(['wsa-cleanup', 'ownership', 'total-deadline'].includes(row.stage), 'Failure after cleanup');
      observed.failures++;
      previous = row;
      continue;
    }
    if (row.type === 'option') {
      const owner = owners.get(row.id);
      check(!cleanupSeen && owner && !owner.closed && !owner.option && previous?.type === 'operation' && previous.id === row.id && previous.stage === 'get-randomize', 'Option observation out of order');
      check(previous.result === 0 ? typeof row.value === 'boolean' && Number.isInteger(row.length) && row.length >= -1 && row.length <= 65535 : row.value === null && row.length === null, 'Option readback invalid');
      owner.option = true;
      previous = row;
      continue;
    }
    check(stages.has(row.stage) && Number.isSafeInteger(row.result) && row.result >= -2147483648 && row.result <= 2147483647 && natural(row.error, 65535) && validResult(row), 'Operation result/error invalid');
    observed.operation_records++;
    check(observed.operation_records <= MAX_OPERATIONS, 'Operation output cap');
    if (row.error === 10055) error10055++;
    const success = row.error === 0 && row.result >= 0;
    if (row.stage === 'wsa-cleanup') {
      check(!cleanupSeen && row.id === summary.attempts, 'Duplicate/invalid global cleanup');
      cleanupSeen = true;
      // A failed cleanup trace may still retain resources, but never qualifies.
    } else {
      check(!cleanupSeen && row.id < summary.attempts, 'Operation outside owned lifetime');
      if (row.stage === 'socket') {
        check(row.id === owners.size && row.ms >= Math.floor(row.id / 6) * 50, 'Creation order/schedule invalid');
        if (row.id % 6 === 0) check(liveSockets === 0 && liveEvents === 0, 'Prior pool remains live');
        owners.set(row.id, { socket: success, closed: false, closeAttempted: false, event: false, writeEvent: false, stages: new Set(), option: false, pending: false, retained: false });
        observed.attempts++;
        if (success) { liveSockets++; observed.sockets_opened++; }
      }
      const owner = owners.get(row.id);
      check(owner, 'Operation before socket creation');
      if (!cleanupStages.has(row.stage)) check(!owner.closeAttempted, 'I/O after close began');
      const once = !['exchange-begin', 'write-reset', 'write-submit', 'write-wait', 'write-complete-sync', 'write-complete-async', 'http-recv', 'read-wait', 'read-enumerate', 'read-events', 'read-event-error', 'close-event-error', 'http-complete'].includes(row.stage);
      if (once) { check(!owner.stages.has(row.stage), 'Repeated lifecycle stage'); owner.stages.add(row.stage); }
      if (row.stage === 'event-create' || row.stage === 'write-event-create') {
        const key = row.stage === 'event-create' ? 'event' : 'writeEvent';
        check(owner.socket && !owner.closed && !owner[key], 'Event not owned by live socket');
        if (success) { owner[key] = true; liveEvents++; observed.events_opened++; }
      }
      if (row.stage.startsWith('write-') && !['write-event-create', 'write-retained'].includes(row.stage)) check(owner.writeEvent, 'Write operation without owned event');
      if (row.stage === 'write-reset') check(!owner.pending, 'Reset/reuse with pending write');
      if (row.stage === 'write-submit') { check(!owner.pending, 'Concurrent writes reuse storage'); owner.pending = row.result === -1 && row.error === 997; }
      if (completions.has(row.stage)) {
        if (success) {
          observed[row.stage === 'write-complete-sync' ? 'sync_writes' : 'async_writes']++;
          observed.bytes_sent += row.result;
        }
        owner.pending = !success && row.error === 996;
      }
      if (row.stage === 'http-recv' && success) observed.bytes_received += row.result;
      if (row.stage === 'http-complete') observed.successes++;
      if (row.stage === 'pending-at-close') {
        check(row.result === Number(owner.pending), 'Pending ownership observation mismatch');
        owner.closeAttempted = true;
        observed.pending_at_close += row.result;
      }
      if (row.stage === 'shutdown') check(owner.socket && !owner.closed && owner.closeAttempted, 'Unowned/premature shutdown');
      if (row.stage === 'closesocket') {
        check(owner.socket && !owner.closed && owner.stages.has('shutdown'), 'Unowned/premature socket close');
        if (success) { owner.closed = true; liveSockets--; observed.sockets_closed++; }
      }
      if (['post-close-wait', 'post-close-signal', 'write-retained'].includes(row.stage)) {
        check(owner.pending && owner.closed && liveSockets === 0 && owner.writeEvent, 'Post-close observation without pending owner/group close');
        if (row.stage === 'post-close-signal') {
          check(previous?.stage === 'post-close-wait' && previous.id === row.id && previous.result === 0 && previous.error === 0, 'Signal without successful wait');
          observed.post_close_signals++;
          // Notification does not prove a terminal result or release ownership.
        }
        if (row.stage === 'write-retained') { owner.retained = true; observed.retained_writes++; }
      }
      if (row.stage === 'event-close' || row.stage === 'write-event-close') {
        const key = row.stage === 'event-close' ? 'event' : 'writeEvent';
        check(owner.closeAttempted && (!owner.socket || owner.closed) && liveSockets === 0 && owner[key], 'Event release before group socket close');
        check(key !== 'writeEvent' || (!owner.pending && !owner.retained), 'Pending/retained write event released');
        if (success) { owner[key] = false; liveEvents--; observed.events_closed++; }
      }
      check(liveSockets >= 0 && liveSockets <= 6 && liveEvents >= 0 && liveEvents <= 12, 'Live resource cap');
    }
    previous = row;
  }
  check(cleanupSeen, 'Missing Winsock cleanup');
  check(summary.elapsed_ms >= previousMs, 'Summary time precedes observations');
  for (const [key, value] of Object.entries(observed)) check(summary[key] === value, `Counter mismatch: ${key}`);
  if (error10055) check(records.some(row => row.type === 'failure' && row.error === 10055), 'Unreported operation 10055');
  const passed = summary.complete && summary.failures === 0 && error10055 === 0 && summary.successes === summary.attempts * 6 && summary.sockets_opened === summary.attempts && summary.sockets_closed === summary.attempts && summary.events_opened === summary.attempts * 2 && summary.events_closed === summary.attempts * 2 && summary.pending_at_close === 0 && summary.post_close_signals === 0 && summary.retained_writes === 0;
  if (passed) qualify(records, header, summary);
  return { header, summary, passed, error10055, reproduction: 'inconclusive' };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    if (process.argv.length !== 3) throw new Error('Usage: node validate-output.mjs output.jsonl');
    check(fs.statSync(process.argv[2]).size <= MAX_BYTES, 'Oversized output');
    const result = validateOutput(fs.readFileSync(process.argv[2], 'utf8'));
    console.log(JSON.stringify(result));
    process.exitCode = result.passed ? 0 : 1;
  } catch (error) { console.error(error.message); process.exitCode = 2; }
}
