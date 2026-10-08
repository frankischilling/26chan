import test from 'node:test';
import assert from 'node:assert/strict';
import { runInNewContext } from 'node:vm';
import { setImmediate as nextTurn } from 'node:timers/promises';
import { closeReportPopup } from './helpers/report-popup-close.js';

// These are helper control-flow tests with mocked Playwright pages and events.
// A mocked isTrusted value does not qualify real browser dispatch or closure;
// report-popup.spec.js and thread-watcher.spec.js retain that responsibility.
const targetClosed = () => new Error('locator.click: Target page, context or browser has been closed\nCall log:\n  - performing click action');

function fixture({ event = 'trusted', clickError, closes = true, closeEvent = true,
  openerCloses = false, browserDisconnects = false, earlyCloseError = false,
  synchronousClickError = false, observerError, openerEvaluateErrors = new Map() } = {}) {
  const trace = [], openerWindow = {}, listeners = [];
  const button = { id: 'report-popup-close' };
  const closeError = new Error('Timed out waiting for the popup close event');
  let popupClosed = false, openerClosed = false, connected = true;
  let resolveClose, rejectClose, clicks = 0, evaluations = 0, observedBeforeSource = false;
  const document = {
    addEventListener(type, listener, options) {
      assert.equal(type, 'click');
      assert.equal(options.capture, true, 'observation must precede the source button handler');
      assert.equal(options.once, true);
      listeners.push(listener);
      trace.push('observe');
    },
  };
  const browser = { isConnected: () => connected };
  const opener = {
    isClosed: () => openerClosed,
    context: () => ({ browser: () => browser }),
    evaluate: async (fn, arg) => {
      evaluations++;
      if (openerEvaluateErrors.has(evaluations)) throw openerEvaluateErrors.get(evaluations);
      return runInNewContext(`(${fn})(arg)`, { window: openerWindow, arg });
    },
  };
  const control = {
    evaluate: async (fn, arg) => {
      if (observerError) throw observerError;
      return runInNewContext(`(${fn})(button, arg)`, {
        window: { opener: openerWindow }, document, button, arg,
      });
    },
    click() {
      clicks++;
      trace.push('click');
      assert.equal(typeof resolveClose, 'function', 'close event must be armed before click');
      assert.equal(listeners.length, 1, 'capture observer must be installed before click');
      if (synchronousClickError) {
        rejectClose(closeError);
        throw clickError;
      }
      return (async () => {
        if (earlyCloseError) await nextTurn();
        if (event !== 'missing') {
          const dispatched = {
            isTrusted: event !== 'untrusted',
            // An identically named substitute must not match the real control.
            target: event === 'wrong-control' ? { id: button.id } : button,
          };
          for (const listener of listeners) listener(dispatched);
        }
        observedBeforeSource = Object.values(openerWindow).some(value => value === true);
        trace.push('source-handler');
        popupClosed = closes;
        openerClosed = openerCloses;
        connected = !browserDisconnects;
        if (closeEvent) {
          trace.push('close');
          resolveClose();
        } else rejectClose(closeError);
        await nextTurn();
        trace.push('click-settled');
        if (clickError) throw clickError;
      })();
    },
    dispatchEvent() { assert.fail('The helper must not synthesize clicks'); },
  };
  const popup = {
    opener: async () => opener,
    isClosed: () => popupClosed,
    locator(selector) {
      assert.equal(selector, '#report-popup-close');
      return control;
    },
    waitForEvent(type, options) {
      assert.equal(type, 'close');
      assert.equal(options.timeout, 5000, 'a missing closure must have a bounded wait');
      trace.push('wait-close');
      return new Promise((resolve, reject) => {
        resolveClose = resolve;
        rejectClose = reject;
        if (earlyCloseError) reject(closeError);
      });
    },
    close() { assert.fail('The source handler, not the helper, must close the popup'); },
  };
  return { popup, opener, trace, openerWindow, closeError,
    get clicks() { return clicks; },
    get evaluations() { return evaluations; },
    get observedBeforeSource() { return observedBeforeSource; },
    run: () => closeReportPopup(popup, opener) };
}

test('accepts only the trusted Close click and popup-only closure when input acknowledgement races', async () => {
  const f = fixture({ clickError: targetClosed() });
  await f.run();
  assert.equal(f.clicks, 1);
  assert.equal(f.observedBeforeSource, true);
  assert.deepEqual(f.trace, ['observe', 'wait-close', 'click', 'source-handler', 'close', 'click-settled']);
  assert.deepEqual(f.openerWindow, {}, 'consumed evidence is removed from the opener');
});

test('accepts a genuine Close click and closure without an acknowledgement error', async () => {
  const f = fixture();
  await f.run();
  assert.equal(f.clicks, 1);
  assert.equal(f.observedBeforeSource, true);
  assert.equal(f.popup.isClosed(), true);
  assert.equal(f.opener.isClosed(), false);
});

test('accepts the exact target-closed diagnostic without an appended call log', async () => {
  await fixture({ clickError: new Error('locator.click: Target page, context or browser has been closed') }).run();
});

