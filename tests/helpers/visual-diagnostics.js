import { test as base, expect } from '@playwright/test';
import { writeFile } from 'node:fs/promises';
import { writeSync } from 'node:fs';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { visualNetlogEnabled, visualNetlogPlan, prepareVisualNetlog, finishVisualNetlog, trackVisualNetlogFailure } from './visual-netlog.js';
import { browserControlEnabled, createBrowserControlTrigger } from './browser-control-trigger.js';

export { expect };

export async function attachComparedImage(info, name, bytes) {
  const path = info.outputPath(`${name}.png`);
  // Body-only attachments are not written to disk by the line reporter.
  await writeFile(path, bytes);
  await info.attach(name, { path, contentType: 'image/png' });
}

// Synthetic visual fixtures only. Never collect form values, storage, cookies,
// request bodies, response bodies or full URLs from a posting/staff session.
export async function readVisualState(page) {
  return page.evaluate(() => {
    const node = selector => document.querySelector(selector);
    const path = value => {
      if (!value) return null;
      try {
        const parsed = new URL(value, location.href);
        return ['http:', 'https:'].includes(parsed.protocol) ? parsed.pathname.slice(0, 128) : null;
      } catch { return null; }
    };
    const finite = value => Number.isFinite(value) ? Math.round(value * 1000) / 1000 : null;
    const color = element => {
      if (!element) return null;
      const value = getComputedStyle(element).backgroundColor;
      return /^rgba?\([0-9., /%]+\)$/.test(value) && value.length <= 64 ? value : null;
    };
    const select = selector => {
      const value = node(selector)?.value;
      return ['small', 'large', 'on', 'off'].includes(value) ? value : null;
    };
    const threads = Array.from(document.querySelectorAll('.thread')).slice(0, 4);
    const paper = getComputedStyle(document.documentElement).getPropertyValue('--paper').trim();
    return {
      path: location.pathname.slice(0, 128), ready: document.readyState,
      viewport: [innerWidth, innerHeight], scroll: [scrollX, scrollY],
      sourceForm: !!node('form.postEditor'), nativeForm: !!node('form.nativePostForm'),
      quickReply: !!node('#quickReply'), replyParent: !!node('#qrResto'),
      postMenus: document.querySelectorAll('.postMenuBtn').length,
      size: select('#size-ctrl'), teaser: select('#teaser-ctrl'), spoilers: select('#theme-nospoiler'),
      reveal: document.body.classList.contains('reveal-img-spoilers'),
      background: {
        root: color(document.documentElement), body: color(document.body), hover: color(node('#image-hover')),
        paper: /^(?:#[a-fA-F0-9]{3}|#[a-fA-F0-9]{6})$/.test(paper) ? paper : null,
      },
      threads: threads.map(element => ({
        id: /^t[0-9]+$/.test(element.id) ? element.id : null,
        closed: element.dataset.closed === 'true', archived: element.dataset.archived === 'true',
        rect: [element.getBoundingClientRect().width, element.getBoundingClientRect().height],
      })),
      images: Array.from(document.querySelectorAll('#threads img')).slice(0, 4).map(image => {
        const rect = image.getBoundingClientRect();
        return {
          id: /^[a-zA-Z0-9_-]{1,64}$/.test(image.id) ? image.id : '',
          source: path(image.getAttribute('src')),
          spoilerSource: path(image.dataset.spoilerSrc),
          complete: image.complete,
          size: [image.width, image.height], natural: [image.naturalWidth, image.naturalHeight],
          rect: [finite(rect.x), finite(rect.y), finite(rect.width), finite(rect.height)],
        };
      }),
      events: Array.isArray(window.ownedVisualEvents) ? window.ownedVisualEvents.slice(-8) : [],
    };
  });
}

export const test = base.extend({
  visualNetlogState: [async ({}, use) => {
    await use({ failed: false });
  }, { scope: 'worker' }],
  // Register before the context-dependent auto fixture so failed context setup
  // is observed too. Test teardown precedes worker browser/options teardown.
  visualNetlogFailure: [async ({ visualNetlogState }, use, info) => {
    await trackVisualNetlogFailure(visualNetlogState, use, info);
  }, { auto: true }],
  launchOptions: [async ({ launchOptions, visualNetlogState }, use, workerInfo) => {
    let plan = null;
    if (visualNetlogEnabled(process.platform, process.env)) {
      try {
        plan = visualNetlogPlan(launchOptions, workerInfo.project.outputDir, workerInfo.workerIndex);
        if (plan) await prepareVisualNetlog(plan);
        else console.log('Synthetic NetLog unavailable: caller supplied logging options.');
      } catch { plan = null; console.log('Synthetic NetLog unavailable: setup.'); }
    }
    try { await use(plan?.options ?? launchOptions); }
    finally {
      // The browser depends on launchOptions, so it closes before this teardown.
      // Diagnostics must never replace the original test/worker result.
      if (plan) {
        try { console.log(`Synthetic NetLog artifact: ${await finishVisualNetlog(plan, visualNetlogState)}.`); }
        catch { console.log('Synthetic NetLog unavailable: incomplete capture.'); }
      }
    }
  }, { scope: 'worker' }],
  visualDiagnostics: [async ({ context }, use, info) => {
    // Only the opt-in owned launcher enables this observation. No request is
    // retried, intercepted or reclassified. The control has a separate process.
    const trigger = browserControlEnabled(process.platform, process.env)
      ? createBrowserControlTrigger(line => {
        // A bounded synchronous descriptor write throws into the trigger's
        // catch on EPIPE; it cannot emit an unhandled Writable error event.
        writeSync(2, line);
      }) : null;
    if (trigger) context.on('requestfailed', trigger);
    const errors = [], scripts = [], failedScripts = [], stylesheets = [], failedStylesheets = [];
    const observed = new Set();
    const observe = page => {
      if (observed.has(page)) return;
      observed.add(page);
      page.on('pageerror', error => {
        if (errors.length < 8) errors.push({ name: error.name.slice(0, 64), message: error.message.slice(0, 512) });
      });
      page.on('response', response => {
        const type = response.request().resourceType();
        const records = type === 'script' ? scripts : type === 'stylesheet' ? stylesheets : null;
        if (!records || records.length >= 16) return;
        records.push({ path: new URL(response.url()).pathname.slice(0, 128), status: response.status() });
      });
      page.on('requestfailed', request => {
        const type = request.resourceType();
        const records = type === 'script' ? failedScripts : type === 'stylesheet' ? failedStylesheets : null;
        if (!records || records.length >= 16) return;
        const error = request.failure()?.errorText;
        records.push({
          path: new URL(request.url()).pathname.slice(0, 128),
          error: /^net::[A-Z0-9_]{1,80}$/.test(error ?? '') ? error : 'unavailable',
        });
      });
    };
    context.on('page', observe);
    for (const page of context.pages()) observe(page);
    await context.addInitScript(() => {
      window.ownedVisualEvents = [];
      for (const type of ['click', 'change']) window.addEventListener(type, event => {
        const target = event.target;
        const id = target instanceof Element ? target.id : '';
        window.ownedVisualEvents.push({
          type, tag: target instanceof Element ? target.tagName : '',
          id: /^[a-zA-Z0-9_-]{1,64}$/.test(id) ? id : '', prevented: event.defaultPrevented,
          quote: target instanceof Element && !!target.closest('.postInfo > .postNum > a[title="Reply to this post"]'),
          quickReply: !!document.getElementById('quickReply'),
        });
        if (window.ownedVisualEvents.length > 8) window.ownedVisualEvents.shift();
      }, { passive: true });
    });
    try { await use({ failedScripts, failedStylesheets }); }
    finally { if (trigger) context.off('requestfailed', trigger); }
    if (info.status !== info.expectedStatus) {
      const pages = await Promise.all(context.pages().slice(0, 4)
        .map(page => readVisualState(page).catch(() => ({ unavailable: true }))));
      console.log('Synthetic visual failure state:', JSON.stringify({ errors, scripts, failedScripts, stylesheets, failedStylesheets, pages }));
      if (process.platform === 'win32' && process.env.WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS === '1') {
        try {
          const script = fileURLToPath(new URL('../../scripts/windows-visual-resources.ps1', import.meta.url));
          const result = await promisify(execFile)('pwsh', ['-NoProfile', '-File', script, '-Phase', 'failure'],
            { timeout: 10_000, maxBuffer: 8192, windowsHide: true });
          console.log('Synthetic visual host resources:', JSON.stringify(JSON.parse(result.stdout)));
        } catch { console.log('Synthetic visual host resources: unavailable'); }
      }
    }
  }, { auto: true }],
});
