// Execute only the pinned still-image geometry/default fragments in bounded VM
// contexts, then compare their synthetic geometry with the rewrite's pure helper.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { imageSize } from '../apps/public/client/native-images.js';

assert.equal(process.argv.length, 3, 'usage: node scripts/verify-public-image-controls.mjs <pinned-extension.js>');
const reference = JSON.parse(await readFile(new URL('../docs/public-image-controls-reference.json', import.meta.url), 'utf8'));
const bytes = await readFile(resolve(process.argv[2]));
assert.equal(bytes.length, reference.extension.bytes);
assert.equal(createHash('sha256').update(bytes).digest('hex'), reference.extension.sha256);
const source = bytes.toString('utf8');

function closingBrace(text, open) {
  let depth = 0, quote = null, escaped = false;
  for (let index = open; index < text.length; index += 1) {
    const character = text[index];
    if (quote) {
      if (escaped) escaped = false;
      else if (character === '\\') escaped = true;
      else if (character === quote) quote = null;
      continue;
    }
    if (character === '"' || character === "'" || character === '`') {
      quote = character;
      continue;
    }
    if (character === '{') depth += 1;
    else if (character === '}' && --depth === 0) return index;
  }
  throw new Error('unterminated-reference-fragment');
}

function functionAssignment(name) {
  const marker = name + '=function';
  const start = source.indexOf(marker);
  assert.notEqual(start, -1, name);
  const open = source.indexOf('{', start + marker.length);
  assert.notEqual(open, -1, name);
  return source.slice(start, closingBrace(source, open) + 1) + ';';
}

function objectAssignment(name) {
  const marker = name + '=';
  const start = source.indexOf(marker);
  assert.notEqual(start, -1, name);
  const open = source.indexOf('{', start + marker.length);
  assert.notEqual(open, -1, name);
  return name + '=' + source.slice(open, closingBrace(source, open) + 1) + ';';
}

const run = (code, context) => vm.runInNewContext(code, context, { timeout: 50 });
const configContext = { Config: null };
run(objectAssignment('Config'), configContext);
for (const [key, expected] of Object.entries(reference.defaults)) {
  assert.equal(configContext.Config[key], expected, 'Config.' + key);
}
for (const key of reference.implicit_false) {
  assert.equal(Object.hasOwn(configContext.Config, key), false, 'Config.' + key + ' should be absent');
}

const settingsContext = { SettingsMenu: {} };
run(objectAssignment('SettingsMenu.options'), settingsContext);
const imageSettings = settingsContext.SettingsMenu.options['Images &amp; Media'];
assert.ok(imageSettings);
for (const [key, expected] of Object.entries(reference.settings)) {
  assert.equal(imageSettings[key]?.[0], expected.label, key + ' label');
  assert.equal(imageSettings[key]?.[1], expected.tip, key + ' tip');
}

const expansionCode = functionAssignment('ImageExpansion.onLoadStart');
const hoverCode = functionAssignment('ImageHover.onLoadStart');
const close = (actual, expected, label) => assert.ok(Math.abs(actual - expected) < 1e-9, label + ': ' + actual + ' != ' + expected);
const styleNumber = value => value === undefined || value === '' ? null : Number.parseFloat(value);

function sourceExpansion(entry) {
  const [width, height] = entry.image, [clientWidth, clientHeight] = entry.viewport;
  const image = { naturalWidth: width, naturalHeight: height, style: {} };
  const file = {};
  const anchor = { parentNode: file, style: {} };
  const thumb = {
    parentNode: anchor,
    style: {},
    removeAttribute() {},
    getBoundingClientRect() { return { left: entry.left }; },
  };
  const context = {
    ImageExpansion: {},
    image,
    thumb,
    Config: { centeredThreads: false, fitToScreenExpansion: entry.fit_to_screen, threadHiding: false },
    Main: { tid: 1, hasMobileLayout: false },
    $: { docEl: { clientWidth, clientHeight }, addClass() {} },
    document: {},
  };
  run(expansionCode + 'ImageExpansion.onLoadStart(image,thumb);', context);
  return [styleNumber(image.style.maxWidth), styleNumber(image.style.maxHeight)];
}

for (const entry of reference.geometry.expansion_cases) {
  const sourceSize = sourceExpansion(entry);
  const native = imageSize(entry.image[0], entry.image[1],
    entry.viewport[0] - entry.left - 25, entry.fit_to_screen ? entry.viewport[1] : Infinity);
  assert.ok(native, entry.name);
  for (const [index, expected] of entry.expected.entries()) {
    close(sourceSize[index], expected, 'source expansion ' + entry.name);
    close([native.width, native.height][index], expected, 'native expansion ' + entry.name);
  }
}

assert.match(source, /#image-hover\s*\{[^}]*max-width:\s*100%;[^}]*max-height:\s*100%;[^}]*top:\s*0px;[^}]*right:\s*0px;[^}]*z-index:\s*9002;/);
function sourceHover(entry) {
  const [width, height] = entry.image, [innerWidth, innerHeight] = entry.viewport;
  const image = { naturalWidth: width, naturalHeight: height, style: { display: 'none' } };
  const target = { getBoundingClientRect() { return { right: entry.right }; } };
  const context = { ImageHover: {}, image, target, window: { innerWidth } };
  run(hoverCode + 'ImageHover.onLoadStart(image,target);', context);
  let renderedWidth = styleNumber(image.style.maxWidth) ?? width;
  let renderedHeight = height * (renderedWidth / width);
  if (renderedHeight > innerHeight) {
    const ratio = innerHeight / renderedHeight;
    renderedWidth *= ratio;
    renderedHeight = innerHeight;
  }
  assert.equal(image.style.display, '');
  return [renderedWidth, renderedHeight];
}

for (const entry of reference.geometry.hover_cases) {
  const sourceSize = sourceHover(entry);
  const native = imageSize(entry.image[0], entry.image[1],
    entry.viewport[0] - entry.right - 20, entry.viewport[1]);
  assert.ok(native, entry.name);
  for (const [index, expected] of entry.expected.entries()) {
    close(sourceSize[index], expected, 'source hover ' + entry.name);
    close([native.width, native.height][index], expected, 'native hover ' + entry.name);
  }
}

assert.match(source, /Config\.imageHoverBg&&\(t\.style\.backgroundColor="inherit"\)/);
const noPicturesStart = source.indexOf('.noPictures a.fileThumb');
assert.notEqual(noPicturesStart, -1);
const noPictures = source.slice(noPicturesStart, source.indexOf('.spinner', noPicturesStart)).toLowerCase();
assert.match(noPictures, /img:not\(\.expanded-thumb\)\s*\{[^}]*opacity:\s*0;/);
for (const color of Object.values(reference.presentation.noPictures.border_colors)) {
  assert.ok(noPictures.includes('border: 1px solid ' + color.toLowerCase()), color);
}

console.log('PASS pinned extension v1191 hash, image defaults and settings instructions');
console.log(`PASS ${reference.geometry.expansion_cases.length} expansion and ${reference.geometry.hover_cases.length} hover geometry cases against imageSize`);
console.log('PASS pinned hover/background and noPictures presentation instructions');
