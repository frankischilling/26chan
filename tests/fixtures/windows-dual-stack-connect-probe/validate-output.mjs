import { readFileSync, statSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// This reader has no dependency on the native producer or its counters.
const MAX_BYTES = 2 * 1024 * 1024;
const MAX_RECORDS = 12_000;
const MAX_DIAGNOSTIC_MS = 40_000;
const HEADER = Object.freeze({
  type: 'header', schema: 1, profile: 'dual-stack-connect', mode: 'plain',
  groups: 40, lanes: 6, fallback_ms: 300, group_ms: 700, work_ms: 30_000,
  cleanup_ms: 35_000, max_live: 12, max_starts: 480,
});
const SETUP = ['socket', 'event-create', 'event-select', 'set-randomize', 'get-randomize', 'connect'];
const STAGES = new Set([...SETUP, 'event-ready', 'completion-api', 'completion-consumed', 'close-intent', 'closesocket', 'event-close']);
const DEADLINES = new Set(['startup', 'work-deadline', 'group-deadline', 'cleanup-deadline', 'wsa-cleanup']);
const OP_KEYS = ['type', 'id', 'stage', 'result', 'error', 'ms'];
const GROUP_KEYS = ['type', 'id', 'ms'];
const FAILURE_KEYS = ['type', 'id', 'stage', 'error', 'ms'];
const COUNTERS = [
  'starts', 'ipv4_successes', 'expected_refusals', 'closed_before_handling',
  'ready_before_close', 'sockets_opened', 'sockets_closed', 'events_opened',
  'events_closed', 'max_live', 'failures', 'operation_records',
];
const SUMMARY_KEYS = ['type', ...COUNTERS, 'complete', 'elapsed_ms'];

function emptySummary() {
  return { type: 'summary', ...Object.fromEntries(COUNTERS.map(key => [key, 0])), complete: false, elapsed_ms: 0, qualified: false };
}

function reject(message, code = 'invalid-evidence', summary) {
  const error = new Error(message);
  error.code = code;
  if (summary) error.summary = summary;
  throw error;
}

function requireThat(condition, message) {
  if (!condition) reject(message);
}

function integer(value, minimum = 0, maximum = Number.MAX_SAFE_INTEGER) {
  return Number.isSafeInteger(value) && value >= minimum && value <= maximum;
}

function exactKeys(record, keys) {
  requireThat(Object.keys(record).length === keys.length && keys.every(key => Object.hasOwn(record, key)), 'Unexpected record fields');
}

// The schema contains flat objects only. Parse primitive tokens explicitly so
// duplicate decoded keys cannot disappear inside JSON.parse, including escapes.
function parseRecord(line) {
  let at = 0;
  const whitespace = () => { while (/[\x20\t\r]/.test(line[at] ?? '') && at < line.length) at++; };
  const string = () => {
    const start = at;
    requireThat(line[at++] === '"', 'Invalid JSON record');
    let escaped = false;
    while (at < line.length) {
      const character = line[at++];
      if (!escaped && character === '"') {
        try { return JSON.parse(line.slice(start, at)); } catch { reject('Invalid JSON string'); }
      }
      if (!escaped && character === '\\') escaped = true;
      else escaped = false;
    }
    reject('Unterminated JSON string');
  };
  whitespace();
  requireThat(line[at++] === '{', 'Record must be a JSON object');
  const result = Object.create(null);
  whitespace();
  if (line[at] !== '}') {
    while (true) {
      whitespace();
      const key = string();
      requireThat(!Object.hasOwn(result, key), 'Duplicate JSON key');
      whitespace();
      requireThat(line[at++] === ':', 'Invalid JSON record');
      whitespace();
      let value;
      if (line[at] === '"') value = string();
      else {
        const token = /^(?:-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?|true|false|null)/.exec(line.slice(at));
        requireThat(token !== null, 'Invalid JSON value');
        at += token[0].length;
        value = JSON.parse(token[0]);
      }
      result[key] = value;
      whitespace();
      if (line[at] === '}') break;
      requireThat(line[at++] === ',', 'Invalid JSON record');
    }
  }
  at++;
  whitespace();
  requireThat(at === line.length, 'Trailing JSON data');
  return result;
}

function parse(text) {
  requireThat(typeof text === 'string', 'Evidence must be text');
  requireThat(Buffer.byteLength(text, 'utf8') <= MAX_BYTES, 'Evidence exceeds size limit');
  const lines = text.split('\n');
  if (lines.at(-1) === '') lines.pop();
  requireThat(lines.length >= 2 && lines.length <= MAX_RECORDS, 'Invalid record count');
  return lines.map(parseRecord);
}

function operationError(row) {
  if (row.result !== -1) return false;
  if (row.stage === 'connect' && row.error === 10035) return false;
  if (row.id % 2 === 0 && ['connect', 'completion-consumed'].includes(row.stage) && row.error === 10061) return false;
  return true;
}

function validResult(row, successResults = [0]) {
  requireThat((successResults.includes(row.result) && row.error === 0) || (row.result === -1 && row.error > 0), 'Invalid operation result');
}

/**
 * Validate one complete schema-1 JSONL trace. Returns only independently derived
 * counters. Malformed, failed, incomplete, and zero-exposure evidence throws.
 * Failed/inconclusive errors carry a fixed-schema `summary` for the CLI.
 */
export function validate(text) {
  const records = parse(text);
  const header = records[0];
  exactKeys(header, Object.keys(HEADER));
  requireThat(header.mode === 'plain' || header.mode === 'randomized', 'Invalid mode');
  for (const [key, expected] of Object.entries(HEADER)) {
    if (key !== 'mode') requireThat(header[key] === expected, 'Invalid header');
  }
  const declared = records.at(-1);
  exactKeys(declared, SUMMARY_KEYS);
  requireThat(declared.type === 'summary', 'Missing final summary');
  for (const key of COUNTERS) requireThat(integer(declared[key]), 'Invalid summary counter');
  requireThat(typeof declared.complete === 'boolean' && integer(declared.elapsed_ms, 0, MAX_DIAGNOSTIC_MS), 'Invalid summary status');

  const derived = emptySummary();
  const states = new Map();
  const groups = new Map();
  let activeGroup = -1;
  let liveSockets = 0;
  let liveEvents = 0;
  let lastMs = 0;
  let expectedFailure = null;
  let expectedCompletion = null;
  let fatal = false;

  for (let index = 1; index < records.length - 1; index++) {
    const row = records[index];
    requireThat(['group', 'operation', 'failure'].includes(row.type), 'Unexpected record type');
    exactKeys(row, row.type === 'group' ? GROUP_KEYS : row.type === 'operation' ? OP_KEYS : FAILURE_KEYS);
    requireThat(integer(row.id, 0, row.type === 'group' ? 39 : 479), 'Invalid operation ID');
    requireThat(integer(row.ms, 0, MAX_DIAGNOSTIC_MS) && row.ms >= lastMs, 'Invalid timestamp order or bound');
    lastMs = row.ms;
    if (expectedFailure) {
      requireThat(row.type === 'failure' && row.id === expectedFailure.id && row.stage === expectedFailure.stage && row.error === expectedFailure.error, 'Missing immediate failure record');
    }
    if (expectedCompletion !== null) {
      requireThat(row.type === 'operation' && row.stage === 'completion-consumed' && row.id === expectedCompletion, 'Missing immediate completion result');
      expectedCompletion = null;
    }
    if (row.type === 'group') {
      requireThat(!fatal && row.id === activeGroup + 1 && liveSockets === 0 && liveEvents === 0, 'Groups must close resources before advancing');
      activeGroup = row.id;
      groups.set(row.id, { start: row.ms });
      continue;
    }
    requireThat(integer(row.error), 'Invalid error number');
    if (row.type === 'failure') {
      requireThat(row.error > 0 && (STAGES.has(row.stage) || DEADLINES.has(row.stage)), 'Invalid failure record');
      if (STAGES.has(row.stage)) {
        requireThat(expectedFailure !== null, 'Orphan operation failure');
      } else {
        requireThat(expectedFailure === null, 'Failure does not match operation');
        if (row.stage === 'wsa-cleanup') requireThat(row.id === 0 && [...states.values()].every(state => !state.opened || (state.close && (!state.event || state.close.result === -1 || state.eventClose))), 'Invalid process cleanup failure');
        if (row.stage === 'startup') requireThat(row.id === 0 && derived.operation_records === 0, 'Invalid startup failure');
        if (row.stage === 'work-deadline') requireThat(row.id === 0 && row.ms >= HEADER.work_ms, 'Invalid work deadline');
        if (row.stage === 'cleanup-deadline') requireThat(row.id === 0 && row.ms > HEADER.cleanup_ms, 'Invalid cleanup deadline');
        if (row.stage === 'group-deadline') {
          const group = groups.get(row.id / 12);
          requireThat(row.id % 12 === 0 && group && row.ms >= group.start + HEADER.group_ms, 'Invalid group deadline');
        }
      }
      expectedFailure = null;
      derived.failures++;
      fatal = true;
      continue;
    }

    requireThat(STAGES.has(row.stage) && integer(row.result, -1), 'Invalid operation stage or result');
    requireThat(!fatal || ['event-ready', 'completion-api', 'completion-consumed', 'close-intent', 'closesocket', 'event-close'].includes(row.stage), 'New work after fatal failure');
    derived.operation_records++;
    const groupId = Math.floor(row.id / 12);
    let state = states.get(row.id);
    if (row.stage === 'socket') {
      requireThat(!state, 'Duplicate socket ownership');
      if (groupId !== activeGroup) {
        reject('Socket has no active group record');
      }
      requireThat(groups.has(groupId) && groupId === activeGroup, 'Group order violation');
      state = { next: 0, opened: false, event: false, setupFailed: false, connect: null, ready: null, completionApi: null, consumed: null, intent: null, close: null, eventClose: null };
      states.set(row.id, state);
    }
    requireThat(state, 'Operation has no socket ownership');
    requireThat(!state.close || row.stage === 'event-close', 'Socket API after close attempt');
    requireThat(!state.intent || ['closesocket', 'event-close'].includes(row.stage), 'Socket API after close intent');
    requireThat(!state.eventClose, 'Operation after event close attempt');

    if (SETUP.includes(row.stage)) {
      requireThat(!state.setupFailed && SETUP[state.next] === row.stage, 'Missing or out-of-order setup operation');
      validResult(row, row.stage === 'get-randomize' ? [0, 1] : [0]);
      if (row.stage === 'get-randomize' && row.result !== -1) {
        requireThat(row.result === (header.mode === 'randomized' ? 1 : 0), 'Socket option readback mismatch');
      }
      if (row.stage === 'socket' && row.result === 0) {
        state.opened = true;
        derived.sockets_opened++;
        liveSockets++;
        derived.max_live = Math.max(derived.max_live, liveSockets);
        requireThat(liveSockets <= HEADER.max_live, 'Socket live cap exceeded');
      }
      if (row.stage === 'event-create' && row.result === 0) {
        state.event = true;
        derived.events_opened++;
        liveEvents++;
      }
      if (row.stage === 'connect') {
        requireThat(row.ms < HEADER.work_ms, 'Connect begins after work deadline');
        requireThat(row.ms < groups.get(groupId).start + HEADER.group_ms, 'Connect begins after group deadline');
        if (row.id % 2 === 1) {
          const ipv6 = states.get(row.id - 1)?.connect;
          requireThat(ipv6 && row.ms >= ipv6.ms + HEADER.fallback_ms, 'IPv4 fallback interval is missing');
        }
        state.connect = row;
        derived.starts++;
      }
      state.next++;
      if (operationError(row)) state.setupFailed = true;
    } else if (row.stage === 'event-ready') {
      requireThat(state.connect?.result === -1 && state.connect.error === 10035 && !state.ready && !state.consumed, 'Unexpected or repeated readiness');
      validResult(row, [0, 258]);
      state.ready = row;
    } else if (row.stage === 'completion-api') {
      const deferred = row.id % 2 === 0 && Math.floor(row.id / 2) % 6 >= 3;
      requireThat(state.connect?.error === 10035 && state.ready?.result === 0 && !state.completionApi && !state.consumed && !deferred, 'Completion API without eligible signaled event');
      validResult(row);
      state.completionApi = row;
      if (row.result === 0) expectedCompletion = row.id;
    } else if (row.stage === 'completion-consumed') {
      const deferred = row.id % 2 === 0 && Math.floor(row.id / 2) % 6 >= 3;
      requireThat(state.connect?.error === 10035 && state.ready?.result === 0 && state.completionApi?.result === 0 && !state.consumed && !deferred, 'Completion without eligible signaled event');
      validResult(row);
      state.consumed = row;
    } else if (row.stage === 'close-intent') {
      requireThat(state.opened && !state.intent, 'Missing or duplicate close ownership');
      const pending = state.connect?.result === -1 && state.connect.error === 10035;
      if (pending) requireThat(state.ready, 'Pending socket has no final readiness observation');
      const deferred = row.id % 2 === 0 && Math.floor(row.id / 2) % 6 >= 3;
      if (pending && state.ready.result === 0 && !deferred) requireThat(state.consumed || state.completionApi?.result === -1, 'Signaled consuming lane has no completion');
      const unhandled = pending && !state.consumed;
      requireThat(row.error === 0 && row.result === (unhandled ? 1 : 0), 'Close intent does not match completion handling');
      state.intent = row;
      if (unhandled) {
        derived.closed_before_handling++;
        if (state.ready.result === 0) derived.ready_before_close++;
      }
    } else if (row.stage === 'closesocket') {
      requireThat(state.opened && state.intent && !state.close, 'Missing or duplicate socket close');
      validResult(row);
      state.close = row;
      if (row.result === 0) {
        derived.sockets_closed++;
        liveSockets--;
      }
    } else if (row.stage === 'event-close') {
      requireThat(state.event && state.close?.result === 0 && !state.eventClose, 'Event close has no released socket owner');
      validResult(row);
      state.eventClose = row;
      if (row.result === 0) {
        derived.events_closed++;
        liveEvents--;
      }
    }

    if ((row.stage === 'connect' || row.stage === 'completion-consumed') && row.result === 0 && row.id % 2 === 1) derived.ipv4_successes++;
    if ((row.stage === 'connect' || row.stage === 'completion-consumed') && row.result === -1 && row.error === 10061 && row.id % 2 === 0) derived.expected_refusals++;
    if (operationError(row)) expectedFailure = row;
  }
  requireThat(expectedFailure === null, 'Missing final failure record');
  requireThat(expectedCompletion === null, 'Missing final completion result');
  for (const state of states.values()) {
    if (state.opened) requireThat(state.intent && state.close, 'Opened socket has no close attempt');
    if (state.event && state.close?.result === 0) requireThat(state.eventClose, 'Created event has no close attempt');
    if (state.event && state.close?.result === -1) requireThat(!state.eventClose, 'Event released after failed socket close');
  }
  requireThat(declared.elapsed_ms >= lastMs, 'Summary precedes operation time');
  derived.elapsed_ms = declared.elapsed_ms;
  derived.complete = derived.starts === HEADER.max_starts && derived.ipv4_successes === 240 && derived.failures === 0 && derived.elapsed_ms <= HEADER.cleanup_ms && liveSockets === 0 && liveEvents === 0;
  for (const key of COUNTERS) requireThat(declared[key] === derived[key], 'Summary counter disagrees with operations');
  requireThat(declared.complete === derived.complete, 'Summary completeness disagrees with operations');
  derived.qualified = derived.complete && derived.closed_before_handling > 0;
  if (!derived.complete) reject('Evidence is failed or incomplete', 'nonqualifying-evidence', derived);
  if (!derived.qualified) reject('Evidence has no cancellation exposure', 'inconclusive-evidence', derived);
  return derived;
}

function main() {
  let summary = emptySummary();
  let status = 1;
  try {
    requireThat(process.argv.length === 3, 'Expected one evidence file');
    const info = statSync(process.argv[2]);
    requireThat(info.isFile() && info.size <= MAX_BYTES, 'Invalid evidence file');
    const bytes = readFileSync(process.argv[2]);
    const text = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
    summary = validate(text);
    status = 0;
  } catch (error) {
    if (error.summary) summary = error.summary;
  }
  // Never print file paths, rejected keys, addresses, or native error strings.
  process.stdout.write(`${JSON.stringify(summary)}\n`);
  process.exitCode = status;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