for (const event of ['missing', 'untrusted', 'wrong-control']) {
  for (const raced of [false, true]) {
    test(`rejects ${event} click evidence even when closure ${raced ? 'races' : 'succeeds'}`, async () => {
      const f = fixture({ event, clickError: raced ? targetClosed() : undefined });
      await assert.rejects(f.run(), /The exact report Close control must receive a trusted click/);
      assert.equal(f.clicks, 1);
      assert.equal(f.observedBeforeSource, false);
      assert.deepEqual(f.openerWindow, {}, 'invalid evidence must also be cleaned up');
    });
  }
}

test('never swallows generic click failures or near-matching target-closed diagnostics', async () => {
  for (const message of ['locator.click: permission denied',
    'locator.click: Unexpected failure: Target page, context or browser has been closed',
    'locator.click: Target page, context or browser has been closed unexpectedly',
    'locator.click: Target page, context or browser has been closed\nOriginal failure: permission denied',
    'Target page, context or browser has been closed']) {
    const error = new Error(message), f = fixture({ clickError: error });
    await assert.rejects(f.run(), actual => actual === error);
    assert.equal(f.observedBeforeSource, true);
    assert.equal(f.clicks, 1);
    assert.deepEqual(f.openerWindow, {}, 'generic failures must not leave a marker');
  }
});

for (const [option, diagnostic] of [
  ['openerCloses', /The report opener must remain open/],
  ['browserDisconnects', /The report browser must remain connected/],
]) {
  test(`does not accept the target-closed race when ${option}`, async () => {
    const error = targetClosed(), f = fixture({ [option]: true, clickError: error });
    await assert.rejects(f.run(), actual => actual === error);
    assert.equal(f.evaluations, 1, 'do not try to clean an unavailable opener');
  });
  test(`requires popup-only closure even without a click error when ${option}`, async () => {
    await assert.rejects(fixture({ [option]: true }).run(), diagnostic);
  });
}

test('requires closure after a successful click', async () => {
  const f = fixture({ closes: false, closeEvent: false });
  await assert.rejects(f.run(), error => error === f.closeError);
  assert.equal(f.observedBeforeSource, true);
  assert.deepEqual(f.openerWindow, {}, 'a rejected close wait must not leave a marker');
});

test('does not tolerate target-closed input failure while the popup remains open', async () => {
  const error = targetClosed(), f = fixture({ clickError: error, closes: false, closeEvent: false });
  await assert.rejects(f.run(), actual => actual === error);
});

test('requires the prearmed close event even when isClosed returns true', async () => {
  const f = fixture({ closeEvent: false, clickError: targetClosed() });
  await assert.rejects(f.run(), error => error === f.closeError);
});

test('requires isClosed as well as the close event', async () => {
  await assert.rejects(fixture({ closes: false }).run(), /The report Close click must close the popup/);
});

test('owns an early close-wait rejection while a later successful click is still pending', async () => {
  const f = fixture({ earlyCloseError: true });
  await assert.rejects(f.run(), error => error === f.closeError);
  assert.equal(f.clicks, 1);
});

test('preserves a generic click error while also owning the rejected close wait', async () => {
  const error = new Error('locator.click: genuine input failure');
  const f = fixture({ clickError: error, earlyCloseError: true, closes: false, closeEvent: false });
  await assert.rejects(f.run(), actual => actual === error);
});

test('owns both rejections even when click throws synchronously', async () => {
  const error = new Error('locator.click: synchronous input failure');
  const f = fixture({ clickError: error, synchronousClickError: true });
  await assert.rejects(f.run(), actual => actual === error);
  assert.equal(f.clicks, 1);
});

test('rejects an ordinary tab and a popup associated with a different opener before clicking', async () => {
  const f = fixture();
  await assert.rejects(closeReportPopup(f.opener, f.opener), /must be separate/);
  f.popup.opener = async () => null;
  await assert.rejects(f.run(), /must belong to this opener/);
  assert.equal(f.clicks, 0);
});

test('cleans up initialized evidence if installing the observer fails', async () => {
  const error = new Error('Observer setup failed'), f = fixture({ observerError: error });
  await assert.rejects(f.run(), actual => actual === error);
  assert.equal(f.clicks, 0);
  assert.deepEqual(f.openerWindow, {});
});

test('cleanup failure cannot replace the original click failure', async () => {
  const error = new Error('locator.click: original failure');
  const cleanupError = new Error('Opener became unavailable during cleanup');
  const f = fixture({ clickError: error, openerEvaluateErrors: new Map([[2, cleanupError]]) });
  await assert.rejects(f.run(), actual => actual === error);
  assert.equal(f.evaluations, 2);
});

test('cleanup failure cannot replace the original close-wait failure', async () => {
  const f = fixture({ closes: false, closeEvent: false,
    openerEvaluateErrors: new Map([[2, new Error('Cleanup failed')]]) });
  await assert.rejects(f.run(), error => error === f.closeError);
});
