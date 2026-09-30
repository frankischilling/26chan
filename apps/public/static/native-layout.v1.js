const SETTINGS_KEY = '4chan-settings';
const NEVER_MOBILE_KEY = '4chan_never_show_mobile';
const LAYOUT_ATTRIBUTE = 'data-native-thread-layout';
const THEME_PATH = '/static/theme.css';
export const THEME_READY_EVENT = 'boardThemeChanged';
const THEME_FAMILIES = new Set(['futaba', 'burichan', 'tomorrow', 'photon']);

function record(value) {
  return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
}

export function sourceMobileLayout(matches, neverMobile) {
  return matches === true && neverMobile !== 'true';
}

export function nativeThreadLayout(settings, mobileLayout = false) {
  const config = record(settings);
  if (config.disableAll === true) return null;
  if (config.compactThreads === true && mobileLayout !== true) return 'compact';
  if (config.centeredThreads === true) return 'centered';
  return null;
}

export function nativeDarkTheme(settings) {
  const config = record(settings);
  return config.disableAll !== true && config.darkTheme === true;
}

export function darkThemeStylesheetHref(requestedHref, locationHref) {
  if (typeof requestedHref !== 'string' || requestedHref.length === 0 || requestedHref.length > 512
    || typeof locationHref !== 'string' || locationHref.length > 2048) return null;
  let page;
  let requested;
  try {
    page = new URL(locationHref);
    requested = new URL(requestedHref, page);
  } catch { return null; }
  if (!['http:', 'https:'].includes(page.protocol) || requested.origin !== page.origin
    || requested.protocol !== page.protocol || requested.username || requested.password
    || requested.pathname !== THEME_PATH || requested.hash) return null;
  const keys = [...requested.searchParams.keys()];
  if (keys.some(key => key !== 'worksafe') || requested.searchParams.getAll('worksafe').length > 1) return null;
  const worksafe = requested.searchParams.get('worksafe');
  if (worksafe !== null && worksafe !== 'true' && worksafe !== 'false') return null;
  const query = worksafe === null ? 'theme=tomorrow' : `worksafe=${worksafe}&theme=tomorrow`;
  return `${THEME_PATH}?${query}`;
}

function emptyThemeController() {
  return { setDark() { return false; }, destroy() {} };
}

