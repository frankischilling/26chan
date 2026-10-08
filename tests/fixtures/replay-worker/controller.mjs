// Test controller: one worker, one command/frame, one acknowledgement, no RAF.
// Deadlines are parent-owned and do not reset when a worker sends progress.
export class ProbeController {
  constructor(canvas, { deadlineMs = 5000, maxStarts = 8, WorkerClass = Worker,
    timers = globalThis, fault = null } = {}) {
    if (!Number.isInteger(deadlineMs) || deadlineMs < 20 || deadlineMs > 10000) throw new Error('invalid deadline');
    if (!Number.isInteger(maxStarts) || maxStarts < 1 || maxStarts > 8) throw new Error('invalid restart budget');
    this.canvas = canvas; this.deadlineMs = deadlineMs; this.maxStarts = maxStarts;
    this.WorkerClass = WorkerClass; this.timers = timers; this.fault = fault;
    this.generation = 0; this.starts = 0; this.entry = null;
    this.stats = { terminated: 0, presented: 0, closedBitmaps: 0, staleMessages: 0, timeouts: 0, stallEntries: 0 };
  }
  async start(caseId) {
    this.close('replaced');
    if (++this.starts > this.maxStarts) throw new Error('restart budget exhausted');
    const generation = ++this.generation;
    const entry = { generation, worker: null, pending: null, awaitingAck: null, sequence: 0, ready: false, timer: null };
    this.entry = entry;
    // Install protection BEFORE worker construction/import/initialization.
    const promise = this.expect(entry, 'ready', 0);
    try {
      let url = new URL('./worker.mjs', import.meta.url);
      if (this.fault !== null) {
        if (!['startup', 'command'].includes(this.fault)) throw new Error('unknown owned fault');
        url = new URL(`./fault-worker.mjs?mode=${this.fault}`, import.meta.url);
      }
      entry.worker = new this.WorkerClass(url, { type: 'module' });
      entry.worker.onmessage = event => this.receive(entry, event.data);
      entry.worker.onerror = event => this.fail(entry, new Error(event.message || 'worker error'));
      entry.worker.onmessageerror = () => this.fail(entry, new Error('worker message clone failed'));
      entry.worker.postMessage({ type: 'init', generation, caseId });
    } catch (error) { this.fail(entry, error); }
    return promise;
  }
  expect(entry, type, sequence) {
    if (entry.pending) throw new Error('command already in flight');
    const promise = new Promise((resolve, reject) => { entry.pending = { type, sequence, resolve, reject }; });
    this.arm(entry); return promise;
  }
  arm(entry) {
    this.timers.clearTimeout(entry.timer);
    entry.timer = this.timers.setTimeout(() => {
      this.stats.timeouts++; this.fail(entry, new Error('parent deadline exceeded'));
    }, this.deadlineMs);
  }
  fail(entry, error) {
    if (entry !== this.entry) return;
    this.close(error.message, error);
  }
  close(reason = 'closed', error = new Error(reason)) {
    const entry = this.entry;
    if (!entry) return;
    this.entry = null; ++this.generation;
    this.timers.clearTimeout(entry.timer);
    if (entry.worker) { entry.worker.terminate(); this.stats.terminated++; }
    const pending = entry.pending; entry.pending = null; entry.awaitingAck = null;
    pending?.reject(error);
  }
  disposeBitmap(bitmap) {
    if (bitmap) { bitmap.close(); this.stats.closedBitmaps++; }
  }
  receive(entry, message) {
    const bitmap = message?.bitmap;
    if (entry !== this.entry || message?.generation !== entry.generation) {
      this.disposeBitmap(bitmap); this.stats.staleMessages++; return;
    }
    if (this.fault !== null && message?.type === 'stall-entered') {
      this.stats.stallEntries++; return; // diagnostic never renews the deadline
    }
    if (message?.type === 'error') { this.disposeBitmap(bitmap); this.fail(entry, new Error(message.message)); return; }
    const pending = entry.pending;
    if (!pending || message.type !== pending.type || message.sequence !== pending.sequence) {
      this.disposeBitmap(bitmap); this.fail(entry, new Error('unexpected worker response')); return;
    }
    try {
      if (message.type === 'frame') {
        if (!bitmap || entry.awaitingAck) throw new Error('invalid frame ownership');
        this.canvas.width = bitmap.width; this.canvas.height = bitmap.height;
        const ctx = this.canvas.getContext('2d');
        ctx.drawImage(bitmap, 0, 0);
        message.presented = Array.from(ctx.getImageData(0, 0, this.canvas.width, this.canvas.height).data);
        this.stats.presented++;
        entry.awaitingAck = message.sequence;
        // The original command deadline also bounds held acknowledgements.
      } else {
        this.timers.clearTimeout(entry.timer);
        if (message.type === 'ready') entry.ready = true;
        if (message.type === 'acked') entry.awaitingAck = null;
      }
      entry.pending = null;
      const { bitmap: ignored, ...result } = message;
      pending.resolve(result);
    } catch (error) { this.fail(entry, error); }
    finally { this.disposeBitmap(bitmap); }
  }
  command(type) {
    const entry = this.entry;
    if (!entry?.ready) return Promise.reject(new Error('worker not ready'));
    if (entry.pending || entry.awaitingAck !== null) return Promise.reject(new Error('frame acknowledgement required'));
    const sequence = ++entry.sequence;
    const promise = this.expect(entry, 'frame', sequence);
    try { entry.worker.postMessage({ type, generation: entry.generation, sequence }); }
    catch (error) { this.fail(entry, error); }
    return promise;
  }
  initial() { return this.command('initial'); }
  step() { return this.command('step'); }
  acknowledge() {
    const entry = this.entry;
    if (!entry || entry.pending || entry.awaitingAck === null) return Promise.reject(new Error('no frame to acknowledge'));
    const sequence = entry.awaitingAck;
    // Do not extend the old deadline: an ack does not buy more frame lifetime.
    const promise = new Promise((resolve, reject) => { entry.pending = { type: 'acked', sequence, resolve, reject }; });
    try { entry.worker.postMessage({ type: 'ack', generation: entry.generation, sequence }); }
    catch (error) { this.fail(entry, error); }
    return promise;
  }
}
