import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { seasonalStyle, seasonalChange, snowCount, rotateSkeleton, spookyController } from '../../apps/public/client/native-seasonal-event.js';
const fixture = JSON.parse(await readFile(new URL('../../fixtures/seasonal-event-reference.json', import.meta.url), 'utf8'));
const pins = {
  initial: '6f91d3c80f6ac2512ffecfcc5cf549efc5189434627fad022318e2a7a301ad98',
  change: '3475d0355d6a6e9da3c37179c383470c16399af43d84eacd7f54ef80dbd212e1',
  rotate: 'fbf3b916b897cbb6bf6eed8c63719128393c0d26cb84032eddc2415b88553da0',
  spooky_init: '915a6e1ab2bc6c33c47821cb67ed4744032371066caa0ae0312d4d7854f03296',
  spooky_cleanup: '0d06ce82b032bf92c21b1bcceabc7145e61ec4a50afc988f5904049b60ed0149',
  snow_init: 'deb2f298c67a8f02d84b673467f06663d50b89ea26d05c0bbd8cddce877fc11a',
};
assert.deepEqual(fixture.sha256, pins);
assert.deepEqual(fixture.sources, {
  'imgboard.php': 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445',
  'js/core.js': 'a9ab67bea1f51fcdaaac1a879098a4d5888622008bf7f26f1fe18861ea52aab5',
});

for (const [name, source] of Object.entries(fixture.functions)) {
  assert.equal(createHash('sha256').update(source).digest('hex'), pins[name]);
}
function run(names, globals, invoke) {
  const context = vm.createContext(globals);
  vm.runInContext(names.map(name => fixture.functions[name]).join('\n'), context, { timeout: 1000 });
  vm.runInContext(invoke, context, { timeout: 1000 });
  return context;
}
function random(values) {
  let next = 0;
  const draw = () => { assert.ok(next < values.length, 'unexpected random draw'); return values[next++]; };
  draw.used = () => next;
  return draw;
}
function drawing() {
  const children = [], listeners = new Map();
  const body = {
    get firstElementChild() { return children[0] || null; },
    insertBefore(node) { children.unshift(node); node.parentNode = body; },
    removeChild(node) { children.splice(children.indexOf(node), 1); },
  };
  const document = {
    body,
    getElementById(id) { return children.find(node => node.id === id) || null; },
    createElement(tag) { assert.equal(tag, 'img'); return {}; },
    addEventListener(name, listener) { listeners.set(name, listener); },
    removeEventListener(name, listener) { if (listeners.get(name) === listener) listeners.delete(name); },
  };
  const view = () => children.map(({ id, className, alt, src }) => ({ id, className, alt, src: src?.replace('//s.4cdn.org/image', '/static/events') }));
  return { document, view, listeners };
}
test('event version opt-out stays separate from preferred style and ordinary Tomorrow', () => {
  for (const event of [undefined, '', 'tomorrow', 'spooky', 'spooky2017']) {
    for (const version of [0, 1, 2]) {
      for (const stopped of [null, 'tomorrow-1', 'spooky-1', `${event}-${version}`]) {
        for (const preferred of ['Yotsuba B', 'Yotsuba', 'Burichan', 'Futaba', 'Tomorrow', 'Photon']) {
          let selected;
          run(['initial'], { window: { style_group: 'ws_style', css_event: event, css_event_v: version }, style_group: 'ws_style',
            readCookie: () => preferred, getPreferredStyleSheet: () => 'Yotsuba B New',
            localStorage: { getItem: key => key === '4chan_stop_css_event' ? stopped : null },
            setActiveStyleSheet: (title, initial) => { assert.equal(initial, true); selected = title; },
          }, 'initStyleSheet()');
          assert.equal(seasonalStyle(event, version, stopped, preferred), selected);
        }
      }
    }
  }
});
test('source skeleton thresholds, old-image retention and draw order match exactly', () => {
  for (const count of [undefined, 0, 1, -1]) {
    for (const draws of [[0.329, 0, 0, 0], [0.33, 0, 0.25, 0.01], [0.9, 0.999999, 0.2499, 0.0099], [0.9, 0.5, 0.9, 0.9]]) {
      const source = drawing(), native = drawing(), a = random(draws), b = random(draws);
      for (const d of [source, native]) d.document.body.insertBefore({ id: 'skellington', src: 'old' });
      const event = { detail: { count } };
      run(['rotate'], { document: source.document, Math: { random: a, floor: Math.floor }, event }, 'fc_skelrot(event)');
      rotateSkeleton(native.document, b, event);
      assert.deepEqual(native.view(), source.view());
      assert.equal(b.used(), a.used());
    }
  }
});
test('desktop init, update listener and cleanup preserve source ownership', () => {
  for (const desktop of [false, true]) {
    const source = drawing(), native = drawing();
    const a = random(Array(24).fill(0.5)), b = random(Array(24).fill(0.5));
    const sourceWindow = { matchMedia: () => ({ matches: desktop }) };
    const nativeWindow = { document: native.document, matchMedia: sourceWindow.matchMedia };
    const context = run(['rotate', 'spooky_init', 'spooky_cleanup'], { window: sourceWindow, document: source.document, Math: { random: a, floor: Math.floor } }, 'fc_spooky_init(); fc_spooky_init()');
    const controller = spookyController(nativeWindow, b);
    controller.start(); controller.start();
    assert.deepEqual(native.view(), source.view());
    assert.equal(native.listeners.size, source.listeners.size);
    assert.equal(nativeWindow.dark_captcha, sourceWindow.dark_captcha);
    vm.runInContext('fc_spooky_cleanup()', context);
    controller.stop();
    assert.deepEqual(native.view(), source.view());
    assert.equal(native.listeners.size, 0);
    assert.equal(nativeWindow.dark_captcha, false);
  }
});
test('Christmas uses 50 through 99 desktop flakes without enabling ordinary Tomorrow', () => {
  for (const draw of [0, 0.02, 0.5, 0.98, 0.999999]) {
    for (const desktop of [false, true]) {
      let count = null;
      run(['snow_init'], { window: { matchMedia: () => ({ matches: desktop }) }, Math: { floor: Math.floor, random: () => draw },
        fc_spawn_snow: value => { count = value; } }, 'fc_tomorrow_init()');
      assert.equal(count, desktop ? snowCount(() => draw) : null);
    }
  }
});

