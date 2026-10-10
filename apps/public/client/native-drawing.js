import { createDrawingUpload, drawingDimensions } from './native-drawing-core.js';
import { createDrawingPainter } from './native-drawing-painter.js';
import { sendDrawingPost, uploadTarget } from './native-quick-reply-transport.js';

// The media server, not a post's filename or any user-supplied link, chooses
// the URL. JPEG uploads also appear here as approved, normalized .png output.
export function drawingEditImageUrl(href, mediaOrigin, board) {
  if (board !== 'i' || typeof href !== 'string' || typeof mediaOrigin !== 'string') return null;
  try {
    const base = new URL(mediaOrigin), image = new URL(href);
    if (!['http:', 'https:'].includes(base.protocol) || base.href !== `${base.origin}/`
      || image.origin !== base.origin || image.username || image.password || image.search || image.hash) return null;
    const match = new RegExp(`^/${board}/([1-9][0-9]{0,15})\\.png$`).exec(image.pathname);
    if (!match || BigInt(match[1]) > 9007199254740991n) return null;
    return image.href;
  } catch { return null; }
}

// Source Edit links exist only on a live /i/ thread page.
// Re-scan on updater commits: its newly inserted posts reuse the same renderer.
export function mountDrawingEditLinks({ root = document, section, board, mediaOrigin, eligible, onEdit }) {
  if (board !== 'i' || !section || typeof eligible !== 'function' || typeof onEdit !== 'function') return null;
  let disposed = false;
  const candidate = cell => {
    const id = cell.id?.startsWith('fT') ? cell.id.slice(2) : '';
    const container = cell.closest('.postContainer');
    if (!/^[1-9][0-9]{0,18}$/.test(id) || BigInt(id) > 9223372036854775807n
      || !container || container.id !== `pc${id}` || container.parentElement !== section) return null;
    const href = cell.querySelector(':scope > a[href]')?.href;
    const url = drawingEditImageUrl(href, mediaOrigin, board);
    return url ? { id, url } : null;
  };
  function refresh() {
    if (disposed) return;
    const enabled = eligible() && section.isConnected;
    for (const cell of section.querySelectorAll('.fileText')) {
      const old = cell.querySelector('[data-drawing-edit-wrap]');
      const source = enabled ? candidate(cell) : null;
      if (!source) { old?.remove(); continue; }
      if (old) continue;
      const wrapper = root.createElement('small'), link = root.createElement('a');
      wrapper.dataset.drawingEditWrap = '';
      link.href = '#'; link.dataset.drawingEdit = '';
      link.title = 'Open in Tegaki'; link.setAttribute('aria-label', `Edit image from post ${source.id} in Tegaki`);
      link.textContent = 'Edit'; wrapper.append(' ', link); cell.append(wrapper);
    }
  }
  function click(event) {
    const link = event.target?.closest?.('[data-drawing-edit]');
    if (!link || !section.contains(link)) return;
    event.preventDefault();
    if (disposed || !eligible() || !section.isConnected) return;
    const cell = link.closest('.fileText');
    const source = cell && candidate(cell);
    if (!source) return;
    const container = cell.closest('.postContainer');
    // Source QR imports through a[class="fileThumb"] exactly. Spoiler thumbnails
    // have an extra imgspoiler class, so their visible Edit links cannot import.
    const thumb = container.querySelector(`#f${source.id} a[class="fileThumb"]`);
    if (thumb && drawingEditImageUrl(thumb.href, mediaOrigin, board) === source.url) onEdit(source);
  }
  root.addEventListener('click', click);
  root.addEventListener('4chanThreadUpdated', refresh);
  root.addEventListener('boardThreadStateChanged', refresh);
  refresh();
  return { refresh, dispose() {
    disposed = true;
    root.removeEventListener('click', click);
    root.removeEventListener('4chanThreadUpdated', refresh);
    root.removeEventListener('boardThreadStateChanged', refresh);
    for (const link of section.querySelectorAll('[data-drawing-edit-wrap]')) link.remove();
  } };
}

