import { isSpoilerAssetPath } from './native-spoiler-assets.js';
import { configureSpoilerPreference } from './native-spoilers.js';

// Only approved, normalized images from the configured media origin may load.
export const IMAGE_LIMITS = Object.freeze({ expansions: 8, reveals: 1501, retired: 20001, loadMs: 10000, dimension: 1024 });

export function attachmentTarget(raw, mediaOrigin) {
  if (typeof raw !== 'string' || typeof mediaOrigin !== 'string' || raw.length > 512
    || /[\u0000-\u0020\u007f\\]/.test(raw)) return null;
  try {
    const origin = new URL(mediaOrigin);
    if (!['http:', 'https:'].includes(origin.protocol) || origin.origin !== mediaOrigin) return null;
    const match = /^\/([a-z0-9]{1,10})\/([1-9][0-9]{0,18})\.(?:png|gif|pdf|webm|mp4)$/.exec(raw.slice(mediaOrigin.length));
    if (!raw.startsWith(mediaOrigin) || !match || BigInt(match[2]) > 9223372036854775807n) return null;
    return { url: raw, thumbnail: raw.slice(0, raw.lastIndexOf('.')) + 's.jpg' };
  } catch { return null; }
}

// Documents open normally; never send them to an image loader or expansion.
export function imageTarget(raw, mediaOrigin) {
  const target = attachmentTarget(raw, mediaOrigin);
  return target && /\.(?:png|gif)$/.test(target.url) ? target : null;
}

export function videoTarget(raw, mediaOrigin) {
  const target = attachmentTarget(raw, mediaOrigin);
  return target && /\.(?:webm|mp4)$/.test(target.url) ? target : null;
}

export function imageSize(width, height, availableWidth, availableHeight = Infinity) {
  if (![width, height, availableWidth].every(Number.isFinite) || width < 1 || height < 1
    || availableWidth < 1 || !(availableHeight >= 1)) return null;
  const ratio = Math.min(1, availableWidth / width, availableHeight / height);
  return { width: width * ratio, height: height * ratio };
}

