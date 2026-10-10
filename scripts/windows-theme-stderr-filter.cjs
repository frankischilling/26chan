'use strict';

// This module does not read/write streams or files. Only the owned early debug
// sink may turn an exact Chromium source message into EVENT_MARKER. The outer
// collector deliberately does not recognize raw browser/debug output.
const CARRY_BYTES = 4096;
const EVENT_LIMIT = 16;
const ARTIFACT_BYTES = 8192;
const EVENT_MARKER = '[owned-windows-transport] synchronous-connect-error 10055';
const TRUNCATED_MARKER = '[owned-windows-transport] synchronous-connect-events-truncated';
const UNAVAILABLE_MARKER = '[owned-windows-transport] probe-unavailable';
const SETUP_REFUSED_MARKER = '[owned-windows-transport] probe-setup-refused';
const CONTROL_COORDINATOR_READY_MARKER = '[owned-browser-control] stderr-probe-coordinator-ready';
const CONTROL_WORKER_READY_MARKER = '[owned-browser-control] stderr-probe-worker-ready';
const COUNTER_LIMIT = 0xffffffff;

// Chromium 151 uses a colon before the source line, not parentheses. Accept
// either source-path separator, but no general paths, whitespace normalization,
// Unicode lookalikes, terminal escapes, debug suffixes, or partial matches.
// Process/thread identifiers are optional Chromium logging flags; timestamp is
// mandatory here, with at most two numeric identifier fields before it.
const CHROMIUM_CONNECT_LINE = /^(?:\d{4}-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12]\d|3[01])T(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d\.\d{3}Z pw:browser \[pid=\d{1,10}\]\[err\] )?\[(?:\d{1,10}:){0,2}(?:0[1-9]|1[0-2])(?:0[1-9]|[12]\d|3[01])\/(?:[01]\d|2[0-3])[0-5]\d[0-5]\d\.\d{3}:ERROR:net[\\/]socket[\\/]tcp_socket_win\.cc:[1-9]\d{0,6}\] connect failed: 10055$/;

function recognizeChromiumConnectLine(text) {
  if (typeof text !== 'string' || text.length === 0 || text.length > CARRY_BYTES) return false;
  // Checking before matching also closes JavaScript's permissive final-$ newline
  // behavior. Never strip controls: doing so can join contaminated tokens.
  for (let index = 0; index < text.length; index++) {
    const code = text.charCodeAt(index);
    if (code < 0x20 || code > 0x7e) return false;
  }
  return CHROMIUM_CONNECT_LINE.test(text);
}

