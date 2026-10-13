// Test-only full-vocabulary native qualification assets. No public routes.
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
const root = new URL('../../../', import.meta.url), output = new URL('./full-generated/', import.meta.url);
const source = await readFile(new URL('apps/public/vendor/tegaki/0.9.4/tegaki.min.js', root));
assert.equal(createHash('sha256').update(source).digest('hex'), 'daea182c52df0c032eadbecb4de8f91f634a61bf82aaf35dda077fab50e68744');
await mkdir(output, { recursive: true });
for (const name of ['native-replay-data.js', 'native-replay-envelope.js', 'native-replay-worker-boundary.js', 'native-replay-worker-allocations.js']) {
  await writeFile(new URL(name, output), await readFile(new URL(`apps/public/client/${name}`, root)));
}
const runtime = await readFile(new URL('apps/public/client/native-replay-runtime.inc.js', root));
await writeFile(new URL('runtime.js', output), Buffer.concat([source, Buffer.from('\n'), runtime]));
const reference = Buffer.from(`
import { decodeReplayCandidateWire, prepareFullReplayCandidate } from './native-replay-data.js';
export function renderReference(bytes) {
  Tegaki.open({replayMode:true,saveReplay:false});
  const prepared = prepareFullReplayCandidate(decodeReplayCandidateWire(bytes), Tegaki.replayViewer);
  Object.assign(Tegaki.replayViewer, prepared.metadata); Tegaki.replayViewer.toolMap = prepared.toolMap;
  Tegaki.initFromReplay(); Tegaki.init(); Tegaki.setTool(Tegaki.defaultTool);
  for (const event of prepared.events) event.dispatch();
  const canvas = Tegaki.flatten();
  return Array.from(canvas.getContext('2d').getImageData(0,0,canvas.width,canvas.height).data);
}
`);
await writeFile(new URL('reference.js', output), Buffer.concat([source, Buffer.from('\n'), reference]));
