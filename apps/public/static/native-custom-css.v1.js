const CSS_KEY = '4chan-css';
const SETTINGS_KEY = '4chan-settings';

export const CUSTOM_CSS_LIMITS = Object.freeze({
  bytes: 16384,
  rules: 64,
  selectorsPerRule: 8,
  declarationsPerRule: 16,
  declarations: 256,
});

const SELECTORS = new Map([
  ['.post', '.post'],
  ['div.post', '.post'],
  ['.op', '.post.op'],
  ['div.op', '.post.op'],
  ['.reply', '.post.reply'],
  ['div.reply', '.post.reply'],
  ['.postmessage', '.post .postMessage'],
  ['blockquote.postmessage', '.post .postMessage'],
  ['.postinfo', '.post .postInfo'],
  ['.name', '.post .postInfo .name'],
  ['.subject', '.post .postInfo .subject'],
  ['.quotelink', '.post .postMessage .quotelink'],
  ['a.quotelink', '.post .postMessage .quotelink'],
  ['.filetext', '.post .fileText'],
]);

const CATALOG_SELECTORS = new Map([
  ['.thread', '#threads > .thread'],
  ['.teaser', '#threads > .thread .teaser'],
  ['.meta', '#threads > .thread .meta'],
  ['.thumb', '#threads > .thread .thumb'],
  ['.txt-sub', '#threads .txt-sub'],
  ['.txt-rep', '#threads .txt-rep'],
  ['.txt-date', '#threads .txt-date'],
]);

const COLOR_PROPERTIES = new Set(['color', 'background-color', 'border-color']);
const FONT_SIZES = new Set([
  '8px', '9px', '10px', '11px', '12px', '13px', '14px', '15px', '16px', '18px', '20px', '22px', '24px',
  '0.75em', '0.8em', '0.9em', '1em', '1.1em', '1.2em', '1.25em', '1.5em',
]);
const FONT_WEIGHTS = new Set(['normal', 'bold', '400', '700']);
const FONT_STYLES = new Set(['normal', 'italic']);
const LINE_HEIGHTS = new Set(['normal', '1', '1.1', '1.2', '1.25', '1.3', '1.4', '1.5', '1.6', '1.75', '2']);
const TEXT_ALIGNS = new Set(['left', 'right', 'center', 'justify']);
const LETTER_SPACING = new Set(['normal', '-1px', '-0.5px', '0', '0.5px', '1px', '2px']);
const SPACING = new Set([
  '0', '1px', '2px', '3px', '4px', '5px', '6px', '8px', '10px', '12px', '16px', '20px', '24px',
  '0.25em', '0.5em', '0.75em', '1em',
]);
const SPACING_PROPERTIES = new Set([
  'margin', 'margin-top', 'margin-right', 'margin-bottom', 'margin-left',
  'padding', 'padding-top', 'padding-right', 'padding-bottom', 'padding-left',
]);
const FONT_FAMILIES = new Map([
  ['arial', 'Arial'],
  ['helvetica', 'Helvetica'],
  ['arial, helvetica, sans-serif', 'Arial, Helvetica, sans-serif'],
  ['verdana', 'Verdana'],
  ['tahoma', 'Tahoma'],
  ['georgia', 'Georgia'],
  ['"times new roman", times, serif', '"Times New Roman", Times, serif'],
  ['monospace', 'monospace'],
  ['sans-serif', 'sans-serif'],
  ['serif', 'serif'],
]);

const mounts = new WeakMap();

function invalid(error) {
  return { status: 'invalid', error };
}

function normalizeColor(value) {
  return /^#[0-9a-f]{3}(?:[0-9a-f]{3})?$/i.test(value) ? value.toLowerCase() : null;
}

function normalizeSpacing(value, shorthand) {
  const parts = value.split(/\s+/);
  if (parts.length < 1 || parts.length > (shorthand ? 4 : 1) || parts.some(part => !SPACING.has(part))) return null;
  return parts.join(' ');
}

