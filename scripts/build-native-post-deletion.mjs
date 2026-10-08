import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { build, version } from 'esbuild';

const root = new URL('../', import.meta.url), args = process.argv.slice(2);
assert.ok(args.length === 0 || (args.length === 1 && args[0] === '--check'), 'Use no arguments or --check');
const manifest = JSON.parse(await readFile(new URL('package.json', root)));
const lock = JSON.parse(await readFile(new URL('package-lock.json', root)));
assert.equal(version, '0.28.2');
assert.equal(manifest.devDependencies.esbuild, version);
assert.equal(lock.packages['node_modules/esbuild'].version, version);
const result = await build({
  absWorkingDir: fileURLToPath(root), entryPoints: ['apps/public/client/native-post-deletion.js'],
  outfile: 'apps/public/static/native-post-deletion.v1.js', bundle: true, platform: 'browser',
  format: 'esm', target: ['es2022'], minify: true, charset: 'ascii', legalComments: 'inline',
  write: false, metafile: true, logLevel: 'silent',
  banner: { js: '/*! Build with npm run build:native-post-deletion. */' },
});
assert.equal(result.outputFiles.length, 1);
assert.deepEqual(Object.keys(result.metafile.inputs).sort(), [
  'apps/public/client/native-post-deletion.js',
  'apps/public/static/thread-watcher-core.v1.js',
].sort());
const [output] = Object.values(result.metafile.outputs);
assert.deepEqual(output.imports, []);
assert.deepEqual([...output.exports].sort(), ['DELETION_LIMITS', 'deletionResult', 'mountNativeDeletion', 'sendNativeDeletion']);
const bytes = result.outputFiles[0].contents;
assert.ok(bytes.length <= 16384, 'Deletion controls exceed the 16 KiB release budget');
const target = new URL('apps/public/static/native-post-deletion.v1.js', root);
if (args[0] === '--check') {
  assert.ok(Buffer.from(bytes).equals(await readFile(target)), 'Deletion controls are stale; run npm run build:native-post-deletion');
  console.log(`Native post deletion matches its pinned sources (${bytes.length} bytes).`);
} else {
  await writeFile(target, bytes);
  console.log(`Built native post deletion (${bytes.length} bytes).`);
}
