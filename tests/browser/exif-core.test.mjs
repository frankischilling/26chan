import { readFile } from 'node:fs/promises';
import test from 'node:test';
import assert from 'node:assert/strict';
import { exifTarget, mountExif } from '../../assets/exif.js';

test('EXIF identifiers are finite source media numbers, never selectors or URLs', () => {
  for (const id of ['exif1', 'exif1730000000000', 'exif9007199254740991', '3xif42']) assert.equal(exifTarget(id), id);
  for (const id of [null, {}, 1, '', 'exif0', 'exif01', 'exif-1', 'exif9007199254740992', 'exif1"],body', 'javascript:alert(1)', 'exif1\n', 'eXif1']) assert.equal(exifTarget(id), null);
});

test('delegated EXIF toggle stays inside its comment, supports replacement nodes and tears down', () => {
  const listeners = new Set();
  const attributes = new Map();
  const table = { id: 'exif42', getAttribute: name => attributes.get(name) ?? null, setAttribute: (name, value) => attributes.set(name, value) };
  const scope = { querySelector: selector => selector === 'table[data-exif-table="exif42"]' ? table : null };
  const link = { getAttribute: () => 'exif42', closest: () => scope, setAttribute: (name, value) => attributes.set(name, value) };
  const doc = { contains: node => node === link, addEventListener: (_, fn) => listeners.add(fn), removeEventListener: (_, fn) => listeners.delete(fn) };
  let prevented = 0;
  const event = { button: 0, target: { closest: () => link }, preventDefault: () => prevented++ };
  const first = mountExif(doc);
  const second = mountExif(doc);
  assert.equal(listeners.size, 1);
  const [click] = listeners;
  click(event);
  assert.equal(attributes.get('data-exif-expanded'), 'true');
  assert.equal(attributes.get('aria-expanded'), 'true');
  click(event);
  assert.equal(attributes.get('data-exif-expanded'), 'false');
  click({ ...event, ctrlKey: true });
  assert.equal(prevented, 2);
  table.id = 'exif99'; click(event);
  assert.equal(prevented, 2);
  first.destroy(); assert.equal(listeners.size, 1);
  second.destroy(); assert.equal(listeners.size, 0);
});

test('EXIF table geometry and theme colors retain the pinned source declarations', async () => {
  const reference = JSON.parse(await readFile(new URL('../../fixtures/exif-comment-source.json', import.meta.url), 'utf8'));
  const css = await readFile(new URL('../../assets/comment-markup.css', import.meta.url), 'utf8');
  assert.ok(css.includes('min-width: 450px;'));
  assert.ok(css.includes('min-width: 150px; font-size: 8pt;'));
  for (const [source, theme] of [['yotsubanew', null], ['yotsubluenew', 'yotsuba-b'], ['futabanew', 'futaba'], ['burichannew', 'burichan'], ['photon', 'photon'], ['tomorrow', 'tomorrow']]) {
    const expected = /color: (#[0-9a-f]+);/.exec(reference.styles[source].rules[1])[1];
    const sheet = theme ? await readFile(new URL(`../../apps/public/static/themes/${theme}.css`, import.meta.url), 'utf8') : '';
    const actual = /--exif-color: (#[0-9a-f]+);/.exec(sheet)?.[1] ?? '#707070';
    assert.equal(actual, expected, source);
  }
});
