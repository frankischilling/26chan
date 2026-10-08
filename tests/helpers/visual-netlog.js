import { randomUUID } from 'node:crypto';
import { lstat, mkdir, readFile, readdir, realpath, rename, unlink } from 'node:fs/promises';
import path from 'node:path';

// Keep both original paths private; mutable caller fields cannot redirect I/O.
const ownedPaths = new WeakMap();

function originalPaths(plan) {
  const paths = ownedPaths.get(plan);
  if (!paths) throw new Error('Unowned diagnostic plan');
  return paths;
}

async function unlinkedDirectory(directory) {
  return await realpath(directory) === directory && (await lstat(directory)).isDirectory();
}

export const NETLOG_CAPTURE_MIB = 8;
export const NETLOG_ARTIFACT_BYTES = 12 * 1024 * 1024;
// At most four accepted logs / 48 MiB per shard; eight shards total 32 / 384 MiB.
export const NETLOG_FILES_PER_SHARD = 4;

export function visualNetlogEnabled(platform, env) {
  return platform === 'win32' && env.VISUAL_FIXTURE_SERVER === '1' && env.WINDOWS_VISUAL_NETLOG === '1';
}

export function visualNetlogPlan(options, outputDir, workerIndex, id = randomUUID()) {
  if (!Number.isSafeInteger(workerIndex) || workerIndex < 0 || !/^[a-zA-Z0-9-]+$/.test(id)) throw new Error('Invalid diagnostic identity');
  // Do not override caller-supplied logging settings or enable sensitive logging.
  if ((options.args ?? []).some(arg => /^--(?:log-net-log|net-log-)/.test(arg))) return null;
  const name = `worker-${workerIndex}-${id}.json`;
  const pending = path.resolve(outputDir, 'netlogs-pending', name);
  const accepted = path.resolve(outputDir, 'netlogs', name);
  const plan = { pending, accepted, options: { ...options, args: [...(options.args ?? []),
    `--log-net-log=${pending}`, '--net-log-capture-mode=Default', `--net-log-max-size-mb=${NETLOG_CAPTURE_MIB}`] } };
  ownedPaths.set(plan, { pending, accepted });
  return plan;
}

export function acceptableNetlogSize(bytes) {
  return Number.isSafeInteger(bytes) && bytes > 0 && bytes <= NETLOG_ARTIFACT_BYTES;
}

export async function prepareVisualNetlog(plan) {
  await mkdir(path.dirname(originalPaths(plan).pending), { recursive: true });
}

export async function acceptVisualNetlog(plan) {
  // Chromium's bounded writer uses temporary chunks and an approximate 8 MiB
  // budget. Only complete, post-close JSON passing this hard limit is uploaded.
  // Sources: chromium/src/net/log/file_net_log_observer.h and
  // services/network/public/cpp/network_switches.cc (Default capture mode).
  const { pending, accepted } = originalPaths(plan);
  const directory = path.dirname(accepted);
  // Validate both parents before reading the capture or creating an artifact
  // directory. Canonical comparison rejects symlink/junction traversal.
  if (!await unlinkedDirectory(path.dirname(pending)) ||
      !await unlinkedDirectory(path.dirname(directory))) return 'unsafe-path';
  const file = await lstat(pending);
  if (!file.isFile() || file.isSymbolicLink() || file.nlink !== 1) return 'unsafe-path';
  if (!acceptableNetlogSize(file.size)) return 'size-limit';
  try { await mkdir(directory); }
  catch (error) { if (error.code !== 'EEXIST') throw error; }
  if (!await unlinkedDirectory(directory)) return 'unsafe-path';
  // Do not replace an existing artifact, including a link at the final path.
  try { await lstat(accepted); return 'existing-path'; }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
  const parsed = JSON.parse(await readFile(pending, 'utf8'));
  if (!parsed || typeof parsed !== 'object' || !Array.isArray(parsed.events) || !parsed.constants) return 'incomplete';
  if ((await readdir(directory)).filter(name => /^worker-[0-9]+-[a-zA-Z0-9-]+\.json$/.test(name)).length >= NETLOG_FILES_PER_SHARD) return 'file-limit';
  // The existing themes runner is serial (one worker); no concurrent admission.
  await rename(pending, accepted);
  return 'accepted';
}

// A test-scoped auto fixture calls this without depending on context/browser.
// Its teardown runs even when subsequent context setup or the test body fails.
export async function trackVisualNetlogFailure(state, use, info) {
  try { await use(); }
  finally {
    if (info.status !== 'skipped' && info.status !== info.expectedStatus) state.failed = true;
  }
}

export async function finishVisualNetlog(plan, state) {
  const { pending } = originalPaths(plan);
  if (state.failed) return acceptVisualNetlog(plan);
  // Browser closure precedes this call. Only this worker's exact pending file
  // is removed; sibling captures and Chromium's temporary files are untouched.
  try {
    // Never traverse a symlink/reparse directory or remove a linked capture.
    const directory = path.dirname(pending);
    if (!await unlinkedDirectory(directory)) return 'unsafe-path';
    const file = await lstat(pending);
    if (!file.isFile() || file.isSymbolicLink() || file.nlink !== 1) return 'unsafe-path';
    await unlink(pending);
  }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
  return 'discarded-success';
}
