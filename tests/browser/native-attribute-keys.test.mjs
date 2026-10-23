import assert from 'node:assert/strict';
import test from 'node:test';
import { attributeKeys, exactAttributes } from '../../apps/public/client/native-attribute-keys.js';

test('attribute recipes compare every own key and literal value independent of order', () => {
  assert.equal(attributeKeys({ title: 'x', class: 'name' }), 'class,title');
  assert.ok(exactAttributes({ title: 'x', class: 'name' }, { class: 'name', title: 'x' }));
  assert.ok(!exactAttributes({ title: 'x', class: 'name', onclick: '' }, { class: 'name', title: 'x' }));
  assert.ok(!exactAttributes({ class: 'name' }, { class: 'name', title: 'x' }));
  assert.ok(!exactAttributes({ title: 1 }, { title: '1' }));
  assert.ok(!exactAttributes(Object.create({ title: 'x' }), { title: 'x' }));
});
