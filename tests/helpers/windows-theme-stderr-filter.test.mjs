import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const {
  CARRY_BYTES, EVENT_LIMIT, ARTIFACT_BYTES, EVENT_MARKER, TRUNCATED_MARKER, UNAVAILABLE_MARKER,
  SETUP_REFUSED_MARKER,
  recognizeChromiumConnectLine, createStderrCollector,
} = require('../../scripts/windows-theme-stderr-filter.cjs');

const raw = '[8765:4321:1008/030405.123:ERROR:net\\socket\\tcp_socket_win.cc:244] connect failed: 10055';
const debug = `2026-10-08T03:04:05.123Z pw:browser [pid=8765][err] ${raw}`;
const marker = `${EVENT_MARKER}\n`;
const sentinel = 'PRIVATE_SENTINEL_C:\\private\\secret-file_https://secret.invalid:54321/PRIVATE';
const fresh = () => createStderrCollector({ now: () => 100 });

function collect(input) {
  const collector = fresh();
  collector.push(input);
  return collector.finish();
}

function assertBounded(result) {
  assert.ok(Buffer.byteLength(JSON.stringify(result), 'utf8') <= ARTIFACT_BYTES);
  assert.ok(result.events.length <= EVENT_LIMIT);
  assert.ok(result.counters.carry_peak_bytes <= CARRY_BYTES);
  assert.ok(['positive-only', 'inconclusive'].includes(result.branch_evidence));
  for (const value of Object.values(result.counters)) {
    assert.ok(Number.isSafeInteger(value) && value >= 0 && value <= 0xffffffff);
  }
  for (const event of result.events) {
    assert.deepEqual(Object.keys(event), ['event', 'os_error', 'receipt_ms']);
    assert.equal(event.event, 'synchronous-connect-error');
    assert.equal(event.os_error, 10055);
    assert.ok(Number.isSafeInteger(event.receipt_ms) && event.receipt_ms >= 0);
  }
}

test('CJS API and fixed marker/constants are pinned', () => {
  assert.equal(CARRY_BYTES, 4096);
  assert.equal(EVENT_LIMIT, 16);
  assert.equal(ARTIFACT_BYTES, 8192);
  assert.equal(EVENT_MARKER, '[owned-windows-transport] synchronous-connect-error 10055');
  assert.equal(TRUNCATED_MARKER, '[owned-windows-transport] synchronous-connect-events-truncated');
  assert.equal(UNAVAILABLE_MARKER, '[owned-windows-transport] probe-unavailable');
  assert.equal(SETUP_REFUSED_MARKER, '[owned-windows-transport] probe-setup-refused');
  assertBounded(fresh().snapshot());
});

test('recognizer accepts only the complete Chromium source format and standard debug prefix', () => {
  for (const line of [
    raw, debug, raw.replaceAll('\\', '/'), debug.replaceAll('\\', '/'),
    raw.replace('8765:4321:', ''), raw.replace('8765:4321:', '8765:'),
    debug.replace('8765:4321:', ''), debug.replace('8765:4321:', '8765:'),
  ]) {
    assert.equal(recognizeChromiumConnectLine(line), true);
  }
  for (const line of [
    '', 'connect failed: 10055', EVENT_MARKER, TRUNCATED_MARKER,
    raw.replace('244]', '(244)]'), raw.replace('.cc:244', '.cc(244)'),
    raw.replace('tcp_socket_win.cc', 'tcp_socket_posix.cc'),
    raw.replace('net\\socket', 'net\\other'), raw.replace('net\\socket', 'C:\\net\\socket'),
    raw.replace('ERROR', 'WARNING'), raw.replace('ERROR', 'INFO'), raw.replace('ERROR', 'error'),
    raw.replace('10055', '10054'), raw.replace('10055', '-10055'), raw.replace('10055', '100550'),
    raw.replace('connect failed', 'Connect failed'), raw.replace('connect failed', 'connect  failed'),
    raw.replace('244]', '0]'), raw.replace('244]', '12345678]'),
    raw.replace('8765:4321:', '8765:4321:1111:'),
    raw.replace('8765:', '12345678901:'), raw.replace('1008/030405', '1308/030405'),
    raw.replace('1008/030405', '1008/250405'), raw.replace('.123:', '.123456:'),
    `prefix ${raw}`, `${raw} suffix`, `${raw} `, ` ${raw}`, `${raw}\n`, `${raw}\r\n`,
    `${raw}\n${raw}`, `${raw}\u2028`, `${raw}\u2029`,
    debug.replace('[err]', '[out]'), debug.replace('pw:browser', 'pw:api'),
    debug.replace('T03:04', 'T24:04'), debug.replace('.123Z', '.123+00:00'),
    debug.replace('[pid=8765]', '[pid=secret]'), debug.replace('Z pw:', 'Z  pw:'),
    `${'x'.repeat(CARRY_BYTES)}${raw}`, `${raw}${'x'.repeat(CARRY_BYTES)}`,
  ]) assert.equal(recognizeChromiumConnectLine(line), false);
});

