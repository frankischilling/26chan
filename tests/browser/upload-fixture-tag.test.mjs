import assert from 'node:assert/strict';
import test from 'node:test';
import { uploadFixtureTag } from './helpers/upload-fixture-tag.js';

test('native modes sharing one source keep their follow-up comments distinct', () => {
  const flags = [[], ['--attachment-only'], ['--javascript'], ['--attachment-only', '--javascript']];
  const comments = flags.map(mode => `Owned reply to an approved upload ${uploadFixtureTag('public-upload-u12345678.png', mode)}`);
  assert.equal(new Set(comments).size, 4);
});

test('inline Quick Reply does not reuse the previous mode thread or follow-up comment', () => {
  const normal = uploadFixtureTag('public-upload-u12345678.png', []);
  const inline = uploadFixtureTag('public-upload-u12345678.png', ['--inline']);
  assert.notEqual(normal, inline);
  assert.notEqual(inline, uploadFixtureTag('public-upload-u12345678.png', ['--inline', '--no-spoilers']));
});

test('tags are stable across flag order and private directory changes, but distinguish sources', () => {
  const flags = ['--attachment-only', '--javascript'];
  const tag = uploadFixtureTag('/private/one/source.png', flags);
  assert.match(tag, /^[0-9a-f]{12}$/);
  assert.equal(tag, uploadFixtureTag('/private/two/source.png', [...flags].reverse()));
  assert.deepEqual(flags, ['--attachment-only', '--javascript']);
  assert.notEqual(tag, uploadFixtureTag('/private/one/other.png', flags));
});
