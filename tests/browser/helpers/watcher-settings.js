import { expect } from '@playwright/test';

export function watcherSettingsOpener(page) {
  return page.locator('#settingsWindowLink[data-native-settings-ready]:visible, #settingsWindowLinkMobile[data-native-settings-ready]:visible')
    .or(page.getByRole('navigation', { name: 'Persistent board navigation', exact: true }).getByRole('button', { name: 'Settings', exact: true }))
    .or(page.getByRole('navigation', { name: 'Custom board navigation', exact: true }).getByRole('link', { name: 'Settings', exact: true }))
    .first();
}

const nativeSettingCategories = new Map([
  ['Quotes & Replying', 'quotePreview backlinks inlineQuotes quickReply persistentQR'],
  ['Monitoring', 'threadUpdater alwaysAutoUpdate threadWatcher threadAutoWatcher autoScroll updaterSound fixedThreadWatcher threadStats'],
  ['Filters & Post Hiding', 'filter threadHiding hideStubs'],
  ['Navigation', 'threadExpansion dropDownNav classicNav autoHideNav customMenu alwaysDepage topPageNav stickyNav keyBinds'],
  ['Images & Media', 'imageExpansion fitToScreenExpansion imageHover imageHoverBg revealSpoilers noPictures embedYouTube embedSoundCloud'],
  ['Miscellaneous', 'linkify darkTheme customCSS IDColor compactThreads centeredThreads localTime forceHTTPS'],
].flatMap(([category, keys]) => keys.split(' ').map(key => [key, category])));

export async function openNativeSettingsCategory(dialog, name) {
  const category = dialog.getByRole('button', { name, exact: true });
  await expect(category).toBeVisible();
  if (await category.getAttribute('aria-expanded') === 'false') await category.click();
  await expect(category).toHaveAttribute('aria-expanded', 'true');
  const panel = dialog.locator(`[id="${await category.getAttribute('aria-controls')}"]`);
  await expect(panel).toBeVisible();
  return panel;
}

export async function openSettingControl(dialog, key) {
  const control = dialog.locator(`.menuOption[data-option="${key}"]`);
  // Catalog retains its separate editor; disableAll lives outside the categories.
  if (await dialog.getAttribute('id') !== 'theme' && key !== 'disableAll') {
    const category = nativeSettingCategories.get(key);
    expect(category, `Known native settings category for ${key}`).toBeTruthy();
    const panel = await openNativeSettingsCategory(dialog, category);
    await expect(panel.locator(`.menuOption[data-option="${key}"]`)).toHaveCount(1);
  }
  await expect(control).toBeVisible();
  return control;
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
  for (const [key, value] of Object.entries(values)) await (await openSettingControl(dialog, key)).setChecked(value);
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
