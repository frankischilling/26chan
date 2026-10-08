import { CASES } from './cases.mjs';
import { ProbeController } from './controller.mjs';
import { mismatch, equal, compareObservation, comparisonNegative } from './comparison.mjs';
import { checkSemanticCheckpoint } from './semantic-checkpoints.mjs';
const canvas = document.getElementById('presentation'), container = document.getElementById('realms');
let controller = null, frame = null;
function dispose() { controller?.close(); controller = null; frame?.remove(); frame = null; }
async function createReference(id) {
  frame = document.createElement('iframe');
  frame.title = 'Original DOM Tegaki reference';
  const ready = new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error('reference startup deadline')), 10000);
    frame.onload = async () => {
      try {
        const probe = await frame.contentWindow.referenceProbeReady;
        if (!probe) throw new Error('reference module failed');
        clearTimeout(timeout); resolve(probe);
      } catch (error) { clearTimeout(timeout); reject(error); }
    };
    frame.onerror = () => { clearTimeout(timeout); reject(new Error('reference load failed')); };
  });
  frame.src = `./reference.html?case=${encodeURIComponent(id)}`; container.appendChild(frame);
  return ready;
}
async function differential(id, negativeChannel = null) {
  if (!CASES.some(item => item.id === id)) throw new Error('unknown case');
  dispose();
  controller = new ProbeController(canvas);
  try {
    const reference = await createReference(id), ready = await controller.start(id);
    equal(ready.eventCount, reference.eventCount, 'event count');
    let readbackDifferences = 0, comparisons = 0, semanticAssertions = 0, negativeResult = null, finalState, finalAudit;
    for (let index = 0; index <= ready.eventCount; index++) {
      if (index > 0) reference.dispatchNext();
      const expected = reference.inspect(), flat = reference.flatten();
      const actual = await (index === 0 ? controller.initial() : controller.step());
      compareObservation(actual, expected, flat.bytes, index);
      semanticAssertions += checkSemanticCheckpoint(id, expected, flat.bytes, 'reference');
      semanticAssertions += checkSemanticCheckpoint(id, actual.state, actual.flattened, 'worker');
      if (negativeChannel && index === ready.eventCount) {
        negativeResult = comparisonNegative(actual, expected, flat.bytes, index, negativeChannel);
      }
      for (const layer of expected.layers) if (mismatch(layer.authoritative, layer.nativeReadback)) readbackDifferences++;
      equal(actual.presentationCount, index + 1, 'presentation count');
      finalState = expected; finalAudit = actual.audit; comparisons++;
      await controller.acknowledge();
    }
    return { caseId: id, comparisons, semanticAssertions, negativeResult, eventCount: ready.eventCount, readbackDifferences,
      finalLayerOrder: finalState.layers.map(layer => layer.id), finalActive: finalState.active,
      paintedPixels: finalState.layers.reduce((sum, layer) => sum + layer.authoritative.filter((value, i) => i % 4 === 3 && value > 0).length, 0),
      capabilities: ready.capabilities, audit: finalAudit, controller: { ...controller.stats },
      claims: { nativeDifferentialExecuted: true, cssCompositorEquivalence: false,
        playbackTimingEquivalence: false, repeatedPresentationOutsideOneTimeCostModel: true } };
  } finally { dispose(); }
}
window.replayProbe = Object.freeze({
  cases: CASES.map(item => item.id), differential, dispose,
  comparisonNegative: channel => differential('pencil-pressure', channel),
  async coldReset(first = 'tone-initial-and-settings', second = 'tiny-pen') {
    const firstResult = await differential(first), secondResult = await differential(second), repeated = await differential(first);
    equal(repeated, firstResult, 'cold reset');
    return { first: firstResult.caseId, second: secondResult.caseId, repeated: repeated.caseId, freshReferenceAndWorkerEachTime: true };
  },
  async backpressure() {
    dispose(); controller = new ProbeController(canvas);
    try {
      await controller.start('pencil-pressure'); const initial = await controller.initial();
      let blocked = false; try { await controller.step(); } catch (error) { blocked = /acknowledgement/.test(error.message); }
      await new Promise(resolve => setTimeout(resolve, 80));
      const presentedWhileHeld = controller.stats.presented;
      await controller.acknowledge(); const next = await controller.step(); await controller.acknowledge();
      return { blocked, presentedWhileHeld, initialIndex: initial.state.eventIndex, nextIndex: next.state.eventIndex, stats: { ...controller.stats } };
    } finally { dispose(); }
  },
  async withheldAck() {
    dispose(); controller = new ProbeController(canvas, { deadlineMs: 5000 });
    try {
      await controller.start('pencil-pressure');
      controller.deadlineMs = 80; await controller.initial();
      await new Promise(resolve => setTimeout(resolve, 200));
      return { stopped: controller.entry === null, stats: { ...controller.stats } };
    } finally { dispose(); }
  },
  async termination(mode) {
    dispose(); controller = new ProbeController(canvas, { deadlineMs: mode === 'startup' ? 750 : 5000, fault: mode });
    let started = performance.now();
    let error = '';
    try {
      await controller.start('pencil-pressure');
      if (mode === 'command') { controller.deadlineMs = 750; started = performance.now(); await controller.initial(); }
    }
    catch (caught) { error = caught.message; }
    const result = { error, elapsedMs: performance.now() - started, stopped: controller.entry === null, stats: { ...controller.stats } };
    dispose(); return result;
  },
  async staleBitmap() {
    dispose(); controller = new ProbeController(canvas);
    try {
      await controller.start('pencil-pressure'); const retired = controller.entry;
      await controller.start('tiny-pen');
      const scratch = new OffscreenCanvas(1, 1);
      scratch.getContext('2d').fillRect(0, 0, 1, 1);
      const bitmap = scratch.transferToImageBitmap();
      // Explicit late-callback injection, using a genuine native ImageBitmap.
      // This does not claim that the browser delivered a task after terminate.
      controller.receive(retired, { type: 'frame', generation: retired.generation, sequence: 1, bitmap });
      return { bitmapWidthAfterClose: bitmap.width, currentReady: controller.entry.ready, stats: { ...controller.stats } };
    } finally { dispose(); }
  },
  async replace() {
    dispose(); controller = new ProbeController(canvas);
    try {
      await controller.start('tone-initial-and-settings'); await controller.initial();
      const before = controller.entry.generation;
      await controller.start('tiny-pen'); const current = controller.entry.generation;
      const initial = await controller.initial(); await controller.acknowledge();
      return { before, current, dimensions: initial.state.dimensions, stats: { ...controller.stats } };
    } finally { dispose(); }
  },
});
const select = document.getElementById('case');
for (const item of CASES) { const option = document.createElement('option'); option.value = item.id; option.textContent = item.id; select.appendChild(option); }
document.getElementById('run').onclick = async event => {
  event.target.disabled = true;
  try { document.getElementById('status').textContent = JSON.stringify(await differential(select.value), null, 2); }
  catch (error) { document.getElementById('status').textContent = `FAILED: ${error.stack}`; }
  finally { event.target.disabled = false; }
};
