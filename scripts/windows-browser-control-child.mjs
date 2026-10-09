import { chromium } from 'playwright';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { readFile, rename, lstat } from 'node:fs/promises';
import { visualNetlogPlan, prepareVisualNetlog, finishVisualNetlog } from '../tests/helpers/visual-netlog.js';
import { TARGET, STARTUP_MS, NAVIGATION_MS, CLOSE_MS, diagnosticPlan, bounded, inspectControlNetlog } from './windows-browser-control-core.mjs';

export async function runControl({ launch, send, messages, root, version }) {
  let browser, page, plan, triggered = false, stopping = false, navigation = null;
  let result = { navigation: 'not-triggered', status: null, network_error: null, netlog: 'not-triggered', tcp: null };
  const safeSend = value => { try { send(value); } catch { } };
  const stop = async () => {
    if (stopping) return;
    stopping = true;
    if (navigation) await bounded(navigation, NAVIGATION_MS + 1000, null);
    const closed = browser ? await bounded(browser.close().then(() => true), CLOSE_MS, false) : false;
    if (plan && closed) {
      try {
        result.netlog = await finishVisualNetlog(plan, { failed: triggered });
        if (result.netlog === 'accepted') {
          const log = JSON.parse(await readFile(plan.accepted, 'utf8'));
          result.tcp = inspectControlNetlog(log);
          const destination = path.join(root, 'control-netlog.json');
          try { await lstat(destination); throw new Error('Existing artifact'); }
          catch (error) { if (error.code !== 'ENOENT') throw error; }
          await rename(plan.accepted, destination);
        }
      } catch { result.netlog = 'unavailable'; }
    } else if (triggered) result.netlog = 'incomplete-close';
    safeSend({ type: 'finished', ...result, closed });
    messages.off('message', onMessage);
    messages.off('disconnect', onDisconnect);
    messages.disconnect?.();
  };
  const onDisconnect = () => { void stop().catch(() => {}); };
  const onMessage = message => {
    if (message?.type === 'stop') { void stop().catch(() => safeSend({ type: 'unavailable' })); return; }
    if (message?.type !== 'navigate' || triggered || stopping || !page) return;
    triggered = true;
    safeSend({ type: 'started' });
    navigation = (async () => {
      try {
        const response = await page.goto(TARGET, { waitUntil: 'load', timeout: NAVIGATION_MS });
        const status = response?.status();
        result.status = Number.isInteger(status) && status >= 100 && status <= 599 ? status : null;
        result.navigation = response?.url() === TARGET && status === 200 ? 'http-200' : 'unexpected-response';
      } catch { result.navigation = 'failed-or-timeout'; }
      safeSend({ type: 'completed', navigation: result.navigation, status: result.status, network_error: result.network_error });
    })().catch(() => { result.navigation = 'unavailable'; });
  };
  messages.on('message', onMessage);
  messages.on('disconnect', onDisconnect);
  try {
    if (version !== '1.62.0') throw new Error('Pinned browser package required');
    plan = visualNetlogPlan({}, root, 0, 'control');
    await prepareVisualNetlog(plan);
    if (stopping) return;
    // This Node process has no Playwright Test fixtures or their default options.
    browser = await launch({ ...plan.options, headless: true, timeout: STARTUP_MS });
    if (stopping) { await bounded(browser.close(), CLOSE_MS, null); return; }
    const context = await browser.newContext();
    if (stopping) { await bounded(browser.close(), CLOSE_MS, null); return; }
    page = await context.newPage();
    if (stopping) { await bounded(browser.close(), CLOSE_MS, null); return; }
    page.on('requestfailed', request => {
      try {
        const error = request.failure()?.errorText;
        if (request.url() === TARGET && /^net::[A-Z0-9_]{1,80}$/.test(error ?? '')) result.network_error = error;
      } catch { }
    });
    safeSend({ type: 'ready', package_version: version });
  } catch {
    safeSend({ type: 'unavailable' });
    await stop();
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    diagnosticPlan(process.env);
    const root = path.resolve(fileURLToPath(new URL('../', import.meta.url)), 'test-results/windows-browser-control-7');
    const version = createRequire(import.meta.url)('playwright/package.json').version;
    await runControl({ launch: options => chromium.launch(options), send: message => {
      if (process.connected) process.send(message, () => {});
    }, messages: process, root, version });
  } catch { if (process.connected) process.send({ type: 'unavailable' }, () => {}); process.exitCode = 1; }
}