function normalizeValue(property, value) {
  if (COLOR_PROPERTIES.has(property)) return normalizeColor(value);
  if (property === 'font-family') return FONT_FAMILIES.get(value.toLowerCase()) ?? null;
  if (property === 'font-size') return FONT_SIZES.has(value) ? value : null;
  if (property === 'font-weight') return FONT_WEIGHTS.has(value.toLowerCase()) ? value.toLowerCase() : null;
  if (property === 'font-style') return FONT_STYLES.has(value.toLowerCase()) ? value.toLowerCase() : null;
  if (property === 'line-height') return LINE_HEIGHTS.has(value.toLowerCase()) ? value.toLowerCase() : null;
  if (property === 'text-align') return TEXT_ALIGNS.has(value.toLowerCase()) ? value.toLowerCase() : null;
  if (property === 'letter-spacing') return LETTER_SPACING.has(value.toLowerCase()) ? value.toLowerCase() : null;
  if (SPACING_PROPERTIES.has(property)) return normalizeSpacing(value, property === 'margin' || property === 'padding');
  return null;
}

function skipWhitespace(raw, start) {
  let index = start;
  while (index < raw.length && /\s/.test(raw[index])) index++;
  return index;
}

export function parseCustomCSS(raw) {
  return parseStyles(raw, SELECTORS, selector => `.board ${selector}`, 'post');
}

export function parseCatalogCSS(raw) {
  return parseStyles(raw, CATALOG_SELECTORS, selector => selector, 'catalog');
}

