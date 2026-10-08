// Genuine Worker integration probes. Deliberately does NOT import or instantiate
// ProbeController: forbidden messages reach worker.mjs's real onmessage handler.
export const RAW_WORKER_SCENARIOS = Object.freeze([
  'step-without-ack', 'wrong-ack-sequence', 'stale-ack-generation',
  'ack-before-frame', 'duplicate-ack', 'stale-command-generation',
  'skipped-command-sequence', 'duplicate-command-sequence',
  'step-before-initial', 'repeated-initial', 'extra-message-field',
  'valid-exact-ack-releases-one-command',
]);
export async function probeRawWorkerProtocol(scenario) {
  if (!RAW_WORKER_SCENARIOS.includes(scenario)) throw new Error('Unknown direct worker scenario');
  const generation = 17, received = [], queue = [];
  let worker = null, waiter = null, fatal = null, bitmapsClosed = 0;
  function fail(error) { fatal = error; waiter?.reject(error); waiter = null; }
  // Independent bounded test lifetime, installed before constructing the Worker.
  const lifetime = setTimeout(() => { worker?.terminate(); fail(new Error('direct worker lifetime exceeded')); }, 15000);
  function next() {
    if (fatal) return Promise.reject(fatal);
    if (queue.length) return Promise.resolve(queue.shift());
    if (waiter) throw new Error('duplicate direct worker receive');
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { waiter = null; reject(new Error('direct worker response deadline exceeded')); }, 5000);
      waiter = { resolve(value) { clearTimeout(timer); resolve(value); }, reject(error) { clearTimeout(timer); reject(error); } };
    });
  }
  async function expect(type, sequence, message = undefined) {
    const value = await next();
    if (value.type !== type || value.generation !== generation || (sequence !== undefined && value.sequence !== sequence)
        || (message !== undefined && value.message !== message)) {
      throw new Error(`Expected ${type}/${sequence}/${message}; received ${JSON.stringify(value)}`);
    }
    return value;
  }
  const send = (type, sequence, extra = {}) => worker.postMessage({ type, generation, sequence, ...extra });
  async function initial() {
    send('initial', 1); const value = await expect('frame', 1);
    if (value.eventIndex !== 0 || !value.nativeBitmap || value.closedWidth !== 0) throw new Error('Invalid direct initial frame');
  }
  async function ack() { send('ack', 1); await expect('acked', 1); }
  async function quiet(ms) {
    const count = received.length;
    await new Promise(resolve => setTimeout(resolve, ms));
    if (fatal) throw fatal;
    if (received.length !== count || queue.length) throw new Error('Unexpected worker output while quiescent');
  }
  try {
    worker = new Worker(new URL('./worker.mjs', import.meta.url), { type: 'module' });
    worker.onerror = event => fail(new Error(event.message || 'direct native Worker error'));
    worker.onmessageerror = () => fail(new Error('direct native Worker clone failure'));
    worker.onmessage = ({ data }) => {
      const bitmap = data?.bitmap;
      const summary = { type: data?.type, generation: data?.generation, sequence: data?.sequence,
        message: data?.message, eventIndex: data?.state?.eventIndex };
      if (bitmap) {
        try { summary.nativeBitmap = bitmap instanceof ImageBitmap; }
        finally { bitmap.close(); bitmapsClosed++; summary.closedWidth = bitmap.width; }
      }
      received.push(summary);
      if (waiter) { const current = waiter; waiter = null; current.resolve(summary); } else queue.push(summary);
    };
    worker.postMessage({ type: 'init', generation, caseId: 'pencil-pressure' });
    await expect('ready', 0);
    let expectedError;
    switch (scenario) {
      case 'step-without-ack':
        await initial(); send('step', 2); expectedError = 'frame acknowledgement required'; break;
      case 'wrong-ack-sequence':
        await initial(); send('ack', 2); expectedError = 'invalid acknowledgement'; break;
      case 'stale-ack-generation':
        await initial(); send('ack', 1, { generation: generation - 1 }); expectedError = 'invalid acknowledgement'; break;
      case 'ack-before-frame':
        send('ack', 1); expectedError = 'invalid acknowledgement'; break;
      case 'duplicate-ack':
        await initial(); await ack(); send('ack', 1); expectedError = 'invalid acknowledgement'; break;
      case 'stale-command-generation':
        send('initial', 1, { generation: generation - 1 }); expectedError = 'invalid generation'; break;
      case 'skipped-command-sequence':
        send('initial', 2); expectedError = 'invalid command sequence'; break;
      case 'duplicate-command-sequence':
        await initial(); await ack(); send('step', 1); expectedError = 'invalid command sequence'; break;
      case 'step-before-initial':
        send('step', 1); expectedError = 'missing initial presentation'; break;
      case 'repeated-initial':
        await initial(); await ack(); send('initial', 2); expectedError = 'duplicate initial presentation'; break;
      case 'extra-message-field':
        send('initial', 1, { extra: true }); expectedError = 'invalid protocol record'; break;
      case 'valid-exact-ack-releases-one-command': {
        await initial(); await quiet(80); await ack(); send('step', 2);
        const frame = await expect('frame', 2);
        if (frame.eventIndex !== 1 || !frame.nativeBitmap || frame.closedWidth !== 0) throw new Error('Ack did not release exactly the next event');
        send('ack', 2); await expect('acked', 2); await quiet(80);
        return { scenario, validRelease: true, received, bitmapsClosed, controllerUsed: false };
      }
    }
    await expect('error', undefined, expectedError);
    // The handler calls close on a protocol error. A bounded observation after
    // further valid-looking messages checks no continuation; it is not a hard
    // process-termination or memory-reclamation guarantee.
    send('ack', 1); send('step', 2); await quiet(150);
    return { scenario, expectedError, received, bitmapsClosed, controllerUsed: false, noContinuationObserved: true };
  } finally {
    clearTimeout(lifetime); worker?.terminate();
    if (waiter) { waiter.reject(new Error('direct worker probe disposed')); waiter = null; }
  }
}
