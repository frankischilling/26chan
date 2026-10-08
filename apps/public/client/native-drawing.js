import { createDrawingUpload, drawingDimensions } from './native-drawing-core.js';
import { createDrawingPainter } from './native-drawing-painter.js';
import { sendDrawingPost, uploadTarget } from './native-quick-reply-transport.js';

export function mountNativeDrawing({ board, source, uploadForm }) {
  const template = source?.querySelector('.painter-ctrl');
  if (source?.dataset.drawingAllowed !== 'true' || !template || !/^[a-z0-9]{1,10}$/.test(board)
    || !uploadTarget(source.elements.namedItem('resto')?.value) || !uploadForm?.querySelector('input[name=upfile]')) return null;
  try { drawingDimensions(source.dataset.drawingWidth, source.dataset.drawingHeight); } catch { return null; }
  const painter = createDrawingPainter({ activeChanged: active => {
    document.documentElement.dataset.nativeDrawingActive = String(active);
  } });
  const node = (tag, text) => { const el = document.createElement(tag); if (text) el.textContent = text; return el; };
  function controlsFor(form, controls, { fileInput, key, target, prepare, accept, clear, approved,
    changed = () => {}, canOpen = () => true }) {
    const draw = controls.querySelector('[data-drawing-draw]'), reset = controls.querySelector('[data-drawing-clear]');
    const width = controls.querySelector('[data-drawing-width]'), height = controls.querySelector('[data-drawing-height]');
    const status = controls.querySelector('[data-drawing-status]');
    if (!draw || !reset || !width || !height || !status) return null;
    let data = false, exporting = false, loading = false, disposed = false, error = '', clearing = null;
    function render() {
      if (disposed) return;
      draw.textContent = data || exporting ? 'Edit' : 'Draw'; draw.disabled = loading || exporting || painter.active() || !canOpen();
      reset.disabled = !data && !exporting; width.disabled = height.disabled = data || exporting || loading;
      if (fileInput) {
        fileInput.style.visibility = data || exporting ? 'hidden' : '';
        if (data || exporting) fileInput.disabled = true;
        else if (!loading && canOpen()) fileInput.disabled = false;
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
        data = false; error = ''; if (fileInput) fileInput.value = ''; render(); return true;
      })().finally(() => { clearing = null; });
      return clearing;
    }
    const client = {
      key, target: target(), disposed: () => disposed, pending: () => data || exporting,
      error(value) { error = value; exporting = false; render(); },
      loading(value) { loading = value; render(); },
      exporting() { data = true; exporting = true; error = ''; render(); },
      async prepare() { error = ''; const result = await prepare(); render(); return result; },
      async finished(file) { data = true; exporting = false; error = ''; render(); await accept(file); render(); },
      cancelled: clearDrawing, clear: clearDrawing,
      clearForReplacement: () => disposed ? true : clear(),
      replaced() { data = false; exporting = false; error = ''; render(); },
    };
    draw.addEventListener('click', event => {
      event.preventDefault();
      if (!disposed && !loading && !exporting && canOpen()) void painter.open(client, width.value, height.value);
    });
    reset.addEventListener('click', event => { event.preventDefault(); if (canOpen()) void clearDrawing(); });
    render();
    return { clear: clearDrawing, sync: render, pending: () => data || exporting,
      blocked: () => painter.active() || loading || exporting || (data && !approved()),
      status(value) { if (!error && !exporting && !loading) status.textContent = value; },
      reset() { painter.invalidate(client); data = false; exporting = false; error = ''; render(); },
      dispose({ destroy = false } = {}) { painter.invalidate(client, { destroy }); disposed = true; data = false; exporting = false; },
    };
  }

  const fileInput = uploadForm.querySelector('input[name=upfile]'), comment = source.elements.namedItem('com');
  const originalRequired = comment?.required;
  const buttons = [...source.querySelectorAll('button[type=submit],input[type=submit]')];
  const status = template.querySelector('[data-drawing-status]');
  const check = node('button', 'Check status'); check.type = 'button'; check.dataset.drawingCheck = ''; check.hidden = true; template.append(check);
  let spoiler = null;
  if (source.dataset.spoilers === 'true') {
    const label = node('label'); spoiler = node('input'); spoiler.type = 'checkbox'; spoiler.name = 'spoiler'; spoiler.value = 'on';
    spoiler.dataset.drawingSpoiler = ''; spoiler.disabled = true; label.append(spoiler, ' Spoiler?'); template.append(label);
  }
  let ordinary, busy = false, postController, postEpoch = 0, dead = false;
  function capabilities(receipt) {
    source.querySelectorAll('[data-drawing-capability]').forEach(el => el.remove());
    if (receipt?.state === 'approved') for (const key of ['upload_id', 'upload_capability']) {
      const input = node('input'); input.type = 'hidden'; input.name = key; input.value = receipt[key]; input.dataset.drawingCapability = ''; source.append(input);
    }
    if (comment) comment.required = receipt?.state === 'approved' ? false : originalRequired;
    if (spoiler) { spoiler.disabled = receipt?.state !== 'approved' || busy; if (!receipt) spoiler.checked = false; }
  }
  const transfer = createDrawingUpload({ board, target: () => source.elements.namedItem('resto').value, changed: state => {
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
    approved: () => transfer.snapshot().approved, changed: sync, canOpen: () => !busy });
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
    if (dead || busy || ordinary.blocked() || !transfer.snapshot().approved || !source.reportValidity()) return;
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
  window.addEventListener('pagehide', () => {
    dead = true; postEpoch++; postController?.abort(); ordinary.dispose(); transfer.dispose(); painter.dispose();
  }, { once: true });
  return { active: painter.active,
    mountQuickReply({ form, fileInput, target, prepare, accept, clear, approved, changed, canOpen }) {
      const controls = node('div'); controls.id = 'qr-painter-ctrl'; controls.className = 'painter-ctrl desktop';
      for (const child of template.childNodes) controls.append(child.cloneNode(true));
      for (const element of controls.querySelectorAll('[id]')) element.removeAttribute('id');
      controls.querySelector('[data-drawing-check]')?.remove(); controls.querySelector('[data-drawing-spoiler]')?.closest('label')?.remove();
      for (const element of controls.querySelectorAll('input')) { element.disabled = false; element.removeAttribute('name'); }
      controls.querySelector('[data-drawing-width]').value = source.dataset.drawingWidth;
      controls.querySelector('[data-drawing-height]').value = source.dataset.drawingHeight;
      form.querySelector('#qrForm').append(controls);
      return controlsFor(form, controls, { fileInput, key: 'quick-reply', target, prepare, accept, clear, approved, changed, canOpen });
    },
  };
}
