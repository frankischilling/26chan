import { FILTER_LIMITS, readFilterRules, readNativeFilters, filterColor } from './native-filter.v1.js';
import { CUSTOM_CSS_LIMITS, parseCustomCSS } from './native-custom-css.v1.js';
import { customBoards } from './native-display.v1.js';
import { readWatcherPosition } from './watcher-position.v1.js';
import { CATALOG_FILTER_LIMITS, readCatalogFilters } from './catalog-filter-core.v1.js';

export const SETTINGS_TRANSFER_LIMITS = Object.freeze({
  encodedChars: 1048576,
  decodedChars: 393216,
  decodedBytes: 524288,
  settingsChars: 4096,
  filtersChars: FILTER_LIMITS.settings,
  catalogFiltersChars: CATALOG_FILTER_LIMITS.storage,
  cssBytes: CUSTOM_CSS_LIMITS.bytes,
  catalogSettingsChars: 1024,
  existingValueChars: 4194304,
  objectNodes: 4096,
});

export const SETTINGS_TRANSFER_STORAGE_KEYS = Object.freeze([
  '4chan-settings',
  '4chan-filters',
  '4chan-css',
  'catalog-filters',
  'catalog-settings',
]);

export const SETTINGS_TRANSFER_INACTIVE_COMPATIBILITY_KEYS = Object.freeze([
  'forceHTTPS',
  'unmuteWebm',
]);

const PAYLOAD_KEYS = new Set(['settings', 'filters', 'css', 'catalogFilters', 'catalogSettings']);
const PROTOTYPE_KEYS = new Set(['__proto__', 'prototype', 'constructor']);
const BOOLEAN_SETTINGS = new Set([
  'threadUpdater', 'alwaysAutoUpdate', 'threadWatcher', 'threadAutoWatcher', 'fixedThreadWatcher',
  'autoScroll', 'updaterSound', 'filter', 'threadHiding', 'hideStubs', 'dropDownNav', 'classicNav',
  'autoHideNav', 'topPageNav', 'stickyNav', 'alwaysDepage', 'customMenu', 'localTime', 'threadExpansion',
  'threadStats', 'quickReply', 'persistentQR', 'linkify', 'quotePreview', 'backlinks', 'inlineQuotes',
  'keyBinds', 'imageExpansion', 'fitToScreenExpansion', 'imageHover', 'imageHoverBg', 'revealSpoilers',
  'noPictures', 'embedYouTube', 'embedSoundCloud', 'darkTheme', 'customCSS', 'compactThreads',
  'centeredThreads', 'disableAll', 'IDColor', 'forceHTTPS', 'unmuteWebm',
]);
const INACTIVE_COMPATIBILITY_SETTINGS = new Set(SETTINGS_TRANSFER_INACTIVE_COMPATIBILITY_KEYS);
const inactiveSetting = (key, httpsAvailable) => INACTIVE_COMPATIBILITY_SETTINGS.has(key)
  && !(key === 'forceHTTPS' && httpsAvailable === true);
const POSITION_SETTINGS = new Set(['TW-position', 'TN-position', 'SN-position']);
const CATALOG_ORDERS = new Set(['alt', 'absdate', 'date', 'r']);
const encoder = new TextEncoder();
const mounts = new WeakMap();

