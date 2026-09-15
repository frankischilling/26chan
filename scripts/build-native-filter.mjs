import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { build, version as esbuildVersion } from 'esbuild';

const root = new URL('../', import.meta.url);
const args = process.argv.slice(2);
assert.ok(args.length === 0 || (args.length === 1 && args[0] === '--check'), 'Use no arguments or --check');
const json = async path => JSON.parse(await readFile(new URL(path, root), 'utf8'));
const manifest = await json('package.json');
const lock = await json('package-lock.json');
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
  // Retain copyright holders' names verbatim, including non-ASCII names.
  notices.push(`${name} ${installed.version}\n${text}`);
}
const result = await build({
  absWorkingDir: fileURLToPath(root), entryPoints: ['apps/public/client/native-filter.js'],
  outfile: 'apps/public/static/native-filter.v1.js', bundle: true, platform: 'browser',
  format: 'esm', target: ['es2022'], minify: true, charset: 'ascii', legalComments: 'inline',
  write: false, metafile: true, logLevel: 'silent',
  banner: { js: `/*! Build with npm run build:native-filter.\n\n${notices.join('\n\n')}\n*/` },
});
assert.equal(result.outputFiles.length, 1, 'The worker must be one fixed release resource');
const outputs = Object.values(result.metafile.outputs);
assert.equal(outputs.length, 1);
assert.deepEqual(outputs[0].imports, [], 'No imports or external resources may remain in the worker');
assert.deepEqual([...outputs[0].exports].sort(), ['BLACKLIST_LIMITS', 'FILTER_LIMITS', 'NativeCatalogTransport', 'NativeFilterMatcher', 'NativeWatchLock', 'autoWatchBoards', 'catalogApiUrl', 'collectAutoWatches', 'markNativeTrackedQuotes', 'mountNativeFilters', 'mountNativeReplyHiding', 'mountNativeThreadHiding', 'mountNativeThreadUpdater', 'mountNativeKeybinds', 'mountNativeQuickReply', 'mountNativeLinkification', 'mountNativeQuotePreview', 'planAutoWatches', 'readBlacklist', 'readFilterRules', 'readNativeFilters', 'runNativeFilterJob', 'writeBlacklist'].sort());
for (const path of Object.keys(result.metafile.inputs)) {
  assert.ok(path === 'apps/public/static/thread-watcher-core.v1.js'
    || ['apps/public/client/', 'node_modules/parse5/', 'node_modules/entities/'].some(prefix => path.startsWith(prefix)), `Unexpected worker source: ${path}`);
}
const bytes = result.outputFiles[0].contents;
assert.ok(bytes.length <= 262144, 'Worker bundle exceeds its 256 KiB release budget');
const target = new URL('apps/public/static/native-filter.v1.js', root);
if (args[0] === '--check') {
  assert.ok(Buffer.from(bytes).equals(await readFile(target)), 'Worker bundle is stale; run npm run build:native-filter');
  console.log(`Native filter bundle matches its pinned sources (${bytes.length} bytes).`);
} else {
  await writeFile(target, bytes);
  console.log(`Built native filter bundle (${bytes.length} bytes).`);
}
