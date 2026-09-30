// Only approved, normalized images from the configured media origin may load.
export const IMAGE_LIMITS = Object.freeze({ expansions: 8, reveals: 1001, loadMs: 10000, dimension: 1024 });

export function imageTarget(raw, mediaOrigin) {
  if (typeof raw !== 'string' || typeof mediaOrigin !== 'string' || raw.length > 512
    || /[\u0000-\u0020\u007f\\]/.test(raw)) return null;
  try {
    const origin = new URL(mediaOrigin);
    if (!['http:', 'https:'].includes(origin.protocol) || origin.origin !== mediaOrigin) return null;
    const match = /^\/([a-z0-9]{1,10})\/([1-9][0-9]{0,18})\.png$/.exec(raw.slice(mediaOrigin.length));
    if (!raw.startsWith(mediaOrigin) || !match || BigInt(match[2]) > 9223372036854775807n) return null;
    return { url: raw, thumbnail: raw.slice(0, -4) + 's.jpg' };
  } catch { return null; }
}

export function imageSize(width, height, availableWidth, availableHeight = Infinity) {
  if (![width, height, availableWidth].every(Number.isFinite) || width < 1 || height < 1
    || availableWidth < 1 || !(availableHeight >= 1)) return null;
  const ratio = Math.min(1, availableWidth / width, availableHeight / height);
  return { width: width * ratio, height: height * ratio };
}

