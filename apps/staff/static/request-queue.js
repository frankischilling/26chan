'use strict';
// The server owns authorization and review decisions. No queue data is persisted.
(() => {
  const filter = document.getElementById('filter-form');
  const search = document.getElementById('search-box');
  const status = document.getElementById('queue-status');
  const undo = document.getElementById('queue-undo');
  const dark = document.getElementById('cfg-cb-dt');
  const keys = document.getElementById('queue-keybinds');
  if (!filter || !search || !status || !undo || !dark || !keys) return;
  let focused = null;
  let pending = null;
  let sending = false;
  let uncertain = false;
  let refresh = null;
  let generation = 0;
  let leaving = false;
  const rows = () => [...document.querySelectorAll('#br-list .ban-request')];
  const visible = () => rows().filter(row => !row.hidden && !row.classList.contains('processing'));
  const say = message => { status.textContent = message; };
  const saved = key => { try { return localStorage.getItem(key); } catch { return null; } };
  const save = (key, value) => {
    try { if (value === null) localStorage.removeItem(key); else localStorage.setItem(key, value); } catch { /* Preferences are optional. */ }
  };
  dark.checked = saved('dark-theme') !== null;
  keys.checked = saved('request-queue-keybinds') !== 'off';
  document.documentElement.classList.toggle('dark-theme', dark.checked);
  dark.addEventListener('change', () => {
    document.documentElement.classList.toggle('dark-theme', dark.checked);
    save('dark-theme', dark.checked ? '1' : null);
  });
  keys.addEventListener('change', () => save('request-queue-keybinds', keys.checked ? null : 'off'));
  filter.hidden = false;
  document.getElementById('queue-preferences').hidden = false;

  function focus(row) {
    focused?.classList.remove('focused');
    focused = row;
    if (!row) return;
    row.classList.add('focused');
    row.focus({ preventScroll: true });
    const rect = row.getBoundingClientRect();
    if (rect.top < 0 || rect.bottom > document.documentElement.clientHeight) row.scrollIntoView({ block: 'nearest' });
  }
  function next() {
    const list = rows();
    const available = visible();
    focus(list.slice(list.indexOf(focused) + 1).find(row => available.includes(row)) || available[0] || null);
  }
  function previous() {
    const list = rows();
    const index = list.indexOf(focused);
    const available = visible();
    focus(index < 0 ? null : list.slice(0, index).reverse().find(row => available.includes(row)) || null);
  }
  function applyFilter() {
    const query = search.value.toLowerCase();
    for (const row of rows()) {
      row.hidden = query !== '' && !['.post-name', '.post-content', '.ban-request-template']
        .some(selector => row.querySelector(selector)?.textContent.toLowerCase().includes(query));
    }
    document.getElementById('reset-btn').hidden = query === '';
    if (focused?.hidden) focus(null);
  }
  filter.addEventListener('submit', event => { event.preventDefault(); applyFilter(); });
  filter.addEventListener('reset', () => { search.value = ''; applyFilter(); });
  function title() {
    const board = document.getElementById('content').dataset.board;
    const label = board ? `/${board}/` : 'All';
    document.title = `Ban Requests (${rows().length}) - ${label}`;
    document.getElementById('title').textContent = `Ban Requests - ${label}`;
  }
  title();

  // Read-only refreshes may be superseded. A pending/sent review is never retried.
  async function reload() {
    if (pending || sending) { say('Finish or undo the pending review before refreshing.'); return; }
    refresh?.abort();
    const controller = new AbortController();
    refresh = controller;
    const current = ++generation;
    say('Loading requests…');
    try {
      const response = await fetch(location.href, { credentials: 'same-origin', cache: 'no-store', redirect: 'error', signal: controller.signal });
      if (!response.ok) throw new Error('refresh');
      const text = await response.text();
      if (text.length > 4 * 1024 * 1024) throw new Error('size');
      const page = new DOMParser().parseFromString(text, 'text/html');
      const content = page.getElementById('content');
      const menu = page.getElementById('board-menu');
      if (!content?.hasAttribute('data-board') || !menu) throw new Error('page');
      if (current !== generation || leaving) return;
      focus(null);
      document.getElementById('content').replaceWith(document.importNode(content, true));
      document.getElementById('board-menu').replaceWith(document.importNode(menu, true));
      uncertain = false;
      applyFilter();
      title();
      say('Requests refreshed.');
    } catch (error) {
      if (error.name !== 'AbortError' && current === generation && !leaving) say('Could not refresh requests. Reload the page or sign in again.');
    } finally { if (current === generation) refresh = null; }
  }
  document.getElementById('refresh-btn').addEventListener('click', event => { event.preventDefault(); void reload(); });

  function cancel() {
    if (!pending) return;
    clearTimeout(pending.timer);
    const row = pending.row;
    row.classList.remove('processing');
    pending = null;
    undo.hidden = true;
    focus(row);
    say('Pending review cancelled. Nothing was submitted.');
  }
  undo.addEventListener('click', cancel);
  async function send(job) {
    if (pending !== job || leaving) return;
    pending = null;
    sending = true;
    undo.hidden = true;
    say('Submitting review…');
    let message;
    try {
      const response = await fetch('/ban-requests/review', {
        method: 'POST', credentials: 'same-origin', cache: 'no-store', redirect: 'error',
        headers: { Accept: 'application/json' }, body: job.data,
      });
      if (!response.ok) throw new Error('review');
      const result = await response.json();
      if (typeof result.changed !== 'boolean' || !Number.isSafeInteger(result.cleared) || result.cleared < 0) throw new Error('result');
      message = result.changed
        ? `Request ${job.approved ? 'approved' : 'denied'}. ${result.cleared} pending request(s) cleared.`
        : 'This request was already reviewed or is no longer available.';
    } catch {
      uncertain = true;
      message = 'Review could not be confirmed. Refresh to check its state before taking another action. It will not be retried automatically.';
    } finally {
      sending = false;
      job.row.classList.remove('processing');
    }
    if (leaving) return;
    if (!uncertain) {
      uncertain = true;
      await reload();
      if (uncertain) message += ' The queue could not be refreshed. Reload before another review.';
    }
    say(message);
  }
  document.addEventListener('submit', event => {
    const form = event.target;
    if (!(form instanceof HTMLFormElement) || new URL(form.action).pathname !== '/ban-requests/review') return;
    event.preventDefault();
    if (pending || sending || uncertain || refresh) {
      say(uncertain ? 'Refresh to check the previous review before submitting again.' : 'Finish or undo the current action before submitting another review.');
      return;
    }
    const button = event.submitter;
    if (button?.disabled || !form.reportValidity()) return;
    const row = form.closest('.ban-request');
    if (!row || row.hidden) return;
    const data = new URLSearchParams(new FormData(form));
    if (button?.name) data.set(button.name, button.value);
    if (!['accept', 'deny'].includes(data.get('decision'))) return;
    const job = { row, data, approved: data.get('decision') === 'accept', timer: null };
    pending = job;
    row.classList.add('processing');
    undo.hidden = false;
    say(`${job.approved ? 'Approval' : 'Denial'} pending for 3 seconds. Use Undo or U to cancel.`);
    job.timer = setTimeout(() => void send(job), 3000);
    if (focused === row) next();
  });
  document.addEventListener('click', event => {
    if (!(event.target instanceof Element) || event.target.closest('a,button,input,select,textarea,summary,label')) return;
    const row = event.target.closest('.ban-request');
    if (row && !row.hidden) focus(row);
  });
  document.addEventListener('keydown', event => {
    if (!keys.checked || event.repeat || event.isComposing || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (event.key === 'Escape' && event.target instanceof Element && event.target.closest('.request-amend')) {
      event.preventDefault();
      event.target.closest('.request-amend').open = false;
      focus(event.target.closest('.ban-request'));
      return;
    }
    if (!(event.target instanceof Element) || event.target.closest('input,textarea,select,button,a,summary,[contenteditable]')) return;
    const key = event.key.toLowerCase();
    if (!['arrowright', 'arrowleft', 'a', 'd', 'r', 'u', 'c', 'w', 'escape'].includes(key)) return;
    event.preventDefault();
    if (key === 'arrowright') next();
    else if (key === 'arrowleft') previous();
    else if (key === 'r') void reload();
    else if (key === 'u') cancel();
    else if (key === 'escape') {
      for (const panel of document.querySelectorAll('.request-amend[open]')) panel.open = false;
      focus(focused);
    } else if (key === 'w') {
      const button = focused?.querySelector('button[data-review="Warn"]');
      if (button && !button.disabled) button.click();
      else say('This request cannot be accepted as a warning.');
    } else if (key === 'c') {
      const panel = focused?.querySelector('.request-amend');
      if (panel) { panel.open = true; panel.querySelector('select')?.focus(); }
    } else {
      const button = focused?.querySelector(`button[name="decision"][value="${key === 'a' ? 'accept' : 'deny'}"]`);
      if (button && !button.disabled) button.click();
    }
  });
  window.addEventListener('pagehide', () => {
    leaving = true;
    ++generation;
    refresh?.abort();
    if (pending) clearTimeout(pending.timer);
    pending = null;
  });
  window.addEventListener('pageshow', event => {
    leaving = false;
    // Browser history must not restore an old authorized queue or pending timer.
    if (event.persisted) location.reload();
  });
})();