test('recognizer never coerces non-string inputs or returns their data', () => {
  let coerced = false;
  for (const value of [undefined, null, 10055, 1n, Symbol(sentinel), Buffer.from(raw), {
    toString() { coerced = true; throw new Error(sentinel); },
  }]) assert.equal(recognizeChromiumConnectLine(value), false);
  assert.equal(coerced, false);
});

test('all ASCII controls and Unicode lookalikes fail closed, without token joining', () => {
  for (let code = 0; code < 0x20; code++) {
    assert.equal(recognizeChromiumConnectLine(raw.replace('connect', `con${String.fromCharCode(code)}nect`)), false);
  }
  for (const line of [
    `\x1b[31m${raw}\x1b[0m`, raw.replace('connect', 'con\x1b[0mnect'),
    raw.replace('10055', '10\x1b[0m055'), raw.replace('net\\socket', 'net\\\x1b[0msocket'),
    `\x1b]0;${sentinel}\x07${raw}`, `\x7f${raw}`, `\x9b31m${raw}`,
    raw.replace('10055', '１００５５'), raw.replace('connect', 'cоnnect'),
    raw.replace('tcp_socket_win.cc', 'tcp_socket_win．cc'), raw.replace(':ERROR:', ':ERR\u200dOR:'),
  ]) assert.equal(recognizeChromiumConnectLine(line), false);
});

test('outer collector accepts fixed owned markers only; raw logs remain inconclusive', () => {
  const result = collect(`${raw}\n${debug}\n${sentinel}\n`);
  assert.equal(result.branch_evidence, 'inconclusive');
  assert.equal(result.events.length, 0);
  assert.equal(result.counters.discarded_lines, 3);
  assert.equal(result.counters.matched_lines, 0);
  assert.ok(!JSON.stringify(result).includes(sentinel));
  assertBounded(result);
});

test('every byte split, including CRLF split, preserves exact framing and output', () => {
  const input = Buffer.from(`${sentinel}\n${EVENT_MARKER}\r\n${marker}${debug}\n${TRUNCATED_MARKER}\n`);
  const expected = collect(input);
  assert.equal(expected.events.length, 2);
  assert.equal(expected.truncated, true);
  for (let split = 0; split <= input.length; split++) {
    const collector = fresh();
    collector.push(input.subarray(0, split));
    collector.push(input.subarray(split));
    assert.deepEqual(collector.finish(), expected);
  }
});

test('many chunk sizes and mixed Buffer/string chunks produce identical bounded output', () => {
  const input = `${marker}${sentinel}\n${EVENT_MARKER}\r\n${raw}\n`.repeat(11);
  const expected = collect(input);
  for (const size of [...Array.from({ length: 97 }, (_, index) => index + 1), 127, 255, 1024, 4095, 4096, 4097]) {
    const collector = fresh();
    for (let offset = 0, part = 0; offset < input.length; offset += size, part++) {
      const chunk = input.slice(offset, offset + size);
      collector.push(part % 2 ? Buffer.from(chunk) : chunk);
      assertBounded(collector.snapshot());
    }
    assert.deepEqual(collector.finish(), expected);
  }
});

