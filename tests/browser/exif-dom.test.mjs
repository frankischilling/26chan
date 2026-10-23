import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';

const script = await readFile(new URL('../../assets/exif.js', import.meta.url), 'utf8');
const css = await readFile(new URL('../../assets/comment-markup.css', import.meta.url), 'utf8');
const fragment = label => `<blockquote id="${label}" class="postMessage"><span class="abbr">[EXIF data available. Click <a href="#exif1730000000000" data-exif-toggle="exif1730000000000" aria-expanded="false">here</a> to show/hide.]</span><br><table class="exif" id="exif1730000000000" data-exif-table="exif1730000000000"><tr><td>Camera</td><td>&lt;img src=x onerror=alert(1)&gt;</td></tr></table></blockquote>`;

test('desktop and mobile EXIF toggles stay within a post or its cloned preview', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    for (const viewport of [{ width: 1280, height: 720 }, { width: 390, height: 844 }]) {
      const page = await browser.newPage({ viewport });
      const unexpected = [];
      await page.route('**/*', route => {
        const url = route.request().url();
        if (url === 'https://exif.example/') return route.fulfill({ contentType: 'text/html', body: `<!doctype html><html><head><link rel="stylesheet" href="/comment.css"><script type="module" src="/exif.js"></script></head><body>${fragment('owned')}${fragment('preview')}</body></html>`, headers: { 'content-security-policy': "default-src 'none'; script-src 'self'; style-src 'self'; base-uri 'none'" } });
        if (url === 'https://exif.example/exif.js') return route.fulfill({ contentType: 'text/javascript', body: `${script}\nglobalThis.__ownedExifReady = true;` });
        if (url === 'https://exif.example/comment.css') return route.fulfill({ contentType: 'text/css', body: css });
        unexpected.push(url); return route.abort();
      });
      await page.goto('https://exif.example/');
      await page.waitForFunction(() => globalThis.__ownedExifReady === true);
      const owned = page.locator('#owned'), preview = page.locator('#preview');
      assert.equal(await owned.locator('table').isVisible(), false);
      assert.equal(await preview.locator('table').isVisible(), false);
      await preview.getByRole('link', { name: 'here' }).click();
      assert.equal(await preview.locator('table').isVisible(), true);
      assert.equal(await owned.locator('table').isVisible(), false);
      assert.equal(await preview.getByRole('link', { name: 'here' }).getAttribute('aria-expanded'), 'true');
      assert.equal(await preview.locator('td').last().textContent(), '<img src=x onerror=alert(1)>');
      assert.equal(await page.locator('blockquote img, blockquote script, [onclick], [onerror]').count(), 0);
      await owned.getByRole('link', { name: 'here' }).focus();
      await page.keyboard.press('Enter');
      assert.equal(await owned.locator('table').isVisible(), true);
      await preview.getByRole('link', { name: 'here' }).click();
      assert.equal(await preview.locator('table').isVisible(), false);
      assert.equal(await owned.locator('table').isVisible(), true);
      assert.deepEqual(unexpected, []);
      await page.close();
    }
  } finally { await browser.close(); }
});
