// Data-only validation of an actual server snapshot. No DOM or browser runs.
import assert from 'node:assert/strict';
import {Worker} from 'node:worker_threads';
import {once} from 'node:events';
import {parseUpdaterSnapshot,updaterContext} from '../../apps/public/client/native-updater-snapshot.js';
const [origin,thread,mediaOrigin]=process.argv.slice(2);
const context=updaterContext({origin,thread,board:'f',mediaOrigin});
const chunks=[];let length=0;
for await(const chunk of process.stdin){length+=chunk.length;assert.ok(length<=4194304);chunks.push(chunk);}
const raw=new TextDecoder('utf-8',{fatal:true}).decode(Buffer.concat(chunks));
const result=parseUpdaterSnapshot(raw,context);assert.equal(result.status,'ok');
const worker=new Worker(new URL('./helpers/swf-snapshot-worker.mjs',import.meta.url),{resourceLimits:{maxOldGenerationSizeMb:128,stackSizeMb:4}});
let timer;
try {
  const deadline=new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error('Release snapshot worker deadline')),2000);});
  const [ready]=await Promise.race([once(worker,'message'),deadline]);assert.deepEqual(ready,{ready:true});
  const response=once(worker,'message');worker.postMessage({kind:'updater-snapshot',raw,context});
  const [compiled]=await Promise.race([response,deadline]);assert.deepEqual(compiled,result,'Pinned release worker and source parser must produce identical recipes');
} finally {clearTimeout(timer);await worker.terminate();}

const op=result.snapshot.posts[0].tree;
function descendants(node){return typeof node==='string'?[]:[node,...node.children.flatMap(descendants)];}
const nodes=descendants(op);
assert.equal(nodes.filter(node=>node.attrs.class==='fileText').length,1);
assert.equal(nodes.filter(node=>node.tag==='iframe'||node.attrs['data-player']).length,0);
assert.ok(nodes.some(node=>node.tag==='a'&&node.attrs.href.startsWith(`${mediaOrigin}/f/`)&&node.attrs.href.endsWith('.swf')));
console.log('Actual upload-board snapshot passes the worker/main-thread grammar without an active embed.');
