import { QUICK_REPLY_UPLOAD_LIMITS, uploadTarget, uploadQuickReplyFile, checkQuickReplyUpload,
  cancelQuickReplyUpload } from './native-quick-reply-transport.js';

// The isolated decoder accepts at most 1024 pixels on either axis. These are
// deployment safety limits, not the unused 100–800 values in the source config.
export const DRAWING_LIMITS = Object.freeze({ side: 1024, pixels: 1024 * 1024, bytes: QUICK_REPLY_UPLOAD_LIMITS.bytes });
export function drawingDimensions(width, height) {
  const values = [width, height].map(value => typeof value === 'number' ? value
    : typeof value === 'string' && /^[1-9][0-9]{0,3}$/.test(value) ? Number(value) : NaN);
  if (values.some(value => !Number.isSafeInteger(value) || value < 1 || value > DRAWING_LIMITS.side)
    || values[0] * values[1] > DRAWING_LIMITS.pixels) throw new Error('Choose whole canvas dimensions from 1 to 1024 pixels.');
  return { width: values[0], height: values[1] };
}

export async function drawingPngFile(canvas) {
  const dimensions = drawingDimensions(canvas?.width, canvas?.height);
  if (typeof canvas.toBlob !== 'function') throw new Error('The drawing could not be exported. Edit it and try Finish again.');
  const blob = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Drawing export timed out. Edit it and try Finish again.')), 10_000);
    try { canvas.toBlob(value => { clearTimeout(timer); value ? resolve(value) : reject(new Error('Drawing export failed.')); }, 'image/png'); }
    catch (error) { clearTimeout(timer); reject(error); }
  });
  if (!(blob instanceof Blob) || blob.type !== 'image/png' || blob.size < 33 || blob.size > DRAWING_LIMITS.bytes) {
    throw new Error('The drawing exceeds the supported PNG upload bounds.');
  }
  const bytes = new Uint8Array(await blob.slice(0, 33).arrayBuffer()), view = new DataView(bytes.buffer);
  if ([137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82].some((value, index) => bytes[index] !== value)
    || view.getUint32(16) !== dimensions.width || view.getUint32(20) !== dimensions.height) throw new Error('Drawing export produced an invalid PNG.');
  return new File([blob], 'tegaki.png', { type: 'image/png' });
}

// Owns exactly one upload capability. Every asynchronous completion is tied to
// both its generation and its target; stale responses can never reattach it.
export function createDrawingUpload({ board, target, changed = () => {},
  upload = uploadQuickReplyFile, check = checkQuickReplyUpload, cancel = cancelQuickReplyUpload,
  schedule = setTimeout, unschedule = clearTimeout }) {
  let receipt = null, phase = 'empty', error = '', generation = 0, controller, timer, polls = 0, disposed = false, posting = false;
  let operation = null, intent = 0;
  const initialTarget = target();
  if (!uploadTarget(initialTarget)) throw new Error('Invalid upload target.');
  const snapshot = () => ({ receipt, phase, error, busy: !!controller, approved: receipt?.state === 'approved' && !controller,
    pending: phase !== 'empty', canCheck: !!receipt && !controller && receipt.state !== 'approved' });
  const notify = () => changed(snapshot());
  function stop() { unschedule(timer); timer = null; controller?.abort(); controller = null; generation++; }
  function reset() { stop(); receipt = null; phase = 'empty'; error = ''; polls = 0; notify(); }
  const valid = (epoch, id) => !disposed && epoch === generation && target() === id;
  const cleanup = (value, id) => { if (value && !posting) void cancel({ board, thread: id, receipt: value, keepalive: true }).catch(() => {}); };
  function poll() {
    unschedule(timer);
    if (!disposed && ['queued', 'processing'].includes(receipt?.state) && polls < 3) {
      timer = schedule(() => { timer = null; void status(); }, [1000, 2000, 4000][polls++]);
    }
  }
  async function status() {
    if (disposed || posting || controller || !receipt) return false;
    const id = target(), epoch = ++generation, expected = receipt;
    controller = new AbortController(); phase = 'checking'; error = ''; notify();
    try {
      const next = await check({ board, thread: id, receipt: expected, signal: controller.signal });
      if (!valid(epoch, id)) return false;
      receipt = next; phase = next.state; controller = null; notify(); poll(); return true;
    } catch (failure) {
      if (!valid(epoch, id)) return false;
      controller = null; phase = receipt?.state ?? 'empty'; error = failure.message; notify(); return false;
    }
  }
  async function clear(external = true) {
    if (external) intent++;
    if (disposed || posting) return false;
    if (operation) return operation;
    stop();
    if (!receipt) { reset(); return true; }
    const expected = receipt, id = expected.resto, epoch = ++generation;
    controller = new AbortController(); phase = 'canceling'; error = ''; notify();
    operation = (async () => {
      try {
        await cancel({ board, thread: id, receipt: expected, signal: controller.signal });
        if (!valid(epoch, id)) return false;
        reset(); return true;
      } catch (failure) {
        if (!valid(epoch, id)) return false;
        controller = null; phase = receipt.state; error = failure.message; notify(); return false;
      } finally { operation = null; }
    })();
    return operation;
  }
  async function select(file) {
    if (disposed || posting) return false;
    const id = target(), request = ++intent;
    if (!uploadTarget(id) || !await clear(false) || disposed || target() !== id || request !== intent) return false;
    stop();
    const epoch = ++generation; controller = new AbortController(); phase = 'uploading'; error = ''; notify();
    try {
      const next = await upload({ board, thread: id, file, signal: controller.signal });
      if (!valid(epoch, id)) { cleanup(next, id); return false; }
      receipt = next; controller = null; phase = next.state; notify(); poll(); return true;
    } catch (failure) {
      if (!valid(epoch, id)) return false;
      controller = null; phase = 'failed'; error = failure.message; notify(); return false;
    }
  }
  function retire() { intent++; posting = false; reset(); }
  function dispose() { if (disposed) return; intent++; const previous = receipt; stop(); disposed = true; receipt = null; cleanup(previous, previous?.resto); }
  return { select, clear, status, snapshot, dispose, retire,
    posting(value) { posting = value; },
    resetTarget() { intent++; const previous = receipt; stop(); receipt = null; phase = 'empty'; error = ''; polls = 0; cleanup(previous, previous?.resto); notify(); },
  };
}
