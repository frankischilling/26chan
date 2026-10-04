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
  absWorkingDir: fileURLToPath(root), entryPoints: ['apps/public/client/native-images.js'],
  outfile: 'apps/public/static/native-images.v1.js', bundle: true, platform: 'browser',
  format: 'esm', target: ['es2022'], minify: true, charset: 'ascii', legalComments: 'inline',
  write: false, metafile: true, logLevel: 'silent',
  banner: { js: '/*! Build with npm run build:native-images. */' },
});
assert.equal(result.outputFiles.length, 1);
assert.deepEqual(Object.keys(result.metafile.inputs).sort(), [
  'apps/public/client/native-images.js', 'apps/public/client/native-spoiler-assets.js',
  'apps/public/client/native-spoilers.js',
].sort());
const [output] = Object.values(result.metafile.outputs);
assert.deepEqual(output.imports, []);
assert.deepEqual([...output.exports].sort(), ['IMAGE_LIMITS', 'imageSize', 'imageTarget', 'mountNativeImages']);
const bytes = result.outputFiles[0].contents;
assert.ok(bytes.length <= 16384, 'Image controls exceed the 16 KiB release budget');
const target = new URL('apps/public/static/native-images.v1.js', root);
if (args[0] === '--check') {
  assert.ok(Buffer.from(bytes).equals(await readFile(target)), 'Image controls are stale; run npm run build:native-images');
  console.log(`Native image controls match their pinned sources (${bytes.length} bytes).`);
} else {
  await writeFile(target, bytes);
  console.log(`Built native image controls (${bytes.length} bytes).`);
}
