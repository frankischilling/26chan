import { installWorkerBoundary } from './worker-boundary.mjs';
import { WorkerCommandGate, exactMessage } from './protocol.mjs';
// ORDER IS SECURITY-RELEVANT: static source import here would run too early.
const boundary = installWorkerBoundary();
const gate = new WorkerCommandGate();
let probe = null, initializing = false, initialSent = false;
function nativeCapabilities() {
  const canvas = new OffscreenCanvas(2, 2), ctx = canvas.getContext('2d');
  if (!ctx || ctx.canvas !== canvas || typeof canvas.transferToImageBitmap !== 'function') throw new Error('Native worker canvas unsupported');
  const pixels = new ImageData(2, 2); pixels.data.set([255, 0, 0, 255]);
  ctx.putImageData(pixels, 0, 0, 0, 0, 1, 1);
  if (ctx.getImageData(0, 0, 1, 1).data[0] !== 255) throw new Error('Native worker pixel probe failed');
  ctx.globalAlpha = 0.5; canvas.width = 2;
  if (ctx.globalAlpha !== 1 || ctx.getImageData(0, 0, 1, 1).data.some(Boolean)) throw new Error('Native width reset semantics failed');
  const bitmap = canvas.transferToImageBitmap(); bitmap.close();
  return { nativeImageData: true, nativeOffscreen2D: true, dirtyPut: true, widthReset: true, bitmapTransfer: true };
}
self.onmessage = async ({ data: message }) => {
  let bitmap;
  try {
    if (message?.type === 'init') {
      exactMessage(message, ['type', 'generation', 'caseId']);
      if (initializing || probe) throw new Error('duplicate initialization');
      initializing = true; gate.initialize(message.generation);
      const capabilities = nativeCapabilities();
      const { createQualificationProbe } = await import('./generated/consumer-v1.mjs');
      if (gate.closed) return;
      probe = await createQualificationProbe(message.caseId, 'worker', boundary);
      self.postMessage({ type: 'ready', generation: gate.generation, sequence: 0, eventCount: probe.eventCount, capabilities });
      return;
    }
    if (!probe) throw new Error('worker not ready');
    exactMessage(message, ['type', 'generation', 'sequence']);
    if (message.type === 'ack') {
      gate.acknowledge(message);
      self.postMessage({ type: 'acked', generation: gate.generation, sequence: message.sequence }); return;
    }
    gate.begin(message);
    if (message.type === 'initial') {
      if (initialSent) throw new Error('duplicate initial presentation');
      initialSent = true;
    } else {
      if (!initialSent) throw new Error('missing initial presentation');
      probe.dispatchNext();
    }
    const state = probe.inspect(), flattened = probe.flatten();
    // Transferring this independent presentation surface clears only that
    // surface. Layer/history buffers are copied as ordinary diagnostic arrays.
    bitmap = flattened.canvas.transferToImageBitmap();
    self.postMessage({ type: 'frame', generation: gate.generation, sequence: message.sequence,
      state, flattened: flattened.bytes, presentationCount: flattened.presentationCount,
      repeatedPresentationOutsideOneTimeCostModel: true, audit: boundary.audit(), bitmap }, [bitmap]);
    bitmap = null; // ownership transferred to the receiving parent
  } catch (error) {
    bitmap?.close(); gate.close();
    self.postMessage({ type: 'error', generation: gate.generation, message: error.message });
    self.close();
  }
};
