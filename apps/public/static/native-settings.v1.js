// Release-owned settings controls. Stored strings never become HTML.
import { parseCatalogCSS } from './native-custom-css.v1.js';

// Config/ConfigMobile and Main.run in the supplied extension.js:8795–8847,
// 9444–9451, 9483–9486. This snapshot is only for proven first-run pages.
const startupDefaults = Object.freeze({
  quotePreview: true, backlinks: true, quickReply: true, threadUpdater: true, threadHiding: true,
  alwaysAutoUpdate: false, topPageNav: false, threadWatcher: false, threadAutoWatcher: false,
  imageExpansion: true, fitToScreenExpansion: false, threadExpansion: true, alwaysDepage: false,
  localTime: true, stickyNav: false, keyBinds: false, inlineQuotes: false, filter: false,
  revealSpoilers: false, imageHover: false, threadStats: true, IDColor: true, noPictures: false,
  embedYouTube: true, embedSoundCloud: false, updaterSound: false, customCSS: false,
  autoScroll: false, hideStubs: false, compactThreads: false, centeredThreads: false,
  dropDownNav: false, autoHideNav: false, classicNav: false, fixedThreadWatcher: false,
  persistentQR: false, forceHTTPS: false, darkTheme: false, linkify: false, unmuteWebm: false,
  disableAll: false,
});

export function settingsStartupDefaults({ mobileLayout = false, mobileDevice = false, disabled = false } = {}) {
  const settings = { ...startupDefaults };
  if (!disabled) {
    if (mobileLayout === true) Object.assign(settings, { embedYouTube: false, compactThreads: false, linkify: true });
    if (mobileDevice === true) Object.assign(settings, { topPageNav: false, dropDownNav: true });
  }
  return settings;
}

// Validate only for implicit first-open persistence. Unknown safe preferences
// are retained, never normalized or dropped. Failed reads must not call this as
// though they were a successful absent read.
export function readSettingsStartup(raw) {
  if (raw === null || raw === '') return { status: 'ok', settings: {} };
  if (typeof raw !== 'string' || raw.length > 4096) return { status: 'invalid' };
  try {
    const settings = JSON.parse(raw);
    if (!settings || typeof settings !== 'object' || Array.isArray(settings)) return { status: 'invalid' };
    const pending = [settings];
    while (pending.length) {
      const value = pending.pop();
      for (const key of Object.keys(value)) {
        if (['__proto__', 'prototype', 'constructor'].includes(key)) return { status: 'invalid' };
        if (typeof value[key] === 'number' && !Number.isFinite(value[key])) return { status: 'invalid' };
        if (value[key] && typeof value[key] === 'object') pending.push(value[key]);
      }
    }
    for (const key of [...Object.keys(startupDefaults), 'customMenu', 'imageHoverBg']) {
      if (Object.hasOwn(settings, key) && typeof settings[key] !== 'boolean') return { status: 'invalid' };
    }
    return { status: 'ok', settings };
  } catch { return { status: 'invalid' }; }
}

// Source SettingsMenu.options at 545b781: presentation availability only.
// Hidden preferences remain stored and are still honored by their runtimes.
const mobileSettings = new Set(`quotePreview backlinks quickReply threadUpdater alwaysAutoUpdate
  threadWatcher threadAutoWatcher threadStats threadHiding threadExpansion alwaysDepage
  imageExpansion revealSpoilers noPictures linkify darkTheme customCSS IDColor localTime disableAll`.split(/\s+/));
const desktopOnlySettings = new Set(`inlineQuotes persistentQR autoScroll updaterSound fixedThreadWatcher
  filter hideStubs dropDownNav classicNav autoHideNav customMenu topPageNav stickyNav keyBinds
  fitToScreenExpansion imageHover imageHoverBg embedYouTube embedSoundCloud compactThreads centeredThreads`.split(/\s+/));

