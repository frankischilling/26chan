import { expect } from '@playwright/test';

export async function openWatcherSettings(page) {
  await page.locator('#settingsWindowLink:visible, #settingsWindowLinkMobile:visible').click();
  const dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
  await expect(dialog).toBeVisible();
  const monitoring = dialog.getByRole('button', { name: 'Monitoring', exact: true });
  if (await dialog.getAttribute('id') !== 'theme') {
    await expect(monitoring).toBeVisible();
    await expect(monitoring).toHaveAccessibleName('Monitoring');
    if (await monitoring.getAttribute('aria-expanded') === 'false') await monitoring.click();
    await expect(monitoring).toHaveAttribute('aria-expanded', 'true');
  }
  return dialog;
}

export async function saveWatcherSettings(page, values, { reload = true } = {}) {
  const dialog = await openWatcherSettings(page);
  for (const [key, value] of Object.entries(values)) await dialog.locator(`.menuOption[data-option="${key}"]`).setChecked(value);
  const loaded = reload && await dialog.getAttribute('id') !== 'theme' ? page.waitForEvent('load') : null;
  await dialog.getByRole('button', { name: 'Save Settings', exact: true }).click();
  if (loaded) await loaded;
  await expect(page.getByRole('dialog', { name: 'Settings', exact: true })).toHaveCount(0);
}
