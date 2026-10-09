import { lstat, realpath, readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { CONTROL_SCHEMA, controlIdentity, SUMMARY_BYTES, inspectControlNetlog } from './windows-browser-control-core.mjs';
import { NETLOG_ARTIFACT_BYTES } from '../tests/helpers/visual-netlog.js';

async function ownedJson(root, name, limit) {
  if (await realpath(root) !== root || !(await lstat(root)).isDirectory()) throw new Error('Unsafe evidence root');
  const filename = path.join(root, name), entry = await lstat(filename);
  if (!entry.isFile() || entry.isSymbolicLink() || entry.nlink !== 1 || entry.size < 1 || entry.size > limit) throw new Error('Invalid evidence file');
  return JSON.parse(await readFile(filename, 'utf8'));
}
export function qualifySummary(summary, ownership, shard) {
  try { controlIdentity(shard); } catch { return 'invalid-shard'; }
  if (summary?.shard !== shard || ownership?.shard !== shard) return 'shard-mismatch';
  if (summary?.schema !== CONTROL_SCHEMA || summary.phase !== 'finished' || summary.control !== 'ready' ||
      summary.package_version !== '1.62.0' || summary.control_closed !== true || summary.control_process_exited !== true ||
      summary.original_stderr_artifact !== 'saved' || !Number.isInteger(summary.original?.code) ||
      !['exit', 'signal', 'spawn-error', 'unavailable', 'diagnostic-setup-refused'].includes(summary.original.kind)) return 'invalid-summary';
  if (ownership?.schema !== CONTROL_SCHEMA || ownership.phase !== 'finished' || ownership.hard_deadline !== false ||
      ownership.forced_cleanup !== false || ownership.tree_exited !== true || ownership.cleanup_verified !== true ||
      ownership.original_coordinator_exit !== summary.original.code) return 'unverified-cleanup';
  if (summary.trigger === 'not-observed') {
    return summary.navigation === 'not-triggered' && summary.netlog === 'discarded-success' && summary.conclusion === 'inconclusive' &&
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
  if (outcome === 'no-trigger') {
    try { await lstat(path.join(root, 'control-netlog.json')); throw new Error('Unexpected capture'); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
  } else if (outcome === 'triggered') {
    const log = await ownedJson(root, 'control-netlog.json', NETLOG_ARTIFACT_BYTES);
    if (!log.constants || !Array.isArray(log.events) || JSON.stringify(inspectControlNetlog(log)) !== JSON.stringify(summary.tcp)) throw new Error('Capture does not match summary');
  } else throw new Error('Diagnostic evidence did not qualify');
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