function record(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function sameKeys(value, keys) {
  if (!record(value)) return false;
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  return actual.length === expected.length && actual.every((key, index) => key === expected[index]);
}

function safeJSON(raw, maximum, label) {
  if (typeof raw !== 'string' || raw.length > maximum) return { status: 'invalid', error: `${label} is too large.` };
  try { return { status: 'ok', value: JSON.parse(raw) }; }
  catch { return { status: 'invalid', error: `${label} is not valid JSON.` }; }
}

function safeObjectTree(value) {
  const stack = [value];
  let nodes = 0;
  while (stack.length) {
    const current = stack.pop();
    if (!current || typeof current !== 'object') continue;
    if (++nodes > SETTINGS_TRANSFER_LIMITS.objectNodes) return false;
    for (const key of Object.keys(current)) {
      if (PROTOTYPE_KEYS.has(key)) return false;
      const next = current[key];
      if (next && typeof next === 'object') stack.push(next);
    }
  }
  return true;
}

export function validateTransferSettings(raw) {
  const parsed = safeJSON(raw, SETTINGS_TRANSFER_LIMITS.settingsChars, 'Settings');
  if (parsed.status !== 'ok' || !record(parsed.value) || !safeObjectTree(parsed.value)) {
    return { status: 'invalid', error: parsed.error ?? 'Settings must be an object without reserved property names.' };
  }
  const value = parsed.value;
  for (const [key, entry] of Object.entries(value)) {
    if (BOOLEAN_SETTINGS.has(key)) {
      if (typeof entry !== 'boolean') return { status: 'invalid', error: `Setting "${key}" must be true or false.` };
      continue;
    }
    if (key === 'customMenuList') {
      if (typeof entry !== 'string' || customBoards(entry) === null) {
        return { status: 'invalid', error: 'The custom board list is invalid.' };
      }
      continue;
    }
    if (POSITION_SETTINGS.has(key)) {
      if (readWatcherPosition(entry) === null) return { status: 'invalid', error: `Setting "${key}" has an invalid position.` };
      continue;
    }
    if (key === 'QR-position') {
      const sourceCoordinates = typeof entry === 'string' && readWatcherPosition(entry) !== null;
      if (!sourceCoordinates && (!sameKeys(entry, ['left', 'top']) || !Number.isFinite(entry.left) || !Number.isFinite(entry.top))) {
        return { status: 'invalid', error: 'The Quick Reply position is invalid.' };
      }
      continue;
    }
    return { status: 'invalid', error: `Setting "${key}" is not supported by this version.` };
  }
  return { status: 'ok', value };
}

export function validateTransferFilters(raw) {
  if (typeof raw !== 'string' || raw.length > SETTINGS_TRANSFER_LIMITS.filtersChars) {
    return { status: 'invalid', error: 'Filters are too large.' };
  }
  let value;
  try { value = JSON.parse(raw); }
  catch { return { status: 'invalid', error: 'Filters are not valid JSON.' }; }
  if (!safeObjectTree(value) || readFilterRules(raw).status !== 'ok' || readNativeFilters(raw).status !== 'ok') {
    return { status: 'invalid', error: 'Filters do not match the supported filter format.' };
  }
  const rules = readFilterRules(raw).rules;
  try {
    if (rules.some(rule => filterColor(rule.color) === null)) {
      return { status: 'invalid', error: 'A filter color is invalid.' };
    }
  } catch { return { status: 'invalid', error: 'Filter colors could not be checked.' }; }
  return { status: 'ok', count: rules.length };
}

export function validateTransferCSS(raw) {
  if (typeof raw !== 'string' || raw.length > SETTINGS_TRANSFER_LIMITS.cssBytes) {
    return { status: 'invalid', error: 'Custom CSS is too large.' };
  }
  const parsed = parseCustomCSS(raw);
  if (parsed.status !== 'ok') return { status: 'invalid', error: `Custom CSS is invalid: ${parsed.error}` };
  return { status: 'ok', count: parsed.rules.length };
}

export function validateTransferCatalogFilters(raw) {
  const parsed = safeJSON(raw, SETTINGS_TRANSFER_LIMITS.catalogFiltersChars, 'Catalog filters');
  if (parsed.status !== 'ok' || !safeObjectTree(parsed.value)) {
    return { status: 'invalid', error: parsed.error ?? 'Catalog filters contain reserved property names.' };
  }
  const checked = readCatalogFilters(raw);
  if (checked.status !== 'ok') return { status: 'invalid', error: 'Catalog filters do not match the supported catalog rule format.' };
  try {
    if (checked.rules.some(rule => filterColor(rule.color) === null)) {
      return { status: 'invalid', error: 'A catalog filter color is invalid.' };
    }
  } catch { return { status: 'invalid', error: 'Catalog filter colors could not be checked.' }; }
  return { status: 'ok', count: checked.rules.length };
}

export function validateCatalogSettings(raw) {
  const parsed = safeJSON(raw, SETTINGS_TRANSFER_LIMITS.catalogSettingsChars, 'Catalog settings');
  if (parsed.status !== 'ok' || !sameKeys(parsed.value, ['orderby', 'large', 'extended']) || !safeObjectTree(parsed.value)
    || !CATALOG_ORDERS.has(parsed.value.orderby) || typeof parsed.value.large !== 'boolean'
    || typeof parsed.value.extended !== 'boolean') {
    return { status: 'invalid', error: parsed.error ?? 'Catalog settings do not match the supported format.' };
  }
  return { status: 'ok', value: parsed.value };
}

export function checkTransferValues(values) {
  if (!record(values) || !safeObjectTree(values)) {
    return { status: 'invalid', error: 'Restore values must be an object without reserved property names.' };
  }
  const keys = Object.keys(values);
  if (!keys.includes('4chan-settings') || keys.some(key => !SETTINGS_TRANSFER_STORAGE_KEYS.includes(key))) {
    return { status: 'invalid', error: 'Restore values contain unsupported storage keys.' };
  }
  const normalized = {};
  for (const key of keys) {
    const raw = values[key];
    if (typeof raw !== 'string') return { status: 'invalid', error: `Restore value for ${key} must be text.` };
    const checked = key === '4chan-settings' ? validateTransferSettings(raw)
      : key === '4chan-filters' ? validateTransferFilters(raw)
        : key === '4chan-css' ? (raw === '' ? { status: 'invalid', error: 'Empty Custom CSS must be omitted.' } : validateTransferCSS(raw))
          : key === 'catalog-filters' ? validateTransferCatalogFilters(raw) : validateCatalogSettings(raw);
    if (checked.status !== 'ok') return { status: 'invalid', error: checked.error };
    normalized[key] = raw;
  }
  return { status: 'ok', values: normalized };
}

function checkedPayload(payload, { httpsAvailable = false } = {}) {
  if (!record(payload) || !safeObjectTree(payload)) {
    return { status: 'invalid', error: 'The restore payload must be an object without reserved property names.' };
  }
  const keys = Object.keys(payload);
  if (!keys.includes('settings') || keys.some(key => !PAYLOAD_KEYS.has(key))) {
    return { status: 'invalid', error: 'The restore payload contains unsupported fields.' };
  }
  if (typeof payload.settings !== 'string') return { status: 'invalid', error: 'The restore payload is missing settings.' };
  for (const key of ['filters', 'css', 'catalogFilters', 'catalogSettings']) {
    if (Object.hasOwn(payload, key) && typeof payload[key] !== 'string') {
      return { status: 'invalid', error: `The ${key} field must be text.` };
    }
  }

  const settings = validateTransferSettings(payload.settings);
  if (settings.status !== 'ok') return settings;
  const values = { '4chan-settings': payload.settings };
  const details = [`Preferences: ${Object.keys(settings.value).length} saved value${Object.keys(settings.value).length === 1 ? '' : 's'}`];
  const review = {
    settings: Object.entries(settings.value).sort(([left], [right]) => left.localeCompare(right)).map(([key, value]) => ({
      key,
      value: typeof value === 'object' ? JSON.stringify(value) : String(value),
      inactiveCompatibility: inactiveSetting(key, httpsAvailable),
    })),
    filters: null,
    css: null,
    catalogFilters: null,
    catalogSettings: null,
  };

  if (Object.hasOwn(payload, 'filters')) {
    const filters = validateTransferFilters(payload.filters);
    if (filters.status !== 'ok') return filters;
    values['4chan-filters'] = payload.filters;
    details.push(`Filters: ${filters.count}`);
    review.filters = { count: filters.count, raw: payload.filters };
  }
  if (Object.hasOwn(payload, 'css') && payload.css !== '') {
    const css = validateTransferCSS(payload.css);
    if (css.status !== 'ok') return css;
    values['4chan-css'] = payload.css;
    details.push(`Custom CSS: ${css.count} rule${css.count === 1 ? '' : 's'}`);
    review.css = { count: css.count, raw: payload.css };
  }
  if (Object.hasOwn(payload, 'catalogFilters')) {
    const filters = validateTransferCatalogFilters(payload.catalogFilters);
    if (filters.status !== 'ok') return filters;
    values['catalog-filters'] = payload.catalogFilters;
    details.push(`Catalog filters: ${filters.count}`);
    review.catalogFilters = { count: filters.count, raw: payload.catalogFilters };
  }
  if (Object.hasOwn(payload, 'catalogSettings')) {
    const catalog = validateCatalogSettings(payload.catalogSettings);
    if (catalog.status !== 'ok') return catalog;
    values['catalog-settings'] = payload.catalogSettings;
    details.push('Catalog display preferences');
    review.catalogSettings = payload.catalogSettings;
  }
  const checkedValues = checkTransferValues(values);
  if (checkedValues.status !== 'ok') return checkedValues;
  return { status: 'ok', payload, values: checkedValues.values, details, review };
}

function encodePayload(payload) {
  const decoded = JSON.stringify(payload);
  if (decoded.length > SETTINGS_TRANSFER_LIMITS.decodedChars) return { status: 'invalid', error: 'The settings export is too large.' };
  if (encoder.encode(decoded).byteLength > SETTINGS_TRANSFER_LIMITS.decodedBytes) return { status: 'invalid', error: 'The settings export is too large.' };
  const encoded = encodeURIComponent(decoded);
  if (encoded.length > SETTINGS_TRANSFER_LIMITS.encodedChars) return { status: 'invalid', error: 'The settings export is too large.' };
  return { status: 'ok', decoded, encoded };
}

export function parseSettingsTransferHash(hash, capabilities) {
  if (typeof hash !== 'string' || !hash.startsWith('#cfg=')) return { status: 'none' };
  if (hash.length > SETTINGS_TRANSFER_LIMITS.encodedChars + 5) {
    return { status: 'invalid', error: 'The restore link is too large.' };
  }
  const encoded = hash.slice(5);
  if (!encoded || encoded.length > SETTINGS_TRANSFER_LIMITS.encodedChars) {
    return { status: 'invalid', error: 'The restore link is too large.' };
  }
  let decoded;
  try { decoded = decodeURIComponent(encoded); }
  catch { return { status: 'invalid', error: 'The restore link is not correctly encoded.' }; }
  if (decoded.length > SETTINGS_TRANSFER_LIMITS.decodedChars) return { status: 'invalid', error: 'The restore payload is too large.' };
  if (encoder.encode(decoded).byteLength > SETTINGS_TRANSFER_LIMITS.decodedBytes) {
    return { status: 'invalid', error: 'The restore payload is too large.' };
  }
  let payload;
  try { payload = JSON.parse(decoded); }
  catch { return { status: 'invalid', error: 'The restore payload is not valid JSON.' }; }
  return checkedPayload(payload, capabilities);
}

function readForExport(readItem, key) {
  try {
    const value = readItem(key);
    return value === null || typeof value === 'string' ? { status: 'ok', value } : { status: 'invalid' };
  } catch { return { status: 'unavailable' }; }
}

export function buildSettingsTransfer(readItem, { httpsAvailable = false } = {}) {
  if (typeof readItem !== 'function') return { status: 'invalid', error: 'Settings storage is unavailable.' };
  const storedSettings = readForExport(readItem, '4chan-settings');
  if (storedSettings.status !== 'ok') return { status: 'invalid', error: 'Settings storage could not be read.' };
  const settings = storedSettings.value ?? '{}';
  const settingsResult = validateTransferSettings(settings);
  if (settingsResult.status !== 'ok') return { status: 'invalid', error: `Stored settings cannot be exported: ${settingsResult.error}` };
  const payload = { settings };
  const inactiveCompatibility = Object.keys(settingsResult.value).filter(key => inactiveSetting(key, httpsAvailable));

  for (const [storageKey, field, validate] of [
    ['4chan-filters', 'filters', validateTransferFilters],
    ['4chan-css', 'css', validateTransferCSS],
    ['catalog-filters', 'catalogFilters', validateTransferCatalogFilters],
    ['catalog-settings', 'catalogSettings', validateCatalogSettings],
  ]) {
    const stored = readForExport(readItem, storageKey);
    if (stored.status !== 'ok') return { status: 'invalid', error: `${storageKey} could not be read.` };
    if (stored.value === null || stored.value === '') continue;
    const checked = validate(stored.value);
    if (checked.status !== 'ok') return { status: 'invalid', error: `Stored ${field} cannot be exported: ${checked.error}` };
    payload[field] = stored.value;
  }
  const encoded = encodePayload(payload);
  if (encoded.status !== 'ok') return encoded;
  return { status: 'ok', payload, encoded: encoded.encoded, inactiveCompatibility };
}

export function canonicalBoardURL(href) {
  let url;
  try { url = new URL(href); }
  catch { return null; }
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) return null;
  const match = url.pathname.match(/^\/([a-z0-9]{1,10})(?:\/|$)/);
  return match ? `${url.origin}/${match[1]}/` : null;
}