test('markers are not recognized before LF or from an unterminated final line', () => {
  const collector = fresh();
  collector.push(EVENT_MARKER);
  assert.equal(collector.snapshot().events.length, 0);
  const result = collector.finish();
  assert.equal(result.branch_evidence, 'inconclusive');
  assert.equal(result.counters.unterminated_lines, 1);
  assert.equal(result.counters.discarded_lines, 1);
  assert.equal(result.counters.lines_seen, 0);
  for (const tail of [TRUNCATED_MARKER, `${EVENT_MARKER}\r`, sentinel]) {
    const result = collect(tail);
    assert.equal(result.events.length, 0);
    assert.equal(result.truncated, false);
    assert.equal(result.counters.unterminated_lines, 1);
    assert.ok(!JSON.stringify(result).includes(sentinel));
  }
});

test('oversized prefixes and suffixes never admit a valid-looking truncated line', () => {
  for (const input of [
    `${'x'.repeat(CARRY_BYTES + 1)}${marker}`,
    `${EVENT_MARKER}${'x'.repeat(CARRY_BYTES + 1)}\n`,
    `${'x'.repeat(CARRY_BYTES)}${EVENT_MARKER}`,
    `${'x'.repeat(CARRY_BYTES + 1)}\r${marker}`,
  ]) {
    for (const size of [1, 17, CARRY_BYTES, input.length]) {
      const collector = fresh();
      for (let offset = 0; offset < input.length; offset += size) collector.push(input.slice(offset, offset + size));
      const result = collector.finish();
      assert.equal(result.events.length, 0);
      assert.equal(result.counters.oversized_lines, 1);
      assertBounded(result);
    }
  }
  const recovered = collect(`${'x'.repeat(CARRY_BYTES + 1)}${marker}${marker}`);
  assert.equal(recovered.events.length, 1);
  assert.equal(recovered.counters.oversized_lines, 1);
  assert.equal(collect(`${'x'.repeat(CARRY_BYTES)}\n`).counters.oversized_lines, 0);
  assert.equal(collect(`${'x'.repeat(CARRY_BYTES + 1)}\n`).counters.oversized_lines, 1);
});

test('huge Buffer and string input retain no more than a 4 KiB carry', () => {
  for (const input of ['x'.repeat(2 * 1024 * 1024), Buffer.alloc(2 * 1024 * 1024, 0x78)]) {
    const collector = fresh();
    collector.push(input);
    collector.push(EVENT_MARKER);
    const partial = collector.snapshot();
    assert.equal(partial.events.length, 0);
    assert.equal(partial.counters.carry_peak_bytes, CARRY_BYTES);
    const final = collector.finish();
    assert.equal(final.counters.oversized_lines, 1);
    assert.equal(final.counters.unterminated_lines, 1);
    assertBounded(final);
  }
});

test('ANSI and control-contaminated marker lines are discarded through their delimiter', () => {
  const lines = [
    `\x1b[31m${EVENT_MARKER}\x1b[0m`, EVENT_MARKER.replace('connect', 'con\x1b[0mnect'),
    `${sentinel}\r${EVENT_MARKER}`, `${EVENT_MARKER}\r${EVENT_MARKER}`,
    `${EVENT_MARKER}\r\r`, `${EVENT_MARKER}\x00`, `\x1b]0;${sentinel}\x07${EVENT_MARKER}`,
    ...Array.from({ length: 33 }, (_, index) => index === 32 ? 127 : index)
      .filter(code => code !== 10)
      .map(code => EVENT_MARKER.replace('connect', `con${String.fromCharCode(code)}nect`)),
  ];
  for (const line of lines) {
    const result = collect(`${line}\n`);
    assert.equal(result.events.length, 0);
    assert.equal(result.counters.invalid_lines, 1);
    assert.equal(result.counters.ansi_control_lines, 1);
    assert.ok(!JSON.stringify(result).includes(sentinel));
  }
  assert.equal(collect(`${EVENT_MARKER}\r\n`).events.length, 1);
});