export function settingAvailable(key, mobileLayout) {
  return mobileLayout ? mobileSettings.has(key)
    : key !== 'darkTheme' && (mobileSettings.has(key) || desktopOnlySettings.has(key));
}

// Presentation only: firstRun is the source's raw-storage truthiness check,
// not permission to initialize, repair, import, or overwrite stored preferences.
// Capture once per page; checkbox values are still read afresh on every open.
export function captureSettingsPresentation(storageRead, mobileLayout) {
  return Object.freeze({
    firstRun: storageRead?.status === 'ok' && (storageRead.raw === null || storageRead.raw === ''),
    mobileLayout: mobileLayout === true,
  });
}

export function settingsOptionChecked(key, initial, presentation) {
  if (key === 'linkify') return initial.disableAll === true ? initial.linkify === true
    : presentation.mobileLayout || initial.linkify === true;
  if (key === 'embedYouTube') return typeof initial.embedYouTube === 'boolean' ? initial.embedYouTube
    : !presentation.mobileLayout;
  return undefined;
}

export const CATALOG_THEME_LIMITS = Object.freeze({ storage: 24576, css: 16384 });

export function catalogDropDownEnabled(raw, mobileLayout) {
  if (mobileLayout || typeof raw !== 'string' || raw.length > 4096) return false;
  try {
    const settings = JSON.parse(raw);
    return !!settings && typeof settings === 'object' && !Array.isArray(settings)
      && !Object.keys(settings).some(key => ['__proto__', 'prototype', 'constructor'].includes(key))
      && settings.disableAll !== true && settings.dropDownNav !== false;
  } catch { return false; }
}

export function readCatalogTheme(raw) {
  if (raw === null) return { status: 'ok', theme: {} };
  if (typeof raw !== 'string' || raw.length > CATALOG_THEME_LIMITS.storage) return { status: 'invalid' };
  try {
    const value = JSON.parse(raw);
    if (!value || typeof value !== 'object' || Array.isArray(value)
      || Object.keys(value).some(key => !['nobinds', 'nospoiler', 'newtab', 'css'].includes(key))) return { status: 'invalid' };
    const theme = {};
    for (const key of ['nobinds', 'nospoiler', 'newtab']) {
      if (value[key] !== undefined && typeof value[key] !== 'boolean') return { status: 'invalid' };
      if (value[key] === true) theme[key] = true;
    }
    if (value.css !== undefined) {
      if (typeof value.css !== 'string' || value.css.length > CATALOG_THEME_LIMITS.css) return { status: 'invalid' };
      if (value.css) theme.css = value.css;
    }
    const styles = parseCatalogCSS(theme.css ?? '');
    return { status: 'ok', theme, ...(styles.status === 'ok' ? { styles } : { cssError: styles.error }) };
  } catch { return { status: 'invalid' }; }
}

export function writeCatalogTheme(value) {
  let raw;
  try { raw = JSON.stringify(value); } catch { return { status: 'invalid' }; }
  const checked = readCatalogTheme(raw);
  if (checked.status !== 'ok' || checked.cssError) return { status: 'invalid', error: checked.cssError ?? 'Catalog settings are invalid.' };
  raw = Object.keys(checked.theme).length ? JSON.stringify(checked.theme) : null;
  return { ...checked, raw };
}

