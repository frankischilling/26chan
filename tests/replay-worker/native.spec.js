import { test, expect } from '@playwright/test';
import { CASES } from '../fixtures/replay-worker/cases.mjs';
// These are native-browser tests, not Node canvas mocks. Capability failure is
// a failing qualification test, never a skip or a hidden DOM fallback.
test.beforeEach(async ({ page }) => {
  await page.goto('/'); await page.waitForFunction(() => !!window.replayProbe);
});
for (const { id } of CASES) {
  test(`native differential: ${id}`, async ({ page }) => {
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    const result = await page.evaluate(id => window.replayProbe.differential(id), id);
    expect(result.comparisons).toBe(result.eventCount + 1);
    expect(result.semanticAssertions).toBeGreaterThan(0);
    expect(result.capabilities.nativeOffscreen2D).toBe(true);
    expect(result.controller.presented).toBe(result.comparisons);
    expect(result.controller.closedBitmaps).toBe(result.comparisons);
    expect(result.audit.sourceCanvasSurfaces).toBe(result.finalLayerOrder.length + 1);
    expect(result.claims.cssCompositorEquivalence).toBe(false);
    expect(errors).toEqual([]);
    if (id === 'pencil-pressure') expect(result.paintedPixels).toBeGreaterThan(0);
    if (id === 'eight-layers') expect(result.finalLayerOrder).toHaveLength(8);
  });
}
test('cold reset reconstructs state across tool and dimension changes', async ({ page }) => {
  expect((await page.evaluate(() => window.replayProbe.coldReset())).freshReferenceAndWorkerEachTime).toBe(true);
});
test('delayed acknowledgement applies backpressure without idle frames', async ({ page }) => {
  const result = await page.evaluate(() => window.replayProbe.backpressure());
  expect(result.blocked).toBe(true); expect(result.presentedWhileHeld).toBe(1);
  expect(result.initialIndex).toBe(0); expect(result.nextIndex).toBe(1);
  expect(result.stats.closedBitmaps).toBe(2);
});
test('missing acknowledgement retires the worker on the parent deadline', async ({ page }) => {
  const result = await page.evaluate(() => window.replayProbe.withheldAck());
  expect(result.stopped).toBe(true); expect(result.stats.timeouts).toBe(1); expect(result.stats.terminated).toBe(1);
});
for (const mode of ['startup', 'command']) test(`parent terminates a synchronously stalled ${mode}`, async ({ page }) => {
  const result = await page.evaluate(mode => window.replayProbe.termination(mode), mode);
  expect(result.error).toBe('parent deadline exceeded'); expect(result.stopped).toBe(true);
  expect(result.stats.terminated).toBe(1); expect(result.stats.timeouts).toBe(1); expect(result.stats.stallEntries).toBe(1);
  // Observational generous test timeout, not an OS CPU/RSS or hard-latency guarantee.
  expect(result.elapsedMs).toBeLessThan(1900);
});
test('replacement invalidates a held frame generation and starts cold', async ({ page }) => {
  const result = await page.evaluate(() => window.replayProbe.replace());
  expect(result.current).toBeGreaterThan(result.before); expect(result.dimensions).toEqual([1, 1]);
  expect(result.stats.terminated).toBe(1); expect(result.stats.closedBitmaps).toBe(2);
});

test('injected stale-generation native bitmap is closed without presentation', async ({ page }) => {
  const result = await page.evaluate(() => window.replayProbe.staleBitmap());
  expect(result.bitmapWidthAfterClose).toBe(0); expect(result.currentReady).toBe(true);
  expect(result.stats.staleMessages).toBe(1); expect(result.stats.presented).toBe(0); expect(result.stats.closedBitmaps).toBe(1);
});

// Direct-worker tests bypass ProbeController so removal of the real worker's
// gate.begin/acknowledge calls cannot hide behind controller-side rejection.
for (const scenario of [
  'step-without-ack', 'wrong-ack-sequence', 'stale-ack-generation',
  'ack-before-frame', 'duplicate-ack', 'stale-command-generation',
  'skipped-command-sequence', 'duplicate-command-sequence',
  'step-before-initial', 'repeated-initial', 'extra-message-field',
]) test(`direct native Worker rejects ${scenario}`, async ({ page }) => {
  const result = await page.evaluate(async scenario => {
    const { probeRawWorkerProtocol } = await import('/raw-worker-protocol.mjs');
    return probeRawWorkerProtocol(scenario);
  }, scenario);
  expect(result.controllerUsed).toBe(false); expect(result.noContinuationObserved).toBe(true);
  expect(result.received.at(-1).type).toBe('error');
  expect(result.received.filter(message => message.type === 'frame').length).toBe(result.bitmapsClosed);
  expect(result.received.filter(message => message.type === 'frame').every(message => message.eventIndex === 0)).toBe(true);
});
test('direct native Worker exact acknowledgement releases exactly one next command', async ({ page }) => {
  const result = await page.evaluate(async () => {
    const { probeRawWorkerProtocol } = await import('/raw-worker-protocol.mjs');
    return probeRawWorkerProtocol('valid-exact-ack-releases-one-command');
  });
  expect(result.controllerUsed).toBe(false); expect(result.validRelease).toBe(true); expect(result.bitmapsClosed).toBe(2);
  expect(result.received.map(message => message.type)).toEqual(['ready', 'frame', 'acked', 'frame', 'acked']);
  expect(result.received.filter(message => message.type === 'frame').map(message => message.eventIndex)).toEqual([0, 1]);
});
for (const channel of ['authoritative', 'nativeReadback', 'sourceFlatten', 'receivedBitmap']) {
  test(`a one-byte native observation mutation fails the ${channel} comparator`, async ({ page }) => {
    const result = await page.evaluate(channel => window.replayProbe.comparisonNegative(channel), channel);
    expect(result.negativeResult.channel).toBe(channel); expect(result.negativeResult.changedBytes).toBe(1);
    expect(result.negativeResult.originalUnchanged).toBe(true); expect(result.semanticAssertions).toBeGreaterThan(0);
  });
}
