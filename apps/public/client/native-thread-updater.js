import { NativeUpdaterTransport } from './native-updater-transport.js';
import { postId } from '../static/thread-watcher-core.v1.js';

function build(node) {
  if (typeof node === 'string') return document.createTextNode(node);
  const element = document.createElement(node.tag);
  for (const [key, value] of Object.entries(node.attrs)) element.setAttribute(key, value);
  for (const child of node.children) element.append(build(child));
  return element;
}

export function mountNativeThreadUpdater({ board, thread, mediaOrigin, settings, applied }) {
  const section = document.getElementById(`t${thread}`);
  if (!postId(thread) || !section || section.dataset.archived === 'true') return null;
  const transport = new NativeUpdaterTransport({ board, thread, mediaOrigin });
  const controls = [], statuses = [], mobileLinks = [];
  let busy = false, dead = false, stopped = false, generation = 0;
  let currentCycle = null;
  const disabled = () => settings().disableAll === true;
  for (const nav of document.querySelectorAll('.threadNav')) {
    const group = document.createElement('span'); group.className = 'nativeUpdater';
    const status = document.createElement('span'); status.className = 'nativeUpdaterStatus';
    status.setAttribute('role', 'status'); status.setAttribute('aria-live', 'polite'); statuses.push(status);
    if (nav.classList.contains('mobile')) {
      const link = nav.querySelector('[data-thread-refresh]');
      if (!link) continue;
      link.addEventListener('click', event => {
        if (disabled() || event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
        event.preventDefault(); void update();
      });
      mobileLinks.push(link);
    } else {
      const link = document.createElement('a'); link.href = `/${board}/thread/${thread}`;
      link.textContent = 'Update'; link.dataset.cmd = 'update';
      link.addEventListener('click', event => {
        if (event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
        event.preventDefault(); void update();
      });
      group.append(' [', link, '] ');
    }
    group.append(status); nav.append(group); controls.push(group);
  }
  function status(text, error = false) {
    for (const node of statuses) { node.textContent = text; node.classList.toggle('tu-error', error); }
  }
  function sync() {
    if (disabled()) { generation++; currentCycle?.abort(); currentCycle = null; transport.cancel(); busy = false; status(''); }
    for (const node of controls) { node.hidden = disabled(); node.setAttribute('aria-busy', String(busy)); }
    for (const node of mobileLinks) {
      node.textContent = disabled() ? 'Refresh' : 'Update'; node.dataset.cmd = 'update';
      node.dataset.updaterReady = String(!disabled());
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
  async function update() {
    if (disabled() || stopped || dead || busy) return;
    const current = ++generation;
    const cycle = new AbortController(); currentCycle = cycle;
    busy = true; status('Updating...'); sync();
    const result = await transport.refresh({ signal: cycle.signal });
    if (current !== generation || disabled() || stopped || !section.isConnected) return;
    try {
      if (result.status !== 'ok') {
        if (result.status === 'http-error' && result.httpStatus === 404) {
          dead = true; status('This thread has been pruned or deleted', true);
        } else if (result.status === 'cooldown') status('Please wait before updating again.');
        else if (result.status !== 'cancelled') status('Connection Error. Open the thread page to retry.', true);
        return;
      }
      const { snapshot } = result;
      const existing = [...section.querySelectorAll(':scope > .postContainer')];
      const last = existing.at(-1)?.id.slice(2);
      if (!postId(last)) throw new Error('invalid-current-thread');
      const additions = snapshot.posts.filter(post => BigInt(post.no) > BigInt(last));
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
      threadState(snapshot);
      if (additions.length) {
        await applied?.(snapshot, cycle.signal);
        if (current !== generation || disabled() || stopped) return;
        const event = new Event('4chanThreadUpdated'); event.detail = { count: additions.length };
        document.dispatchEvent(event);
      }
      status(additions.length ? `${additions.length} new post${additions.length === 1 ? '' : 's'}` : 'No new posts');
      if (snapshot.archived) { dead = true; status('This thread is archived', true); }
    } catch { status('Thread update could not be applied. Open the thread page to continue.', true); }
    finally {
      cycle.abort(); if (currentCycle === cycle) currentCycle = null;
      if (current === generation) { busy = false; sync(); }
    }
  }
  window.addEventListener('pagehide', () => { stopped = true; generation++; currentCycle?.abort(); currentCycle = null; transport.cancel(); }, { once: true });
  sync();
  return { update, sync };
}
