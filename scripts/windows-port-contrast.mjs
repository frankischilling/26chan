import { spawn } from 'node:child_process';
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { arch, release } from 'node:os';
import { armName, binding, checkEnvironment, demand, qualifyPair, runnerIdentity } from './windows-port-contrast-core.mjs';
import { checkedFile, contrastDirectory, installedBrowser, verifiedFixture } from '../tests/helpers/windows-port-contrast.js';
import { copyContrastNetlogs, verifyAdmittedCopies } from './windows-port-contrast-netlog.mjs';
import { normalizeChildOutcome } from './windows-theme-stderr.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
function json(file, maximum = 65536) { checkedFile(file, maximum); return JSON.parse(readFileSync(file, 'utf8')); }
function save(file, value) { const body = JSON.stringify(value); demand(Buffer.byteLength(body) <= 65536); writeFileSync(file, body, { flag: 'wx' }); }

async function admit() {
  const run = binding();
  demand(/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(process.env.GITHUB_REPOSITORY ?? ''));
  demand(['pull_request', 'workflow_dispatch'].includes(process.env.GITHUB_EVENT_NAME));
  const api = `https://api.github.com/repos/${process.env.GITHUB_REPOSITORY}/actions`;
  const get = async suffix => {
    const response = await fetch(`${api}/${suffix}`, { headers: {
      Authorization: `Bearer ${process.env.GH_TOKEN}`, Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28',
    }, signal: AbortSignal.timeout(15000) });
    demand(response.ok && !response.headers.get('link')?.includes('rel="next"'));
    const body = await response.text(); demand(Buffer.byteLength(body) <= 1048576); return JSON.parse(body);
  };
  const current = await get(`runs/${run.run}`);
  demand(current.head_sha === run.head && current.run_attempt === 1 && Number.isSafeInteger(current.workflow_id));
  const runs = await get(`workflows/${current.workflow_id}/runs?head_sha=${run.head}&per_page=100`);
  demand(Array.isArray(runs.workflow_runs) && runs.total_count === runs.workflow_runs.length);
  demand(runs.workflow_runs.length === 1 && String(runs.workflow_runs[0].id) === run.run);
  console.log('One matched pair admitted for this exact head and first attempt.');
}
function fixture() {
  demand(process.platform === 'win32');
  const source = path.join(root, 'target/debug/examples/visual-fixtures.exe');
  const info = checkedFile(source);
  const directory = path.join(root, '.local/windows-port-contrast-fixture');
  mkdirSync(directory, { recursive: true });
  copyFileSync(source, path.join(directory, 'visual-fixtures.exe'), 1);
  save(path.join(directory, 'manifest.json'), { schema: 1, binding: binding(), fixture: 'visual-fixtures.exe',
    fixture_sha256: info.sha256, fixture_bytes: info.bytes, rust: '1.94.0' });
  console.log(`Shared fixture SHA-256: ${info.sha256}`);
}
async function run() {
  checkEnvironment(process.env, process.platform);
  const runBinding = binding();
  runnerIdentity(process.env, release(), arch(), process.version);
  verifiedFixture();
  const browser = installedBrowser();
  const directory = contrastDirectory();
  mkdirSync(path.dirname(directory), { recursive: true });
  mkdirSync(directory); // Reject stale proofs or a second arm execution.
  const env = { ...process.env, WINDOWS_PORT_CONTRAST_OWNED: '1', WINDOWS_PORT_BROWSER_SHA256: browser.sha256 };
  const launcher = path.join(root, 'scripts/windows-theme-stderr.mjs');
  const args = [launcher, '--shard', '7/8', '--output', 'test-results/windows-themes-7'];
  let original;
  try {
    const child = spawn(process.execPath, args, { cwd: root, env, shell: false, stdio: 'inherit' });
    let spawnError = false;
    child.once('error', () => { spawnError = true; });
    original = await new Promise(resolve => child.once('close', (code, signal) => resolve(normalizeChildOutcome(code, signal, spawnError))));
  } catch { original = normalizeChildOutcome(null, null, true); }
  try { save(path.join(directory, 'original.json'), { schema: 1, binding: runBinding, arm: armName(env.WINDOWS_PORT_ARM), original }); }
  catch { console.error('Original exit evidence could not be retained.'); }
  try {
    const netlog = copyContrastNetlogs(path.join(root, 'test-results/windows-themes-7/netlogs'), path.join(directory, 'netlogs'));
    save(path.join(directory, 'netlog-status.json'), netlog);
  } catch {
    try { save(path.join(directory, 'netlog-status.json'), { schema: 1, status: 'unavailable', files: 0, command_line_redactions: 0, event_chains_preserved: false }); } catch {}
    console.error('Port contrast admitted NetLog evidence unavailable.');
  }
  // The pair checker is separate. Missing diagnostics cannot turn failure green.
  process.exitCode = original.code;
}
function qualify() {
  const identity = binding();
  const readArm = arm => {
    const directory = path.join(root, 'test-results', `windows-port-contrast-${arm}`);
    const evidence = json(path.join(directory, 'evidence.json'));
    const outcome = json(path.join(directory, 'original.json'), 4096);
    demand(outcome.schema === 1 && outcome.arm === arm && JSON.stringify(outcome.binding) === JSON.stringify(identity));
    const netlog = json(path.join(directory, 'netlog-status.json'), 4096);
    verifyAdmittedCopies(path.join(directory, 'netlogs'), netlog);
    return { ...evidence, original: outcome.original, netlog };
  };
  const result = qualifyPair(readArm('enabled'), readArm('disabled'), identity);
  const directory = path.join(root, 'test-results/windows-port-contrast-pair');
  mkdirSync(directory);
  save(path.join(directory, 'summary.json'), result);
  console.log(`Port contrast pair: ${result.interpretation}; enabled exit ${result.enabled_exit}, disabled exit ${result.disabled_exit}.`);
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    demand(process.argv.length === 3 && ['admit', 'fixture', 'run', 'qualify'].includes(process.argv[2]));
    await ({ admit, fixture, run, qualify })[process.argv[2]]();
  } catch { console.error('Port contrast operation rejected or evidence unavailable.'); process.exitCode = 1; }
}
