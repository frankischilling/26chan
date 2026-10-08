import { postId } from '../static/thread-watcher-core.v1.js';

export const DELETION_LIMITS = Object.freeze({ requestMs: 10000, responseBytes: 4096, pending: 4 });
const UNKNOWN = 'Deletion could not be confirmed. Refresh the page before trying again.';
const REJECTED = 'Deletion was rejected. Refresh the page before trying again.';
const safeBoard = board => typeof board === 'string' && /^[a-z0-9]{1,10}$/.test(board);

// This deliberately recognizes only our fixed, current-board success document.
// It does not parse, execute, or insert returned HTML (including error pages).
export function deletionResult(text, board) {
  return safeBoard(board) && typeof text === 'string' && text.length <= DELETION_LIMITS.responseBytes
    && text.trim() === `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Updating index</title><link rel="stylesheet" href="/static/board.css"></head>
<body><main><h1>Updating index</h1><p>The deletion was completed.</p><p><a href="/${board}/">Return to board</a></p></main></body></html>`;
}

function failure(outcome = 'unknown') {
  const error = new Error(outcome === 'rejected' ? REJECTED : UNKNOWN);
  error.deletionOutcome = outcome;
  return error;
}
function cancelBody(body) { try { body?.cancel()?.catch(() => {}); } catch { /* best effort */ } }
async function responseText(response, signal) {
  const contentType = response.headers?.get('content-type')?.split(';')[0].trim().toLowerCase();
  const size = response.headers?.get('content-length');
  if (contentType !== 'text/html' || (size !== null && size !== undefined
    && (!/^[0-9]+$/.test(size) || Number(size) > DELETION_LIMITS.responseBytes))) {
    cancelBody(response.body); throw failure();
  }
  const reader = response.body?.getReader?.();
  if (!reader) throw failure();
  const cancel = () => cancelBody(reader);
  signal.addEventListener('abort', cancel, { once: true });
  const chunks = [];
  let bytes = 0, reads = 0;
  try {
    for (;;) {
      const part = await reader.read();
      if (signal.aborted) throw failure();
      if (part.done) break;
      if (!(part.value instanceof Uint8Array) || ++reads > DELETION_LIMITS.responseBytes
        || (bytes += part.value.byteLength) > DELETION_LIMITS.responseBytes) throw failure();
      chunks.push(part.value);
    }
    const buffer = new Uint8Array(bytes);
    let offset = 0;
    for (const chunk of chunks) { buffer.set(chunk, offset); offset += chunk.byteLength; }
    return new TextDecoder('utf-8', { fatal: true }).decode(buffer);
  } finally {
    signal.removeEventListener('abort', cancel); cancel();
  }
}

export async function sendNativeDeletion({ board, id, fileOnly = false, signal,
  origin = location.origin, fetcher = fetch, requestMs = DELETION_LIMITS.requestMs } = {}) {
  if (!safeBoard(board) || typeof id !== 'string' || !postId(id) || typeof fileOnly !== 'boolean') {
    throw new Error('Invalid deletion target.');
  }
  let base;
  try { base = new URL(origin); } catch { throw new Error('Invalid deletion origin.'); }
  if (!['http:', 'https:'].includes(base.protocol) || base.origin !== origin
    || base.username || base.password || base.pathname !== '/' || base.search || base.hash) {
    throw new Error('Invalid deletion origin.');
  }
  if (!Number.isInteger(requestMs) || requestMs < 1 || requestMs > DELETION_LIMITS.requestMs) {
    throw new Error('Invalid deletion deadline.');
  }
  const url = `${origin}/${board}/imgboard.php`;
  const body = new URLSearchParams({ mode: 'usrdel', [id]: 'delete' });
  if (fileOnly) body.set('onlyimgdel', 'on');
  const controller = new AbortController();
  let rejectAbort;
  const interrupted = new Promise((_, reject) => { rejectAbort = reject; });
  const abort = () => { controller.abort(); rejectAbort(failure()); };
  const timer = setTimeout(abort, requestMs);
  signal?.addEventListener('abort', abort, { once: true });
  try {
    if (signal?.aborted) throw failure();
    return await Promise.race([interrupted, (async () => {
      const response = await fetcher(url, { method: 'POST', body, headers: { Accept: 'text/html' },
        mode: 'same-origin', credentials: 'same-origin', redirect: 'error', cache: 'no-store',
        signal: controller.signal });
      if (controller.signal.aborted || response.redirected || (response.url && response.url !== url)) {
        cancelBody(response.body); throw failure();
      }
      const text = await responseText(response, controller.signal);
      if (controller.signal.aborted) throw failure();
      if (response.status !== 200) {
        throw failure([400, 401, 403, 404, 409, 410, 422, 429].includes(response.status) ? 'rejected' : 'unknown');
      }
      if (!deletionResult(text, board)) throw failure();
      return true;
    })()]);
  } catch (error) {
    if (!controller.signal.aborted && error?.deletionOutcome === 'rejected') throw error;
    throw failure();
  } finally {
    clearTimeout(timer); signal?.removeEventListener('abort', abort); controller.abort();
  }
}