export function mountNativeImages({ root, mediaOrigin = '', settings, projection, mobile, family = 'futaba',
  previewRoot, limits = {} } = {}) {
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
  const expanded = new Map(), revealed = new Map(), retired = new Set();
  let retirementFull = false;
  const isRetired = url => retirementFull || retired.has(url);
  const owner = { kind: 'native-images' };
  let hover = null, feedback = null, feedbackTimer, suspended = false, disposed = false, queued = false;
  function configuration() {
    let value;
    try { value = settings(); } catch { /* Storage denial keeps the ordinary defaults. */ }
    return value && typeof value === 'object' ? value : {};
  }
  const enabled = () => !disposed && !suspended && configuration().disableAll !== true;
  configureSpoilerPreference(document, () => enabled() && configuration().revealSpoilers === true);
  const preview = () => {
    let element;
    try { element = typeof previewRoot === 'function' ? previewRoot() : null; } catch { return null; }
    return element?.ownerDocument === document && element.isConnected && !root.contains(element) ? element : null;
  };
  function* spoilerFiles() {
    yield* root.querySelectorAll('.file[data-image-spoiler="true"]');
    const popup = preview();
    if (popup) yield* popup.querySelectorAll('.file[data-image-spoiler="true"]');
  }
  const setClass = (element, name, value) => {
    if (element.classList.contains(name) !== value) element.classList.toggle(name, value);
  };
  const available = element => (root.contains(element) || preview()?.contains(element)) && element.isConnected
    && !element.closest('.deleted,.post-hidden,.native-thread-hidden,.mobile-post-hidden,[hidden]')
    && element.getClientRects().length > 0;
  function target(anchor) {
    return anchor?.matches?.('a.fileThumb:not(.imgspoiler)') && anchor.closest('.file') && (available(anchor) || (expanded.get(anchor)?.video
        && !anchor.closest('[hidden]') && available(anchor.closest('.file'))))
      && !isRetired(anchor.getAttribute('href')) ? (imageTarget(anchor.getAttribute('href'), mediaOrigin) ?? videoTarget(anchor.getAttribute('href'), mediaOrigin)) : null;
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
    entry.image.onload = entry.image.onloadedmetadata = entry.image.onerror = entry.image.onvolumechange = null;
    clearTimeout(entry.volumeTimer);
    if (entry.video) entry.image.pause();
    entry.image.removeAttribute('src');
    if (entry.video) entry.image.load();
    entry.image.remove(); entry.close?.remove(); entry.duration?.remove();
  }
  function contract(anchor, scroll = false) {
    const entry = expanded.get(anchor);
    if (!entry) return;
    expanded.delete(anchor); release(entry);
    setClass(anchor, 'nativeImageOpen', false); setClass(anchor, 'nativeImageLoading', false);
    setClass(entry.file, 'image-expanded', false); setClass(anchor, 'nativeVideoOpen', false);
    const post = anchor.closest('.post');
    if (scroll && post && post.getBoundingClientRect().top < 0) post.scrollIntoView({ block: 'start' });
  }
  function hideHover() { if (hover) { const previous = hover; hover = null; release(previous); } }
  function dimensions(image) {
    const video = image.localName === 'video';
    const width = video ? image.videoWidth : image.naturalWidth;
    const height = video ? image.videoHeight : image.naturalHeight;
    const limit = video ? 2048 : bounds.dimension;
    return width > 0 && height > 0 && width <= limit && height <= limit
      && (!video || (Number.isFinite(image.duration) && image.duration > 0 && image.duration <= 400));
  }
  function fit(entry, preview = false) {
    const width = preview ? window.innerWidth : document.documentElement.clientWidth;
    const height = document.documentElement.clientHeight;
    const rect = entry.thumb.getClientRects().length ? entry.thumb.getBoundingClientRect() : (entry.video && expanded.has(entry.anchor) ? entry.file : entry.anchor).getBoundingClientRect();
    const left = rect.left + (entry.thumb.getClientRects().length ? 0 : parseFloat(window.getComputedStyle(entry.anchor).borderLeftWidth) || 0);
    const size = imageSize(entry.video ? entry.image.videoWidth : entry.image.naturalWidth, entry.video ? entry.image.videoHeight : entry.image.naturalHeight,
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
    const loaded = () => {
      if (!entry.pending) return;
      if (!dimensions(entry.image)) { failed(); return; }
      entry.pending = false; clearTimeout(entry.timer);
      entry.image.onload = entry.image.onloadedmetadata = entry.image.onerror = null;
      ready();
      if (entry.video && entry.image.isConnected) entry.image.onerror = fail;
    };
    if (entry.video) entry.image.onloadedmetadata = loaded; else entry.image.onload = loaded;
    entry.timer = setTimeout(failed, bounds.loadMs);
    entry.image.src = entry.url;
  }
  function fullImage() {
    const image = document.createElement('img');
    image.alt = 'Image'; image.decoding = 'async'; image.referrerPolicy = 'no-referrer'; image.hidden = true;
    projection?.claim(image, owner);
    return image;
  }
  function mediaElement(file, hover = false) {
    if (!videoTarget(file.url, mediaOrigin)) return fullImage();
    const video = document.createElement('video');
    video.controls = !hover; video.loop = true; video.autoplay = true; video.playsInline = true;
    video.muted = configuration().unmuteWebm !== true; video.preload = 'metadata'; video.hidden = true;
    video.setAttribute('aria-label', 'Video attachment');
    let volume = 0.5;
    try {
      const saved = window.localStorage.getItem('4chan-volume');
      if (saved !== null && saved.trim() !== '' && Number.isFinite(Number(saved))) volume = Math.max(0, Math.min(1, Number(saved)));
    } catch { /* Storage denial retains the default volume. */ }
    video.volume = volume;
    projection?.claim(video, owner);
    return video;
  }
  function playVideo(entry) {
    if (!entry.video) return;
    // Autoplay can be denied by browser policy. Native controls stay available.
    entry.image.play()?.catch(() => {});
    entry.image.onvolumechange = () => {
      clearTimeout(entry.volumeTimer);
      entry.volumeTimer = setTimeout(() => {
        try { window.localStorage.setItem('4chan-volume', String(entry.image.volume)); } catch { /* Optional preference. */ }
      }, 200);
    };
  }
  function expand(anchor) {
    const file = target(anchor), thumb = file && thumbnail(anchor, file);
    if (!file || !thumb) return false;
    if (expanded.has(anchor)) { contract(anchor, true); return true; }
    if (expanded.size >= bounds.expansions) { message('Close another expanded image before opening this one.'); return true; }
    hideHover(); clearMessage();
    const image = mediaElement(file); image.className = 'expanded-thumb';
    const entry = { anchor, thumb, image, file: anchor.closest('.file'), url: file.url, video: image.localName === 'video' };
    expanded.set(anchor, entry);
    if (entry.video) {
      entry.close = document.createElement('button'); entry.close.type = 'button';
      entry.close.className = 'nativeVideoClose'; entry.close.textContent = 'Close';
      entry.close.addEventListener('click', () => contract(anchor, true));
      projection?.claim(entry.close, owner);
      anchor.after(entry.close, image);
    } else anchor.append(image); setClass(anchor, 'nativeImageLoading', true);
    load(entry, () => {
      if (!valid(entry) || configuration().imageExpansion === false || !fit(entry)) { contract(anchor); return; }
      setClass(anchor, 'nativeImageLoading', false); setClass(anchor, 'nativeImageOpen', true);
      setClass(entry.file, 'image-expanded', true); image.hidden = false;
      setClass(anchor, 'nativeVideoOpen', entry.video); playVideo(entry);
    }, () => {
      contract(anchor);
      if (enabled() && available(anchor)) message(entry.video ? 'Video could not be played. Use the filename link to open or download it.' : 'Image could not be loaded. Click the thumbnail to try again.');
    });
    return true;
  }
  function showHover(anchor) {
    if (!enabled() || configuration().imageHover !== true || mobile?.matches || expanded.has(anchor)) return;
    const file = target(anchor), thumb = file && thumbnail(anchor, file);
    if (!file || !thumb || hover?.anchor === anchor) return;
    hideHover();
    const image = mediaElement(file, true); image.id = 'image-hover';
    setClass(image, 'nativeImageBackground', configuration().imageHoverBg === true);
    const entry = { anchor, thumb, image, url: file.url, video: image.localName === 'video' }; hover = entry;
    document.body.append(image);
    load(entry, () => {
      if (hover !== entry || !valid(entry) || configuration().imageHover !== true || !fit(entry, true)) { hideHover(); return; }
      image.hidden = false; playVideo(entry);
      if (entry.video) {
        entry.duration = document.createElement('span'); entry.duration.className = 'nativeVideoDuration';
        const seconds = Math.floor(image.duration);
        const audio = image.mozHasAudio === true || image.webkitAudioDecodedByteCount > 0 || image.audioTracks?.length > 0;
        entry.duration.textContent = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}${audio ? ' (audio)' : ''}`;
        entry.duration.setAttribute('role', 'status'); projection?.claim(entry.duration, owner);
        entry.duration.style.left = `${Math.max(0, thumb.getBoundingClientRect().left)}px`;
        entry.duration.style.top = `${Math.max(0, thumb.getBoundingClientRect().bottom)}px`;
        document.body.append(entry.duration);
      }
    }, () => {
      if (hover !== entry) return;
      hideHover();
      if (enabled() && available(anchor)) message('Image preview could not be loaded.');
    });
  }
  function spoilerData(file) {
    if (file.dataset.imageSpoiler !== 'true' || !available(file)) return null;
    const details = Array.from(file.children).find(element => element.localName === 'details');
    const placeholder = file.querySelector(':scope > a.fileThumb.imgspoiler');
    const link = placeholder ?? details?.querySelector('a[href]'), source = link && attachmentTarget(link.getAttribute('href'), mediaOrigin);
    const caption = file.querySelector(':scope > .fileText > a[href],:scope > p > a[href]');
    if (!source || isRetired(source.url) || caption?.getAttribute('href') !== source.url) return null;
    const filename = file.dataset.imageFilename;
    if (!filename || filename.length > 255 || /[\u0000-\u001f\u007f-\u009f]/.test(filename)
      || new TextEncoder().encode(filename).length > 255) return null;
    const width = file.dataset.thumbnailWidth, height = file.dataset.thumbnailHeight;
    if (!/^[1-9][0-9]{0,3}$/.test(width ?? '') || !/^[1-9][0-9]{0,3}$/.test(height ?? '')
      || Number(width) > bounds.dimension || Number(height) > bounds.dimension) return null;
    if (placeholder && (placeholder.className !== 'fileThumb imgspoiler'
      || !isSpoilerAssetPath(placeholder.querySelector('img')?.getAttribute('src'))
      || caption.parentElement.title !== filename)) return null;
    return { details, placeholder, source, caption, filename, width, height, legacy: file.dataset.thumbnailLegacy === 'true' };
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
    if (entry.placeholder) {
      if (entry.placeholderHidden === null) entry.placeholder.removeAttribute('hidden');
      else entry.placeholder.setAttribute('hidden', entry.placeholderHidden);
      entry.untrackPlaceholder?.();
    }
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
    // into quote recipes; their original spoiler placeholder retains its policy.
    const label = document.createElement('a');
    label.href = data.source.url; label.target = '_blank'; label.rel = 'noopener noreferrer';
    const dot = data.filename.lastIndexOf('.'), stem = dot > 0 ? data.filename.slice(0, dot) : data.filename;
    const op = file.parentElement.classList.contains('op');
    if (data.placeholder) {
      const scale = Math.min(1, (op ? 250 : 125) / image.width, (op ? 250 : 125) / image.height);
      image.width = Math.max(1, Math.floor(image.width * scale)); image.height = Math.max(1, Math.floor(image.height * scale));
    }
    label.textContent = (stem.length > (op ? 40 : 30) ? stem.slice(0, op ? 35 : 25) + '(...)' + data.filename.slice(dot > 0 ? dot : data.filename.length) : data.filename).toWellFormed();
    if (label.textContent !== data.filename) label.title = data.filename;
    projection?.claim(label, owner);
    const captionHidden = data.caption.getAttribute('hidden');
    const untrackCaption = projection?.trackAttributes(data.caption,
      (name, value) => name === 'hidden' ? captionHidden : value);
    const placeholderHidden = data.placeholder?.getAttribute('hidden');
    const untrackPlaceholder = data.placeholder && projection?.trackAttributes(data.placeholder,
      (name, value) => name === 'hidden' ? placeholderHidden : value);
    const entry = { ...data, anchor, image, label, captionHidden, untrackCaption, placeholderHidden, untrackPlaceholder }; revealed.set(file, entry);
    data.caption.hidden = true; data.caption.after(label);
    if (data.placeholder) {
      data.placeholder.hidden = true;
      const caption = document.createElement('div'); caption.className = 'mFileInfo mobile';
      caption.textContent = data.placeholder.querySelector('.mFileInfo')?.textContent ?? '';
      anchor.append(caption);
    }
    anchor.prepend(image); file.append(anchor); setClass(file, 'nativeSpoilerRevealed', true);
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
      if (!data || data.source.url !== entry.source.url || data.details !== entry.details || data.placeholder !== entry.placeholder
        || data.width !== entry.width || data.height !== entry.height || data.legacy !== entry.legacy
        || data.filename !== entry.filename || data.caption !== entry.caption
        || entry.label.parentNode !== data.caption.parentNode || entry.anchor.parentNode !== file) conceal(file);
    }
    if (active && config.revealSpoilers === true) {
      for (const file of spoilerFiles()) {
        if (revealed.has(file)) continue;
        const data = spoilerData(file);
        if (data) reveal(file, data);
        if (revealed.size >= bounds.reveals) break;
      }
    }
    if (!active) clearMessage();
  }
  // Deletion tombstones are exact validated URLs, shared by original posts,
  // inline clones and quote previews. Never evict them while this page survives.
  function retire(raw) {
    const file = attachmentTarget(raw, mediaOrigin);
    if (!file || disposed) return false;
    if (retired.has(file.url)) return true;
    if (retired.size >= bounds.retired) {
      retirementFull = true; refresh();
      message('Refresh this page before opening more images.');
      return false;
    }
    retired.add(file.url); refresh(); return true;
  }
  function schedule() { if (!queued && !disposed) { queued = true; queueMicrotask(refresh); } }
  function preventRetired(event) {
    const link = event.target.closest?.('.file a[href]');
    if (!link || !(root.contains(link) || preview()?.contains(link)) || !isRetired(link.getAttribute('href'))) return false;
    event.preventDefault(); return true;
  }
  function click(event) {
    if (preventRetired(event)) return;
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
  let scrollTimer;
  const scroll = () => {
    clearTimeout(scrollTimer);
    scrollTimer = setTimeout(() => {
      for (const entry of expanded.values()) if (entry.video && !entry.pending) {
        const rect = entry.image.getBoundingClientRect();
        if (rect.bottom < 0 || rect.top > window.innerHeight) entry.image.pause();
      }
    }, 500);
  };
  const storage = event => { if (event.key === null || event.key === '4chan-settings') refresh(); };
  const pagehide = () => { suspended = true; refresh(); };
  const pageshow = event => { if (event.persisted) { suspended = false; refresh(); } };
  const observer = new window.MutationObserver(schedule);
  observer.observe(root, { childList: true, subtree: true, attributes: true, characterData: true,
    attributeFilter: ['href', 'src', 'class', 'hidden', 'data-image-spoiler', 'data-image-filename', 'data-thumbnail-width', 'data-thumbnail-height', 'data-thumbnail-legacy'] });
  if (typeof previewRoot === 'function') observer.observe(document.body, { childList: true });
  const eventRoot = typeof previewRoot === 'function' ? document.body : root;
  eventRoot.addEventListener('auxclick', preventRetired);
  eventRoot.addEventListener('click', click); eventRoot.addEventListener('mouseover', over); eventRoot.addEventListener('mouseout', out);
  document.addEventListener('4chanSettingsSaved', refresh);
  window.addEventListener('storage', storage); window.addEventListener('resize', refresh);
  window.addEventListener('scroll', scroll, { passive: true });
  window.addEventListener('pagehide', pagehide); window.addEventListener('pageshow', pageshow);
  mobile?.addEventListener('change', refresh);
  refresh();
  return { refresh, retire, dispose() {
    disposed = true; clearTimeout(scrollTimer); observer.disconnect(); refresh();
    eventRoot.removeEventListener('auxclick', preventRetired);
    eventRoot.removeEventListener('click', click); eventRoot.removeEventListener('mouseover', over); eventRoot.removeEventListener('mouseout', out);
    document.removeEventListener('4chanSettingsSaved', refresh);
    window.removeEventListener('storage', storage); window.removeEventListener('resize', refresh);
    window.removeEventListener('scroll', scroll);
    window.removeEventListener('pagehide', pagehide); window.removeEventListener('pageshow', pageshow);
    mobile?.removeEventListener('change', refresh);
    if (previousFamily === null) root.removeAttribute('data-image-family');
    else root.setAttribute('data-image-family', previousFamily);
  } };
}
