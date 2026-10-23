import test from 'node:test';
import assert from 'node:assert/strict';
import { createUploadPostSession } from '../../apps/public/client/native-upload-post-session.js';
const receipt = state => ({state,upload_id:'a'.repeat(32),upload_capability:'b'.repeat(64),resto:'0'});
function fixture(overrides={}) {
  const calls=[],file={name:'owned.swf'};
  const session=createUploadPostSession({upload:async value=>{calls.push(['upload',value]);return receipt('queued');},check:async value=>{calls.push(['check',value]);return receipt('approved');},cancel:async value=>{calls.push(['cancel',value]);},send:async fields=>{calls.push(['send',fields]);return {thread:'123',post:'123'};},wait:async()=>{calls.push(['wait']);},...overrides});
  return {session,calls,file,fields:()=>({com:'Owned',filetag:'4'}),signal:new AbortController().signal};
}
test('one submit uploads, waits for approval and posts the selected category with its capability',async()=>{
  const f=fixture();assert.deepEqual(await f.session.submit(f.file,f.fields,()=>true,f.signal),{thread:'123',post:'123'});
  assert.deepEqual(f.calls.map(call=>call[0]),['upload','wait','check','send']);
  assert.equal(f.calls.at(-1)[1].filetag,'4');assert.equal(f.calls.at(-1)[1].upload_id,'a'.repeat(32));assert.equal(f.session.state().hasFile,false);
});
test('expired CAPTCHA retains approval and a new proof posts without reuploading',async()=>{
  const f=fixture();assert.equal(await f.session.submit(f.file,f.fields,()=>false,f.signal),null);assert.equal(f.session.state().approved,true);
  assert.equal(f.calls.some(call=>call[0]==='send'),false);await f.session.submit(f.file,f.fields,()=>true,f.signal);
  assert.equal(f.calls.filter(call=>call[0]==='upload').length,1);assert.equal(f.calls.filter(call=>call[0]==='send').length,1);
});
test('a bounded pending job keeps its receipt and resumes on explicit submit',async()=>{
  let ready=false,checks=0;const f=fixture({check:async()=>{checks++;return receipt(ready?'approved':'processing');}});
  await assert.rejects(f.session.submit(f.file,f.fields,()=>true,f.signal),/still processing/);assert.equal(checks,60);assert.equal(f.session.state().busy,false);
  ready=true;await f.session.submit(f.file,f.fields,()=>true,f.signal);assert.equal(checks,61);assert.equal(f.calls.filter(call=>call[0]==='upload').length,1);
});
test('malformed or lost posting responses prohibit automatic or manual duplicate sends in the same session',async()=>{
  let posts=0;const f=fixture({send:async()=>{posts++;throw new Error('lost response');}});
  await assert.rejects(f.session.submit(f.file,f.fields,()=>true,f.signal),/lost response/);assert.equal(f.session.state().uncertain,true);
  await assert.rejects(f.session.submit(f.file,f.fields,()=>true,f.signal),/may have succeeded/);await assert.rejects(f.session.discard(f.signal),/may have succeeded/);assert.equal(posts,1);
});
test('a definite server refusal retains the file for a corrected explicit submit',async()=>{
  let posts=0;const f=fixture({send:async()=>++posts===1?{error:'Error: Invalid file tag.'}:{thread:'123',post:'123'}});
  assert.deepEqual(await f.session.submit(f.file,f.fields,()=>true,f.signal),{error:'Error: Invalid file tag.'});assert.equal(f.session.state().uncertain,false);
  await f.session.submit(f.file,f.fields,()=>true,f.signal);assert.equal(posts,2);assert.equal(f.calls.filter(call=>call[0]==='upload').length,1);
});
test('file replacement requires acknowledged cancellation, and cancellation failure preserves the receipt',async()=>{
  let fails=true;const f=fixture({cancel:async()=>{if(fails)throw new Error('cancel failed');}});
  await f.session.submit(f.file,f.fields,()=>false,f.signal);await assert.rejects(f.session.submit({},f.fields,()=>true,f.signal),/Cancel the previous/);
  await assert.rejects(f.session.discard(f.signal),/cancel failed/);assert.equal(f.session.state().hasFile,true);
  fails=false;await f.session.discard(f.signal);assert.equal(f.session.state().hasFile,false);
});
test('failed processing never reaches posting and a stopped request retains the known receipt',async()=>{
  const f=fixture({check:async()=>receipt('failed')});await assert.rejects(f.session.submit(f.file,f.fields,()=>true,f.signal),/processing failed/);assert.equal(f.calls.some(call=>call[0]==='send'),false);
  const controller=new AbortController(),g=fixture({wait:async()=>controller.abort()});await assert.rejects(g.session.submit(g.file,g.fields,()=>true,controller.signal),/stopped/);assert.equal(g.session.state().hasFile,true);assert.equal(g.calls.some(call=>call[0]==='send'),false);
});
test('overlapping submissions cannot duplicate an upload',async()=>{
  let release;const f=fixture({upload:()=>new Promise(resolve=>{release=()=>resolve(receipt('approved'));})});
  const first=f.session.submit(f.file,f.fields,()=>true,f.signal);await assert.rejects(f.session.submit(f.file,f.fields,()=>true,f.signal),/already in progress/);release();await first;
});
