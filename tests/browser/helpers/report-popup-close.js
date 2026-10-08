import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';

// The source Close handler calls window.close() during a real click. Chromium
// can close that target before Playwright receives its input acknowledgement.
// This helper is only for report popups, never ordinary tabs or timer closure.
export async function closeReportPopup(popup, opener) {
  assert.notEqual(popup, opener, 'The report popup must be separate from its opener');
  assert.equal(await popup.opener(), opener, 'The report popup must belong to this opener');
  const browser = opener.context().browser();
  assert.equal(opener.isClosed(), false, 'The report opener must remain open');
  assert.equal(browser?.isConnected(), true, 'The report browser must remain connected');

  // Keep the evidence in the opener because the clicked document is destroyed.
  // A fresh marker prevents an earlier popup's click from satisfying this one.
  const marker = `__reportCloseObserved_${randomUUID()}`;
  try {
    await opener.evaluate(key => { window[key] = false; }, marker);
    const control = popup.locator('#report-popup-close');
    await control.evaluate((button, key) => {
      // Document capture runs before the source's button click handler, even
      // though that handler was installed before this observer.
      document.addEventListener('click', event => {
        if (event.isTrusted && event.target === button) window.opener[key] = true;
      }, { capture: true, once: true });
    }, marker);

    // Arm closure before issuing exactly one real click. Own both promises from
    // the start, including a close timeout that can reject before click settles.
    const [closed, clicked] = await Promise.allSettled([
      popup.waitForEvent('close', { timeout: 5000 }),
      Promise.resolve().then(() => control.click()),
    ]);
    if (clicked.status === 'rejected') {
      const expected = clicked.reason instanceof Error
        && /^locator\.click: Target page, context or browser has been closed(?:\nCall log:\n[\s\S]*)?$/.test(clicked.reason.message);
      if (!expected || !popup.isClosed() || opener.isClosed() || !browser?.isConnected()) {
        throw clicked.reason;
      }
    }
    if (closed.status === 'rejected') throw closed.reason;
    assert.equal(popup.isClosed(), true, 'The report Close click must close the popup');
    assert.equal(opener.isClosed(), false, 'The report opener must remain open');
    assert.equal(browser?.isConnected(), true, 'The report browser must remain connected');
    const observed = await opener.evaluate(key => {
      const value = window[key];
      delete window[key];
      return value;
    }, marker);
    assert.equal(observed, true, 'The exact report Close control must receive a trusted click');
  } catch (error) {
    if (!opener.isClosed() && browser?.isConnected()) {
      try { await opener.evaluate(key => { delete window[key]; }, marker); }
      catch { /* Preserve the original failure if the opener becomes unavailable during cleanup. */ }
    }
    throw error;
  }
}
