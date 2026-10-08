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
  const installed = await json(`node_modules/${name}/package.json`), locked = lock.packages[`node_modules/${name}`];
  assert.equal(installed.version, locked.version); assert.ok(locked.integrity?.startsWith('sha512-'));
  notices.push(`${name} ${installed.version}\n${(await readFile(new URL(`node_modules/${name}/${license}`, root), 'utf8')).replace(/\r\n/g, '\n').trim()}`);
}
const result = await build({
  absWorkingDir: fileURLToPath(root), entryPoints: ['apps/public/client/global-search.js'],
  outfile: 'apps/public/static/global-search.v1.js', bundle: true, platform: 'browser', format: 'esm',
  target: ['es2022'], minify: true, charset: 'ascii', legalComments: 'inline', write: false, metafile: true, logLevel: 'silent',
  banner: { js: `/*! Build with npm run build:global-search.\n\n${notices.join('\n\n')}\n*/` },
});
assert.equal(result.outputFiles.length, 1);
for (const path of Object.keys(result.metafile.inputs)) {
  assert.ok(path === 'apps/public/static/thread-watcher-core.v1.js'
    || path.startsWith('apps/public/client/')
    || path.includes('node_modules/parse5/')
    || path.includes('node_modules/entities/'), `Unexpected search source: ${path}`);
}
assert.ok(result.outputFiles[0].contents.length <= 262144, 'Search bundle exceeds 256 KiB');
const target = new URL('apps/public/static/global-search.v1.js', root), bytes = result.outputFiles[0].contents;
if (args[0] === '--check') assert.ok(Buffer.from(bytes).equals(await readFile(target)), 'Global search bundle is stale; run npm run build:global-search');
else await writeFile(target, bytes);
console.log(`Global search bundle matches pinned sources (${bytes.length} bytes).`);
