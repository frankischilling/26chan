// Click-to-load provider embeds. Provider resources are never requested while
// the page is being scanned or while an affordance is merely visible.
export const EMBED_LIMITS = Object.freeze({ nodes: 32768, links: 4096, frames: 8, depth: 32 });

const URL_LIMIT = 2048;
const youtubeId = /^[A-Za-z0-9_-]{11}$/;
const soundCloudSlug = /^[a-z0-9][a-z0-9_-]{0,99}$/;
const mounts = new WeakMap();

function providerURL(raw) {
  if (typeof raw !== 'string' || raw.length < 1 || raw.length > URL_LIMIT
    || /[\u0000-\u0020\u007f\\]/.test(raw)) return null;
  try {
    const url = new URL(raw);
    if (url.protocol !== 'https:' || url.username || url.password || url.port) return null;
    return url;
  } catch { return null; }
}

function youtubeTime(raw) {
  if (raw === null) return 0;
  if (/^[0-9]{1,6}$/.test(raw)) {
    const seconds = Number(raw);
    return seconds <= 604800 ? seconds : null;
  }
  const match = /^(?:(\d{1,5})m)?(?:(\d{1,5})s)?$/.exec(raw);
  if (!match || !match.slice(1).some(Boolean)) return null;
  const minutes = Number(match[1] || 0), seconds = Number(match[2] || 0);
  const total = minutes * 60 + seconds;
  return total <= 604800 ? total : null;
}

function youtubeParameters(url) {
  const values = new Map(), counts = new Map();
  const collect = params => {
    for (const [name, value] of params) {
      if (name !== 'v' && name !== 't') continue;
      const count = (counts.get(name) ?? 0) + 1;
      counts.set(name, count);
      if (count > 1) return false;
      values.set(name, value);
    }
    return true;
  };
  if (!collect(url.searchParams)) return null;
  if (url.hash.length > 1 && !collect(new URLSearchParams(url.hash.slice(1)))) return null;
  return values;
}

export function youtubeTarget(raw) {
  const url = providerURL(raw);
  if (!url) return null;
  const params = youtubeParameters(url);
  if (!params) return null;
  let id, time;
  if ((url.hostname === 'www.youtube.com' || url.hostname === 'youtube.com') && url.pathname === '/watch') {
    if (!params.has('v') || !youtubeId.test(params.get('v'))) return null;
    id = params.get('v'); time = youtubeTime(params.get('t') ?? null);
  } else if (url.hostname === 'youtu.be') {
    const match = /^\/([A-Za-z0-9_-]{11})$/.exec(url.pathname);
    if (!match) return null;
    id = match[1]; time = youtubeTime(params.get('t') ?? null);
  } else return null;
  // v1191 recognizes a broader [ms0-9]+ token. Preserve common decimal/m/s
  // forms as integer seconds; discard unsupported single values instead of
  // forwarding them or suppressing an otherwise valid video embed.
  if (time === null) time = 0;
  const embed = new URL(`https://www.youtube-nocookie.com/embed/${id}`);
  if (time > 0) embed.searchParams.set('start', String(time));
  return { provider: 'youtube', id, start: time, source: url.href, embed: embed.href };
}

export function soundCloudTarget(raw) {
  const url = providerURL(raw);
  if (!url || url.hostname !== 'soundcloud.com' || url.search || url.hash) return null;
  const parts = url.pathname.split('/').filter(Boolean);
  if (![1, 2, 3].includes(parts.length) || !parts.every(part => soundCloudSlug.test(part))) return null;
  if (parts.length === 3 && parts[1] !== 'sets') return null;
  if (url.pathname !== `/${parts.join('/')}`) return null;
  const source = `https://soundcloud.com/${parts.join('/')}`;
  const embed = new URL('https://w.soundcloud.com/player/');
  embed.searchParams.set('url', source);
  embed.searchParams.set('auto_play', 'false');
  embed.searchParams.set('show_comments', 'false');
  embed.searchParams.set('show_user', 'true');
  embed.searchParams.set('show_reposts', 'false');
  embed.searchParams.set('visual', 'false');
  return { provider: 'soundcloud', source, embed: embed.href };
}

export function embedTarget(raw) {
  return youtubeTarget(raw) ?? soundCloudTarget(raw);
}

