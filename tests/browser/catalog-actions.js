import { expect } from '@playwright/test';

export async function openCatalogSearch(page) {
  const toggle = page.locator('#qf-ctrl');
  if (await toggle.isVisible() && await toggle.getAttribute('aria-expanded') === 'false') await toggle.click();
  await expect(page.locator('#qf-box')).toBeVisible();
}

export async function fillCatalogSearch(page, value) {
  await openCatalogSearch(page);
  await page.locator('#qf-box').fill(value);
  if (await page.locator('#qf-ctrl').isVisible()) await page.locator('#qf-box').press('ArrowRight');
}

export async function applyCatalogSearch(page) {
  if (await page.locator('#qf-ctrl').isVisible()) {
    await page.locator('#qf-box').press('Enter');
    await expect.poll(() => page.locator('#qf-box').evaluate(node =>
      node.validationMessage !== '' || (new URL(location.href).searchParams.get('q') || '') === node.value)).toBe(true);
  } else await page.getByRole('button', { name: 'Apply', exact: true }).click();
}