export function installSettings({ catalog, read, save, initializeOnOpen, toggleWatcher, openFilters, clearThreads, openKeybinds, openCustomMenu, openCustomCSS, openExport, openCatalogSettings, optionChecked, hasMobileLayout = () => false, presentation }) {
  // Copy only immutable presentation flags; do not hold a mutable caller object.
  const startup = Object.freeze({ firstRun: presentation?.firstRun === true,
    mobileLayout: presentation ? presentation.mobileLayout === true : hasMobileLayout() === true });
  const navigation = document.querySelector('.boardList');
  let active = null;
  let pendingSave = null;
  let pendingInitialization = null;
  let opener = null;
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
  function link(id, text, action) {
    const element = node('a', text);
    element.id = id;
    element.href = '#settings';
    element.addEventListener('click', event => { event.preventDefault(); action(element); });
    return element;
  }
  function close() {
    pendingInitialization?.abort();
    pendingInitialization = null;
    pendingSave?.abort();
    pendingSave = null;
    if (!active) return;
    active.close();
    active.remove();
    active = null;
    opener?.focus();
  }
  function open(source) {
    if (catalog && typeof openCatalogSettings === 'function') { openCatalogSettings(source); return; }
    if (active) { close(); return; }
    opener = source;
    const initial = read();
    const mobileLayout = startup.mobileLayout;
    const dialog = node('dialog', undefined, `nativeSettings ${catalog ? 'catalogSettings panel' : 'extensionSettings UIPanel'}`);
    dialog.id = catalog ? 'theme' : 'settingsMenu';
    dialog.setAttribute('aria-labelledby', 'native-settings-title');
    const content = catalog ? dialog : node('div', undefined, 'extPanel nativeSettingsBody');
    const header = node('h2', undefined, 'panelHeader');
    const title = node('span', 'Settings');
    title.id = 'native-settings-title';
    header.append(title);
    const dismiss = button('\u00d7', close, 'panelCtrl');
    dismiss.id = catalog ? 'theme-close' : 'settings-close';
    dismiss.setAttribute('aria-label', 'Close settings');
    header.append(dismiss);
    content.append(header);
    const form = node('form');
    const fields = new Map();
    function option(parent, key, label, tip, className) {
      if (!catalog && !settingAvailable(key, mobileLayout)) return null;
      const row = node('li', undefined, className);
      const caption = node('label');
      const input = node('input', undefined, 'menuOption');
      input.type = 'checkbox';
      input.dataset.option = key;
      input.id = catalog && key === 'threadWatcher' ? 'theme-tw' : `setting-${key}`;
      const checked = optionChecked?.(key, initial, startup);
      input.checked = (typeof checked === 'boolean' ? checked
        : (['threadHiding', 'threadUpdater', 'threadExpansion', 'threadStats', 'quickReply', 'quotePreview', 'backlinks', 'imageExpansion', 'localTime', 'IDColor'].includes(key) ? initial[key] !== false : initial[key] === true))
        && (!catalog || initial.disableAll !== true);
      fields.set(key, { input, initial: input.checked });
      caption.append(input, document.createTextNode(` ${label}`));
      row.append(caption);
      parent.append(row);
      if (tip) parent.append(node('li', tip, `settings-tip ${className || ''}`));
      return input;
    }
    const categories = [];
    function addCategory(id, label) {
      const heading = node('h3', undefined, 'settings-cat-lbl');
      const list = node('ul', undefined, 'settings-cat');
      list.id = `settings-${id}`;
      list.hidden = !startup.firstRun;
      const expand = button(label, () => setExpanded(list.hidden), 'settings-expand');
      function setExpanded(expanded) {
        list.hidden = !expanded;
        expand.setAttribute('aria-expanded', String(expanded));
      }
      expand.setAttribute('aria-controls', list.id);
      expand.setAttribute('aria-label', label);
      setExpanded(!list.hidden);
      heading.append(expand);
      form.append(heading, list);
      categories.push({ list, expand, setExpanded });
      return list;
    }
    if (catalog) {
      form.append(node('h4', 'Options'));
      const options = node('ul', undefined, 'clickset');
      option(options, 'threadWatcher', 'Thread Watcher');
      form.append(options);
    } else {
      const all = node('p', undefined, 'settingsExpandAll');
      all.id = 'settings-exp-all';
      all.append('[', link('settings-expand-all', 'Expand All Settings', () => {
        for (const category of categories) category.setExpanded(true);
      }), ']');
      form.append(all);
      const quotesCategory = addCategory('quotes', 'Quotes & Replying');
      option(quotesCategory, 'quotePreview', 'Quote preview', 'Show post when mousing over post links');
      option(quotesCategory, 'backlinks', 'Backlinks', 'Show who has replied to a post');
      option(quotesCategory, 'inlineQuotes', 'Inline quote links', 'Clicking quote links will inline expand the quoted post, Shift-click to bypass inlining');
      option(quotesCategory, 'quickReply', 'Quick Reply', 'Quickly respond to a post by clicking its post number');
      option(quotesCategory, 'persistentQR', 'Persistent Quick Reply', 'Keep Quick Reply window open after posting');
      const monitoringCategory = addCategory('monitoring', 'Monitoring');
      option(monitoringCategory, 'threadUpdater', 'Thread updater', 'Append new posts to bottom of thread without refreshing the page');
      option(monitoringCategory, 'alwaysAutoUpdate', 'Auto-update by default', 'Always auto-update threads');
      option(monitoringCategory, 'threadWatcher', 'Thread Watcher', "Keep track of threads you're watching and see when they receive new posts");
      option(monitoringCategory, 'threadAutoWatcher', 'Automatically watch threads you create', '', 'settings-sub');
      option(monitoringCategory, 'autoScroll', 'Auto-scroll with auto-updated posts', 'Automatically scroll the page as new posts are added');
      option(monitoringCategory, 'updaterSound', 'Sound notification', 'Play a sound when somebody replies to your post(s)');
      option(monitoringCategory, 'fixedThreadWatcher', 'Pin Thread Watcher to the page', 'Thread Watcher will scroll with you');
      option(monitoringCategory, 'threadStats', 'Thread statistics', 'Display reply and image counts; italics indicate a reached bump or image limit');
      const filtersCategory = addCategory('filters', 'Filters & Post Hiding');
      const filter = option(filtersCategory, 'filter', 'Filter and highlight specific threads/posts', 'Enable pattern-based filters');
      filter?.parentElement.parentElement.append(' [', link('filters-edit', 'Edit', source => openFilters?.(source)), ']');
      const hiding = option(filtersCategory, 'threadHiding', 'Thread hiding', 'Hide entire threads by clicking the minus button');
      hiding.parentElement.parentElement.append(' [', link('thread-hiding-clear', 'Clear History', () => clearThreads?.()), ']');
      option(filtersCategory, 'hideStubs', 'Hide thread stubs', "Don't display stubs of hidden threads");
      const navigationCategory = addCategory('navigation', 'Navigation');
      option(navigationCategory, 'threadExpansion', 'Thread expansion', 'Expand omitted replies on board indexes');
      option(navigationCategory, 'dropDownNav', 'Use persistent drop-down navigation bar', 'Keep board navigation at the top of the window');
      option(navigationCategory, 'classicNav', 'Use traditional board list', 'Show board links instead of the selection menu', 'settings-sub');
      option(navigationCategory, 'autoHideNav', 'Auto-hide on scroll', 'Hide persistent navigation while scrolling down', 'settings-sub');
      const customMenu = option(navigationCategory, 'customMenu', 'Custom board list', 'Only show selected boards in the board navigation');
      customMenu?.parentElement.parentElement.append(' [', link('custom-menu-edit', 'Edit', source => openCustomMenu?.(source)), ']');
      option(navigationCategory, 'alwaysDepage', 'Always use infinite scroll', 'Load later index pages as you approach the bottom');
      option(navigationCategory, 'topPageNav', 'Page navigation at top of page', 'Hold Shift and drag to move the page switcher');
      option(navigationCategory, 'stickyNav', 'Navigation arrows', 'Show Top and Bottom arrows; hold Shift and drag to move');
      const keys = option(navigationCategory, 'keyBinds', 'Use keyboard shortcuts', 'Enable handy keyboard shortcuts for common actions');
      keys?.parentElement.parentElement.append(' [', link('keybinds-open', 'Show', source => openKeybinds?.(source)), ']');
      const imagesCategory = addCategory('images', 'Images & Media');
      option(imagesCategory, 'imageExpansion', 'Image expansion', 'Enable inline image expansion, limited to browser width');
      option(imagesCategory, 'fitToScreenExpansion', 'Fit expanded images to screen', 'Limit expanded images to both browser width and height');
      option(imagesCategory, 'imageHover', 'Image hover', 'Mouse over images to view full size, limited to browser size');
      option(imagesCategory, 'imageHoverBg', 'Set a background color for transparent images', '', 'settings-sub');
      option(imagesCategory, 'revealSpoilers', "Don't spoiler images", 'Show image thumbnail and original filename instead of spoiler placeholders');
      option(imagesCategory, 'noPictures', 'Hide thumbnails', "Don't display thumbnails while browsing");
      option(imagesCategory, 'embedYouTube', 'Embed YouTube links', 'Load a YouTube player only after you select Embed');
      option(imagesCategory, 'embedSoundCloud', 'Embed SoundCloud links', 'Load a SoundCloud player only after you select Embed');
      const miscellaneousCategory = addCategory('miscellaneous', 'Miscellaneous');
      option(miscellaneousCategory, 'linkify', 'Linkify URLs', 'Make user-posted links clickable');
      option(miscellaneousCategory, 'darkTheme', 'Use a dark theme', 'Use the Tomorrow theme while browsing');
      const customCSS = option(miscellaneousCategory, 'customCSS', 'Custom CSS', 'Use saved colors, typography and spacing for posts');
      if (typeof openCustomCSS === 'function') {
        customCSS.parentElement.parentElement.append(' [', link('custom-css-edit', 'Edit', source => openCustomCSS(source)), ']');
      }
      option(miscellaneousCategory, 'IDColor', 'Color user IDs', 'Assign colors to user IDs on boards that use them');
      option(miscellaneousCategory, 'compactThreads', 'Force long posts to wrap', 'Limit thread width to 75% of the board');
      option(miscellaneousCategory, 'centeredThreads', 'Center threads', 'Center post containers at 75% of the board width');
      option(miscellaneousCategory, 'localTime', 'Convert dates to local time', 'Display post dates in your local time zone');
      const global = node('ul');
      option(global, 'disableAll', 'Disable the native extension', '', 'settings-off');
      form.append(global);
    }
    const message = node('p', '', 'settingsMessage');
    message.setAttribute('role', 'status');
    const actions = node('div', undefined, 'center');
    let exportButton;
    if (typeof openExport === 'function') {
      exportButton = button('Export Settings', event => { if (!pendingSave && !pendingInitialization) openExport(event.currentTarget); });
      exportButton.id = 'settings-export';
      actions.append(exportButton);
    }
    const submit = node('button', 'Save Settings');
    submit.type = 'submit';
    submit.id = catalog ? 'theme-save' : 'settings-save';
    actions.append(submit);
    form.append(message, actions);
    form.addEventListener('submit', async event => {
      event.preventDefault();
      if (submit.disabled) return;
      pendingInitialization?.abort();
      pendingInitialization = null;
      submit.disabled = true;
      if (exportButton) exportButton.disabled = true;
      const controller = new AbortController();
      pendingSave = controller;
      const changes = {};
      for (const [key, field] of fields) {
        if (catalog || field.input.checked !== field.initial) changes[key] = field.input.checked;
      }
      try {
        const result = await save(changes, controller.signal);
        if (controller.signal.aborted || active !== dialog || !dialog.isConnected) return;
        if (result === false) {
          message.textContent = 'Settings could not be saved. Try again.';
          return;
        }
        document.dispatchEvent(new CustomEvent(catalog ? '4chanCatalogThemeApplied' : '4chanSettingsSaved'));
        close();
        // The extension applies settings through navigation. A volatile fallback
        // stays on this page so unavailable storage cannot discard the changes.
        if (!catalog && result.persisted) location.assign(location.pathname + location.search);
      } catch {
        if (!controller.signal.aborted && active === dialog) message.textContent = 'Settings could not be saved. Try again.';
      } finally {
        if (pendingSave === controller) pendingSave = null;
        submit.disabled = false;
        if (exportButton) exportButton.disabled = false;
      }
    });
    content.append(form);
    if (!catalog) dialog.append(content);
    dialog.addEventListener('cancel', event => { event.preventDefault(); close(); });
    dialog.addEventListener('click', event => {
      if (event.target !== dialog) return;
      const bounds = dialog.getBoundingClientRect();
      if (!catalog || event.clientX < bounds.left || event.clientX > bounds.right
        || event.clientY < bounds.top || event.clientY > bounds.bottom) close();
    });
    document.body.append(dialog);
    if (catalog) dialog.style.top = `${window.scrollY + 60}px`;
    active = dialog;
    dialog.showModal();
    if (!catalog && startup.firstRun && typeof initializeOnOpen === 'function') {
      const controller = new AbortController();
      pendingInitialization = controller;
      if (exportButton) exportButton.disabled = true;
      Promise.resolve().then(() => controller.signal.aborted ? false : initializeOnOpen(controller.signal)).then(result => {
        if (controller.signal.aborted || active !== dialog) return;
        if (result === false) message.textContent = 'Initial settings could not be saved. You can still edit settings.';
        else if (result?.status === 'ok' && result.persisted === false) message.textContent = 'Settings are available only in this tab because browser storage is unavailable.';
      }).catch(() => {
        if (!controller.signal.aborted && active === dialog) message.textContent = 'Initial settings could not be saved. You can still edit settings.';
      }).finally(() => {
        if (pendingInitialization !== controller) return;
        pendingInitialization = null;
        if (active === dialog && exportButton && !pendingSave) exportButton.disabled = false;
      });
    }
    if (catalog) fields.get('threadWatcher').input.focus();
    else if (categories[0].list.hidden) categories[0].expand.focus();
    else fields.values().next().value.input.focus();
  }
  let watcher;
  const publicLinks = ['#boardNavDesktop #settingsWindowLink', '#boardNavDesktopFoot #settingsWindowLinkBot', '#boardNavMobile #settingsWindowLinkMobile']
    .map(selector => document.querySelector(selector));
  if (publicLinks.every(element => element instanceof HTMLAnchorElement)) {
    for (const anchor of publicLinks) {
      anchor.setAttribute('aria-haspopup', 'dialog'); anchor.setAttribute('data-native-settings-ready', '');
      anchor.addEventListener('click', event => { event.preventDefault(); open(anchor); });
    }
    watcher = link('watcher-open-mobile', 'TW', toggleWatcher);
    watcher.setAttribute('aria-controls', 'threadWatcher'); watcher.hidden = true;
    publicLinks[2].before(watcher, ' ');
  } else {
    const desktop = node('span', undefined, 'settingsDesktop');
    const desktopLink = link('settingsWindowLink', 'Settings', open);
    desktopLink.setAttribute('aria-haspopup', 'dialog');
    desktop.append('[', desktopLink, ']');
    const mobile = node('span', undefined, 'settingsMobile');
    watcher = link('watcher-open-mobile', 'TW', toggleWatcher);
    watcher.setAttribute('aria-controls', 'threadWatcher');
    watcher.hidden = true;
    const mobileLink = link('settingsWindowLinkMobile', 'Settings', open);
    mobileLink.setAttribute('aria-haspopup', 'dialog');
    mobile.append(watcher, ' ', mobileLink);
    const navigationLinks = node('span');
    navigationLinks.id = 'navtopright';
    navigationLinks.append(desktop, mobile);
    navigation?.append(navigationLinks);
    desktopLink.setAttribute('data-native-settings-ready', ''); mobileLink.setAttribute('data-native-settings-ready', '');
  }
  window.addEventListener('pagehide', close);
  document.addEventListener('4chanPreferencesRestored', close);
  return {
    open,
    setWatcherEnabled(enabled, visible) {
      watcher.hidden = !enabled;
      watcher.setAttribute('aria-expanded', String(visible));
    },
  };
}