function parseStyles(raw, allowedSelectors, scope, label) {
  if (typeof raw !== 'string') return invalid('Custom CSS must be text.');
  if (raw.length > CUSTOM_CSS_LIMITS.bytes) return invalid('Custom CSS must be 16 KiB or smaller.');
  const bytes = new TextEncoder().encode(raw).byteLength;
  if (bytes > CUSTOM_CSS_LIMITS.bytes) return invalid('Custom CSS must be 16 KiB or smaller.');
  if (/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/.test(raw)) return invalid('Control characters are not allowed.');
  if (raw.includes('\\')) return invalid('CSS escapes are not supported.');
  if (raw.includes('/*') || raw.includes('*/')) return invalid('CSS comments are not supported.');
  if (raw.includes('@')) return invalid('CSS at-rules are not allowed.');
  if (/\b(?:url|var|attr|calc|min|max|clamp|image|image-set)\s*\(/i.test(raw)) return invalid('CSS functions are not allowed.');

  const rules = [];
  let declarationCount = 0;
  let position = 0;
  while ((position = skipWhitespace(raw, position)) < raw.length) {
    if (rules.length >= CUSTOM_CSS_LIMITS.rules) return invalid(`Use at most ${CUSTOM_CSS_LIMITS.rules} rules.`);
    const open = raw.indexOf('{', position);
    if (open === -1) return invalid(`Rule ${rules.length + 1} is missing an opening brace.`);
    if (raw.slice(position, open).includes('}')) return invalid(`Rule ${rules.length + 1} has invalid brace order.`);
    const close = raw.indexOf('}', open + 1);
    if (close === -1 || raw.slice(open + 1, close).includes('{')) return invalid(`Rule ${rules.length + 1} has invalid braces.`);

    const selectorText = raw.slice(position, open).trim();
    if (!selectorText) return invalid(`Rule ${rules.length + 1} needs a selector.`);
    const selectorInputs = selectorText.split(',').map(value => value.trim());
    if (!selectorInputs.length || selectorInputs.length > CUSTOM_CSS_LIMITS.selectorsPerRule || selectorInputs.some(value => !value)) {
      return invalid(`Rule ${rules.length + 1} may use at most ${CUSTOM_CSS_LIMITS.selectorsPerRule} selectors.`);
    }
    const selectors = [];
    for (const selector of selectorInputs) {
      const normalized = allowedSelectors.get(selector.toLowerCase());
      if (!normalized) return invalid(`Rule ${rules.length + 1}: selector "${selector}" is not an allowed ${label} selector.`);
      if (!selectors.includes(normalized)) selectors.push(normalized);
    }

    const body = raw.slice(open + 1, close).trim();
    if (!body) return invalid(`Rule ${rules.length + 1} needs at least one declaration.`);
    const declarations = [];
    const seen = new Set();
    for (const input of body.split(';')) {
      const declaration = input.trim();
      if (!declaration) continue;
      if (declarations.length >= CUSTOM_CSS_LIMITS.declarationsPerRule) {
        return invalid(`Rule ${rules.length + 1} may use at most ${CUSTOM_CSS_LIMITS.declarationsPerRule} declarations.`);
      }
      if (declarationCount >= CUSTOM_CSS_LIMITS.declarations) return invalid(`Use at most ${CUSTOM_CSS_LIMITS.declarations} declarations.`);
      const colon = declaration.indexOf(':');
      if (colon <= 0 || declaration.indexOf(':', colon + 1) !== -1) return invalid(`Rule ${rules.length + 1} has an invalid declaration.`);
      const property = declaration.slice(0, colon).trim().toLowerCase();
      const value = declaration.slice(colon + 1).trim();
      if (!property || !value) return invalid(`Rule ${rules.length + 1} has an incomplete declaration.`);
      if (property.startsWith('--') || /\bvar\s*\(/i.test(value)) return invalid('CSS variables are not allowed.');
      if (seen.has(property)) return invalid(`Rule ${rules.length + 1}: property "${property}" is duplicated.`);
      const normalized = normalizeValue(property, value);
      if (normalized === null) return invalid(`Rule ${rules.length + 1}: "${property}: ${value}" is not an allowed value.`);
      seen.add(property);
      declarations.push([property, normalized]);
      declarationCount++;
    }
    if (!declarations.length) return invalid(`Rule ${rules.length + 1} needs at least one declaration.`);
    rules.push({ selectors, declarations });
    position = close + 1;
  }

  const css = rules.map(rule => `${rule.selectors.map(scope).join(', ')} { ${rule.declarations
    .map(([property, value]) => `${property}: ${value};`).join(' ')} }`).join('\n');
  return { status: 'ok', bytes, rules, css };
}

function record(value) {
  return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
}

export function mountNativeCustomCSS({ root, settings, readCSS, saveCSS } = {}) {
  const document = root?.ownerDocument;
  const window = document?.defaultView;
  if (!root || !window || !root.isConnected || !root.matches?.('.board') || typeof settings !== 'function'
    || typeof readCSS !== 'function' || typeof saveCSS !== 'function') return null;
  mounts.get(root)?.destroy();

  let sheet = null;
  try {
    if (typeof window.CSSStyleSheet === 'function' && Array.isArray(document.adoptedStyleSheets)) sheet = new window.CSSStyleSheet();
  } catch { sheet = null; }
  let active = null;
  let suspended = false;
  let destroyed = false;
  let appliedRaw = null;
  let controller = null;

  function configuration() {
    try { return record(settings()); }
    catch { return {}; }
  }

  function featureEnabled(config = configuration()) {
    return config.disableAll !== true && config.customCSS === true;
  }

  function storedCSS() {
    try { return readCSS(); }
    catch { return null; }
  }

  function removeSheet() {
    appliedRaw = null;
    if (!sheet) return;
    try {
      const adopted = document.adoptedStyleSheets;
      if (adopted.includes(sheet)) document.adoptedStyleSheets = adopted.filter(candidate => candidate !== sheet);
    } catch { /* Safe application is unavailable in this browser. */ }
  }

  function apply(raw) {
    if (!sheet || raw === appliedRaw) return;
    const parsed = parseCustomCSS(raw);
    if (parsed.status !== 'ok' || parsed.rules.length === 0) { removeSheet(); return; }
    try {
      sheet.replaceSync(parsed.css);
      const adopted = document.adoptedStyleSheets;
      if (!adopted.includes(sheet)) document.adoptedStyleSheets = [...adopted, sheet];
      appliedRaw = raw;
    } catch { removeSheet(); }
  }

  function close(focus = true) {
    const current = active;
    if (!current) return;
    active = null;
    current.saveController?.abort();
    try { current.dialog.close(); } catch { /* Detached dialog. */ }
    current.dialog.remove();
    if (focus && current.opener?.isConnected) current.opener.focus();
  }

  function refresh() {
    if (destroyed) return;
    if (!root.isConnected) { destroy(); return; }
    if (suspended) return;
    const config = configuration();
    const enabled = featureEnabled(config);
    if (active?.saveController && (config.disableAll === true || (active.saveStartedEnabled && !enabled))) {
      active.saveController.abort();
      active.saveController = null;
      active.submit.disabled = false;
      active.message.textContent = 'The pending save was cancelled because Custom CSS was disabled.';
    }
    if (!enabled) { removeSheet(); return; }
    const raw = storedCSS();
    if (typeof raw !== 'string' || raw === '') { removeSheet(); return; }
    apply(raw);
  }

  function node(tag, text, className) {
    const element = document.createElement(tag);
    if (text !== undefined) element.textContent = text;
    if (className) element.className = className;
    return element;
  }

  function open(opener = null) {
    if (destroyed || suspended) return;
    if (!root.isConnected) { destroy(); return; }
    close(false);
    const raw = storedCSS();
    const expected = raw;
    const dialog = node('dialog', undefined, 'nativeSettings extensionSettings nativeCustomCSS');
    dialog.id = 'customCSSMenu';
    dialog.setAttribute('aria-labelledby', 'custom-css-title');
    const body = node('div', undefined, 'extPanel nativeSettingsBody');
    const heading = node('h2', undefined, 'panelHeader');
    const title = node('span', 'Custom CSS'); title.id = 'custom-css-title';
    const dismiss = node('button', '\u00d7', 'panelCtrl');
    dismiss.type = 'button'; dismiss.setAttribute('aria-label', 'Close Custom CSS');
    heading.append(title, dismiss);
    const form = node('form');
    const help = node('details', undefined, 'customCSSHelp');
    const helpSummary = node('summary', 'Allowed CSS syntax');
    const selectorHelp = node('p', 'Selectors: .post, .op, .reply, .postMessage, .postInfo, .name, .subject, .quotelink, .fileText.');
    const propertyHelp = node('p', 'Properties: color, background-color, border-color, font-family, font-size, font-weight, font-style, line-height, text-align, letter-spacing, margin, and padding. Colors use #rgb or #rrggbb.');
    const valueHelp = node('p', 'Font families: Arial; Helvetica; Arial, Helvetica, sans-serif; Verdana; Tahoma; Georgia; "Times New Roman", Times, serif; monospace; sans-serif; serif. Font sizes: 8-16, 18, 20, 22, or 24px; 0.75, 0.8, 0.9, 1, 1.1, 1.2, 1.25, or 1.5em. Weight: normal, bold, 400, or 700. Style: normal or italic. Line height: normal, 1, 1.1, 1.2, 1.25, 1.3, 1.4, 1.5, 1.6, 1.75, or 2. Alignment: left, right, center, or justify. Letter spacing: normal, -1px, -0.5px, 0, 0.5px, 1px, or 2px. Margin/padding values: 0; 1, 2, 3, 4, 5, 6, 8, 10, 12, 16, 20, or 24px; 0.25, 0.5, 0.75, or 1em. Shorthands may use up to four values; side properties use one.');
    const exampleHelp = node('p');
    exampleHelp.append('Example: ', node('code', '.postMessage { color: #336699; font-size: 14px; }'));
    help.append(helpSummary, selectorHelp, propertyHelp, valueHelp, exampleHelp);
    const label = node('label', 'Post CSS'); label.htmlFor = 'customCSSBox';
    const input = node('textarea'); input.id = 'customCSSBox'; input.rows = 12;
    input.maxLength = CUSTOM_CSS_LIMITS.bytes; input.spellcheck = false; input.autocapitalize = 'off';
    if (typeof raw === 'string' && raw.length <= CUSTOM_CSS_LIMITS.bytes) input.value = raw;
    const message = node('p', '', 'settingsMessage customCSSEditorMessage'); message.setAttribute('role', 'status');
    const parsedStored = typeof raw === 'string' ? parseCustomCSS(raw) : null;
    if (typeof raw === 'string' && (raw.length > CUSTOM_CSS_LIMITS.bytes || parsedStored?.status === 'invalid')) {
      message.textContent = raw.length > CUSTOM_CSS_LIMITS.bytes
        ? 'Stored CSS is too large to edit here. Saving a valid draft will replace it.'
        : `Saved CSS is not applied: ${parsedStored.error}`;
    }
    const actions = node('div', undefined, 'center');
    const submit = node('button', 'Save CSS'); submit.type = 'submit';
    const cancel = node('button', 'Cancel'); cancel.type = 'button';
    actions.append(submit, cancel); form.append(help, label, input, message, actions);
    body.append(heading, form); dialog.append(body); document.body.append(dialog);
    active = { dialog, opener, input, message, submit, expected, saveController: null, saveStartedEnabled: false };

    const closeEditor = () => close();
    dismiss.addEventListener('click', closeEditor);
    cancel.addEventListener('click', closeEditor);
    dialog.addEventListener('cancel', event => { event.preventDefault(); closeEditor(); });
    form.addEventListener('submit', async event => {
      event.preventDefault();
      const current = active;
      if (!current || current.dialog !== dialog || submit.disabled) return;
      const config = configuration();
      if (config.disableAll === true) {
        message.textContent = 'Custom CSS cannot be saved while native features are disabled.';
        return;
      }
      const submittedRaw = input.value;
      const parsed = parseCustomCSS(submittedRaw);
      if (parsed.status !== 'ok') { message.textContent = parsed.error; return; }
      const saveController = new AbortController();
      current.saveController = saveController;
      current.saveStartedEnabled = featureEnabled(config);
      submit.disabled = true;
      message.textContent = 'Saving...';
      try {
        const result = await saveCSS(submittedRaw, current.expected, saveController.signal);
        if (active !== current || saveController.signal.aborted) return;
        if (result?.status === 'conflict') {
          message.textContent = 'Saved CSS changed in another tab. Close and reopen the editor before saving again.';
          return;
        }
        if (result?.status === 'invalid') {
          message.textContent = 'Custom CSS must be valid and no larger than 16 KiB.';
          return;
        }
        if (result?.status !== 'ok') {
          message.textContent = 'Custom CSS could not be saved. Try again.';
          return;
        }
        current.expected = submittedRaw === '' ? null : submittedRaw;
        message.textContent = result.persisted === false
          ? 'CSS is saved only in this tab. Browser storage or cross-tab locking is unavailable.'
          : 'CSS saved.';
        refresh();
      } catch {
        if (active === current && !saveController.signal.aborted) message.textContent = 'Custom CSS could not be saved. Try again.';
      } finally {
        if (active === current && current.saveController === saveController) {
          current.saveController = null;
          current.saveStartedEnabled = false;
          submit.disabled = false;
        }
      }
    });
    dialog.showModal();
    input.focus();
  }

  const onSettings = () => refresh();
  const onStorage = event => {
    if (event.key === null || event.key === CSS_KEY || event.key === SETTINGS_KEY) refresh();
  };
  const onPageHide = event => {
    if (!event.persisted) { destroy(); return; }
    suspended = true;
    close(false);
    removeSheet();
  };
  const onPageShow = event => {
    if (event.persisted && !destroyed) { suspended = false; refresh(); }
  };
  const detachObserver = new window.MutationObserver(() => {
    if (!root.isConnected) destroy();
  });

  function destroy() {
    if (destroyed) return;
    destroyed = true;
    close(false);
    removeSheet();
    document.removeEventListener('4chanSettingsSaved', onSettings);
    window.removeEventListener('storage', onStorage);
    window.removeEventListener('pagehide', onPageHide);
    window.removeEventListener('pageshow', onPageShow);
    detachObserver.disconnect();
    if (mounts.get(root) === controller) mounts.delete(root);
  }

  document.addEventListener('4chanSettingsSaved', onSettings);
  window.addEventListener('storage', onStorage);
  window.addEventListener('pagehide', onPageHide);
  window.addEventListener('pageshow', onPageShow);
  controller = { open, refresh, destroy };
  mounts.set(root, controller);
  detachObserver.observe(document.documentElement, { childList: true, subtree: true });
  refresh();
  return controller;
}
