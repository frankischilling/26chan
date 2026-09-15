import { NativeUpdaterTransport } from './native-updater-transport.js';
import { NativeUpdaterSchedule } from './native-updater-schedule.js';
import { useUpdaterTail } from './native-updater-tail.js';
import { notificationKind, notificationIcon } from './native-tracked-quotes.js';
import { postId } from '../static/thread-watcher-core.v1.js';

function build(node) {
  if (typeof node === 'string') return document.createTextNode(node);
  const element = document.createElement(node.tag);
  for (const [key, value] of Object.entries(node.attrs)) element.setAttribute(key, value);
  for (const child of node.children) element.append(build(child));
  return element;
}

export function mountNativeThreadUpdater({ board, thread, worksafe, mediaOrigin, settings, applied, projection }) {
  const section = document.getElementById(`t${thread}`);
  if (!postId(thread) || !section || section.dataset.archived === 'true') return null;
  const transport = new NativeUpdaterTransport({ board, thread, mediaOrigin });
  const controls = [], statuses = [], mobileLinks = [], autoInputs = [], soundControls = [];
  let busy = false, dead = false, stopped = false, generation = 0;
  let currentCycle = null;
  let lastQuickReply = null, postedTimer = null, postedPending = false;
  function requestPostedUpdate() {
    clearTimeout(postedTimer);
    postedTimer = setTimeout(() => { if (!busy) { postedPending = false; void update(); } }, 500);
  }
  function posted(post, ready) {
    if (!postId(post)) return;
    lastQuickReply = post;
    Promise.resolve(ready).catch(() => {}).then(() => {
      if (stopped || disabled()) return;
      postedPending = true; requestPostedUpdate();
    });
  }
  let tailSize = Number(section.dataset.tailSize || 0), lastUpdated = Date.now();
  let wasDisabled = true, hadAuto = false, unread = 0, marker = null;
  const icon = document.querySelector('link[rel="shortcut icon"]');
  let currentIcon = null;
  let audio = null, audioEnabled = false;
  function syncSound() {
    const available = settings().updaterSound === true && !disabled();
    if (available && !audio) { audio = document.createElement('audio'); audio.src = '/static/notifications/beep.ogg'; }
    if (!available && audio) { audio.pause(); audio.removeAttribute('src'); audio.load(); audio = null; audioEnabled = false; }
    for (const { wrapper, input } of soundControls) { wrapper.hidden = !available; input.checked = audioEnabled; }
  }
  function setIcon(kind) {
    const path = notificationIcon(worksafe, kind);
    if (!icon || !path) return;
    currentIcon = kind; icon.href = path; document.head.append(icon);
  }
  const title = document.title, sessionKey = `4chan-auto-${thread}`;
  let wanted = settings().alwaysAutoUpdate === true;
  try { wanted ||= Boolean(sessionStorage.getItem(sessionKey)); } catch { /* Auto remains usable without storage. */ }
  const disabled = () => settings().disableAll === true || settings().threadUpdater === false;
  const schedule = new NativeUpdaterSchedule({ poll: () => { void update(false); }, tick: seconds => status(String(seconds)) });
  function remember(value) {
    wanted = value;
    try { if (value) sessionStorage.setItem(sessionKey, '1'); else sessionStorage.removeItem(sessionKey); }
    catch { /* Session preference is optional, never request authority. */ }
  }
  function toggleAuto() {
    if (disabled() || stopped || dead || busy) { sync(); return; }
    if (schedule.auto) { schedule.stop(); remember(false); status(''); setIcon(null); }
    else { hadAuto = true; schedule.start(); remember(true); }
    sync();
  }
  for (const nav of document.querySelectorAll('.threadNav')) {
    const mobile = nav.classList.contains('mobile');
    const group = document.createElement(mobile ? 'div' : 'span'); group.className = `nativeUpdater${mobile ? ' btn-row' : ''}`;
    const status = document.createElement('span'); status.className = 'nativeUpdaterStatus';
    status.setAttribute('role', 'status'); status.setAttribute('aria-live', 'polite'); statuses.push(status);
    if (mobile) {
      const link = nav.querySelector('[data-thread-refresh]');
      if (!link) continue;
      link.addEventListener('click', event => {
        if (disabled() || event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
        event.preventDefault(); void update();
      });
      const wrapper = link.parentElement, anchor = document.createComment('refresh');
      wrapper.before(anchor); group.append(wrapper, ' ');
      mobileLinks.push({ link, wrapper, anchor, group });
    } else {
      const link = document.createElement('a'); link.href = `/${board}/thread/${thread}`;
      link.textContent = 'Update'; link.dataset.cmd = 'update';
      link.addEventListener('click', event => {
        if (event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
        event.preventDefault(); void update();
      });
      group.append(' [', link, '] ');
    }
    const label = document.createElement('label'), input = document.createElement('input');
    input.type = 'checkbox'; input.dataset.cmd = 'auto'; input.title = 'Fetch new replies automatically';
    input.addEventListener('change', toggleAuto); label.append(input, 'Auto'); autoInputs.push(input);
    if (mobile) {
      const button = document.createElement('span'); button.className = 'mobileib button'; button.append(label);
      group.append(button); status.classList.add('mobile-tu-status');
    } else {
      group.append('[', label, '] ');
      const wrapper = document.createElement('span'), caption = document.createElement('label'), sound = document.createElement('input');
      sound.type = 'checkbox'; sound.dataset.cmd = 'sound'; sound.title = 'Play a sound on new replies to your posts';
      sound.addEventListener('change', () => { audioEnabled = !audioEnabled; syncSound(); });
      caption.append(sound, 'Sound'); wrapper.append('[', caption, '] '); group.append(wrapper);
      soundControls.push({ wrapper, input: sound });
    }
    group.append(status); nav.append(group); controls.push(group);
  }
  function status(text, error = false) {
    for (const node of statuses) { node.textContent = text; node.classList.toggle('tu-error', error); }
  }
  function sync() {
    syncSound();
    if (disabled() || stopped) {
      if (!wasDisabled) {
        generation++; currentCycle?.abort(); currentCycle = null; transport.cancel(); busy = false;
        schedule.suspend(); status('');
        if (!stopped) setIcon(null);
      }
      wasDisabled = true;
    } else if (wasDisabled) {
      wasDisabled = false;
      if (wanted && !dead) { hadAuto = true; schedule.start(); remember(true); }
    }
    for (const node of controls) { node.hidden = disabled(); node.setAttribute('aria-busy', String(busy)); }
    for (const input of autoInputs) { input.checked = schedule.auto; input.disabled = busy || dead; }
    for (const { link, wrapper, anchor, group } of mobileLinks) {
      if (disabled() && wrapper.parentNode === group) anchor.after(wrapper);
      else if (!disabled() && wrapper.parentNode !== group) group.prepend(wrapper);
      link.textContent = disabled() ? 'Refresh' : 'Update'; link.dataset.cmd = 'update';
      link.dataset.updaterReady = String(!disabled());
    }
  }
  function threadState(snapshot) {
    section.dataset.sticky = String(snapshot.sticky);
    section.dataset.closed = String(snapshot.closed);
    section.dataset.archived = String(snapshot.archived);
    const info = document.getElementById(`pi${thread}`);
    for (const span of info.querySelectorAll(':scope > span:not([class]), :scope > .nativeThreadState')) {
      if (['Sticky', 'Closed', 'Archived'].includes(span.textContent)) span.remove();
    }
    for (const [show, text] of [[snapshot.sticky, 'Sticky'], [snapshot.closed && !snapshot.archived, 'Closed'], [snapshot.archived, 'Archived']]) {
      if (show) { const span = document.createElement('span'); span.className = 'nativeThreadState'; span.textContent = text; info.append(' ', span); }
    }
    // A current closed/archive state makes existing posting forms read-only;
    // the server independently authorizes every write, including stale pages.
    for (const form of document.querySelectorAll('form.postEditor, form.postForm')) {
      for (const input of form.querySelectorAll('input, textarea, select, button')) {
        if (snapshot.closed || snapshot.archived) {
          if (!input.disabled) { input.disabled = true; input.dataset.updaterDisabled = 'true'; }
        } else if (input.dataset.updaterDisabled === 'true') { input.disabled = false; delete input.dataset.updaterDisabled; }
      }
    }
  }
  function atBottom() { return document.documentElement.scrollHeight <= Math.ceil(innerHeight + scrollY); }
  function onScroll() {
    if (!hadAuto || document.hidden || !atBottom()) return;
    if (!dead) setIcon(null);
    if (!marker) return;
    unread = 0; document.title = title; marker.classList.remove('newPostsMarker'); marker = null;
  }
  async function update(forced = true) {
    if (disabled() || stopped || dead || busy) return;
    const current = ++generation;
    const cycle = new AbortController(); currentCycle = cycle;
    let added = 0;
    schedule.begin(); busy = true; status('Updating...'); sync();
    const currentPosts = [...section.querySelectorAll(':scope > .postContainer')];
    const known = new Set(currentPosts.map(post => post.id.slice(2)));
    const tail = useUpdaterTail(tailSize, currentPosts.slice(1).map(post => Date.parse(post.querySelector('.postInfo time')?.dateTime)), lastUpdated, Date.now());
    const result = await transport.refresh({ signal: cycle.signal, tail, known });
    if (current !== generation || disabled() || stopped || !section.isConnected) return;
    if (!['cancelled', 'busy', 'cooldown', 'invalid-context', 'unavailable'].includes(result.status)) lastUpdated = Date.now();
    try {
      if (result.status === 'not-modified') { status('No new posts'); return; }
      if (result.status !== 'ok') {
        if (result.status === 'http-error' && result.httpStatus === 404) {
          dead = true; setIcon('dead'); schedule.stop(); remember(false); status('This thread has been pruned or deleted', true);
        } else if (result.status === 'cooldown') status('Please wait before updating again.');
        else if (result.status !== 'cancelled') status('Connection Error. Open the thread page to retry.', true);
        return;
      }
      const { snapshot } = result;
      if (snapshot.tail_id === null) { tailSize = snapshot.tail_size; section.dataset.tailSize = String(tailSize); }
      const existing = [...section.querySelectorAll(':scope > .postContainer')];
      const last = existing.at(-1)?.id.slice(2);
      if (!postId(last)) throw new Error('invalid-current-thread');
      const additions = snapshot.posts.filter(post => BigInt(post.no) > BigInt(last));
      const fromQuickReply = additions.length === 1 && additions[0].no === lastQuickReply;
      if (fromQuickReply) lastQuickReply = null;
      const previous = existing.at(-1);
      const scroll = settings().autoScroll === true && document.hidden
        && document.documentElement.scrollHeight === Math.ceil(innerHeight + scrollY);
      // Build the entire append detached. Validate every ID against the live
      // page before inserting anything, including names used by form labels.
      function checkIds(tree) {
        if (typeof tree === 'string') return;
        if (tree.attrs.id && document.getElementById(tree.attrs.id)) throw new Error('duplicate-dom-id');
        for (const child of tree.children) checkIds(child);
      }
      // Check every recipe before constructing elements: an img can fetch as
      // soon as src is assigned, even while it is detached from the document.
      for (const post of additions) checkIds(post.tree);
      const fragment = document.createDocumentFragment();
      for (const post of additions) {
        const element = build(post.tree);
        fragment.append(element);
      }
      section.append(fragment);
      added = additions.length;
      threadState(snapshot);
      document.dispatchEvent(new Event('boardThreadStateChanged'));
      if (additions.length) {
        const offset = previous.offsetTop;
        await applied?.(snapshot, cycle.signal);
        if (current !== generation || disabled() || stopped) return;
        const moved = previous.offsetTop - offset;
        if (moved) window.scrollBy(0, moved);
        if (!forced && !fromQuickReply && document.documentElement.scrollHeight > innerHeight) {
          const posts = additions.map(post => document.getElementById(`p${post.no}`));
          const you = posts.some(post => projection ? projection.query(post, '.ql-tracked') : post.querySelector('.ql-tracked'));
          setIcon(notificationKind(currentIcon, { you,
            highlighted: posts.some(post => post.classList.contains('filter-hl')), unread }));
          if (you && audioEnabled && document.hidden && audio) {
            try { audio.play()?.catch(() => {}); } catch { /* Browser playback policy cannot break insertion. */ }
          }
          if (!marker && last !== thread) {
            marker = previous.querySelector(':scope > .post'); marker?.classList.add('newPostsMarker');
          }
          unread += additions.length; document.title = `(${unread}) ${title}`;
        }
        if (scroll) window.scrollTo(0, document.documentElement.scrollHeight);
        const event = new Event('4chanThreadUpdated'); event.detail = { count: additions.length };
        document.dispatchEvent(event);
      }
      status(additions.length ? `${additions.length} new post${additions.length === 1 ? '' : 's'}` : 'No new posts');
      if (snapshot.archived) { dead = true; setIcon('dead'); schedule.stop(); remember(false); status('This thread is archived', true); }
    } catch { transport.invalidate(); status('Thread update could not be applied. Open the thread page to continue.', true); }
    finally {
      cycle.abort(); if (currentCycle === cycle) currentCycle = null;
      if (current === generation) { busy = false; schedule.finish(added, forced); sync(); if (postedPending) requestPostedUpdate(); }
    }
  }
  document.addEventListener('visibilitychange', () => schedule.visibility());
  document.addEventListener('scroll', onScroll, { passive: true });
  window.addEventListener('pagehide', () => { stopped = true; clearTimeout(postedTimer); postedPending = false; audio?.pause(); sync(); });
  window.addEventListener('pageshow', event => { if (event.persisted) { stopped = false; sync(); } });
  sync();
  return { update, sync, toggleAuto, posted };
}