function stylesheetFamily(document) {
  let family = '';
  try {
    family = document.defaultView.getComputedStyle(document.documentElement)
      .getPropertyValue('--watcher-icon-family').trim().replace(/['"]/g, '');
  } catch { return null; }
  return THEME_FAMILIES.has(family) ? family : 'futaba';
}

export function createNativeThemeOverride({ link } = {}) {
  const document = link?.ownerDocument;
  const window = document?.defaultView;
  if (!link || !window || link.tagName !== 'LINK'
    || !link.relList?.contains('stylesheet')) return emptyThemeController();
  let requestedHref = null;
  let darkHref = null;
  let pending = null;
  let overriding = false;
  let failed = false;
  let destroyed = false;

  function expectLoad(href, family = null) {
    pending = href ? { href, family } : null;
  }

  function restoreOwned(trackLoad = true) {
    if (overriding && link.getAttribute('href') === darkHref && requestedHref !== null) {
      if (trackLoad) expectLoad(requestedHref);
      else pending = null;
      link.setAttribute('href', requestedHref);
    } else if (!trackLoad || !pending || link.getAttribute('href') !== pending.href) pending = null;
    overriding = false;
  }

  function setDark(enabled) {
    if (destroyed) return false;
    if (enabled !== true) {
      restoreOwned();
      failed = false;
      return true;
    }
    if (failed) return false;
    if (overriding && link.getAttribute('href') === darkHref) return true;
    if (overriding) overriding = false;
    const current = link.getAttribute('href');
    const next = darkThemeStylesheetHref(current, window.location.href);
    if (next === null) return false;
    requestedHref = current;
    darkHref = next;
    overriding = true;
    expectLoad(darkHref, 'tomorrow');
    link.setAttribute('href', darkHref);
    return true;
  }

  function onLoad() {
    if (destroyed || !link.isConnected || !pending || link.getAttribute('href') !== pending.href) return;
    const family = stylesheetFamily(document);
    if (family === null || (pending.family !== null && family !== pending.family)) return;
    pending = null;
    document.dispatchEvent(new window.CustomEvent(THEME_READY_EVENT));
  }

  function onError() {
    if (!destroyed && overriding && link.getAttribute('href') === darkHref) {
      restoreOwned();
      failed = true;
    }
  }

  link.addEventListener('load', onLoad);
  link.addEventListener('error', onError);
  return {
    setDark,
    destroy() {
      if (destroyed) return;
      destroyed = true;
      restoreOwned(false);
      link.removeEventListener('load', onLoad);
      link.removeEventListener('error', onError);
    },
  };
}

export function mountNativeLayout({ root, settings, mobile, readNeverMobile = () => null, themeStylesheet = null } = {}) {
  const document = root?.ownerDocument;
  const window = document?.defaultView;
  if (!root || !window || typeof settings !== 'function') {
    return { refresh() {}, destroy() {} };
  }
  const originalLayout = root.getAttribute(LAYOUT_ATTRIBUTE);
  const originalNeverMobile = root.getAttribute('data-native-never-mobile');
  const originalMobileDark = root.classList.contains('m-dark');
  const theme = createNativeThemeOverride({ link: themeStylesheet });
  let ownedLayout;
  let ownedNeverMobile;
  let ownedMobileDark = false;
  let suspended = false;
  let destroyed = false;

  function configuration() {
    try { return record(settings()); }
    catch { return {}; }
  }

  function mobileLayout() {
    let neverMobile = null;
    try { neverMobile = readNeverMobile(); } catch { /* Use the public mobile default. */ }
    return sourceMobileLayout(mobile?.matches === true, neverMobile);
  }

  function restoreLayout() {
    if (ownedLayout === undefined) return;
    if (root.getAttribute(LAYOUT_ATTRIBUTE) === ownedLayout) {
      if (originalLayout === null) root.removeAttribute(LAYOUT_ATTRIBUTE);
      else root.setAttribute(LAYOUT_ATTRIBUTE, originalLayout);
    }
    ownedLayout = undefined;
  }

  function applyLayout(mode) {
    if (mode === null) { restoreLayout(); return; }
    root.setAttribute(LAYOUT_ATTRIBUTE, mode);
    ownedLayout = mode;
  }

  function refresh() {
    if (destroyed || suspended) return;
    const config = configuration();
    const isMobile = mobileLayout(), dark = nativeDarkTheme(config);
    applyLayout(nativeThreadLayout(config, isMobile));
    ownedNeverMobile = mobile?.matches === true && !isMobile ? 'true' : 'false';
    root.setAttribute('data-native-never-mobile', ownedNeverMobile);
    if (isMobile && dark && !originalMobileDark) {
      root.classList.add('m-dark'); ownedMobileDark = true;
    } else if (ownedMobileDark) {
      root.classList.remove('m-dark'); ownedMobileDark = false;
    }
    theme.setDark(dark && !isMobile);
  }

  const onStorage = event => {
    if (event.key === null || event.key === SETTINGS_KEY || event.key === NEVER_MOBILE_KEY) refresh();
  };
  const onPageHide = event => {
    if (event.persisted) suspended = true;
    else destroy();
  };
  const onPageShow = event => {
    if (event.persisted && !destroyed) { suspended = false; refresh(); }
  };

  function destroy() {
    if (destroyed) return;
    destroyed = true;
    document.removeEventListener('4chanSettingsSaved', refresh);
    window.removeEventListener('storage', onStorage);
    window.removeEventListener('pagehide', onPageHide);
    window.removeEventListener('pageshow', onPageShow);
    mobile?.removeEventListener?.('change', refresh);
    restoreLayout();
    if (root.getAttribute('data-native-never-mobile') === ownedNeverMobile) {
      if (originalNeverMobile === null) root.removeAttribute('data-native-never-mobile');
      else root.setAttribute('data-native-never-mobile', originalNeverMobile);
    }
    if (ownedMobileDark) root.classList.remove('m-dark');
    theme.destroy();
  }

  document.addEventListener('4chanSettingsSaved', refresh);
  window.addEventListener('storage', onStorage);
  window.addEventListener('pagehide', onPageHide);
  window.addEventListener('pageshow', onPageShow);
  mobile?.addEventListener?.('change', refresh);
  refresh();
  return { refresh, destroy };
}
