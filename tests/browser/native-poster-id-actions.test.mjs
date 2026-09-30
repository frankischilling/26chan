import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium, expect } from '@playwright/test';

const post = (no, id) => `<article class="postContainer"><div class="post reply" id="p${no}"><div class="postInfo"><span class="posteruid">(ID: <span class="hand">${id}</span>)</span></div><blockquote class="postMessage">Synthetic post</blockquote></div></article>`;

const staffPost = (no, label = 'Mod', nameClass = 'capcodeMod', group = 'id_mod', title = 'Highlight posts by Moderators') =>
  `<article class="postContainer"><div class="post reply" id="p${no}"><div class="postInfo"><span class="nameBlock ${nameClass}"><span class="name">Owned staff</span> <strong class="capcode hand ${group}" title="${title}">## ${label}</strong></span></div><blockquote class="postMessage">Synthetic staff post</blockquote></div></article>`;

async function fixture(thread, action) {
  const origin = 'https://id-actions.example';
  const source = await readFile(new URL('../../apps/public/static/native-display.v1.js', import.meta.url), 'utf8');
  const styles = await readFile(new URL('../../apps/public/static/board.css', import.meta.url), 'utf8');
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage(); page.setDefaultTimeout(5000);
    const unexpected = [], errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.route('**/*', async route => {
      const url = new URL(route.request().url());
      if (url.href === `${origin}/static/native-display.v1.js`) return route.fulfill({ contentType: 'text/javascript', body: source });
      if (url.href === `${origin}/static/board.css`) return route.fulfill({ contentType: 'text/css', body: styles });
      if (url.href === `${origin}/test/`) return route.fulfill({ contentType: 'text/html',
        headers: { 'content-security-policy': `default-src 'none'; script-src ${origin}/static/native-display.v1.js; style-src ${origin}/static/board.css; base-uri 'none'` },
        body: `<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="/static/board.css"></head><body><main id="owned"><section class="thread" id="t1">${post(1, 'AAAAAAAA')}${post(2, 'AAAAAAAA')}${post(3, 'BBBBBBBB')}</section><div id="quote-preview"><div class="post" id="p99"><div class="postInfo"><span class="posteruid"><span class="hand" id="copy">AAAAAAAA</span></span></div></div></div></main></body></html>` });
      unexpected.push(url.href); await route.abort();
    });
    await page.goto(`${origin}/test/`);
    await page.evaluate(async thread => {
      const api = await import('/static/native-display.v1.js'); window.config = {};
      window.root = document.getElementById('owned');
      window.controller = api.mountNativePosterIdActions({ root, settings: () => config, thread });
    }, thread);
    await action(page);
    assert.deepEqual(unexpected, []); assert.deepEqual(errors, []);
  } finally { await browser.close(); }
}

test('ID controls toggle matching posts, preserve quote highlights and handle keyboard and live replies', async () => {
  await fixture(true, async page => {
    const first = page.locator('#p1 .hand'), second = page.locator('#p2 .hand');
    await expect(first).toHaveAttribute('role', 'button');
    await expect(page.locator('#copy')).not.toHaveAttribute('role', 'button');
    await page.evaluate(() => document.getElementById('p1').classList.add('highlight'));
    await first.click();
    await expect(page.locator('#p1')).toHaveClass(/poster-id-highlight/);
    await expect(page.locator('#p2')).toHaveClass(/poster-id-highlight/);
    await expect(page.locator('#p3')).not.toHaveClass(/poster-id-highlight/);
    await expect(second).toHaveAttribute('aria-pressed', 'true');
    await second.press('Enter');
    await expect(page.locator('#p1')).not.toHaveClass(/poster-id-highlight/);
    await expect(page.locator('#p1')).toHaveClass(/\bhighlight\b/);
    await first.press(' ');
    await page.evaluate(html => document.getElementById('t1').insertAdjacentHTML('beforeend', html), post(4, 'AAAAAAAA'));
    await expect(page.locator('#p4')).toHaveClass(/poster-id-highlight/);
    await expect(page.locator('#p4 .hand')).toHaveAttribute('aria-pressed', 'true');
    await page.locator('#p3 .posteruid').click({ position: { x: 2, y: 2 } });
    await expect(page.locator('#p1')).not.toHaveClass(/poster-id-highlight/);
    await expect(page.locator('#p3')).toHaveClass(/poster-id-highlight/);
    await page.evaluate(() => {
      document.querySelector('#p1 .hand').setAttribute('role', 'note'); controller.destroy();
    });
    await expect(first).toHaveAttribute('role', 'note');
    await expect(first).not.toHaveAttribute('tabindex', '0');
    await expect(page.locator('#p3')).not.toHaveClass(/poster-id-highlight/);
    await expect(page.locator('#p1')).toHaveClass(/\bhighlight\b/);
  });
});

