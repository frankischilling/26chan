import test from 'node:test';
import assert from 'node:assert/strict';
import { acquireReplay, replayUrl } from '../../apps/public/client/native-replay-acquire.js';
const context={mediaOrigin:'https://media.example',board:'i',tim:'123'};
test('acquisition derives only a canonical approved-media URL and omits credentials',async()=>{
  const bytes=new Uint8Array(288);bytes[5]=12;
  const received=await acquireReplay({...context,fetcher:async(url,options)=>{
    assert.equal(url,'https://media.example/replay/i/123.ibr');assert.equal(options.credentials,'omit');
    assert.equal(options.redirect,'error');assert.equal(options.mode,'cors');
    return new Response(bytes,{headers:{'content-type':'application/octet-stream'}});
  }});
  assert.deepEqual(new Uint8Array(received),bytes);
});
test('invalid address components fail before network access',()=>{
  for(const extra of [{mediaOrigin:'https://media.example/path'},{mediaOrigin:'https://user@media.example'},
    {board:'../i'},{tim:'01'},{tim:'9007199254740992'},{tim:1}]) {
    const c={...context,...extra};assert.throws(()=>replayUrl(c.mediaOrigin,c.board,c.tim));
  }
});
test('missing, oversized or malformed bodies are rejected without decoding',async()=>{
  for(const length of [0,287,289,262401]) await assert.rejects(acquireReplay({...context,
    fetcher:async()=>new Response(new Uint8Array(length),{headers:{'content-type':'application/octet-stream'}})}));
  for(const type of ['text/html','text/javascript','image/png']) await assert.rejects(acquireReplay({...context,
    fetcher:async()=>new Response(new Uint8Array(288),{headers:{'content-type':type}})}));
});
test('deadline rejects even an unresponsive fetch and aborts the request',async()=>{
  let expire,signal;
  const work=acquireReplay({...context,schedule:fn=>{expire=fn;return 1;},unschedule:()=>{},
    fetcher:async(_,options)=>{signal=options.signal;return new Promise(()=>{});}});
  expire();await assert.rejects(work,/timed out/);assert.equal(signal.aborted,true);
});
test('caller cancellation rejects stalled reads and releases reader ownership',async()=>{
  const controller=new AbortController();let released=false,read;
  const work=acquireReplay({...context,signal:controller.signal,fetcher:async()=>new Response(new ReadableStream({
    start(){read=()=>controller.abort();},cancel(){released=true;},
  }),{headers:{'content-type':'application/octet-stream'}})});
  await new Promise(resolve=>setTimeout(resolve,0));read();await assert.rejects(work,/cancelled/);assert.equal(released,true);
});
