import test from 'node:test';
import assert from 'node:assert/strict';
import { readPostPreferences, restorePostPreferences } from '../../apps/public/client/native-post-preferences.js';

test('fixed cookie names decode bounded display values', () => {
  assert.deepEqual(readPostPreferences('unrelated=x; 4chan_name=%E5%90%8D%20%2B%20%3Cuser%3E; options=sage%20nonoko'),
    { name: '名 + <user>', options: 'sage nonoko' });
  assert.deepEqual(readPostPreferences('4chan_name=Plus+literal; options=NONOKO'),
    { name: 'Plus+literal', options: 'NONOKO' });
});

test('old trip suffixes never restore private passwords', () => {
  for (const value of ['User#private', 'User##private', ' User #private#more']) {
    assert.equal(readPostPreferences(`4chan_name=${encodeURIComponent(value)}`).name, 'User');
  }
  assert.equal(readPostPreferences('4chan_name=%23private').name, '');
});

test('duplicate keys have a deterministic empty fallback per field', () => {
  assert.deepEqual(readPostPreferences('4chan_name=First; 4chan_name=Second; options=sage'),
    { name: '', options: 'sage' });
  assert.deepEqual(readPostPreferences('4chan_name=First; options=sage; options=nonoko'),
    { name: 'First', options: '' });
});

test('malformed encoding, controls and excessive UTF-8 bytes are rejected', () => {
  for (const value of ['%', '%GG', '%C0%AF', '%ED%A0%80', '%00', '%0A', '%7F', '%C2%85', 'x'.repeat(101), encodeURIComponent('名'.repeat(34))]) {
    assert.deepEqual(readPostPreferences(`4chan_name=${value}; options=${value}`), { name: '', options: '' });
  }
  assert.equal(readPostPreferences(`4chan_name=${'x'.repeat(100)}`).name.length, 100);
  assert.equal(readPostPreferences(`options=${encodeURIComponent('名'.repeat(33))}`).options, '名'.repeat(33));
});

test('cookie work is finite and unknown keys are ignored', () => {
  for (const raw of [null, undefined, 1, {}, 'x'.repeat(4097)]) {
    assert.deepEqual(readPostPreferences(raw), { name: '', options: '' });
  }
  assert.deepEqual(readPostPreferences('other4chan_name=Wrong; Options=sage; pwd=private'), { name: '', options: '' });
});

test('ordinary form restoration preserves drafts and hidden identities', () => {
  const prior = globalThis.document;
  try {
    globalThis.document = { cookie: '4chan_name=Remembered; options=sage' };
    const inputs = { name: { type: 'text', value: '' }, email: { type: 'text', value: '' }, pwd: { type: 'password', value: '' } };
    const source = { elements: { namedItem: key => inputs[key] } };
    restorePostPreferences(source);
    assert.equal(inputs.name.value, 'Remembered'); assert.equal(inputs.email.value, 'sage');
    assert.equal(inputs.pwd.value, '');
    inputs.name.value = 'Owned draft'; inputs.email.value = 'nonoko'; restorePostPreferences(source);
    assert.equal(inputs.name.value, 'Owned draft'); assert.equal(inputs.email.value, 'nonoko');
    inputs.name = { type: 'hidden', value: '' }; restorePostPreferences(source);
    assert.equal(inputs.name.value, '');
  } finally { globalThis.document = prior; }
});

test('unavailable cookie storage does not break a posting form', () => {
  const prior = globalThis.document;
  try {
    globalThis.document = { get cookie() { throw new DOMException('Owned storage denial', 'SecurityError'); } };
    assert.doesNotThrow(() => restorePostPreferences({ elements: { namedItem() { throw new Error('Not reached'); } } }));
    assert.doesNotThrow(() => restorePostPreferences(null));
  } finally { globalThis.document = prior; }
});
