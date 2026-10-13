import test from 'node:test';
import assert from 'node:assert/strict';
import { createReplaySession } from '../../apps/public/client/native-replay-session.js';
function setup() {
  let deadline;
  const worker = { sent: [], terminated: 0, postMessage(data, transfer) { this.sent.push({ data, transfer }); }, terminate() { this.terminated++; } };
  const session = createReplaySession(new ArrayBuffer(288), {workerFactory:()=>worker,schedule:fn=>{deadline=fn;return 1;},unschedule:()=>{deadline=null;}});
  const bitmap = () => ({ width:2,height:2,closed:0,close(){this.closed++;} });
  const frame = (extra={}) => ({ presentation:{color:'#000000',tool:{id:1,size:1,alpha:1,flow:1,tipId:0,sizeDynamicsEnabled:false,alphaDynamicsEnabled:false,flowDynamicsEnabled:false,preserveAlphaEnabled:false},layers:[{id:1,alpha:1,visible:true,selected:true,active:true,width:1,height:1,pixels:new Uint8ClampedArray(4)}]},start:0,end:1000,type:'frame',sequence:worker.sent.length,index:0,count:2,next:0,bitmap:bitmap(),...extra });
  return {worker,session,frame,expire:()=>deadline()};
}
test('one init owns a private transferable copy and one command at a time',async()=>{
  const f=setup();
  assert.equal(f.worker.sent[0].data.type,'init');
  assert.equal(f.worker.sent[0].transfer[0],f.worker.sent[0].data.bytes);
  await assert.rejects(f.session.advance(0),/busy/);
  const first=f.frame();f.worker.onmessage({data:first});
  assert.equal((await f.session.ready).bitmap,first.bitmap);first.bitmap.close();
  const next=f.session.advance(100);assert.equal(f.worker.sent[1].data.until,100);
  const last=f.frame({index:2,next:null});f.worker.onmessage({data:last});
  assert.equal((await next).index,2);last.bitmap.close();f.session.close();f.session.close();
  assert.equal(f.worker.terminated,1);
});
test('deadline terminates the worker and rejects pending work',async()=>{
  const f=setup();f.expire();await assert.rejects(f.session.ready,/limits/);
  assert.equal(f.worker.terminated,1);await assert.rejects(f.session.advance(2),/closed/);
});
test('late bitmap ownership is released after close',async()=>{
  const f=setup();f.session.close();await assert.rejects(f.session.ready,/closed/);
  const late=f.frame();f.worker.onmessage({data:late});assert.equal(late.bitmap.closed,1);
});
test('invalid sequence, progress, dimensions and response types fail closed',async()=>{
  for(const extra of [{start:-1},{end:0xffffffff},{end:-1},{start:0.5},{sequence:2},{index:3},{count:16385},{next:null},{type:'error'},{bitmap:{width:1025,height:1,close(){}}}]) {
    const f=setup();f.worker.onmessage({data:f.frame(extra)});await assert.rejects(f.session.ready,/limits/);
    assert.equal(f.worker.terminated,1);
  }
});
test('dimensions and total event count cannot change after initialization',async()=>{
  for(const extra of [{start:1},{end:999},{count:3},{bitmap:{width:3,height:2,close(){}}}]) {
    const f=setup();f.worker.onmessage({data:f.frame()});(await f.session.ready).bitmap.close();
    const pending=f.session.advance(1);f.worker.onmessage({data:f.frame(extra)});await assert.rejects(pending,/limits/);
  }
});
test('invalid acquisition lengths never create a worker',()=>{
  for(const bytes of [null,{},new ArrayBuffer(287),new ArrayBuffer(262401)]) {
    assert.throws(()=>createReplaySession(bytes,{workerFactory:()=>{throw new Error('should not create');}}),/Invalid replay bytes/);
  }
});
