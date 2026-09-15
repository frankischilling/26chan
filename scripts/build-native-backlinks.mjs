import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { build, version as esbuildVersion } from 'esbuild';

const root = new URL('../', import.meta.url);
const args = process.argv.slice(2);
assert.ok(args.length === 0 || (args.length === 1 && args[0] === '--check'), 'Use no arguments or --check');
const json = async path => JSON.parse(await readFile(new URL(path, root), 'utf8'));
const manifest = await json('package.json'), lock = await json('package-lock.json');
assert.equal(manifest.devDependencies.parse5, '8.0.1');
assert.equal(manifest.devDependencies.esbuild, '0.28.2');
assert.equal(esbuildVersion, '0.28.2');
const notices = [];
for (const [name, license] of [['parse5', 'LICENSE'], ['entities', 'LICENSE'], ['esbuild', 'LICENSE.md']]) {
  const installed = await json(`node_modules/${name}/package.json`);
  const locked = lock.packages[`node_modules/${name}`];
  assert.equal(installed.version, locked.version, `${name} differs from the lockfile`);
  assert.ok(typeof locked.integrity === 'string' && locked.integrity.startsWith('sha512-'), `${name} has no locked integrity`);
  const text = (await readFile(new URL(`node_modules/${name}/${license}`, root), 'utf8')).replace(/\r\n/g, '\n').trim();
  assert.ok(!text.includes('*/'), 'License text cannot end the bundle notice');
  notices.push(`${name} ${installed.version}\n${text}`);
}
const result = await build({
  absWorkingDir: fileURLToPath(root), entryPoints: ['apps/public/client/native-quote-features.js'],
  outfile: 'apps/public/static/native-backlinks.v1.js', bundle: true, platform: 'browser',
  format: 'esm', target: ['es2022'], minify: true, charset: 'ascii', legalComments: 'inline',
  write: false, metafile: true, logLevel: 'silent',
  banner: { js: `/*! Build with npm run build:native-backlinks.\n\n${notices.join('\n\n')}\n*/` },
});
assert.equal(result.outputFiles.length, 1, 'Quote features must be one fixed release resource');
const outputs = Object.values(result.metafile.outputs);
assert.equal(outputs.length, 1);
assert.deepEqual(outputs[0].imports, [], 'No imports or external resources may remain in the page module');
assert.deepEqual([...outputs[0].exports].sort(), ['BACKLINK_LIMITS', 'INLINE_LIMITS', 'createCommentProjection', 'mountNativeBacklinks', 'mountNativeInlineQuotes']);
const allowed = new Set([
  'apps/public/client/native-quote-features.js',
  'apps/public/client/native-backlinks.js',
  'apps/public/client/native-inline-quotes.js',
  'apps/public/client/native-comment-projection.js',
  'apps/public/client/native-filter-limits.js',
  'apps/public/static/thread-watcher-core.v1.js',
]);
for (const path of Object.keys(result.metafile.inputs)) assert.ok(allowed.has(path), `Unexpected quote-feature source: ${path}`);
const bytes = result.outputFiles[0].contents;
assert.ok(bytes.length <= 32768, 'Quote-feature page module exceeds its 32 KiB release budget');
const target = new URL('apps/public/static/native-backlinks.v1.js', root);
if (args[0] === '--check') {
  assert.ok(Buffer.from(bytes).equals(await readFile(target)), 'Quote-feature module is stale; run npm run build:native-backlinks');
  console.log(`Native quote-feature module matches its pinned sources (${bytes.length} bytes).`);
} else {
  await writeFile(target, bytes);
  console.log(`Built native quote-feature module (${bytes.length} bytes).`);
}
