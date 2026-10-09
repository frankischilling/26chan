import { lstat, mkdir, realpath, writeFile } from 'node:fs/promises';
import path from 'node:path';
// Fixture listener checkpoints only. Never infer browser socket lifetimes.
export const MARKER = '[owned-fixture-lifecycle] ';
export const LINE_BYTES = 2048;
export const RECORD_LIMIT = 82;
export const ARTIFACT_BYTES = 65536;
const LIMIT = 0xffffffff;
const KEYS = ['accepted', 'active', 'peak_active', 'completed', 'cancelled', 'http_parse', 'http_incomplete', 'http_timeout', 'http_other', 'accept_errors'];
const ENDED = ['completed', 'cancelled', 'http_parse', 'http_incomplete', 'http_timeout', 'http_other'];
const exact = (value, keys) => value && typeof value === 'object' && !Array.isArray(value)
  && Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
const count = value => Number.isInteger(value) && value >= 0 && value <= LIMIT;
const power = value => value > 0 && Number.isInteger(Math.log2(value));

export function validateCheckpoint(text) {
  if (typeof text !== 'string' || Buffer.byteLength(text) > LINE_BYTES) throw new Error('Invalid fixture checkpoint');
  const row = JSON.parse(text);
  // Reject duplicate keys, whitespace, alternate encodings and trailing data.
  if (JSON.stringify(row) !== text || !exact(row, ['schema_version', 'scope', 'boundary', 'sequence', 'counters', 'overflow', 'output_failed', 'complete', 'browser_lifetime'])
      || row.schema_version !== 1 || row.scope !== 'fixture-listeners' || row.browser_lifetime !== 'unavailable'
      || !['startup', 'accepted', 'ended', 'error', 'shutdown'].includes(row.boundary)
      || !Number.isInteger(row.sequence) || row.sequence < 1 || row.sequence > RECORD_LIMIT
      || !exact(row.counters, KEYS) || !KEYS.every(key => count(row.counters[key]))
      || !['overflow', 'output_failed', 'complete'].every(key => typeof row[key] === 'boolean')) throw new Error('Invalid fixture checkpoint');
  const c = row.counters;
  const ended = ENDED.reduce((sum, key) => sum + c[key], 0);
  if (row.boundary === 'startup' && (row.sequence !== 1 || KEYS.some(key => c[key] !== 0) || row.overflow || row.output_failed)) throw new Error('Invalid fixture startup');
  if (!row.overflow && (c.accepted !== c.active + ended || c.peak_active < c.active || c.peak_active > c.accepted)) throw new Error('Unreconciled fixture counts');
  if (row.boundary === 'accepted' && !power(c.accepted) || row.boundary === 'ended' && !power(ended)) throw new Error('Invalid fixture boundary');
  const complete = row.boundary === 'shutdown' && !row.overflow && !row.output_failed && c.active === 0 && c.accepted === ended;
  if (row.complete !== complete) throw new Error('Invalid fixture completeness');
  return row;
}

export function createFixtureLifecycleCollector() {
  const checkpoints = [];
  let carry = '', oversized = false, finished = false, invalid = false, truncated = false;
  let errorCheckpoints = 0;
  function line(text) {
    if (!text.startsWith(MARKER)) return;
    if (checkpoints.length >= RECORD_LIMIT) { invalid = true; truncated = true; return; }
    try {
      const row = validateCheckpoint(text.slice(MARKER.length));
      const previous = checkpoints.at(-1);
      if (row.sequence !== checkpoints.length + 1 || (!previous && row.boundary !== 'startup')
          || previous?.boundary === 'shutdown' || previous && (row.boundary === 'startup'
            || KEYS.filter(key => key !== 'active').some(key => row.counters[key] < previous.counters[key])
            || previous.overflow && !row.overflow || previous.output_failed && !row.output_failed)) throw new Error();
      if (row.boundary === 'error' && ++errorCheckpoints > 16) throw new Error();
      if (checkpoints.length >= RECORD_LIMIT) { truncated = true; return; }
      checkpoints.push(row);
    } catch { invalid = true; }
  }
  function consume(text) {
    for (const char of text) {
      if (char === '\n') {
        if (oversized) { if (carry.startsWith(MARKER)) { invalid = true; truncated = true; } }
        else line(carry.endsWith('\r') ? carry.slice(0, -1) : carry);
        carry = ''; oversized = false;
      } else if (carry.length < LINE_BYTES) carry += char;
      else oversized = true;
    }
  }
  return Object.freeze({
    push(chunk, test, result) {
      if (finished || test !== undefined || result !== undefined) return;
      // Pinned Playwright 1.62 prefixes EACH callback fragment and each line,
      // including continuation fragments. Remove only its two exact prefixes.
      if (typeof chunk !== 'string' || chunk.length > 1024 * 1024) { invalid = true; truncated = true; return; }
      const prefixes = ['[WebServer] ', '\x1b[2m[WebServer] \x1b[22m'];
      let start = 0;
      while (start < chunk.length) {
        let end = chunk.indexOf('\n', start);
        if (end === -1) end = chunk.length;
        const fragment = chunk.slice(start, end);
        const prefix = prefixes.find(prefix => fragment.startsWith(prefix));
        if (prefix) consume(fragment.slice(prefix.length) + (end < chunk.length ? '\n' : ''));
        else if (carry) { invalid = true; carry = ''; oversized = false; }
        start = end + 1;
      }
    },
    finish() {
      if (carry && (carry.startsWith(MARKER) || MARKER.startsWith(carry))) invalid = true;
      carry = ''; finished = true;
      const last = checkpoints.at(-1);
      const status = invalid || truncated ? 'invalid' : !last ? 'unavailable' : last.complete ? 'complete' : 'incomplete';
      return { schema_version: 1, scope: 'fixture-listeners', browser_lifetime: 'unavailable', status,
        invalid, truncated, checkpoints: checkpoints.map(row => structuredClone(row)) };
    },
  });
}

export async function saveFixtureLifecycleEvidence(outputDir, evidence) {
  if (typeof outputDir !== 'string' || !/^windows-themes-[1-8]$/.test(path.basename(outputDir))) throw new Error('Invalid fixture artifact directory');
  const directory = path.resolve(outputDir);
  // Reject linked ancestors before creating the final directory. Playwright
  // creates test-results before invoking reporters for this owned theme suite.
  const parent = path.dirname(directory);
  if (await realpath(parent) !== parent || !(await lstat(parent)).isDirectory()) throw new Error('Unsafe fixture artifact directory');
  try { await mkdir(directory); } catch (error) { if (error.code !== 'EEXIST') throw error; }
  if (await realpath(directory) !== directory || !(await lstat(directory)).isDirectory()) throw new Error('Unsafe fixture artifact directory');
  const body = JSON.stringify(evidence);
  if (Buffer.byteLength(body) > ARTIFACT_BYTES) throw new Error('Oversized fixture artifact');
  await writeFile(path.join(directory, 'fixture-lifecycle.json'), body, { flag: 'wx', mode: 0o600 });
}
