// Native full-page controls. The server still owns authorization and ordered
// partial success; this module never deletes optimistically or retries a POST.
const mounts = new WeakMap();
const canonicalId = value => typeof value === 'string' && /^[1-9][0-9]{0,18}$/.test(value)
  && !/\D/.test(value) && BigInt(value) <= 9223372036854775807n;
const canonicalBoard = value => typeof value === 'string' && /^[a-z0-9]{1,10}$/.test(value) && !/[^a-z0-9]/.test(value);
export const SELECTION_POSTS = 20001;

export function selectionIds(ids, fileOnly = false) {
  if (!Array.isArray(ids) || typeof fileOnly !== 'boolean' || ids.length === 0) throw new TypeError('Select a post first.');
  // mode and pwd are always successful controls; onlyimgdel is optional.
  const limit = 20 - 2 - Number(fileOnly);
  if (ids.length > limit) throw new RangeError(`Select at most ${limit} posts for one deletion.`);
  if (!ids.every(canonicalId) || new Set(ids).size !== ids.length) throw new TypeError('The selection changed. Reload before deleting.');
  return [...ids];
}

export function mountSourcePostActions({ document: doc, window: win } = {}) {
  const form = doc?.getElementById('delform'), root = doc?.querySelector('.board');
  if (!win || !(form instanceof win.HTMLFormElement) || !root || root.closest('.catalog')) return null;
  const board = form.dataset.board;
  if (!canonicalBoard(board)) return null;
  mounts.get(form)?.destroy();
  const mode = form.querySelector('input[name="mode"]'), password = form.querySelector('#delPassword');
  const fileOnly = form.querySelector('input[name="onlyimgdel"]'), submit = form.querySelector('input[type="submit"]');
  const report = form.querySelector('#bottomReportBtn'), status = form.querySelector('#deleteSelectionStatus');
  if (![mode, password, fileOnly, submit, report].every(node => node instanceof win.HTMLInputElement) || !status) return null;
  let retired = false, suspended = false, pending = false, api;
  const say = message => { if (status.isConnected) status.textContent = message; };
  const live = () => !retired && !suspended && form.isConnected && root.isConnected
    && doc.getElementById('delform') === form && doc.querySelector('.board') === root
    && form.getAttribute('action') === `/${board}/imgboard.php` && form.method === 'post'
    && !form.hasAttribute('target') && form.enctype === 'application/x-www-form-urlencoded';

  function target(input) {
    const id = input.name;
    if (!canonicalId(id) || input.type !== 'checkbox' || input.value !== 'delete'
      || input.id !== `select${id}` || input.getAttribute('form') !== 'delform'
      || input.form !== form || doc.getElementById(input.id) !== input) return null;
    const header = input.parentElement, post = header?.parentElement, container = post?.parentElement;
    const section = container?.parentElement;
    if (header?.id !== `pi${id}` || !header.classList.contains('postInfo')
      || post?.id !== `p${id}` || !post.classList.contains('post') || doc.getElementById(post.id) !== post
      || container?.id !== `pc${id}` || !container.classList.contains('postContainer')
      || !section?.classList.contains('thread') || !canonicalId(section.id.slice(1)) || section.id !== `t${section.id.slice(1)}`
      || section.parentElement !== root) return null;
    return post;
  }

  function selected() {
    if (!live() || form.elements.length > SELECTION_POSTS + 5
      || mode.form !== form || mode.type !== 'hidden' || mode.name !== 'mode' || mode.value !== 'usrdel' || mode.disabled
      || password.form !== form || password.type !== 'hidden' || password.name !== 'pwd' || password.id !== 'delPassword' || password.disabled
      || fileOnly.form !== form || fileOnly.type !== 'checkbox' || fileOnly.name !== 'onlyimgdel' || fileOnly.value !== 'on' || fileOnly.disabled
      || submit.form !== form || submit.type !== 'submit' || submit.name || submit.disabled
      || report.form !== form || report.type !== 'button' || report.name
      || [mode, password, fileOnly, submit, report].some(node => !form.contains(node)
        || node.hasAttribute('formaction') || node.hasAttribute('formmethod') || node.hasAttribute('formtarget') || node.hasAttribute('formenctype'))) {
      throw new TypeError('The controls changed. Reload before deleting.');
    }
    const chosen = [], known = new Set([mode, password, fileOnly, submit, report]), seen = new Set();
    for (const input of form.elements) {
      if (known.has(input)) continue;
      if (!(input instanceof win.HTMLInputElement)) throw new TypeError('The controls changed. Reload before deleting.');
      const post = target(input);
      if (!post || seen.has(input.name)) throw new TypeError('The selection changed. Reload before deleting.');
      seen.add(input.name);
      if (input.checked && !input.disabled) {
        if (post.parentElement.classList.contains('deleted') || post.closest('.thread').dataset.archived === 'true') {
          throw new TypeError('The selected post is no longer available. Reload before deleting.');
        }
        chosen.push({ id: input.name, post });
      }
    }
    return chosen;
  }

  function onSubmit(event) {
    if (event.defaultPrevented) return;
    try {
      if (pending) throw new TypeError('Deletion was submitted. Reload before trying again.');
      if (event.submitter && event.submitter !== submit) throw new TypeError('The controls changed. Reload before deleting.');
      selectionIds(selected().map(item => item.id), fileOnly.checked);
      pending = true;
      form.setAttribute('aria-busy', 'true');
      say('Submitting deletion. Earlier items may succeed even if a later item is refused.');
      // Keep every successful control intact for the browser's native POST.
    } catch (error) {
      event.preventDefault();
      say(error.message);
    }
  }

  function onReport(event) {
    event.preventDefault();
    try {
      const first = selected()[0];
      if (!first) throw new TypeError('Select a post first.');
      // Source core reports the first selected post, without deleting any.
      const url = `/${board}/imgboard.php?mode=report&no=${first.id}`;
      let popup = null;
      try { popup = win.open(url, `report-popup-${board}-${first.id}-${Date.now()}`,
        'popup,toolbar=0,scrollbars=1,location=0,status=1,menubar=0,resizable=1,width=380,height=510'); } catch { /* Native GET fallback. */ }
      if (!popup) win.location.assign(url);
    } catch (error) { say(error.message); }
  }

  const hide = event => { if (event.persisted) suspended = true; else destroy(); };
  const show = event => {
    if (event.persisted && !retired) {
      suspended = false;
      if (pending) say('Deletion was submitted. Reload before trying again.');
    }
  };
  function destroy() {
    if (retired) return;
    retired = true;
    form.removeEventListener('submit', onSubmit); report.removeEventListener('click', onReport);
    win.removeEventListener('pagehide', hide); win.removeEventListener('pageshow', show);
    if (mounts.get(form) === api) mounts.delete(form);
  }
  form.addEventListener('submit', onSubmit); report.addEventListener('click', onReport);
  win.addEventListener('pagehide', hide); win.addEventListener('pageshow', show);
  api = { destroy }; mounts.set(form, api); return api;
}

if (typeof document !== 'undefined' && typeof window !== 'undefined') mountSourcePostActions({ document, window });