test('malformed UTF-8 and multibyte lookalikes cannot join into a marker at any byte split', () => {
  const bytes = Buffer.concat([
    Buffer.from(EVENT_MARKER.slice(0, 12)), Buffer.from([0xc0, 0x80, 0xff, 0x80]),
    Buffer.from(`${EVENT_MARKER.slice(12)}\n${marker}`),
  ]);
  const expected = collect(bytes);
  assert.equal(expected.events.length, 1);
  assert.equal(expected.counters.invalid_lines, 1);
  for (let split = 0; split <= bytes.length; split++) {
    const collector = fresh();
    collector.push(bytes.subarray(0, split));
    collector.push(bytes.subarray(split));
    assert.deepEqual(collector.finish(), expected);
  }
  for (const line of [
    EVENT_MARKER.replace('10055', '１００５５'), `${EVENT_MARKER}\u2028`,
    EVENT_MARKER.replace('connect', 'con\u200bnect'), `\ufeff${EVENT_MARKER}`,
    `\ud800${EVENT_MARKER}`, `💥${EVENT_MARKER}`,
  ]) {
    assert.equal(collect(`${line}\n`).events.length, 0);
    assert.deepEqual(collect(`${line}\n`), collect(Buffer.from(`${line}\n`)));
  }
});

test('untrusted chunk types are not coerced and poison partial input until LF', () => {
  let coerced = false;
  for (const bad of [undefined, null, 7, Symbol(sentinel), {
    toString() { coerced = true; throw new Error(sentinel); },
  }]) {
    const collector = fresh();
    collector.push(EVENT_MARKER.slice(0, 10));
    collector.push(bad);
    collector.push(`${EVENT_MARKER.slice(10)}\n${marker}`);
    const result = collector.finish();
    assert.equal(result.events.length, 1);
    assert.equal(result.counters.invalid_chunks, 1);
    assert.equal(result.counters.invalid_lines, 1);
    assertBounded(result);
  }
  assert.equal(coerced, false);
});

test('sixteen events is a hard global cap with bounded overflow counts', () => {
  const result = collect(marker.repeat(20_000));
  assert.equal(result.events.length, EVENT_LIMIT);
  assert.equal(result.counters.matched_lines, 20_000);
  assert.equal(result.counters.dropped_events, 20_000 - EVENT_LIMIT);
  assert.equal(result.truncated, true);
  assert.equal(result.branch_evidence, 'positive-only');
  assertBounded(result);
});

test('fixed upstream truncation marker supplies truncation evidence without an event', () => {
  const result = collect(`${TRUNCATED_MARKER}\n${TRUNCATED_MARKER}\r\n`);
  assert.equal(result.truncated, true);
  assert.equal(result.events.length, 0);
  assert.equal(result.branch_evidence, 'inconclusive');
  assert.equal(result.counters.truncation_markers, 2);
  assert.equal(result.counters.dropped_events, 0);
  for (const input of [`x${TRUNCATED_MARKER}\n`, `${TRUNCATED_MARKER}x\n`, `\x1b[0m${TRUNCATED_MARKER}\n`]) {
    assert.equal(collect(input).truncated, false);
  }
});

