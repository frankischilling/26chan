import assert from 'node:assert/strict';
import test from 'node:test';
import { mountNativePostForm } from '../../apps/public/client/native-post-form.js';

function fixture(t, { mobile = false, hash = '', thread = null, openQuickReply = () => false } = {}) {
  function element(...initial) {
    const events = new Map(), attrs = new Map(), classes = new Set(initial);
    return {
      events, attrs, hidden: true, parentElement: { hidden: true },
      addEventListener(type, callback) { events.set(type, callback); },
      setAttribute(name, value) { attrs.set(name, value); },
      click() { events.get('click')({ preventDefault() {} }); },
      scrollIntoView() {},
      classList: {
        add: name => classes.add(name), remove: name => classes.delete(name),
        contains: name => classes.has(name),
        toggle(name, desired = !classes.has(name)) {
          if (desired) classes.add(name);
          else classes.delete(name);
          return desired;
        },
      },
    };
  }
  const desktop = element(), top = element('hidden'), bottom = element(), table = element();
  desktop.firstElementChild = element();
  const source = { ...element(), querySelector: () => table };
  const media = { ...element(), matches: mobile };
  const window = element();
  const globals = {
    window, location: { hash, search: '' }, matchMedia: () => media,
    document: { getElementById: () => desktop, querySelector: () => top,
      querySelectorAll: () => [top, bottom] },
  };
  const original = Object.fromEntries(Object.keys(globals).map(name => [name, Object.getOwnPropertyDescriptor(globalThis, name)]));
  for (const [name, value] of Object.entries(globals)) Object.defineProperty(globalThis, name, { configurable: true, value });
  t.after(() => {
    for (const [name, descriptor] of Object.entries(original)) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
  });
  mountNativePostForm({ source, thread, openQuickReply });
  return { source, desktop, top, bottom, table, media, window };
}

test('a direct mobile #reply link opens the ordinary form without a quote parameter', t => {
  const f = fixture(t, { mobile: true, hash: '#reply' });
  assert.equal(f.table.classList.contains('hideMobile'), false);
  assert.equal(f.top.textContent, 'Close Post Form');
  assert.equal(f.top.attrs.get('aria-expanded'), 'true');
  assert.equal(f.bottom.attrs.get('aria-expanded'), 'true');
  f.top.click();
  assert.equal(f.table.classList.contains('hideMobile'), true);
  assert.equal(f.top.attrs.get('aria-expanded'), 'false');
});

for (const mobile of [false, true]) {
  test(`the in-page first-thread link reveals the form (mobile: ${mobile})`, t => {
    const f = fixture(t, { mobile, hash: '#p42' });
    assert.equal(f.desktop.hidden, false);
    assert.equal(f.table.classList.contains('hideMobile'), true);
    globalThis.location.hash = '#reply';
    f.window.events.get('hashchange')?.();
    assert.equal(f.desktop.hidden, true);
    assert.equal(f.source.classList.contains('postFormOpen'), true);
    assert.equal(f.table.classList.contains('hideMobile'), false);
  });
}

test('the normal mobile reply control still prefers enabled Quick Reply', t => {
  let opened = 0;
  const f = fixture(t, { mobile: true, thread: '42', openQuickReply: () => { opened++; return true; } });
  f.top.click();
  assert.equal(opened, 1);
  assert.equal(f.table.classList.contains('hideMobile'), true);
  assert.equal(f.top.attrs.get('aria-expanded'), 'false');
});