export function settingsTransferURL(href, readItem, capabilities) {
  const base = canonicalBoardURL(href);
  if (!base) return { status: 'invalid', error: 'Settings export is available only on a board page.' };
  const transfer = buildSettingsTransfer(readItem, capabilities);
  if (transfer.status !== 'ok') return transfer;
  return { ...transfer, url: `${base}#cfg=${transfer.encoded}` };
}

function stripTransferHash(window) {
  try { window.history.replaceState(window.history.state, '', window.location.pathname + window.location.search); }
  catch { /* The review remains safe even if history cannot be changed. */ }
}

export function mountNativeSettingsTransfer({ root, readItem, restore, httpsAvailable = false } = {}) {
  const document = root?.ownerDocument;
  const window = document?.defaultView;
  if (!root || !window || !root.isConnected || typeof readItem !== 'function' || typeof restore !== 'function') return null;
  mounts.get(root)?.destroy();

  let active = null;
  let pendingReview = null;
  let suspended = false;
  let destroyed = false;
  let controller = null;

  function node(tag, text, className) {
    const element = document.createElement(tag);
    if (text !== undefined) element.textContent = text;
    if (className) element.className = className;
    return element;
  }

  function button(text, action, className) {
    const element = node('button', text, className);
    element.type = 'button';
    element.addEventListener('click', action);
    return element;
  }

  function panel(titleText, id) {
    const dialog = node('dialog', undefined, 'nativeSettings extensionSettings nativeSettingsTransfer');
    dialog.id = id;
    const titleId = `${id}-title`;
    dialog.setAttribute('aria-labelledby', titleId);
    const body = node('div', undefined, 'extPanel nativeSettingsBody');
    const heading = node('h2', undefined, 'panelHeader');
    const title = node('span', titleText); title.id = titleId;
    const dismiss = node('button', '\u00d7', 'panelCtrl'); dismiss.type = 'button'; dismiss.setAttribute('aria-label', `Close ${titleText}`);
    heading.append(title, dismiss); body.append(heading); dialog.append(body);
    return { dialog, body, dismiss };
  }

  function closeActive(focus = true, keepReview = false) {
    const current = active;
    if (!current) return;
    active = null;
    current.restoreController?.abort();
    try { current.dialog.close(); } catch { /* Detached dialog. */ }
    current.dialog.remove();
    if (current.kind === 'review' && !keepReview) pendingReview = null;
    if (focus && current.opener?.isConnected) current.opener.focus();
  }

  function openError(error, opener = null) {
    if (destroyed || suspended) return;
    closeActive(false);
    const { dialog, body, dismiss } = panel('Restore Settings', 'settingsTransferError');
    const message = node('p', error, 'settingsMessage'); message.setAttribute('role', 'alert');
    const actions = node('div', undefined, 'center');
    const close = button('Close', () => closeActive()); actions.append(close);
    body.append(message, actions); document.body.append(dialog);
    active = { kind: 'error', dialog, opener };
    const dismissAction = () => closeActive();
    dismiss.addEventListener('click', dismissAction);
    dialog.addEventListener('cancel', event => { event.preventDefault(); dismissAction(); });
    dialog.showModal(); close.focus();
  }

  function openExport(opener = null) {
    if (destroyed || suspended) return;
    if (!root.isConnected) { destroy(); return; }
    pendingReview = null;
    closeActive(false);
    const transfer = settingsTransferURL(window.location.href, readItem, { httpsAvailable });
    const { dialog, body, dismiss } = panel('Export Settings', 'exportSettings');
    const intro = node('p', transfer.status === 'ok'
      ? 'Copy and save this URL, or bookmark the Restore link, to move these preferences to another browser.'
      : transfer.error, 'settingsMessage');
    intro.setAttribute('role', transfer.status === 'ok' ? 'status' : 'alert');
    body.append(intro);
    let field = null;
    if (transfer.status === 'ok') {
      if (transfer.inactiveCompatibility.length) {
        body.append(node('p', `This export preserves inactive compatibility values (${transfer.inactiveCompatibility.join(', ')}). They are not enabled by this version.`, 'settingsTransferCompatibility'));
      }
      if (httpsAvailable) body.append(node('p', 'The HTTPS host cookie is not included. Choose Always use HTTPS in Settings on the receiving browser to enable it.', 'settingsTransferHTTPS'));
      field = node('input', undefined, 'export-field'); field.type = 'text'; field.readOnly = true;
      field.value = transfer.url; field.setAttribute('aria-label', 'Settings export URL');
      const fieldRow = node('p', undefined, 'center'); fieldRow.append(field);
      const restoreRow = node('p', undefined, 'center');
      const link = node('a', 'Restore Settings'); link.href = transfer.url; link.target = '_blank'; link.rel = 'noopener noreferrer';
      restoreRow.append('[', link, ']'); body.append(fieldRow, restoreRow);
    }
    const actions = node('div', undefined, 'center');
    const close = button('Close', () => closeActive()); actions.append(close); body.append(actions);
    document.body.append(dialog); active = { kind: 'export', dialog, opener };
    const dismissAction = () => closeActive();
    dismiss.addEventListener('click', dismissAction);
    dialog.addEventListener('cancel', event => { event.preventDefault(); dismissAction(); });
    dialog.showModal();
    if (field) { field.focus(); field.select(); } else close.focus();
  }

  function snapshotExpected(values) {
    const expected = {};
    try {
      for (const key of Object.keys(values)) {
        const current = readItem(key);
        if (current !== null && (typeof current !== 'string' || current.length > SETTINGS_TRANSFER_LIMITS.existingValueChars)) return null;
        expected[key] = current;
      }
    } catch { return null; }
    return expected;
  }

  function openReview(review = pendingReview) {
    if (!review || destroyed || suspended) return;
    if (!root.isConnected) { destroy(); return; }
    closeActive(false, true);
    const { dialog, body, dismiss } = panel('Restore Settings', 'restoreSettings');
    const warning = node('p', 'Review the preferences below. Nothing is changed until you choose Restore Settings.');
    const list = node('ul', undefined, 'settingsTransferReview');
    if (!review.preview.settings.length) list.append(node('li', 'Preferences: browser defaults'));
    for (const setting of review.preview.settings) {
      const suffix = setting.inactiveCompatibility ? ' (inactive compatibility value)'
        : setting.key === 'forceHTTPS' ? ' (HTTPS host cookie is unchanged)' : '';
      list.append(node('li', `${setting.key}: ${setting.value}${suffix}`));
    }
    function rawDetails(summaryText, raw, className) {
      const details = node('details', undefined, className);
      const summary = node('summary', summaryText);
      const content = node('pre', raw);
      details.append(summary, content);
      return details;
    }
    const rawSections = [];
    if (review.preview.filters) rawSections.push(rawDetails(`Filters (${review.preview.filters.count})`, review.preview.filters.raw, 'settingsTransferFilters'));
    if (review.preview.css) rawSections.push(rawDetails(`Custom CSS (${review.preview.css.count} rule${review.preview.css.count === 1 ? '' : 's'})`, review.preview.css.raw, 'settingsTransferCSS'));
    if (review.preview.catalogFilters) rawSections.push(rawDetails(`Catalog filters (${review.preview.catalogFilters.count})`, review.preview.catalogFilters.raw, 'settingsTransferCatalogFilters'));
    if (review.preview.catalogSettings) rawSections.push(rawDetails('Catalog display preferences', review.preview.catalogSettings, 'settingsTransferCatalog'));
    const message = node('p', '', 'settingsMessage settingsTransferMessage'); message.setAttribute('role', 'status');
    const actions = node('div', undefined, 'center');
    const confirm = button('Restore Settings', async () => {
      const current = active;
      if (!current || current.dialog !== dialog || confirm.disabled) return;
      const restoreController = new AbortController(); current.restoreController = restoreController;
      confirm.disabled = true; message.textContent = 'Restoring settings...';
      try {
        const result = await restore(review.values, review.expected, restoreController.signal);
        if (active !== current || restoreController.signal.aborted) return;
        if (result?.status === 'conflict') {
          message.textContent = 'Settings changed after this review opened. Reopen the restore link to review the newer state before trying again.';
          return;
        }
        if (result?.status === 'invalid') {
          message.textContent = 'The settings restore was rejected. Nothing was restored.';
          return;
        }
        if (result?.status === 'unavailable') {
          message.textContent = 'Restore is unavailable because persistent browser storage or cross-tab locking is unavailable. Nothing was changed.';
          return;
        }
        if (result?.status === 'storage-error') {
          message.textContent = result.partial
            ? 'Browser storage failed during restore, and rollback could not fully restore the previous values. Review this browser\'s stored preferences before retrying.'
            : 'Browser storage failed during restore. The previous stored values were restored; retry after resolving the storage problem.';
          return;
        }
        if (result?.status !== 'ok') {
          message.textContent = 'Settings could not be restored. Try again.';
          return;
        }
        pendingReview = null;
        document.dispatchEvent(new window.CustomEvent('4chanPreferencesRestored', {
          detail: { persisted: result.persisted !== false, keys: Object.keys(review.values) },
        }));
        document.dispatchEvent(new window.CustomEvent('4chanSettingsSaved'));
        message.textContent = 'Settings restored.';
        confirm.disabled = true;
        cancel.textContent = 'Close';
      } catch {
        if (active === current && !restoreController.signal.aborted) message.textContent = 'Settings could not be restored. Try again.';
      } finally {
        if (active === current && current.restoreController === restoreController) {
          current.restoreController = null;
          if (pendingReview) confirm.disabled = false;
        }
      }
    });
    const cancel = button('Cancel', () => closeActive());
    actions.append(confirm, cancel); body.append(warning, list, ...rawSections, message, actions); document.body.append(dialog);
    active = { kind: 'review', dialog, opener: review.opener ?? null, restoreController: null };
    const dismissAction = () => closeActive();
    dismiss.addEventListener('click', dismissAction);
    dialog.addEventListener('cancel', event => { event.preventDefault(); dismissAction(); });
    dialog.showModal(); confirm.focus();
  }

  function consumeHash() {
    const hash = window.location.hash;
    if (!hash.startsWith('#cfg=')) return;
    const parsed = parseSettingsTransferHash(hash, { httpsAvailable });
    stripTransferHash(window);
    if (parsed.status !== 'ok') {
      pendingReview = null;
      openError(parsed.error ?? 'The settings restore link is invalid.');
      return;
    }
    const expected = snapshotExpected(parsed.values);
    if (!expected) {
      pendingReview = null;
      openError('Current settings could not be read, so this restore was not started.');
      return;
    }
    pendingReview = { values: parsed.values, expected, details: parsed.details, preview: parsed.review };
    openReview(pendingReview);
  }

  function refresh() {
    if (destroyed) return;
    if (!root.isConnected) { destroy(); return; }
    if (suspended) return;
    consumeHash();
  }

  const onStorage = event => {
    if (event.key !== null && !SETTINGS_TRANSFER_STORAGE_KEYS.includes(event.key)) return;
    if (active?.kind === 'export') openExport(active.opener);
  };
  const onHashChange = () => refresh();
  const onPageHide = event => {
    if (!event.persisted) { destroy(); return; }
    suspended = true;
    closeActive(false, active?.kind === 'review');
  };
  const onPageShow = event => {
    if (!event.persisted || destroyed) return;
    suspended = false;
    if (pendingReview) openReview(pendingReview); else refresh();
  };
  const detachObserver = new window.MutationObserver(() => { if (!root.isConnected) destroy(); });

  function destroy() {
    if (destroyed) return;
    destroyed = true;
    closeActive(false);
    pendingReview = null;
    window.removeEventListener('storage', onStorage);
    window.removeEventListener('hashchange', onHashChange);
    window.removeEventListener('pagehide', onPageHide);
    window.removeEventListener('pageshow', onPageShow);
    detachObserver.disconnect();
    if (mounts.get(root) === controller) mounts.delete(root);
  }

  window.addEventListener('storage', onStorage);
  window.addEventListener('hashchange', onHashChange);
  window.addEventListener('pagehide', onPageHide);
  window.addEventListener('pageshow', onPageShow);
  controller = { openExport, refresh, destroy, hasPendingReview: () => pendingReview !== null };
  mounts.set(root, controller);
  detachObserver.observe(document.documentElement, { childList: true, subtree: true });
  refresh();
  return controller;
}