export function mountNativeEmbeds({ root, settings, hasMobileLayout = () => false, projection, limits = {} } = {}) {
  if (!root?.matches?.('.board') || typeof settings !== 'function' || typeof hasMobileLayout !== 'function'
    || !root.ownerDocument?.defaultView) return null;
  const bounds = { ...EMBED_LIMITS };
  for (const [key, value] of Object.entries(limits)) {
    if (!Object.hasOwn(bounds, key) || !Number.isInteger(value) || value < 1 || value > bounds[key]) {
      throw new RangeError('invalid-embed-limits');
    }
    bounds[key] = value;
  }

  mounts.get(root)?.destroy();
  const document = root.ownerDocument, window = document.defaultView;
  const owner = { kind: 'native-embeds' };
  const entries = new Map(), players = new Set();
  let suspended = false, destroyed = false, queued = false;

  function configuration() {
    let value;
    try { value = settings(); } catch { /* Storage denial uses finite defaults. */ }
    return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
  }

  function mobileLayout() {
    try { return hasMobileLayout() === true; } catch { return false; }
  }

  function mode(target, config, mobile) {
    if (config.disableAll === true) return null;
    if (target.provider === 'youtube') {
      if (mobile) return 'open';
      return config.embedYouTube === false ? null : 'embed';
    }
    return config.embedSoundCloud === true ? 'embed' : null;
  }

  function anchorTarget(anchor) {
    const raw = anchor.getAttribute('href'), direct = embedTarget(raw);
    if (direct) return direct;
    if (!anchor.classList.contains('linkified') || anchor.getAttribute('data-native-linkified') !== 'true') return null;
    try {
      const link = new URL(raw, window.location.origin), params = [...link.searchParams.entries()];
      if (link.origin !== window.location.origin || link.pathname !== '/derefer' || link.hash || params.length !== 1
        || params[0][0] !== 'url') return null;
      const label = projection?.text ? projection.text(anchor) : anchor.textContent;
      if (params[0][1] !== label) return null;
      return embedTarget(label);
    } catch { return null; }
  }

  function sourceContext(anchor) {
    let current = anchor, message = null;
    for (let depth = 0; current && depth <= bounds.depth; depth++, current = current.parentElement) {
      if (current !== anchor && projection?.has?.(current)) return null;
      if (current.hasAttribute?.('hidden') || current.classList?.contains('deleted')
        || current.classList?.contains('post-hidden') || current.classList?.contains('native-thread-hidden')
        || current.classList?.contains('mobile-post-hidden')) return null;
      if (!message && current.classList?.contains('postMessage')) message = current;
      if (current === root) return message;
    }
    return null;
  }

  function scan(config, mobile) {
    if (!root.isConnected || config.disableAll === true) return new Map();
    const targets = new Map();
    const filter = {
      acceptNode(node) {
        return node !== root && projection?.has?.(node)
          ? window.NodeFilter.FILTER_REJECT : window.NodeFilter.FILTER_ACCEPT;
      },
    };
    const walker = document.createTreeWalker(root, window.NodeFilter.SHOW_ELEMENT, filter);
    let nodes = 1, links = 0, node;
    while ((node = walker.nextNode())) {
      if (++nodes > bounds.nodes) return null;
      if (node.localName !== 'a' || !node.hasAttribute('href')) continue;
      if (++links > bounds.links) return null;
      if (!sourceContext(node)) continue;
      const target = anchorTarget(node);
      if (!target) continue;
      const currentMode = mode(target, config, mobile);
      if (currentMode) targets.set(node, { target, mode: currentMode });
    }
    return targets;
  }

  function sameTarget(left, right) {
    return left.provider === right.provider && left.source === right.source && left.embed === right.embed;
  }

  function closePlayer(entry) {
    if (!entry.frame) return;
    const { iframe, container } = entry.frame;
    entry.frame = null; players.delete(entry);
    iframe.removeAttribute('src'); iframe.remove(); container.remove();
    if (entry.toggle.isConnected) entry.toggle.textContent = entry.mode === 'open' ? 'Open' : 'Embed';
  }

  function removeEntry(entry) {
    closePlayer(entry);
    entries.delete(entry.anchor);
    entry.control.remove();
  }

  function createPlayer(entry) {
    if (players.size >= bounds.frames || entry.mode !== 'embed') return false;
    const iframe = document.createElement('iframe'), container = document.createElement('div');
    container.className = `nativeMediaEmbed nativeMediaEmbed${entry.target.provider === 'youtube' ? 'YouTube' : 'SoundCloud'}`;
    iframe.className = 'nativeEmbedFrame';
    iframe.setAttribute('sandbox', 'allow-scripts allow-same-origin allow-popups allow-presentation');
    if (entry.target.provider === 'youtube') {
      iframe.title = 'YouTube video player'; iframe.width = '640'; iframe.height = '360';
      iframe.referrerPolicy = 'strict-origin-when-cross-origin';
      iframe.setAttribute('allow', 'fullscreen; encrypted-media; picture-in-picture');
      iframe.setAttribute('allowfullscreen', '');
    } else {
      iframe.title = 'SoundCloud player'; iframe.width = '500'; iframe.height = '166';
      iframe.referrerPolicy = 'no-referrer'; iframe.setAttribute('allow', 'autoplay');
    }
    projection?.claim?.(container, owner);
    container.append(iframe); entry.control.after(container);
    entry.frame = { iframe, container }; players.add(entry);
    entry.toggle.textContent = 'Remove';
    // Set src last: assigning it is the first operation that can contact a provider.
    iframe.src = entry.target.embed;
    return true;
  }

  function onToggle(event, entry) {
    if (destroyed || suspended || !root.isConnected || entries.get(entry.anchor) !== entry
      || !entry.anchor.isConnected || !entry.control.isConnected || entry.control.parentNode !== entry.anchor.parentNode
      || entry.anchor.nextSibling !== entry.control) { event.preventDefault(); return; }
    const config = configuration(), mobile = mobileLayout();
    const current = anchorTarget(entry.anchor);
    if (!sourceContext(entry.anchor) || !current || !sameTarget(entry.target, current)
      || mode(current, config, mobile) !== entry.mode) { event.preventDefault(); refresh(); return; }
    if (entry.mode === 'open') return;
    if (event.defaultPrevented || event.button !== 0 || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return;
    event.preventDefault();
    if (entry.frame) closePlayer(entry);
    else if (players.size < bounds.frames) createPlayer(entry);
  }

  function createEntry(anchor, descriptor) {
    const control = document.createElement('span'), toggle = document.createElement('a');
    control.className = 'nativeEmbedControls';
    toggle.className = 'nativeEmbedToggle';
    toggle.href = descriptor.target.source; toggle.target = '_blank'; toggle.rel = 'noopener noreferrer';
    toggle.textContent = descriptor.mode === 'open' ? 'Open' : 'Embed';
    control.append(' [', toggle, ']');
    projection?.claim?.(control, owner);
    const entry = { anchor, target: descriptor.target, mode: descriptor.mode, control, toggle, frame: null };
    toggle.addEventListener('click', event => onToggle(event, entry));
    entries.set(anchor, entry); anchor.after(control);
    return entry;
  }

  function reconcile(targets) {
    for (const [anchor, entry] of [...entries]) {
      const descriptor = targets?.get(anchor);
      if (!descriptor || !sameTarget(entry.target, descriptor.target)
        || entry.control.parentNode !== anchor.parentNode || anchor.nextSibling !== entry.control) {
        removeEntry(entry); continue;
      }
      if (entry.mode !== descriptor.mode) {
        closePlayer(entry); entry.mode = descriptor.mode;
        entry.toggle.textContent = descriptor.mode === 'open' ? 'Open' : 'Embed';
      }
      targets.delete(anchor);
    }
    if (!targets) return;
    for (const [anchor, descriptor] of targets) createEntry(anchor, descriptor);
  }

  function refresh() {
    queued = false;
    if (destroyed || suspended) return;
    const targets = scan(configuration(), mobileLayout());
    // Exceeding a scan bound fails closed and removes any already-active player.
    reconcile(targets ?? new Map());
  }

  function schedule(changes = null) {
    if (destroyed || suspended || queued) return;
    if (changes && projection?.originalMutation
      && !changes.some(change => projection.originalMutation(change))) return;
    queued = true;
    queueMicrotask(refresh);
  }

  const observer = new window.MutationObserver(changes => schedule(changes));
  function observe() {
    const target = document.body ?? document.documentElement;
    if (target) observer.observe(target, { childList: true, subtree: true, attributes: true,
      attributeFilter: ['href', 'class', 'hidden'] });
  }
  const onSettings = () => refresh();
  const onStorage = event => {
    if (event.key === null || event.key === '4chan-settings' || event.key === '4chan_never_show_mobile') refresh();
  };
  const onResize = () => schedule();
  const onPageHide = event => {
    if (destroyed) return;
    suspended = true; queued = false; observer.disconnect(); reconcile(new Map());
    if (!event.persisted) destroy();
  };
  const onPageShow = event => {
    if (!event.persisted || destroyed || !suspended) return;
    suspended = false; observe(); refresh();
  };

  function destroy() {
    if (destroyed) return;
    destroyed = true; suspended = true; queued = false; observer.disconnect(); reconcile(new Map());
    document.removeEventListener('4chanSettingsSaved', onSettings);
    window.removeEventListener('storage', onStorage);
    window.removeEventListener('resize', onResize);
    window.removeEventListener('pagehide', onPageHide);
    window.removeEventListener('pageshow', onPageShow);
    if (mounts.get(root)?.destroy === destroy) mounts.delete(root);
  }

  const controller = { refresh, destroy };
  mounts.set(root, controller);
  document.addEventListener('4chanSettingsSaved', onSettings);
  window.addEventListener('storage', onStorage);
  window.addEventListener('resize', onResize);
  window.addEventListener('pagehide', onPageHide);
  window.addEventListener('pageshow', onPageShow);
  observe(); refresh();
  return controller;
}
