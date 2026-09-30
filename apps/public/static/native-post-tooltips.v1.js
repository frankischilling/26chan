import { localQuoteTree } from './native-filter.v1.js';

// Released extension v1191's finite relative-date text. Header admission and
// tooltip DOM ownership are handled separately from this pure formatter.
export function relativePostAge(timestamp, now = Date.now()) {
  if (!Number.isSafeInteger(timestamp) || timestamp < 0 || timestamp > 8_640_000_000_000
    || !Number.isFinite(now) || now < 0 || now > 8_640_000_000_000_000) return null;
  const elapsed = now / 1000 - timestamp;
  if (elapsed < 1) return 'moments ago';
  if (elapsed < 60) return `${Math.trunc(elapsed)} seconds ago`;
  if (elapsed < 3600) {
    const minutes = Math.trunc(elapsed / 60);
    return minutes > 1 ? `${minutes} minutes ago` : 'one minute ago';
  }
  if (elapsed < 86400) {
    const hours = Math.trunc(elapsed / 3600);
    const minutes = Math.trunc(elapsed / 60 - 60 * hours);
    return `${hours > 1 ? `${hours} hours` : 'one hour'}${minutes > 1 ? ` and ${minutes} minutes` : ''} ago`;
  }
  const days = Math.trunc(elapsed / 86400);
  const hours = Math.trunc(elapsed / 3600 - 24 * days);
  return `${days > 1 ? `${days} days` : 'one day'}${hours > 1 ? ` and ${hours} hours` : ''} ago`;
}

