import { lstat, realpath, readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { CONTROL_SCHEMA, controlIdentity, SUMMARY_BYTES, TARGET, inspectControlNetlog, validControlCapture } from './windows-browser-control-core.mjs';
import { NETLOG_ARTIFACT_BYTES } from '../tests/helpers/visual-netlog.js';
import filter from './windows-theme-stderr-filter.cjs';

async function ownedJson(root, name, limit) {
  if (await realpath(root) !== root || !(await lstat(root)).isDirectory()) throw new Error('Unsafe evidence root');
  const filename = path.join(root, name), entry = await lstat(filename);
  if (!entry.isFile() || entry.isSymbolicLink() || entry.nlink !== 1 || entry.size < 1 || entry.size > limit) throw new Error('Invalid evidence file');
  return JSON.parse(await readFile(filename, 'utf8'));
}

// A saved filename alone is insufficient. The coordinator and an actual
// Playwright worker must each report their own installed early stderr probe.
export function qualifyOriginalTransport(record, original) {
  if (!record || !original || !['exit', 'signal'].includes(original.kind) ||
      record.schema_version !== 1 || record.complete !== true || record.truncated !== false ||
      record.setup_refused !== false || record.probe_unavailable !== false || record.stream_error !== false ||
      record.network_child_stderr_coverage !== 'unverified' || record.absent_match !== 'inconclusive' ||
      !Number.isSafeInteger(record.control_probe_coordinator_count) || record.control_probe_coordinator_count < 1 ||
      record.control_probe_coordinator_count > 0xffffffff ||
      !Number.isSafeInteger(record.control_probe_worker_count) || record.control_probe_worker_count < 1 ||
      record.control_probe_worker_count > 0xffffffff ||
      record.child?.kind !== original.kind || record.child.code !== original.code ||
      record.child.signal !== original.signal || !Array.isArray(record.events) ||
      record.events.length > filter.EVENT_LIMIT) return false;
  const counters = record.counters;
  if (!counters || !['bytes_seen', 'lines_seen', 'matched_lines', 'discarded_lines',
    'oversized_lines', 'invalid_lines', 'ansi_control_lines', 'unterminated_lines',
    'dropped_events', 'truncation_markers', 'unavailable_markers', 'setup_refused_markers',
    'invalid_chunks', 'clock_faults', 'carry_peak_bytes'].every(name =>
    Number.isSafeInteger(counters[name]) && counters[name] >= 0 && counters[name] <= 0xffffffff)) return false;
  if (counters.bytes_seen === 0 ||
      counters.lines_seen < record.control_probe_coordinator_count + record.control_probe_worker_count ||
      counters.matched_lines !== record.events.length ||
      ['oversized_lines', 'invalid_lines', 'ansi_control_lines', 'unterminated_lines',
        'dropped_events', 'truncation_markers', 'unavailable_markers', 'setup_refused_markers',
        'invalid_chunks', 'clock_faults'].some(name => counters[name] !== 0)) return false;
  return record.branch_evidence === (record.events.length ? 'positive-only' : 'inconclusive') &&
    record.events.every(event => event?.event === 'synchronous-connect-error' && event.os_error === 10055 &&
      Number.isSafeInteger(event.receipt_ms) && event.receipt_ms >= 0);
}

export function qualifySummary(summary, ownership, shard) {
  let identity;
  try { identity = controlIdentity(shard); } catch { return 'invalid-shard'; }
  if (summary?.shard !== shard || ownership?.shard !== shard ||
      summary?.suite !== identity.suite || ownership?.suite !== identity.suite) return 'shard-mismatch';
  if (summary?.schema !== CONTROL_SCHEMA || summary.phase !== 'finished' || summary.control !== 'ready' ||
      summary.package_version !== '1.62.0' || summary.control_closed !== true || summary.control_process_exited !== true ||
      summary.original_stderr_artifact !== 'saved' || !Number.isInteger(summary.original?.code) ||
      !['exit', 'signal'].includes(summary.original.kind)) return 'invalid-summary';
  if (ownership?.schema !== CONTROL_SCHEMA || ownership.phase !== 'finished' || ownership.hard_deadline !== false ||
      ownership.forced_cleanup !== false || ownership.tree_exited !== true || ownership.cleanup_verified !== true ||
      ownership.original_coordinator_exit !== summary.original.code) return 'unverified-cleanup';
  if (summary.trigger === 'not-observed') {
    return summary.navigation === 'not-triggered' && summary.netlog === 'accepted' && summary.conclusion === 'inconclusive' &&
      [summary.trigger_receipt_ms, summary.dispatch_ms, summary.started_receipt_ms, summary.completion_receipt_ms].every(value => value === null)
      ? 'no-trigger' : 'invalid-no-trigger';
  }
  if (summary.trigger !== 'primary-no-buffer-space' || summary.netlog !== 'accepted' ||
      !['http-200', 'unexpected-response', 'failed-or-timeout'].includes(summary.navigation)) return 'incomplete-control';
  const times = [summary.trigger_receipt_ms, summary.dispatch_ms, summary.started_receipt_ms, summary.completion_receipt_ms];
  if (!times.every(value => Number.isSafeInteger(value) && value >= 0) || times.some((value, index) => index && value < times[index - 1]) ||
      summary.trigger_to_completion_ms !== times[3] - times[0]) return 'invalid-timing';
  const positive = summary.navigation === 'http-200' && summary.status === 200 && summary.tcp?.successful_tcp === 'observed';
  if (summary.conclusion !== (positive ? 'control-connected-after-observed-failure' : 'inconclusive')) return 'invalid-conclusion';
  return 'triggered';
}
export async function checkEvidence(root, shard) {
  const identity = controlIdentity(shard);
  if (path.basename(root) !== path.basename(identity.output) || path.basename(path.dirname(root)) !== 'test-results') {
    throw new Error('Evidence shard path mismatch');
  }
  const summary = await ownedJson(root, 'summary.json', SUMMARY_BYTES);
  const ownership = await ownedJson(root, 'ownership.json', SUMMARY_BYTES);
  const outcome = qualifySummary(summary, ownership, shard);
  if (outcome !== 'no-trigger' && outcome !== 'triggered') throw new Error('Diagnostic summary did not qualify');
  const original = path.join(path.dirname(root), path.basename(identity.originalOutput));
  const stderr = await ownedJson(original, 'transport-stderr.json', filter.ARTIFACT_BYTES);
  if (!qualifyOriginalTransport(stderr, summary.original)) throw new Error('Original stderr probe was unavailable or incomplete');
  const log = await ownedJson(root, 'control-netlog.json', NETLOG_ARTIFACT_BYTES);
  if (!validControlCapture(log) || JSON.stringify(inspectControlNetlog(log)) !== JSON.stringify(summary.tcp)) {
    throw new Error('Control NetLog was missing, incomplete or inconsistent');
  }
  if (outcome === 'no-trigger' && log.events.some(event =>
    event?.type === log.constants.logEventTypes.URL_REQUEST_START_JOB && event.params?.url === TARGET)) {
    throw new Error('Control request exists without a trigger');
  }
  return outcome;
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length !== 2) throw new Error('Fixed evidence root required');
    const identity = controlIdentity(process.env.THEME_SHARD);
    const outcome = await checkEvidence(path.resolve(fileURLToPath(new URL('../', import.meta.url)), identity.output), identity.shard);
    console.log(`Browser control evidence: ${outcome}; original outcome is recorded separately.`);
  } catch { console.error('Browser control evidence did not qualify.'); process.exitCode = 1; }
}