test('loaded-post tooltip excludes projections, updates counts and cancels across preferences and BFCache', async () => {
  await fixture(true, async page => {
    const first = page.locator('#p1 .hand'), tip = page.locator('#native-poster-id-tip');
    await first.hover(); await expect(tip).toHaveText('2 posts by this ID');
    await expect(first).toHaveAttribute('aria-describedby', 'native-poster-id-tip');
    await page.evaluate(html => document.getElementById('t1').insertAdjacentHTML('beforeend', html), post(4, 'AAAAAAAA'));
    await expect(tip).toHaveText('3 posts by this ID');
    await page.evaluate(() => { config.disableAll = true; document.dispatchEvent(new Event('4chanSettingsSaved')); });
    await expect(tip).toHaveCount(0); await expect(first).not.toHaveAttribute('aria-describedby', 'native-poster-id-tip');
    // Click highlighting belongs to the released core and survives extension disablement.
    await first.click(); await expect(page.locator('#p1')).toHaveClass(/poster-id-highlight/);
    await page.evaluate(() => { config.disableAll = false; controller.refresh(); });
    await page.mouse.move(0, 0); await first.hover();
    await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
    await expect(tip).toHaveCount(0);
    await page.evaluate(() => {
      document.querySelector('#p2 .hand').textContent = 'BBBBBBBB';
      window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
    });
    await expect(page.locator('#p2')).not.toHaveClass(/poster-id-highlight/);
    await first.evaluate(element => element.blur());
    await first.focus(); await expect(tip).toHaveText('2 posts by this ID');
    await page.evaluate(() => root.remove());
    assert.equal(await page.evaluate(() => root.querySelector('#p1').classList.contains('poster-id-highlight')), false);
    assert.equal(await page.evaluate(() => root.querySelector('#p1 .hand').hasAttribute('role')), false);
    await expect(tip).toHaveCount(0);
  });
});

test('index tooltips require an expanded thread and omit incomplete bounded counts', async () => {
  await fixture(false, async page => {
    const first = page.locator('#p1 .hand'), tip = page.locator('#native-poster-id-tip');
    await page.clock.install();
    await first.hover(); await page.clock.runFor(550); await expect(tip).toHaveCount(0);
    await page.evaluate(() => document.getElementById('t1').classList.add('tExpanded'));
    await first.focus(); await expect(tip).toHaveText('2 posts by this ID');
    await page.evaluate(() => document.getElementById('t1').classList.remove('tExpanded'));
    await expect(tip).toHaveCount(0);
    await page.evaluate(() => {
      document.getElementById('t1').classList.add('tExpanded');
      const fragment = document.createDocumentFragment();
      for (let index = 0; index < 40001; ++index) fragment.append(document.createElement('i'));
      root.append(fragment); controller.refresh();
    });
    await page.mouse.move(0, 0); await first.hover(); await page.clock.runFor(550); await expect(tip).toHaveCount(0);
    await first.click(); await expect(page.locator('#p1')).toHaveClass(/poster-id-highlight/);
  });
});

test('staff labels highlight their finite groups without ID tooltips or preview controls', async () => {
  await fixture(true, async page => {
    await page.evaluate(html => document.getElementById('t1').insertAdjacentHTML('beforeend', html),
      staffPost(900) + staffPost(901) + staffPost(902, 'Admin', 'capcodeAdmin', 'id_admin', 'Highlight posts by Administrators')
      + staffPost(903, 'Founder', 'capcodeAdmin', 'id_admin', 'Highlight posts by the Founder')
      + staffPost(904, 'Developer', 'capcodeDeveloper', 'id_developer', 'Highlight posts by Developers')
      + staffPost(905, 'Manager', 'capcodeManager', 'id_manager', 'Highlight posts by Managers'));
    const first = page.locator('#p900 .capcode'), admin = page.locator('#p902 .capcode');
    await expect(first).toHaveAttribute('role', 'button'); await expect(first).toHaveAttribute('tabindex', '0');
    await first.click();
    await expect(page.locator('#p900')).toHaveClass(/poster-id-highlight/);
    await expect(page.locator('#p901')).toHaveClass(/poster-id-highlight/);
    await expect(page.locator('#p1')).not.toHaveClass(/poster-id-highlight/);
    await first.press('Enter'); await expect(page.locator('#p900')).not.toHaveClass(/poster-id-highlight/);
    await admin.press(' ');
    await expect(page.locator('#p902')).toHaveClass(/poster-id-highlight/);
    await expect(page.locator('#p903')).toHaveClass(/poster-id-highlight/);
    await expect(page.locator('#p904')).not.toHaveClass(/poster-id-highlight/);
    await page.evaluate(() => document.getElementById('p902').classList.add('highlightPost'));
    await admin.hover(); await page.waitForTimeout(600);
    await expect(page.locator('#native-poster-id-tip')).toHaveCount(0);
    await expect(admin).toHaveAttribute('title', 'Highlight posts by Administrators');
    await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
    await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
    await expect(page.locator('#p903')).toHaveClass(/poster-id-highlight/);
    await page.locator('#p904 .capcode').click();
    await expect(page.locator('#p904')).toHaveClass(/poster-id-highlight/);
    await expect(page.locator('#p902')).not.toHaveClass(/poster-id-highlight/);
    await expect(page.locator('#p902')).toHaveClass(/highlightPost/);
    await page.locator('#p905 .capcode').click();
    await expect(page.locator('#p905')).toHaveClass(/poster-id-highlight/);
    await page.evaluate(() => controller.destroy());
    await expect(first).not.toHaveAttribute('role', 'button');
    await expect(first).toHaveAttribute('title', 'Highlight posts by Moderators');
    await expect(page.locator('#p905')).not.toHaveClass(/poster-id-highlight/);
    await expect(page.locator('#p902')).toHaveClass(/highlightPost/);
  });
});