export function mountNativePostTooltips({ root, context, settings, projection, display }) {
  const document = root?.ownerDocument, window = document?.defaultView;
  if (!window || !context || typeof settings !== 'function') return { clear() {}, destroy() {} };
  let active = null, timer = null, tip = null, suspended = false, destroyed = false;
  const enabled = () => !destroyed && !suspended && root.isConnected && settings().disableAll !== true;
  function describe(target) {
    if (!enabled() || !target?.matches || !root.contains(target)) return null;
    if (target.matches('.thread-stats > [data-tip]') && !target.hasAttribute('data-tip-cb')) {
      const tips = { 'ts-replies': ['Replies', 'Replies (bump limit reached)'],
        'ts-images': ['Images', 'Images (limit reached)'], 'ts-ips': ['Posters'], 'ts-page': ['Page'] };
      const text = target.dataset.tip;
      if (tips[target.className]?.includes(text) && target.getClientRects().length) {
        return { label: target, text, delay: 300, kind: 'stat' };
      }
      return null;
    }
    const date = target.closest('.postInfoM > .dateTime.postNum[data-utc]');
    const filename = target.matches('.file > .fileThumb > .mFileInfo.mobile') && !projection?.within(target) ? target : null;
    const label = date ?? filename ?? (target.matches('.postInfoM > .nameBlock > .name[title]') ? target : null);
    if (!label || !label.getClientRects().length || label.closest('[hidden],.filter-hidden,.reply-hidden,.thread-hidden')) return null;
    const post = label.closest('.post[id]'), article = post?.parentElement;
    if (!post || article?.id !== `pc${post.id.slice(1)}` || !article.matches('.postContainer')
      || !post.closest('.board > .thread[id]')) return null;
    const no = post.id.slice(1), thread = post.closest('.thread').id.slice(1);
    try { localQuoteTree(article, { ...context, thread }, no, projection); } catch { return null; }
    if (filename) {
      const file = label.parentElement.parentElement, caption = file.querySelector(':scope > .fileText > a');
      const full = caption?.getAttribute('title') ?? caption?.textContent;
      const image = label.parentElement.querySelector('img:not(.expanded-thumb)');
      return file.id === `f${no}` && image?.alt === full && typeof full === 'string' && full.length > 0
        && new TextEncoder().encode(full).length <= 255 && !/[\u0000-\u001f\u007f-\u009f]/.test(full)
        ? { label, text: full, delay: 300, kind: 'file' } : null;
    }
    if (date) {
      if (!/^(0|[1-9][0-9]{0,12})$/.test(date.dataset.utc)) return null;
      const timestamp = Number(date.dataset.utc), text = relativePostAge(timestamp);
      return text === null ? null : { label, text, timestamp, delay: 500, kind: 'date' };
    }
    const full = post.querySelector('.postInfo .name')?.textContent;
    return typeof full === 'string' && full.length <= 1024 && label.title === full
      ? { label, text: full, delay: 300, kind: 'name' } : null;
  }
  function clear() {
    window.clearTimeout(timer); timer = null;
    const previous = active; active = null;
    if (previous) {
      if (previous.label.getAttribute('aria-describedby') === previous.described) {
        if (previous.previousDescribed === null) previous.label.removeAttribute('aria-describedby');
        else previous.label.setAttribute('aria-describedby', previous.previousDescribed);
      }
      previous.restore?.();
    }
    tip?.remove(); tip = null;
  }
  function same(current, next) {
    return next && current.label === next.label && current.kind === next.kind
      && (current.kind === 'date' ? current.timestamp === next.timestamp : current.text === next.text);
  }
  function show() {
    timer = null;
    if (!active || !same(active, describe(active.label))) { clear(); return; }
    if (document.getElementById('tooltip')) { clear(); return; }
    tip = document.createElement('div'); tip.id = 'tooltip'; tip.className = 'tip-top';
    tip.setAttribute('role', 'tooltip'); tip.textContent = active.text;
    document.body.append(tip);
    active.previousDescribed = active.label.getAttribute('aria-describedby');
    active.described = `${active.previousDescribed ? `${active.previousDescribed} ` : ''}tooltip`;
    active.label.setAttribute('aria-describedby', active.described);
    const box = active.label.getBoundingClientRect();
    let left = box.left - (tip.offsetWidth - active.label.offsetWidth) / 2;
    if (left < 0) { left = box.left + 2; tip.className = 'tip-top-right'; }
    else if (left + tip.offsetWidth > document.documentElement.clientWidth) {
      left = box.left - tip.offsetWidth + active.label.offsetWidth + 2; tip.className = 'tip-top-left';
    }
    // The released core measures height before applying the horizontal edge
    // correction, which can change wrapping at the right edge.
    const top = box.top - tip.offsetHeight - 5 + window.scrollY;
    tip.style.top = `${top}px`;
    tip.style.left = `${left + window.scrollX}px`;
  }
  function enter(event) {
    const next = describe(event.target);
    if (active && same(active, next)) return;
    clear(); if (!next) return;
    active = next;
    if (next.kind === 'date') next.restore = display?.suppressDateTitle(next.label);
    timer = window.setTimeout(show, next.delay);
  }
  function leave(event) {
    if (active?.label.contains(event.relatedTarget)) return;
    clear();
  }
  function refresh() {
    if (active && !same(active, describe(active.label))) clear();
    if (!root.isConnected) destroy();
  }
  const observer = new window.MutationObserver(refresh);
  function watch() { observer.observe(document.documentElement, { subtree: true, childList: true, attributes: true, characterData: true }); }
  function hide(event) { suspended = true; observer.disconnect(); clear(); if (!event.persisted) destroy(); }
  function resume(event) { if (event.persisted && !destroyed) { suspended = false; watch(); } }
  function destroy() {
    if (destroyed) return;
    destroyed = true; observer.disconnect(); clear();
    root.removeEventListener('mouseover', enter); root.removeEventListener('mouseout', leave);
    root.removeEventListener('focusin', enter); root.removeEventListener('focusout', leave);
    document.removeEventListener('4chanSettingsSaved', refresh); document.removeEventListener('4chanPreferencesRestored', refresh);
    window.removeEventListener('resize', clear); window.removeEventListener('scroll', clear);
    window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', resume);
  }
  root.addEventListener('mouseover', enter); root.addEventListener('mouseout', leave);
  root.addEventListener('focusin', enter); root.addEventListener('focusout', leave);
  document.addEventListener('4chanSettingsSaved', refresh); document.addEventListener('4chanPreferencesRestored', refresh);
  window.addEventListener('resize', clear); window.addEventListener('scroll', clear);
  window.addEventListener('pagehide', hide); window.addEventListener('pageshow', resume);
  watch();
  return { clear, destroy };
}