export function mountNativeDrawing({ board, source, uploadForm }) {
  const template = source?.querySelector('.painter-ctrl');
  const ordinaryAllowed = board !== 'i' && source?.dataset.drawingAllowed === 'true';
  const editAllowed = board === 'i' && source?.dataset.drawingEditAllowed === 'true'
    && source?.elements.namedItem('resto')?.value !== '0';
  if ((!ordinaryAllowed && !editAllowed) || !template || !/^[a-z0-9]{1,10}$/.test(board)
    || !uploadTarget(source.elements.namedItem('resto')?.value) || !uploadForm?.querySelector('input[name=upfile]')) return null;
  try { drawingDimensions(source.dataset.drawingWidth, source.dataset.drawingHeight); } catch { return null; }
  const painter = createDrawingPainter({ activeChanged: active => {
    document.documentElement.dataset.nativeDrawingActive = String(active);
  } });
  const node = (tag, text) => { const el = document.createElement(tag); if (text) el.textContent = text; return el; };
  function controlsFor(form, controls, { fileInput, key, target, prepare, accept, clear, approved,
    changed = () => {}, canOpen = () => true, canDraw = true }) {
    const draw = controls.querySelector('[data-drawing-draw]'), reset = controls.querySelector('[data-drawing-clear]');
    const width = controls.querySelector('[data-drawing-width]'), height = controls.querySelector('[data-drawing-height]');
    const status = controls.querySelector('[data-drawing-status]');
    if (!draw || !reset || !width || !height || !status) return null;
    let data = false, exporting = false, loading = false, disposed = false, error = '', clearing = null;
    let sourcePost = null, elapsed = null;
    const clearSource = () => { sourcePost = null; elapsed = null; };
    function render() {
      if (disposed) return;
      // On edit-only boards, the empty controls are inert. Keep upload and
      // image-loading errors visible without exposing a blank Draw action.
      if (!canDraw) controls.hidden = sourcePost === null && !loading && !error;
      draw.hidden = !canDraw && sourcePost === null;
      draw.textContent = data || exporting || sourcePost ? 'Edit' : 'Draw';
      draw.disabled = loading || exporting || painter.active() || !canOpen() || (!canDraw && !sourcePost);
      reset.hidden = !canDraw && sourcePost === null;
      reset.disabled = !data && !exporting;
      width.hidden = height.hidden = !canDraw;
      width.disabled = height.disabled = !canDraw || data || exporting || loading;
      if (fileInput) {
        fileInput.style.visibility = data || exporting ? 'hidden' : '';
        fileInput.disabled = data || exporting || loading || !canOpen();
      }
      if (error) status.textContent = error;
      else if (exporting) status.textContent = 'Exporting drawing…';
      else if (loading) status.textContent = 'Opening drawing…';
      else if (!data) status.textContent = '';
      changed();
    }
    async function clearDrawing() {
      if (disposed) return true;
      if (clearing) return clearing;
      painter.invalidate(client);
      exporting = false;
      clearing = (async () => {
        if (!await clear()) { render(); return false; }
        data = false; clearSource(); error = ''; if (fileInput) fileInput.value = ''; render(); return true;
      })().finally(() => { clearing = null; });
      return clearing;
    }
    const client = {
      key, target: target(), disposed: () => disposed, allowed: canOpen, pending: () => data || exporting,
      error(value) { error = value; exporting = false; render(); },
      loading(value) { loading = value; if (value) error = ''; render(); },
      suspended() { loading = false; exporting = false; render(); },
      exporting() { data = true; exporting = true; error = ''; render(); },
      async prepare() { error = ''; const result = await prepare(); render(); return result; },
      async finished(file, seconds) {
        data = true; exporting = false; error = '';
        if (sourcePost !== null) elapsed = String(seconds);
        render(); await accept(file); render();
      },
      imported(id) { sourcePost = id ?? null; elapsed = null; render(); },
      sourcePost: () => sourcePost,
      cancelled() { clearSource(); return clearDrawing(); }, clear: clearDrawing,
      clearForReplacement: () => disposed ? true : clear(),
      replaced() { data = false; exporting = false; clearSource(); error = ''; render(); },
    };
    draw.addEventListener('click', event => {
      event.preventDefault();
      if (!disposed && !loading && !exporting && canOpen() && (canDraw || sourcePost !== null)) {
        void painter.open(client, width.value, height.value);
      }
    });
    reset.addEventListener('click', event => { event.preventDefault(); if (canOpen()) void clearDrawing(); });
    render();
    return { clear: clearDrawing, sync: render, pending: () => data || exporting,
      retained: () => painter.retained(client.key, client.target),
      blocked: () => painter.active() || loading || exporting || (data && !approved()),
      annotation: () => data && !exporting && sourcePost && elapsed !== null && approved()
        ? { oe_src: sourcePost, oe_time: elapsed } : null,
      importFromPost(source) {
        if (disposed || loading || exporting || painter.active() || !canOpen()) return Promise.resolve(false);
        return painter.importFromPost(client, source);
      },
      status(value) { if (!error && !exporting && !loading) status.textContent = value; },
      reset() { painter.invalidate(client); data = false; exporting = false; clearSource(); error = ''; render(); },
      dispose({ destroy = false } = {}) { painter.invalidate(client, { destroy }); disposed = true; data = false; exporting = false; clearSource(); },
    };
  }

  const fileInput = uploadForm.querySelector('input[name=upfile]'), comment = source.elements.namedItem('com');
  const originalRequired = comment?.required;
  const buttons = [...source.querySelectorAll('button[type=submit],input[type=submit]')];
  const status = template.querySelector('[data-drawing-status]');
  let ordinary = null, transfer = null, busy = false, postController, postEpoch = 0, dead = false, suspended = false;
  if (ordinaryAllowed) {
    const check = node('button', 'Check status'); check.type = 'button'; check.dataset.drawingCheck = ''; check.hidden = true; template.append(check);
    let spoiler = null;
    if (source.dataset.spoilers === 'true') {
      const label = node('label'); spoiler = node('input'); spoiler.type = 'checkbox'; spoiler.name = 'spoiler'; spoiler.value = 'on';
      spoiler.dataset.drawingSpoiler = ''; spoiler.disabled = true; label.append(spoiler, ' Spoiler?'); template.append(label);
    }
    function capabilities(receipt) {
      source.querySelectorAll('[data-drawing-capability]').forEach(el => el.remove());
      if (receipt?.state === 'approved') for (const key of ['upload_id', 'upload_capability']) {
        const input = node('input'); input.type = 'hidden'; input.name = key; input.value = receipt[key]; input.dataset.drawingCapability = ''; source.append(input);
      }
      if (comment) comment.required = receipt?.state === 'approved' ? false : originalRequired;
      if (spoiler) { spoiler.disabled = receipt?.state !== 'approved' || busy; if (!receipt) spoiler.checked = false; }
    }
    transfer = createDrawingUpload({ board, target: () => source.elements.namedItem('resto').value, changed: state => {
      capabilities(state.approved ? state.receipt : null);
      check.hidden = !state.canCheck; check.disabled = busy;
      const labels = { uploading: 'Uploading tegaki.png…', queued: 'tegaki.png queued for processing.', processing: 'tegaki.png is processing.',
        approved: 'tegaki.png is approved.', checking: 'Checking tegaki.png…', canceling: 'Canceling drawing upload…',
        failed: 'Drawing upload failed. Edit and Finish to try again, or Clear.', incomplete: 'Drawing upload is incomplete. Edit and Finish to try again, or Clear.' };
      if (status) status.textContent = state.error || labels[state.phase] || '';
      ordinary?.sync();
    } });
    const sync = () => { for (const button of buttons) button.disabled = busy || !!ordinary?.blocked() || transfer.snapshot().busy; };
    ordinary = controlsFor(source, template, { fileInput, key: 'ordinary', target: () => source.elements.namedItem('resto').value,
      prepare: () => transfer.clear(), accept: file => transfer.select(file), clear: () => transfer.clear(),
      approved: () => transfer.snapshot().approved, changed: sync, canOpen: () => !busy && !dead && !suspended });
    if (!ordinary) { transfer.dispose(); painter.dispose(); return null; }
    check.addEventListener('click', () => { void transfer.status(); });
    fileInput.addEventListener('change', () => { if (fileInput.files?.length && ordinary.pending()) void ordinary.clear(); });
    uploadForm.addEventListener('submit', event => {
      if (ordinary.pending() || transfer.snapshot().pending) { event.preventDefault(); if (status) status.textContent = 'Clear the drawing before uploading a file.'; }
    });
    source.addEventListener('reset', () => { if (!busy) void ordinary.clear(); });
    source.addEventListener('submit', event => {
      if (!ordinary.pending() && !ordinary.blocked() && !transfer.snapshot().pending && !busy) return;
      event.preventDefault(); void send();
    });
    async function send() {
      if (dead || suspended || busy || ordinary.blocked() || !transfer.snapshot().approved || !source.reportValidity()) return;
      const target = source.elements.namedItem('resto').value, epoch = ++postEpoch;
      const fields = Object.fromEntries(new FormData(source));
      busy = true; transfer.posting(true); ordinary.sync(); sync(); postController = new AbortController();
      if (status) status.textContent = 'Posting drawing…';
      try {
        const result = await sendDrawingPost({ board, thread: target, fields, signal: postController.signal });
        if (dead || epoch !== postEpoch) return;
        if (result.error) { if (status) status.textContent = result.error; return; }
        transfer.retire(); ordinary.reset();
        location.assign(`/${board}/thread/${result.thread}#p${result.post}`);
      } catch (failure) {
        if (dead || epoch !== postEpoch) return;
        // A missing response may mean the one-use approval was consumed. Retain
        // the canvas and all text, but never offer the capability for reuse.
        transfer.retire();
        if (status) status.textContent = failure.message;
      } finally {
        if (!dead && epoch === postEpoch) { busy = false; transfer.posting(false); postController = null; ordinary.sync(); sync(); }
      }
    }
  }
  function onPageHide(event) {
    if (!event.persisted) {
      dead = true; postEpoch++; postController?.abort(); ordinary?.dispose(); transfer?.dispose(); painter.dispose();
      return;
    }
    if (dead) return;
    suspended = true;
    const posting = busy;
    postEpoch++; postController?.abort(); postController = null; busy = false;
    // BFCache freezes this document. A suspended load/export must never resume
    // or attach an upload to an old editor attempt after pageshow. Keep layers.
    painter.suspend();
    if (posting) {
      // A response lost during posting could have consumed the one-use receipt.
      transfer?.retire();
      if (status) status.textContent = 'Posting result uncertain. Check the thread before posting again.';
    } else {
      const receipt = transfer?.snapshot();
      // A fully approved, idle receipt has no outstanding callback and remains
      // owned. All unfinished requests are fenced and their receipts retired.
      if (receipt?.pending && !receipt.approved) transfer.resetTarget();
    }
    ordinary?.sync();
  }
  function onPageShow(event) {
    if (event.persisted && suspended && !dead) {
      suspended = false;
      ordinary?.sync();
    }
  }
  window.addEventListener('pagehide', onPageHide);
  window.addEventListener('pageshow', onPageShow);
  return { active: painter.active, editAllowed,
    mountQuickReply({ form, fileInput, target, prepare, accept, clear, approved, changed, canOpen }) {
      const controls = node('div'); controls.id = 'qr-painter-ctrl'; controls.className = 'painter-ctrl desktop';
      for (const child of template.childNodes) controls.append(child.cloneNode(true));
      if (editAllowed) for (const child of [...controls.childNodes]) {
        // /i/ has no blank-canvas Draw, including the ordinary size label.
        if (child.nodeType === 3 && /^(?:\s*Size\s*|\s*×\s*)$/.test(child.textContent)) child.remove();
      }
      for (const element of controls.querySelectorAll('[id]')) element.removeAttribute('id');
      controls.querySelector('[data-drawing-check]')?.remove(); controls.querySelector('[data-drawing-spoiler]')?.closest('label')?.remove();
      for (const element of controls.querySelectorAll('input')) { element.disabled = false; element.removeAttribute('name'); }
      controls.querySelector('[data-drawing-width]').value = source.dataset.drawingWidth;
      controls.querySelector('[data-drawing-height]').value = source.dataset.drawingHeight;
      form.querySelector('#qrForm').append(controls);
      return controlsFor(form, controls, { fileInput, key: 'quick-reply', target, prepare, accept, clear, approved, changed, canOpen, canDraw: ordinaryAllowed });
    },
  };
}
