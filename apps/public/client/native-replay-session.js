// Own one disposable worker and at most one pending command. A replacement gets
// a fresh source history and allocation budget. The caller owns returned frames.
export function createReplaySession(bytes, {
  workerFactory = () => new Worker('/static/native-replay/worker.v1.js', { type: 'module' }),
  timeoutMs = 10000, schedule = setTimeout, unschedule = clearTimeout,
} = {}) {
  if (!(bytes instanceof ArrayBuffer) || bytes.byteLength < 288 || bytes.byteLength > 262400) throw new Error('Invalid replay bytes.');
  let worker = null, pending = null, sequence = 0, closed = false, dimensions = null, previous = 0, count = null;
  function close() {
    if (closed) return;
    closed = true;
    worker?.terminate(); worker = null;
    if (pending) { unschedule(pending.timer); pending.reject(new Error('Replay closed.')); pending = null; }
  }
  function fail() {
    if (pending) { unschedule(pending.timer); pending.reject(new Error('Replay unavailable or exceeded its limits.')); pending = null; }
    close();
  }
  function receive({ data }) {
    const bitmap = data?.bitmap;
    if (closed || !pending) { bitmap?.close?.(); return; }
    if (!data || data.type !== 'frame' || data.sequence !== sequence
      || !Number.isInteger(data.count) || data.count < 2 || data.count > 16384
      || !Number.isInteger(data.index) || data.index < previous || data.index > data.count
      || (data.next !== null && (!Number.isInteger(data.next) || data.next < 0 || data.next > 0xffffffff))
      || (data.next === null) !== (data.index === data.count)
      || !bitmap || typeof bitmap.close !== 'function'
      || !Number.isInteger(bitmap.width) || !Number.isInteger(bitmap.height)
      || bitmap.width < 1 || bitmap.height < 1 || bitmap.width > 1024 || bitmap.height > 1024
      || (dimensions && (bitmap.width !== dimensions[0] || bitmap.height !== dimensions[1]))
      || (count !== null && count !== data.count)) {
      bitmap?.close?.(); fail(); return;
    }
    dimensions = [bitmap.width, bitmap.height]; previous = data.index; count = data.count;
    const active = pending; pending = null; unschedule(active.timer);
    active.resolve({ bitmap, index: data.index, count: data.count, next: data.next });
  }
  function command(type, values, transfer = []) {
    if (closed || pending) return Promise.reject(new Error('Replay is closed or busy.'));
    return new Promise((resolve, reject) => {
      pending = { resolve, reject, timer: schedule(fail, timeoutMs) };
      try { worker.postMessage({ type, sequence: ++sequence, ...values }, transfer); }
      catch { fail(); }
    });
  }
  try {
    worker = workerFactory(); worker.onmessage = receive; worker.onerror = fail; worker.onmessageerror = fail;
  } catch { closed = true; throw new Error('Replay workers are unavailable.'); }
  const input = bytes.slice(0);
  const ready = command('init', { bytes: input }, [input]);
  return { ready, advance(until) {
    if (!Number.isSafeInteger(until) || until < 0 || until > 0xffffffff) return Promise.reject(new Error('Invalid replay time.'));
    return command('advance', { until });
  }, close };
}
