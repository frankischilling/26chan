import { test as base, expect } from '@playwright/test';
import { writeFile } from 'node:fs/promises';

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
    const select = selector => {
      const value = node(selector)?.value;
      return ['small', 'large', 'on', 'off'].includes(value) ? value : null;
    };
    const threads = Array.from(document.querySelectorAll('.thread')).slice(0, 4);
    return {
      path: location.pathname.slice(0, 128), ready: document.readyState,
      viewport: [innerWidth, innerHeight], scroll: [scrollX, scrollY],
      sourceForm: !!node('form.postEditor'), nativeForm: !!node('form.nativePostForm'),
      quickReply: !!node('#quickReply'), replyParent: !!node('#qrResto'),
      postMenus: document.querySelectorAll('.postMenuBtn').length,
      size: select('#size-ctrl'), teaser: select('#teaser-ctrl'), spoilers: select('#theme-nospoiler'),
      reveal: document.body.classList.contains('reveal-img-spoilers'),
      threads: threads.map(element => ({
        id: /^t[0-9]+$/.test(element.id) ? element.id : null,
        closed: element.dataset.closed === 'true', archived: element.dataset.archived === 'true',
        rect: [element.getBoundingClientRect().width, element.getBoundingClientRect().height],
      })),
      images: Array.from(document.querySelectorAll('#threads img')).slice(0, 4)
        .map(image => ({ complete: image.complete, size: [image.width, image.height], natural: [image.naturalWidth, image.naturalHeight] })),
      events: Array.isArray(window.ownedVisualEvents) ? window.ownedVisualEvents.slice(-8) : [],
    };
  });
}

export const test = base.extend({
  visualDiagnostics: [async ({ context }, use, info) => {
    const errors = [], scripts = [];
    const observed = new Set();
    const observe = page => {
      if (observed.has(page)) return;
      observed.add(page);
      page.on('pageerror', error => {
        if (errors.length < 8) errors.push({ name: error.name.slice(0, 64), message: error.message.slice(0, 512) });
      });
      page.on('response', response => {
        if (scripts.length >= 16 || response.request().resourceType() !== 'script') return;
        scripts.push({ path: new URL(response.url()).pathname.slice(0, 128), status: response.status() });
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
          quote: target instanceof Element && !!target.closest('.postInfo > .postNum'),
          quickReply: !!document.getElementById('quickReply'),
        });
        if (window.ownedVisualEvents.length > 8) window.ownedVisualEvents.shift();
      }, { passive: true });
    });
    await use();
    if (info.status !== info.expectedStatus) {
      const pages = await Promise.all(context.pages().slice(0, 4)
        .map(page => readVisualState(page).catch(() => ({ unavailable: true }))));
      console.log('Synthetic visual failure state:', JSON.stringify({ errors, scripts, pages }));
    }
  }, { auto: true }],
});