function createStderrCollector({ now = Date.now } = {}) {
  const carry = Buffer.alloc(CARRY_BYTES);
  const events = [];
  const counters = {
    bytes_seen: 0,
    lines_seen: 0,
    matched_lines: 0,
    discarded_lines: 0,
    oversized_lines: 0,
    invalid_lines: 0,
    ansi_control_lines: 0,
    unterminated_lines: 0,
    dropped_events: 0,
    truncation_markers: 0,
    unavailable_markers: 0,
    setup_refused_markers: 0,
    invalid_chunks: 0,
    clock_faults: 0,
    carry_peak_bytes: 0,
  };
  let complete = false;
  let truncated = false;
  let probeUnavailable = false;
  let setupRefused = false;
  let controlCoordinatorReadyCount = 0;
  let controlWorkerReadyCount = 0;
  let carryLength = 0;
  let lineLength = 0;
  let oversized = false;
  let invalid = false;
  let control = false;
  let pendingCR = false;
  let lastReceipt = 0;

  function increment(key, amount = 1) {
    counters[key] = Math.min(COUNTER_LIMIT, counters[key] + amount);
  }

  function readClock() {
    try {
      const value = typeof now === 'function' ? now() : null;
      if (typeof value === 'number' && Number.isFinite(value)
          && value >= 0 && value <= Number.MAX_SAFE_INTEGER) return value;
    } catch {
      // Arbitrary errors, including their messages/stacks, never leave the filter.
    }
    increment('clock_faults');
    return null;
  }

  const started = readClock();

  function receiptMillis() {
    // A missing origin cannot be repaired by adopting a later absolute timestamp.
    if (started === null) return 0;
    const current = readClock();
    if (current === null) return lastReceipt;
    const relative = Math.floor(current - started);
    if (!Number.isSafeInteger(relative) || relative < lastReceipt) {
      increment('clock_faults');
      return lastReceipt;
    }
    lastReceipt = relative;
    return relative;
  }

  function clearCarry() {
    carry.fill(0, 0, carryLength);
    carryLength = 0;
  }

  function isMarker(marker) {
    const length = carryLength - (pendingCR ? 1 : 0);
    if (length !== marker.length) return false;
    for (let index = 0; index < length; index++) {
      if (carry[index] !== marker.charCodeAt(index)) return false;
    }
    return true;
  }

  function closeLine(terminated) {
    if (terminated) increment('lines_seen');
    else {
      increment('unterminated_lines');
      // A CR is allowed only as the final byte of a CRLF terminator.
      if (pendingCR) { invalid = true; control = true; }
    }
    if (oversized) increment('oversized_lines');
    if (invalid) increment('invalid_lines');
    if (control) increment('ansi_control_lines');
    if (terminated && !oversized && !invalid && isMarker(EVENT_MARKER)) {
      increment('matched_lines');
      if (events.length < EVENT_LIMIT) {
        events.push({
          event: 'synchronous-connect-error',
          os_error: 10055,
          receipt_ms: receiptMillis(),
        });
      } else { increment('dropped_events'); truncated = true; }
    } else if (terminated && !oversized && !invalid && isMarker(TRUNCATED_MARKER)) {
      increment('truncation_markers');
      truncated = true;
    } else if (terminated && !oversized && !invalid && isMarker(UNAVAILABLE_MARKER)) {
      increment('unavailable_markers');
      probeUnavailable = true;
    } else if (terminated && !oversized && !invalid && isMarker(SETUP_REFUSED_MARKER)) {
      increment('setup_refused_markers');
      setupRefused = true;
    } else if (terminated && !oversized && !invalid && isMarker(CONTROL_COORDINATOR_READY_MARKER)) {
      controlCoordinatorReadyCount = Math.min(COUNTER_LIMIT, controlCoordinatorReadyCount + 1);
    } else if (terminated && !oversized && !invalid && isMarker(CONTROL_WORKER_READY_MARKER)) {
      controlWorkerReadyCount = Math.min(COUNTER_LIMIT, controlWorkerReadyCount + 1);
    } else increment('discarded_lines');
    clearCarry();
    lineLength = 0;
    oversized = false;
    invalid = false;
    control = false;
    pendingCR = false;
  }

  function consume(code, width = 1) {
    if (code === 0x0a) { closeLine(true); return; }
    lineLength = Math.min(CARRY_BYTES + 1, lineLength + width);
    if (lineLength > CARRY_BYTES) oversized = true;
    if (pendingCR) { invalid = true; control = true; }
    pendingCR = code === 0x0d;
    if (code > 0x7e) invalid = true;
    if ((code < 0x20 && code !== 0x0d) || code === 0x7f) {
      invalid = true;
      control = true;
    }
    if (oversized || invalid) {
      // Stay poisoned until LF, even when subsequent chunks look like a marker.
      clearCarry();
      return;
    }
    carry[carryLength++] = code;
    counters.carry_peak_bytes = Math.max(counters.carry_peak_bytes, carryLength);
  }

  function push(chunk) {
    if (complete) return;
    if (Buffer.isBuffer(chunk)) {
      increment('bytes_seen', chunk.length);
      for (const byte of chunk) consume(byte);
      return;
    }
    if (typeof chunk === 'string') {
      increment('bytes_seen', Buffer.byteLength(chunk, 'utf8'));
      // Do not allocate a Buffer the size of an untrusted string chunk. Non-ASCII
      // is invalid, but account for its UTF-8 width without retaining those bytes.
      for (let index = 0; index < chunk.length; index++) {
        const code = chunk.charCodeAt(index);
        let width = code <= 0x7f ? 1 : code <= 0x7ff ? 2 : 3;
        if (code >= 0xd800 && code <= 0xdbff && index + 1 < chunk.length) {
          const next = chunk.charCodeAt(index + 1);
          if (next >= 0xdc00 && next <= 0xdfff) { width = 4; index++; }
        }
        consume(code, width);
      }
      return;
    }
    // Never stringify an unexpected object, and do not join text across it.
    increment('invalid_chunks');
    invalid = true;
    clearCarry();
  }

  function snapshot() {
    // All keys/strings are fixed and all numbers bounded. Even with sixteen
    // maximal timestamps/counters this schema is comfortably below 8 KiB.
    return {
      schema_version: 1,
      branch_evidence: events.length > 0 ? 'positive-only' : 'inconclusive',
      events: events.map(({ event, os_error, receipt_ms }) => ({ event, os_error, receipt_ms })),
      counters: { ...counters },
      complete,
      truncated,
      probe_unavailable: probeUnavailable,
      setup_refused: setupRefused,
      ...(controlCoordinatorReadyCount ? { control_probe_coordinator_count: controlCoordinatorReadyCount } : {}),
      ...(controlWorkerReadyCount ? { control_probe_worker_count: controlWorkerReadyCount } : {}),
    };
  }

  function finish() {
    if (!complete) {
      if (lineLength > 0 || invalid) closeLine(false);
      carry.fill(0);
      complete = true;
    }
    return snapshot();
  }

  return Object.freeze({ push, finish, snapshot });
}

module.exports = Object.freeze({
  CARRY_BYTES, EVENT_LIMIT, ARTIFACT_BYTES, EVENT_MARKER, TRUNCATED_MARKER, UNAVAILABLE_MARKER,
  SETUP_REFUSED_MARKER, CONTROL_COORDINATOR_READY_MARKER, CONTROL_WORKER_READY_MARKER,
  recognizeChromiumConnectLine, createStderrCollector,
});
