import { readFileSync, lstatSync, realpathSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { armName, binding, browserProofStage, contrastEnabled, contrastLaunchOptions, demand, sha256,
  verifyCommandLine, validateManifest, PLAYWRIGHT_VERSION, BROWSER_VERSION, BROWSER_REVISION, CORE_BUNDLE_SHA256 } from '../../scripts/windows-port-contrast-core.mjs';

const root = fileURLToPath(new URL('../../', import.meta.url));
const require = createRequire(import.meta.url);
export function contrastDirectory(env = process.env) {
  return path.join(root, 'test-results', `windows-port-contrast-${armName(env.WINDOWS_PORT_ARM)}`);
}
function owned(env) {
  demand(process.platform === 'win32' && env.WINDOWS_PORT_CONTRAST_OWNED === '1' && contrastEnabled(env));
  binding(env); armName(env.WINDOWS_PORT_ARM);
}
export function checkedFile(file, maximum = 536870912) {
  try {
    const info = lstatSync(file);
    demand(info.isFile() && !info.isSymbolicLink() && info.size > 0 && info.size <= maximum);
    demand(realpathSync(file).toLowerCase() === path.resolve(file).toLowerCase());
    return { bytes: info.size, sha256: sha256(readFileSync(file)) };
  } catch { throw new Error('Port contrast file verification unavailable'); }
}
export function installedBrowser() {
  demand(require('playwright-core/package.json').version === PLAYWRIGHT_VERSION);
  demand(require('playwright/package.json').version === PLAYWRIGHT_VERSION && require('@playwright/test/package.json').version === PLAYWRIGHT_VERSION);
  const packageRoot = path.dirname(require.resolve('playwright-core/package.json'));
  demand(checkedFile(path.join(packageRoot, 'lib/coreBundle.js'), 8388608).sha256 === CORE_BUNDLE_SHA256);
  const pins = JSON.parse(readFileSync(path.join(packageRoot, 'browsers.json'), 'utf8'));
  const matches = pins.browsers.filter(browser => browser.name === 'chromium-headless-shell');
  demand(matches.length === 1 && matches[0].revision === BROWSER_REVISION && matches[0].browserVersion === BROWSER_VERSION);
  const executable = require('playwright-core/lib/coreBundle').registry.registry.findExecutable('chromium-headless-shell').executablePath();
  demand(path.isAbsolute(executable));
  return { executable, ...checkedFile(executable) };
}
export function verifiedFixture(env = process.env) {
  const directory = path.join(root, '.local', 'windows-port-contrast-fixture');
  const manifestFile = path.join(directory, 'manifest.json');
  checkedFile(manifestFile, 4096);
  const manifest = validateManifest(JSON.parse(readFileSync(manifestFile, 'utf8')), binding(env));
  const executable = path.join(directory, 'visual-fixtures.exe');
  const actual = checkedFile(executable);
  demand(actual.sha256 === manifest.fixture_sha256 && actual.bytes === manifest.fixture_bytes);
  return { executable, manifest };
}
export function contrastServerCommand(original, env = process.env) {
  if (!contrastEnabled(env)) return original;
  owned(env);
  demand(original === 'cargo run -p board-public --example visual-fixtures --locked');
  const { executable } = verifiedFixture(env);
  // Fixed artifact location, no caller-supplied executable, shell syntax or args.
  demand(!/["\r\n%&|<>^]/.test(executable));
  return `"${executable}"`;
}
export function portContrastOptions(options, env = process.env) {
  if (!contrastEnabled(env)) return options;
  owned(env);
  installedBrowser();
  return contrastLaunchOptions(options, env.WINDOWS_PORT_ARM);
}
async function verifyPortContrastBrowserInner(browser, workerInfo, env) {
  // Worker stderr is intentionally consumed by the transport-only collector.
  // Only these fixed enum messages use the reporter's inherited stdout route.
  const stage = (name, operation) => browserProofStage(name, operation, message => process.stdout.write(`${message}\n`));
  const enabled = await stage('ownership', () => {
    if (!contrastEnabled(env)) return false;
    owned(env); return true;
  });
  if (!enabled) return;
  await stage('worker', () => demand(Number.isInteger(workerInfo.workerIndex) && workerInfo.workerIndex >= 0 && workerInfo.workerIndex < 148));
  await stage('project', () => demand(workerInfo.project.use.browserName === 'chromium' && workerInfo.project.use.channel === undefined && workerInfo.project.use.headless !== false));
  const expected = await stage('binary', () => installedBrowser());
  let session;
  let proof;
  try {
    session = await stage('session', () => boundedProof(browser.newBrowserCDPSession()));
    const response = await stage('command-line', () => boundedProof(session.send('Browser.getBrowserCommandLine')));
    // Arguments, profile locations and executable paths never leave memory.
    await stage('executable', () => {
      demand(Array.isArray(response.arguments) && typeof response.arguments[0] === 'string');
      demand(path.resolve(response.arguments[0]).toLowerCase() === path.resolve(expected.executable).toLowerCase());
    });
    const version = await stage('version', () => {
      const actual = browser.version(); demand(actual === BROWSER_VERSION); return actual;
    });
    proof = await stage('features', () => verifyCommandLine(response.arguments, env.WINDOWS_PORT_ARM, version));
  } catch { throw new Error('Port contrast browser verification unavailable'); }
  finally {
    if (session) {
      await stage('cleanup', () => boundedProof(session.detach()));
    }
  }
  await stage('binary-match', () => demand(expected.sha256 === env.WINDOWS_PORT_BROWSER_SHA256));
  await stage('record', () => {
    const record = { schema: 1, binding: binding(env), arm: armName(env.WINDOWS_PORT_ARM),
      worker: workerInfo.workerIndex, browser_sha256: expected.sha256, ...proof };
    writeFileSync(path.join(contrastDirectory(env), `browser-${workerInfo.workerIndex}.json`), JSON.stringify(record), { flag: 'wx' });
  });
}

async function boundedProof(promise) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error('Port contrast proof deadline reached')), 5000);
    })]);
  } finally { clearTimeout(timer); }
}
export async function verifyPortContrastBrowser(browser, workerInfo, env = process.env) {
  try { await verifyPortContrastBrowserInner(browser, workerInfo, env); }
  catch { throw new Error('Port contrast browser proof unavailable'); }
}
