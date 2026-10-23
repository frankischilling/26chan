// Controlled loader and stream fixtures; real Ruffle playback is qualified separately.
import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
const source=fs.readFileSync(new URL('../../apps/media-http/static/swf-player.v1.js',import.meta.url),'utf8');

async function run({url='http://127.0.0.1:3002/player-data/f/123',length='8',chunks=[new Uint8Array(8)],ok=true,loadFails=false}={}){
  let start,request,loaded,cancelled=false,appended=0;
  const status={hidden:false,textContent:'Loading'};
  const container={dataset:{movie:url,filename:'owned.swf'},append(){appended++;},replaceChildren(){appended=0;}};
  const document={currentScript:{src:'http://127.0.0.1:3002/player-assets/ruffle-0.7.1-memory-cap-v1/swf-player.v1.js'},getElementById:id=>id==='player'?container:status};
  const window={addEventListener(name,handler,options){assert.equal(name,'DOMContentLoaded');assert.equal(options.once,true);start=handler;}};
  const fetch=async(url,options)=>{
    request={url:String(url),options};let at=0;
    return {ok,headers:{get:name=>name==='content-length'?length:null},body:{getReader(){return {
      async read(){return at<chunks.length?{done:false,value:chunks[at++]}:{done:true};},
      async cancel(){cancelled=true;},
    };}}};
  };
  vm.runInNewContext(source,{window,document,fetch,URL,AbortController,setTimeout,clearTimeout,Uint8Array},{timeout:100});
  const config=window.RufflePlayer.config;
  window.RufflePlayer.newest=()=>({createPlayer:()=>({ruffle:()=>({async load(options){loaded=options;if(loadFails)throw new Error('owned failure');}})})});
  await start();return {status,request,loaded,cancelled,appended,config};
}

test('player receives bounded original bytes without credentials or redirects',async()=>{
  const result=await run({chunks:[new Uint8Array([1,2,3]),new Uint8Array([4,5,6,7,8])]});
  assert.deepEqual(Array.from(result.loaded.data),[1,2,3,4,5,6,7,8]);
  assert.equal(result.request.options.credentials,'omit');assert.equal(result.request.options.redirect,'error');
  assert.equal(result.loaded.allowScriptAccess,false);assert.equal(result.loaded.openUrlMode,'deny');
  assert.equal(result.loaded.maxExecutionDuration,5);assert.equal(result.loaded.swfFileName,'owned.swf');
  assert.equal(result.cancelled,true);assert.equal(result.appended,1);assert.equal(result.status.hidden,true);
  assert.equal(Object.isFrozen(result.config),true);
});

test('invalid routes and other origins never reach fetch',async()=>{
  for(const url of ['https://example.invalid/player-data/f/123','http://127.0.0.1:3002/player-data/f/0','http://127.0.0.1:3002/player-data/f/123?x=1','http://127.0.0.1:3002/player-data/f/123#x','http://name:secret@127.0.0.1:3002/player-data/f/123','http://127.0.0.1:3002/healthz']){
    const result=await run({url});assert.equal(result.request,undefined);assert.equal(result.loaded,undefined);assert.equal(result.status.hidden,false);
  }
});

test('bad lengths, overflow, truncation and load failures leave no player',async()=>{
  for(const options of [{ok:false},{length:null},{length:'7'},{length:'10485761'},{length:'8.5'},{chunks:[new Uint8Array(9)]},{chunks:[new Uint8Array(7)]},{loadFails:true}]){
    const result=await run(options);assert.equal(result.appended,0);assert.equal(result.status.hidden,false);
    assert.equal(result.status.textContent,'This Flash file could not be played. You can still download the original file.');
  }
});
