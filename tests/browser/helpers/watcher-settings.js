import { expect } from '@playwright/test';

export function watcherSettingsOpener(page) {
  return page.locator('#settingsWindowLink:visible, #settingsWindowLinkMobile:visible')
    .or(page.getByRole('navigation', { name: 'Persistent board navigation', exact: true }).getByRole('button', { name: 'Settings', exact: true }))
    .or(page.getByRole('navigation', { name: 'Custom board navigation', exact: true }).getByRole('link', { name: 'Settings', exact: true }))
    .first();
}

export async function openWatcherSettings(page) {
  await watcherSettingsOpener(page).click();
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

export async function saveWatcherSettings(page, values, { reload = true, tabOnly = false } = {}) {
  const dialog = await openWatcherSettings(page);
  const catalog = await dialog.getAttribute('id') === 'theme';
  for (const [key, value] of Object.entries(values)) await dialog.locator(`.menuOption[data-option="${key}"]`).setChecked(value);
  const loaded = reload && !catalog ? page.waitForEvent('load') : null;
  await dialog.getByRole('button', { name: 'Save Settings', exact: true }).click();
  if (loaded) await loaded;
  if (catalog && tabOnly) {
    await expect(dialog.locator('#theme-msg')).toContainText('only in this tab');
    await dialog.getByRole('button', { name: 'Close settings', exact: true }).click();
    await expect(dialog).not.toBeVisible();
  } else if (catalog) await expect(dialog).not.toBeVisible();
  else await expect(page.getByRole('dialog', { name: 'Settings', exact: true })).toHaveCount(0);
}
