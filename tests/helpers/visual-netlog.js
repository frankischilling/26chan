import { randomUUID } from 'node:crypto';
import { mkdir, readFile, readdir, rename, stat } from 'node:fs/promises';
import path from 'node:path';

export const NETLOG_CAPTURE_MIB = 8;
export const NETLOG_ARTIFACT_BYTES = 12 * 1024 * 1024;
// Four existing theme shards: at most 16 accepted logs / 192 MiB per job.
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
  return { pending, accepted, options: { ...options, args: [...(options.args ?? []),
    `--log-net-log=${pending}`, '--net-log-capture-mode=Default', `--net-log-max-size-mb=${NETLOG_CAPTURE_MIB}`] } };
}

export function acceptableNetlogSize(bytes) {
  return Number.isSafeInteger(bytes) && bytes > 0 && bytes <= NETLOG_ARTIFACT_BYTES;
}

export async function prepareVisualNetlog(plan) {
  await mkdir(path.dirname(plan.pending), { recursive: true });
}

export async function acceptVisualNetlog(plan) {
  // Chromium's bounded writer uses temporary chunks and an approximate 8 MiB
  // budget. Only complete, post-close JSON passing this hard limit is uploaded.
  // Sources: chromium/src/net/log/file_net_log_observer.h and
  // services/network/public/cpp/network_switches.cc (Default capture mode).
  if (!acceptableNetlogSize((await stat(plan.pending)).size)) return 'size-limit';
  const parsed = JSON.parse(await readFile(plan.pending, 'utf8'));
  if (!parsed || typeof parsed !== 'object' || !Array.isArray(parsed.events) || !parsed.constants) return 'incomplete';
  const directory = path.dirname(plan.accepted);
  await mkdir(directory, { recursive: true });
  if ((await readdir(directory)).filter(name => /^worker-[0-9]+-[a-zA-Z0-9-]+\.json$/.test(name)).length >= NETLOG_FILES_PER_SHARD) return 'file-limit';
  // The existing themes runner is serial (one worker); no concurrent admission.
  await rename(plan.pending, plan.accepted);
  return 'accepted';
}
