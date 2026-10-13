import assert from 'node:assert/strict';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { build, version } from 'esbuild';
const root = new URL('../', import.meta.url), check = process.argv.includes('--check');
assert.ok(process.argv.length === 2 || (process.argv.length === 3 && check));
const lock = JSON.parse(await readFile(new URL('package-lock.json', root)));
assert.equal(version, lock.packages['node_modules/esbuild'].version);
const source = await readFile(new URL('apps/public/vendor/tegaki/0.9.4/tegaki.min.js', root));
assert.equal(createHash('sha256').update(source).digest('hex'), 'daea182c52df0c032eadbecb4de8f91f634a61bf82aaf35dda077fab50e68744');
const runtime = await readFile(new URL('apps/public/client/native-replay-runtime.inc.js', root), 'utf8');
const notices = await Promise.all(['tegaki-MIT.txt','tegaki-UZIP-MIT.txt'].map(name=>readFile(new URL(`docs/licenses/${name}`,root),'utf8')));
// Source event constructors index TegakiEvents through this.constructor.name.
// Preserve names through bundling; changing them changes the replay language.
const options = { absWorkingDir:fileURLToPath(root), bundle:true, platform:'browser',format:'esm',target:['es2022'],keepNames:true,minify:true,charset:'ascii',write:false,metafile:true,logLevel:'silent' };
const outputs = [
  ['runtime.v1.js', await build({...options, stdin:{contents:`${source}\n${runtime}`,resolveDir:fileURLToPath(new URL('apps/public/client/',root)),sourcefile:'native-replay-runtime.js'},banner:{js:`/*!\n${notices.join('\n')}*/`}})],
  ['worker.v1.js', await build({...options,entryPoints:['apps/public/client/native-replay-worker.js'],external:['./runtime.v1.js']})],
];
const dir = new URL('apps/public/static/native-replay/',root);
if (!check) await mkdir(dir,{recursive:true});
for (const [name,result] of outputs) {
  assert.equal(result.outputFiles.length,1);
  const output=Object.values(result.metafile.outputs)[0];
  assert.deepEqual(output.imports.map(value=>[value.path,value.kind,value.external]),
    name.startsWith('worker') ? [['./runtime.v1.js','dynamic-import',true]] : []);
  for(const input of Object.keys(result.metafile.inputs)) assert.ok([
    'apps/public/client/native-replay-worker.js', 'apps/public/client/native-replay-worker-allocations.js',
    'apps/public/client/native-replay-worker-boundary.js', 'apps/public/client/native-replay-data.js',
    'apps/public/client/native-replay-envelope.js', 'apps/public/client/native-replay-runtime.js',
  ].includes(input), `Unexpected replay dependency: ${input}`);
  const bytes=result.outputFiles[0].contents;
  assert.ok(bytes.length <= (name.startsWith('runtime') ? 150000 : 20000));
  const destination=new URL(name,dir);
  if(check) assert.ok(Buffer.from(bytes).equals(await readFile(destination)),`${name} is stale`);
  else await writeFile(destination,bytes);
  console.log(`${name}: ${bytes.length} bytes`);
}