export function mountNativeDeletion({ root, board, settings, mobileLayout, projection, images, complete,
  confirm = message => root.ownerDocument.defaultView.confirm(message), send = sendNativeDeletion } = {}) {
  if (!root || !safeBoard(board) || typeof settings !== 'function' || typeof mobileLayout !== 'function') return null;
  const document = root.ownerDocument, window = document.defaultView;
  const pending = new Map(), outcomes = new Map();
  let suspended = false, disposed = false, feedback = null;
  const enabled = () => !suspended && !disposed && settings().disableAll !== true;
  function message(text) {
    if (disposed || suspended) return;
    if (!feedback) {
      feedback = document.createElement('p'); feedback.className = 'nativeDeletionFeedback';
      feedback.setAttribute('role', 'status'); feedback.setAttribute('aria-live', 'polite');
      document.body.append(feedback);
    }
    feedback.textContent = text;
  }
  function target(post, fileOnly) {
    const id = typeof post?.id === 'string' && post.id.startsWith('p') ? postId(post.id.slice(1)) : null;
    if (!id || !post.isConnected || document.getElementById(`p${id}`) !== post
      || !post.matches('.post') || post.closest('.board') !== root) return null;
    const container = post.parentElement, section = post.closest('.thread');
    if (container?.id !== `pc${id}` || !container.matches('.postContainer')
      || document.getElementById(container.id) !== container || container.classList.contains('deleted')
      || !section || !root.contains(section) || !postId(section.id.slice(1)) || !section.id.startsWith('t')
      || document.getElementById(section.id) !== section || section.dataset.archived === 'true') return null;
    const form = post.querySelector(`:scope > .postActions form[action="/${board}/delete"]`);
    if (!form || form.method !== 'post' || form.elements.namedItem('no')?.value !== id) return null;
    const entry = { id, post, container, section, form, fileOnly };
    if (fileOnly) {
      const file = post.querySelector(`:scope > .file[id="f${id}"]`);
      const image = file && [...file.querySelectorAll('.fileThumb > img')].find(item => !projection?.has(item));
      if (!file || document.getElementById(file.id) !== file || file.classList.contains('deleted')
        || !image || !form.elements.namedItem('file_only')) return null;
      Object.assign(entry, { file, image, imageSource: image.getAttribute('src'), anchor: image.parentElement,
        fileURL: image.parentElement.getAttribute('href') });
    }
    return entry;
  }
  function current(entry) {
    const next = target(entry.post, entry.fileOnly);
    return next && ['id', 'post', 'container', 'section', 'form', 'file', 'image', 'imageSource', 'anchor', 'fileURL']
      .every(key => next[key] === entry[key]);
  }
  function canDelete(post, fileOnly = false) {
    if (!enabled() || !mobileLayout() || typeof fileOnly !== 'boolean') return false;
    const entry = target(post, fileOnly);
    return !!entry && outcomes.get(entry.id) !== 'deleted'
      && (!fileOnly || outcomes.get(entry.id) !== 'file-deleted');
  }
  function fence(entry) {
    if (pending.get(entry.id) !== entry) return;
    pending.delete(entry.id); outcomes.set(entry.id, 'unknown');
    entry.post.removeAttribute('aria-busy'); entry.controller.abort();
    message(`Post No.${entry.id}: ${UNKNOWN}`);
  }
  function refresh() {
    for (const entry of pending.values()) if (!enabled() || !current(entry)) fence(entry);
  }
  async function remove(post, fileOnly = false) {
    if (!canDelete(post, fileOnly)) return false;
    const entry = target(post, fileOnly);
    if (pending.has(entry.id)) { message(`Post No.${entry.id}: Deletion is already in progress.`); return false; }
    if (outcomes.get(entry.id) === 'unknown') { message(`Post No.${entry.id}: ${UNKNOWN}`); return false; }
    if (pending.size >= DELETION_LIMITS.pending) { message('Wait for the current deletions to finish.'); return false; }
    if (!confirm(fileOnly ? 'Delete file?' : 'Delete post?')) return false;
    // Confirmation can pump an event loop. Recheck the same live nodes/settings.
    if (!enabled() || !mobileLayout() || !current(entry) || pending.has(entry.id)) return false;
    entry.controller = new AbortController(); pending.set(entry.id, entry);
    entry.post.setAttribute('aria-busy', 'true'); message(`Post No.${entry.id}: Deleting ${fileOnly ? 'file' : 'post'}…`);
    try {
      const result = await send({ board, id: entry.id, fileOnly, origin: window.location.origin, signal: entry.controller.signal });
      if (pending.get(entry.id) !== entry) return false;
      if (!enabled() || !current(entry)) { fence(entry); return false; }
      if (result !== true) throw failure();
      if (fileOnly) { entry.file.classList.add('deleted'); entry.image.classList.add('deleted'); }
      else entry.container.classList.add('deleted');
      outcomes.set(entry.id, fileOnly ? 'file-deleted' : 'deleted');
      // The images controller rejects deleted ancestors and releases pending media.
      const urls = fileOnly ? [entry.fileURL] : [...entry.post.querySelectorAll(':scope > .file')]
        .map(file => file.querySelector(':scope > .fileText > a[href],:scope > p > a[href]')?.getAttribute('href'));
      for (const url of urls) if (url) images?.retire?.(url);
      images?.refresh();
      complete?.(entry.post, fileOnly);
      message(`Post No.${entry.id}: ${fileOnly ? 'File' : 'Post'} deleted.`);
      return true;
    } catch (error) {
      if (pending.get(entry.id) !== entry) return false;
      if (!enabled() || !current(entry)) { fence(entry); return false; }
      const rejected = error?.deletionOutcome === 'rejected';
      if (!rejected) outcomes.set(entry.id, 'unknown');
      message(`Post No.${entry.id}: ${rejected ? REJECTED : UNKNOWN}`);
      return false;
    } finally {
      if (pending.get(entry.id) === entry) { pending.delete(entry.id); entry.post.removeAttribute('aria-busy'); }
    }
  }
  function submit(event) {
    const form = event.target;
    if (!form.matches?.('form') || form.method !== 'post'
      || form.action !== `${window.location.origin}/${board}/delete`) return;
    const id = postId(form.elements.namedItem('no')?.value);
    if (!id) return;
    const state = outcomes.get(id);
    // Preserve normal no-script/server form behavior unless it would duplicate an
    // outstanding, uncertain, or completed request. Form fields stay untouched.
    if (pending.has(id) || state === 'unknown' || state === 'deleted'
      || (state === 'file-deleted' && form.elements.namedItem('file_only')?.checked)) {
      event.preventDefault();
      message(`Post No.${id}: ${pending.has(id) ? 'Deletion is already in progress.'
        : state === 'unknown' ? UNKNOWN : 'This selection was already deleted.'}`);
    }
  }
  function preventDeletedFile(event) {
    const link = event.target.closest?.('.file a[href]');
    if (link && root.contains(link) && link.closest('.deleted')) event.preventDefault();
  }
  const storage = event => { if (event.key === null || event.key === '4chan-settings') refresh(); };
  const hide = () => { suspended = true; refresh(); feedback?.remove(); feedback = null; };
  const show = event => {
    if (!event.persisted || disposed) return;
    suspended = false; refresh();
    if ([...outcomes.values()].includes('unknown')) message(UNKNOWN);
  };
  const observer = new window.MutationObserver(refresh);
  observer.observe(root, { childList: true, subtree: true, attributes: true,
    attributeFilter: ['id', 'class', 'src', 'href', 'action', 'method', 'value', 'data-archived'] });
  observer.observe(document.body, { childList: true, subtree: true });
  document.addEventListener('submit', submit, true);
  root.addEventListener('click', preventDeletedFile, true); root.addEventListener('auxclick', preventDeletedFile, true);
  document.addEventListener('4chanSettingsSaved', refresh);
  document.addEventListener('4chanPreferencesRestored', refresh);
  window.addEventListener('storage', storage); window.addEventListener('pagehide', hide); window.addEventListener('pageshow', show);
  return { remove, canDelete, refresh, dispose() {
    disposed = true; refresh(); observer.disconnect(); feedback?.remove(); feedback = null;
    document.removeEventListener('submit', submit, true);
    root.removeEventListener('click', preventDeletedFile, true); root.removeEventListener('auxclick', preventDeletedFile, true);
    document.removeEventListener('4chanSettingsSaved', refresh);
    document.removeEventListener('4chanPreferencesRestored', refresh);
    window.removeEventListener('storage', storage); window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', show);
  } };
}