test('only exact terminated unavailable markers set explicit unavailable evidence without events', () => {
  assert.equal(fresh().snapshot().probe_unavailable, false);
  const input = Buffer.from(`${UNAVAILABLE_MARKER}\n${UNAVAILABLE_MARKER}\r\n`);
  const expected = collect(input);
  assert.equal(expected.probe_unavailable, true);
  assert.equal(expected.counters.unavailable_markers, 2);
  assert.equal(expected.events.length, 0);
  assert.equal(expected.branch_evidence, 'inconclusive');
  assert.equal(expected.truncated, false);
  assertBounded(expected);
  for (let split = 0; split <= input.length; split++) {
    const collector = fresh();
    collector.push(input.subarray(0, split));
    collector.push(input.subarray(split));
    assert.deepEqual(collector.finish(), expected);
  }
  for (const input of [
    UNAVAILABLE_MARKER, `${UNAVAILABLE_MARKER}\r`, `x${UNAVAILABLE_MARKER}\n`,
    `${UNAVAILABLE_MARKER}x\n`, `${UNAVAILABLE_MARKER} ${sentinel}\n`,
    `${sentinel}${UNAVAILABLE_MARKER}\n`, `${UNAVAILABLE_MARKER.toUpperCase()}\n`,
    `\x1b[0m${UNAVAILABLE_MARKER}\n`, `${UNAVAILABLE_MARKER}\x00\n`,
    `${UNAVAILABLE_MARKER.replace('unavailable', 'un\u200bavailable')}\n`,
    `${'x'.repeat(CARRY_BYTES)}${UNAVAILABLE_MARKER}\n`,
  ]) {
    const result = collect(input);
    assert.equal(result.probe_unavailable, false);
    assert.equal(result.counters.unavailable_markers, 0);
    assert.equal(result.events.length, 0);
    assert.ok(!JSON.stringify(result).includes(sentinel));
    assertBounded(result);
  }
  assert.equal(recognizeChromiumConnectLine(UNAVAILABLE_MARKER), false);
});

test('only exact framed setup-refused markers distinguish setup refusal without an event', () => {
  assert.equal(fresh().snapshot().setup_refused, false);
  const input = Buffer.from(`${SETUP_REFUSED_MARKER}\n${SETUP_REFUSED_MARKER}\r\n`);
  const expected = collect(input);
  assert.equal(expected.setup_refused, true);
  assert.equal(expected.counters.setup_refused_markers, 2);
  assert.equal(expected.events.length, 0);
  assert.equal(expected.branch_evidence, 'inconclusive');
  assert.equal(expected.probe_unavailable, false);
  assert.equal(expected.truncated, false);
  assertBounded(expected);
  for (let split = 0; split <= input.length; split++) {
    const collector = fresh();
    collector.push(input.subarray(0, split));
    collector.push(input.subarray(split));
    assert.deepEqual(collector.finish(), expected);
  }
  for (const input of [
    SETUP_REFUSED_MARKER, `${SETUP_REFUSED_MARKER}\r`, `x${SETUP_REFUSED_MARKER}\n`,
    `${SETUP_REFUSED_MARKER}x\n`, `${SETUP_REFUSED_MARKER} ${sentinel}\n`,
    `${sentinel}${SETUP_REFUSED_MARKER}\n`, `${SETUP_REFUSED_MARKER.toUpperCase()}\n`,
    `\x1b[0m${SETUP_REFUSED_MARKER}\n`, `${SETUP_REFUSED_MARKER}\x00\n`,
    `${SETUP_REFUSED_MARKER.replace('refused', 're\u200bfused')}\n`,
    `${'x'.repeat(CARRY_BYTES)}${SETUP_REFUSED_MARKER}\n`,
    Buffer.concat([Buffer.from([0xff, 0x80]), Buffer.from(`${SETUP_REFUSED_MARKER}\n`)]),
  ]) {
    const result = collect(input);
    assert.equal(result.setup_refused, false);
    assert.equal(result.counters.setup_refused_markers, 0);
    assert.equal(result.events.length, 0);
    assert.ok(!JSON.stringify(result).includes(sentinel));
    assertBounded(result);
  }
  assert.equal(recognizeChromiumConnectLine(SETUP_REFUSED_MARKER), false);
});

test('receipt times are relative safe integer milliseconds, never logged timestamps', () => {
  const times = [1_800_000_000_000.25, 1_800_000_000_005.99, 1_800_000_000_010.25];
  const collector = createStderrCollector({ now: () => times.shift() });
  collector.push(marker);
  collector.push(marker);
  const result = collector.finish();
  assert.deepEqual(result.events.map(event => event.receipt_ms), [5, 10]);
  assert.ok(!JSON.stringify(result).includes('1800000000000'));
  assertBounded(result);
});