test('normal style changes retain preference and mark only this event version stopped', () => {
  for (const event of ['', 'spooky', 'tomorrow']) {
    for (const selected of ['_special', 'Tomorrow', 'Yotsuba B New']) {
      const state = { stopped: null, action: null }, cookies = [];
      const link = (title, href) => ({
        href, getAttribute: key => ({ title, rel: 'stylesheet' })[key],
        setAttribute: (key, value) => { assert.equal(key, 'href'); },
      });
      const links = [link('switch', '/old'), link('_special', '/event'), link('Tomorrow', '/tomorrow'), link('Yotsuba B New', '/normal')];
      run(['change'], {
        window: { css_event: event, css_event_v: 2,
          ['fc_' + event + '_init']: () => { state.action = 'init'; },
          ['fc_' + event + '_cleanup']: () => { state.action = 'cleanup'; } },
        style_group: 'ws_style', location: { host: 'boards.4chan.org' },
        document: { querySelectorAll: () => links, getElementsByTagName: () => links },
        localStorage: {
          setItem: (key, value) => { assert.equal(key, '4chan_stop_css_event'); state.stopped = value; },
          removeItem: key => { assert.equal(key, '4chan_stop_css_event'); state.stopped = null; },
        },
        createCookie: (...args) => cookies.push(args), selected,
      }, 'setActiveStyleSheet(selected, false)');
      assert.deepEqual(seasonalChange(event, 2, selected), state);
      assert.equal(cookies.length, selected === '_special' ? 0 : 1);
      if (cookies.length) assert.equal(cookies[0][1], selected);
    }
  }
});
