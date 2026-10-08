import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { build, version } from 'esbuild';

const root = new URL('../', import.meta.url), args = process.argv.slice(2);
assert.ok(!args.length || (args.length === 1 && args[0] === '--check'));
const lock = JSON.parse(await readFile(new URL('package-lock.json', root), 'utf8'));
assert.equal(version, '0.28.2');
assert.equal(version, lock.packages['node_modules/esbuild'].version);
const result = await build({
  absWorkingDir: fileURLToPath(root),
  entryPoints: ['apps/public/client/native-quick-reply.js'],
  outfile: 'apps/public/static/native-quick-reply.v1.js',
  bundle: true, platform: 'browser', format: 'esm', target: ['es2022'], minify: true, charset: 'ascii',
  external: ['../static/thread-watcher-core.v1.js'], write: false, metafile: true, logLevel: 'silent',
  banner: { js: '/*! Build with npm run build:native-quick-reply. Uses the fixed thread-watcher-core.v1.js ID helpers. */' },
});
assert.equal(result.outputFiles.length, 1);
const output = Object.values(result.metafile.outputs)[0];
assert.deepEqual(output.imports.map(item => [item.path, item.kind, item.external]), [
  ['../static/thread-watcher-core.v1.js', 'import-statement', true],
  ['../static/thread-watcher-core.v1.js', 'import-statement', true],
  ['../static/thread-watcher-core.v1.js', 'import-statement', true],
]);
assert.deepEqual(output.exports, ['mountNativeQuickReply']);
for (const path of Object.keys(result.metafile.inputs)) assert.ok([
  'apps/public/client/native-quick-reply.js',
  'apps/public/client/native-quick-reply-transport.js',
  'apps/public/client/native-post-form.js',
  'apps/public/client/native-quick-reply-position.js',
  'apps/public/client/native-quick-reply-cooldown.js',
  'apps/public/client/native-post-preferences.js',
  'apps/public/client/native-post-numbers.js',
  'apps/public/static/watcher-position.v1.js',
].includes(path), `Unexpected Quick Reply source: ${path}`);
const bytes = result.outputFiles[0].contents;
assert.ok(bytes.length <= 32768, 'Quick Reply asset exceeds its 32 KiB budget');
const target = new URL('apps/public/static/native-quick-reply.v1.js', root);
if (args[0] === '--check') assert.ok(Buffer.from(bytes).equals(await readFile(target)), 'Quick Reply bundle is stale');
else await writeFile(target, bytes);
console.log(`Native Quick Reply bundle ${args.length ? 'matches' : 'built'} (${bytes.length} bytes).`);