test('hostile clocks cannot emit non-finite, negative, unsafe, coerced or error data', () => {
  let coerced = false;
  const hostileValues = [NaN, Infinity, -Infinity, -1, Number.MAX_SAFE_INTEGER + 1, sentinel,
    undefined, null, 1n, Symbol(sentinel), { valueOf() { coerced = true; throw new Error(sentinel); } }];
  for (const value of hostileValues) {
    const collector = createStderrCollector({ now: () => value });
    collector.push(marker);
    const result = collector.finish();
    assert.equal(result.events[0].receipt_ms, 0);
    assert.equal(result.counters.clock_faults, 1);
    assertBounded(result);
    assert.ok(!JSON.stringify(result).includes(sentinel));
  }
  const times = [100, 125.9, ...hostileValues, 90, 120, 130];
  const collector = createStderrCollector({ now: () => times.shift() });
  collector.push(marker.repeat(times.length));
  const result = collector.finish();
  assert.deepEqual(result.events.map(event => event.receipt_ms), [25, ...hostileValues.map(() => 25), 25, 25, 30]);
  assert.equal(result.counters.clock_faults, hostileValues.length + 2);
  assert.equal(coerced, false);
  assertBounded(result);
  assert.ok(!JSON.stringify(result).includes(sentinel));
  const throws = createStderrCollector({ now() { throw new Error(sentinel); } });
  throws.push(marker);
  assert.equal(throws.finish().events[0].receipt_ms, 0);
  assert.ok(!JSON.stringify(throws.finish()).includes(sentinel));
});

test('maximal allowed receipt and maximal schema remain below 8 KiB', () => {
  let calls = 0;
  const collector = createStderrCollector({ now: () => calls++ === 0 ? 0 : Number.MAX_SAFE_INTEGER });
  collector.push(marker.repeat(EVENT_LIMIT));
  const result = collector.finish();
  assert.ok(result.events.every(event => event.receipt_ms === Number.MAX_SAFE_INTEGER));
  assertBounded(result);
  const worstCase = structuredClone(result);
  for (const key of Object.keys(worstCase.counters)) worstCase.counters[key] = 0xffffffff;
  assert.ok(Buffer.byteLength(JSON.stringify(worstCase), 'utf8') < ARTIFACT_BYTES);
});

test('snapshots detach mutable state and finish is terminal and idempotent', () => {
  const collector = fresh();
  collector.push(marker);
  const first = collector.snapshot();
  first.events[0].receipt_ms = sentinel;
  first.events.push({ secret: sentinel });
  first.counters.bytes_seen = -999;
  first.branch_evidence = sentinel;
  const final = collector.finish();
  assert.equal(final.events.length, 1);
  assert.equal(final.complete, true);
  assert.ok(!JSON.stringify(final).includes(sentinel));
  collector.push(`${sentinel}\n${marker}`);
  assert.deepEqual(collector.finish(), final);
  assert.deepEqual(collector.snapshot(), final);
  assertBounded(final);
});

test('sentinel-bearing malformed lines never escape through any public result', () => {
  const collector = fresh();
  for (const input of [
    `${sentinel}\n`, `${sentinel}${marker}`, `${EVENT_MARKER}${sentinel}\n`,
    `\x1b[31m${sentinel}\n`, `${'x'.repeat(CARRY_BYTES)}${sentinel}\n`,
    `${raw} ${sentinel}\n`, `${debug} ${sentinel}\n`,
  ]) {
    assert.equal(collector.push(input), undefined);
    assert.ok(!JSON.stringify(collector.snapshot()).includes(sentinel));
  }
  collector.push(marker);
  collector.push(sentinel);
  const result = collector.finish();
  assert.equal(result.events.length, 1);
  assert.ok(!JSON.stringify(result).includes(sentinel));
  assert.ok(!JSON.stringify(result).includes('8765'));
  assert.ok(!JSON.stringify(result).includes('030405'));
  assert.ok(!JSON.stringify(result).includes('tcp_socket_win'));
  assertBounded(result);
});
