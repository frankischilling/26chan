import assert from 'node:assert/strict';
import { expect } from '@playwright/test';

// Viewport changes can finish before the site's resize callback/rAF has run.
// Keep the exact no-overflow condition, but observe the settled layout.
async function pollFit(page, label) {
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), {
    message: `${label}: horizontal overflow after viewport resize`,
    timeout: 1000,
    intervals: [16, 50, 100],
  }).toBe(true);
}

export async function assertViewportFit(page, label) {
  try {
    await pollFit(page, label);
  } catch (error) {
    const layout = await page.evaluate(() => ({
      width: document.documentElement.scrollWidth,
      viewport: innerWidth,
      overflowing: Array.from(document.body.querySelectorAll('*')).filter(element => {
        const style = getComputedStyle(element);
        const rect = element.getBoundingClientRect();
        return style.display !== 'none' && style.visibility !== 'hidden' && rect.width > 0
          && (rect.left < 0 || rect.right > innerWidth);
      }).slice(0, 10).map(element => {
        const rect = element.getBoundingClientRect();
        return { tag: element.tagName, id: element.id, classes: element.className,
          left: rect.left, right: rect.right, width: rect.width };
      }),
    }));
    console.error(`${label}: ${JSON.stringify(layout)}`);
    throw error;
  }
}

export async function qualifyOverflowGuard(page) {
  const previous = await page.evaluate(() => {
    const value = document.documentElement.style.minWidth;
    document.documentElement.style.minWidth = `${innerWidth + 32}px`;
    return value;
  });
  try {
    await assert.rejects(() => pollFit(page, 'owned overflow negative control'));
  } finally {
    await page.evaluate(value => { document.documentElement.style.minWidth = value; }, previous);
  }
  await assertViewportFit(page, 'restored overflow negative control');
}