export function mountNativeImages({ root, mediaOrigin = '', settings, projection, mobile, family = 'futaba',
  limits = {} } = {}) {
  if (!root || typeof settings !== 'function' || !imageTarget(`${mediaOrigin}/a/1.png`, mediaOrigin)) return null;
  const bounds = { ...IMAGE_LIMITS };
  for (const [key, value] of Object.entries(limits)) {
    if (!Object.hasOwn(bounds, key) || !Number.isInteger(value) || value < 1 || value > bounds[key]) {
      throw new RangeError('invalid-image-limits');
    }
    bounds[key] = value;
  }
  const document = root.ownerDocument, window = document.defaultView;
  const previousFamily = root.getAttribute('data-image-family');
  function currentFamily() {
    let value;
    try { value = typeof family === 'function' ? family() : family; } catch { /* Use the finite default. */ }
    return ['burichan', 'tomorrow', 'photon'].includes(value) ? value : 'futaba';
  }
  root.dataset.imageFamily = currentFamily();
  const expanded = new Map(), revealed = new Map();
  const owner = { kind: 'native-images' };
  let hover = null, feedback = null, feedbackTimer, suspended = false, disposed = false, queued = false;
  function configuration() {
    let value;
    try { value = settings(); } catch { /* Storage denial keeps the ordinary defaults. */ }
    return value && typeof value === 'object' ? value : {};
  }
  const enabled = () => !disposed && !suspended && configuration().disableAll !== true;
  const setClass = (element, name, value) => {
    if (element.classList.contains(name) !== value) element.classList.toggle(name, value);
  };
  const available = element => root.contains(element) && element.isConnected
    && !element.closest('.deleted,.post-hidden,.native-thread-hidden,.mobile-post-hidden,[hidden]')
    && element.getClientRects().length > 0;
  function target(anchor) {
    return anchor?.matches?.('a.fileThumb') && anchor.closest('.file') && available(anchor)
      ? imageTarget(anchor.getAttribute('href'), mediaOrigin) : null;
  }
  function thumbnail(anchor, file) {
    return Array.from(anchor.children).find(element => element.localName === 'img'
      && !projection?.has(element) && [file.url, file.thumbnail].includes(element.getAttribute('src')))
      ?? revealed.get(anchor.closest('.file'))?.image;
  }
  function valid(entry) {
    const file = target(entry.anchor);
    return enabled() && file?.url === entry.url && entry.thumb.parentNode === entry.anchor
      && [file.url, file.thumbnail].includes(entry.thumb.getAttribute('src'));
  }
  function message(text) {
    clearTimeout(feedbackTimer);
    if (!feedback) {
      feedback = document.createElement('p'); feedback.className = 'nativeImageFeedback';
      feedback.setAttribute('role', 'status'); document.body.append(feedback);
    }
    feedback.textContent = text;
    feedbackTimer = setTimeout(clearMessage, 4000);
  }
  function clearMessage() { clearTimeout(feedbackTimer); feedback?.remove(); feedback = null; }
  function release(entry) {
    entry.pending = false;
    clearTimeout(entry.timer);
    entry.image.onload = entry.image.onerror = null;
    entry.image.removeAttribute('src'); entry.image.remove();
  }
  function contract(anchor, scroll = false) {
    const entry = expanded.get(anchor);
    if (!entry) return;
    expanded.delete(anchor); release(entry);
    setClass(anchor, 'nativeImageOpen', false); setClass(anchor, 'nativeImageLoading', false);
    setClass(entry.file, 'image-expanded', false);
    const post = anchor.closest('.post');
    if (scroll && post && post.getBoundingClientRect().top < 0) post.scrollIntoView({ block: 'start' });
  }
  function hideHover() { if (hover) { const previous = hover; hover = null; release(previous); } }
  function dimensions(image) {
    return image.naturalWidth > 0 && image.naturalHeight > 0
      && image.naturalWidth <= bounds.dimension && image.naturalHeight <= bounds.dimension;
  }
  function fit(entry, preview = false) {
    const width = preview ? window.innerWidth : document.documentElement.clientWidth;
    const height = document.documentElement.clientHeight;
    const rect = entry.thumb.getClientRects().length ? entry.thumb.getBoundingClientRect() : entry.anchor.getBoundingClientRect();
    const left = rect.left + (entry.thumb.getClientRects().length ? 0 : parseFloat(window.getComputedStyle(entry.anchor).borderLeftWidth) || 0);
    const size = imageSize(entry.image.naturalWidth, entry.image.naturalHeight,
      Math.max(1, width - (preview ? rect.right + 20 : left + 25)),
      preview || configuration().fitToScreenExpansion === true ? height : Infinity);
    if (!size) return false;
    entry.image.style.maxWidth = `${size.width}px`; entry.image.style.maxHeight = `${size.height}px`;
    return true;
  }
  function load(entry, ready, fail) {
    const failed = () => { if (entry.pending) { entry.pending = false; fail(); } };
    entry.pending = true;
    entry.image.onerror = failed;
    entry.image.onload = () => {
      if (!entry.pending) return;
      if (!dimensions(entry.image)) { failed(); return; }
      entry.pending = false; clearTimeout(entry.timer);
      entry.image.onload = entry.image.onerror = null;
      ready();
    };
    entry.timer = setTimeout(failed, bounds.loadMs);
    entry.image.src = entry.url;
  }
  function fullImage() {
    const image = document.createElement('img');
    image.alt = 'Image'; image.decoding = 'async'; image.referrerPolicy = 'no-referrer'; image.hidden = true;
    projection?.claim(image, owner);
    return image;
  }
  function expand(anchor) {
    const file = target(anchor), thumb = file && thumbnail(anchor, file);
    if (!file || !thumb) return false;
    if (expanded.has(anchor)) { contract(anchor, true); return true; }
    if (expanded.size >= bounds.expansions) { message('Close another expanded image before opening this one.'); return true; }
    hideHover(); clearMessage();
    const image = fullImage(); image.className = 'expanded-thumb';
    const entry = { anchor, thumb, image, file: anchor.closest('.file'), url: file.url };
    expanded.set(anchor, entry); anchor.append(image); setClass(anchor, 'nativeImageLoading', true);
    load(entry, () => {
      if (!valid(entry) || configuration().imageExpansion === false || !fit(entry)) { contract(anchor); return; }
      setClass(anchor, 'nativeImageLoading', false); setClass(anchor, 'nativeImageOpen', true);
      setClass(entry.file, 'image-expanded', true); image.hidden = false;
    }, () => {
      contract(anchor);
      if (enabled() && available(anchor)) message('Image could not be loaded. Click the thumbnail to try again.');
    });
    return true;
  }
  function showHover(anchor) {
    if (!enabled() || configuration().imageHover !== true || mobile?.matches || expanded.has(anchor)) return;
    const file = target(anchor), thumb = file && thumbnail(anchor, file);
    if (!file || !thumb || hover?.anchor === anchor) return;
    hideHover();
    const image = fullImage(); image.id = 'image-hover';
    setClass(image, 'nativeImageBackground', configuration().imageHoverBg === true);
    const entry = { anchor, thumb, image, url: file.url }; hover = entry;
    document.body.append(image);
    load(entry, () => {
      if (hover !== entry || !valid(entry) || configuration().imageHover !== true || !fit(entry, true)) { hideHover(); return; }
      image.hidden = false;
    }, () => {
      if (hover !== entry) return;
      hideHover();
      if (enabled() && available(anchor)) message('Image preview could not be loaded.');
    });
  }
  function spoilerData(file) {
    if (file.dataset.imageSpoiler !== 'true' || !available(file)) return null;
    const details = Array.from(file.children).find(element => element.localName === 'details');
    const link = details?.querySelector('a[href]'), source = link && imageTarget(link.getAttribute('href'), mediaOrigin);
    const caption = file.querySelector(':scope > .fileText > a[href],:scope > p > a[href]');
    if (!source || caption?.getAttribute('href') !== source.url) return null;
    const filename = file.dataset.imageFilename;
    if (!filename || filename.length > 255 || /[\u0000-\u001f\u007f-\u009f]/.test(filename)
      || new TextEncoder().encode(filename).length > 255) return null;
    const width = file.dataset.thumbnailWidth, height = file.dataset.thumbnailHeight;
    if (!/^[1-9][0-9]{0,3}$/.test(width ?? '') || !/^[1-9][0-9]{0,3}$/.test(height ?? '')
      || Number(width) > bounds.dimension || Number(height) > bounds.dimension) return null;
    return { details, source, caption, filename, width, height, legacy: file.dataset.thumbnailLegacy === 'true' };
  }
  function conceal(file) {
    const entry = revealed.get(file);
    if (!entry) return;
    revealed.delete(file); contract(entry.anchor);
    if (hover?.anchor === entry.anchor) hideHover();
    entry.label.remove();
    if (entry.captionHidden === null) entry.caption.removeAttribute('hidden');
    else entry.caption.setAttribute('hidden', entry.captionHidden);
    entry.untrackCaption?.();
    entry.image.removeAttribute('src'); entry.anchor.remove(); setClass(file, 'nativeSpoilerRevealed', false);
  }
  function reveal(file, data) {
    if (revealed.size >= bounds.reveals) return;
    const anchor = document.createElement('a'), image = document.createElement('img');
    anchor.className = 'fileThumb'; anchor.href = data.source.url; anchor.target = '_blank'; anchor.rel = 'noopener noreferrer';
    image.alt = data.filename; image.width = Number(data.width); image.height = Number(data.height);
    image.loading = 'lazy'; image.decoding = 'async'; image.referrerPolicy = 'no-referrer';
    projection?.claim(anchor, owner);
    // The generated thumbnail belongs to the reveal entry. It is never copied
    // into quote recipes; their original spoiler details retain the same policy.
    const label = document.createElement('a');
    label.href = data.source.url; label.target = '_blank'; label.rel = 'noopener noreferrer';
    label.textContent = data.filename; projection?.claim(label, owner);
    const captionHidden = data.caption.getAttribute('hidden');
    const untrackCaption = projection?.trackAttributes(data.caption,
      (name, value) => name === 'hidden' ? captionHidden : value);
    const entry = { ...data, anchor, image, label, captionHidden, untrackCaption }; revealed.set(file, entry);
    data.caption.hidden = true; data.caption.after(label);
    anchor.append(image); file.append(anchor); setClass(file, 'nativeSpoilerRevealed', true);
    image.src = data.legacy ? data.source.url : data.source.thumbnail;
  }
  function refresh() {
    queued = false;
    const config = configuration(), active = enabled();
    root.dataset.imageFamily = currentFamily();
    setClass(root, 'noPictures', active && config.noPictures === true);
    for (const [anchor, entry] of expanded) {
      if (!valid(entry) || config.imageExpansion === false) contract(anchor);
      else if (!entry.pending) fit(entry);
    }
    if (hover && (!valid(hover) || config.imageHover !== true || mobile?.matches)) hideHover();
    else if (hover) {
      setClass(hover.image, 'nativeImageBackground', config.imageHoverBg === true);
      if (!hover.pending) fit(hover, true);
    }
    for (const [file, entry] of revealed) {
      const data = active && config.revealSpoilers === true && spoilerData(file);
      if (!data || data.source.url !== entry.source.url || data.details !== entry.details
        || data.width !== entry.width || data.height !== entry.height || data.legacy !== entry.legacy
        || data.filename !== entry.filename || data.caption !== entry.caption
        || entry.label.parentNode !== data.caption.parentNode || entry.anchor.parentNode !== file) conceal(file);
    }
    if (active && config.revealSpoilers === true) {
      for (const file of root.querySelectorAll('.file[data-image-spoiler="true"]')) {
        if (revealed.has(file)) continue;
        const data = spoilerData(file);
        if (data) reveal(file, data);
        if (revealed.size >= bounds.reveals) break;
      }
    }
    if (!active) clearMessage();
  }
  function schedule() { if (!queued && !disposed) { queued = true; queueMicrotask(refresh); } }
  function click(event) {
    const anchor = event.target.closest?.('a.fileThumb');
    if (!enabled() || configuration().imageExpansion === false || event.defaultPrevented || event.button !== 0
      || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey || !anchor) return;
    if (expand(anchor)) event.preventDefault();
  }
  function over(event) {
    const anchor = event.target.closest?.('a.fileThumb');
    if (anchor && !anchor.contains(event.relatedTarget)) showHover(anchor);
  }
  function out(event) { if (hover?.anchor.contains(event.target) && !hover.anchor.contains(event.relatedTarget)) hideHover(); }
  const storage = event => { if (event.key === null || event.key === '4chan-settings') refresh(); };
  const pagehide = () => { suspended = true; refresh(); };
  const pageshow = event => { if (event.persisted) { suspended = false; refresh(); } };
  const observer = new window.MutationObserver(schedule);
  observer.observe(root, { childList: true, subtree: true, attributes: true, characterData: true,
    attributeFilter: ['href', 'src', 'class', 'hidden', 'data-image-spoiler', 'data-image-filename', 'data-thumbnail-width', 'data-thumbnail-height', 'data-thumbnail-legacy'] });
  root.addEventListener('click', click); root.addEventListener('mouseover', over); root.addEventListener('mouseout', out);
  document.addEventListener('4chanSettingsSaved', refresh);
  window.addEventListener('storage', storage); window.addEventListener('resize', refresh);
  window.addEventListener('pagehide', pagehide); window.addEventListener('pageshow', pageshow);
  mobile?.addEventListener('change', refresh);
  refresh();
  return { refresh, dispose() {
    disposed = true; observer.disconnect(); refresh();
    root.removeEventListener('click', click); root.removeEventListener('mouseover', over); root.removeEventListener('mouseout', out);
    document.removeEventListener('4chanSettingsSaved', refresh);
    window.removeEventListener('storage', storage); window.removeEventListener('resize', refresh);
    window.removeEventListener('pagehide', pagehide); window.removeEventListener('pageshow', pageshow);
    mobile?.removeEventListener('change', refresh);
    if (previousFamily === null) root.removeAttribute('data-image-family');
    else root.setAttribute('data-image-family', previousFamily);
  } };
}
