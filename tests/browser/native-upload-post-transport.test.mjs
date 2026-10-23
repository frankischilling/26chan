import test from 'node:test';
import assert from 'node:assert/strict';
import {uploadQuickReplyFile,sendUploadBoardPost} from '../../apps/public/client/native-quick-reply-transport.js';
const origin='https://board.example';
const receipt={upload_id:'a'.repeat(32),upload_capability:'b'.repeat(64),resto:'0',state:'queued'};
const json=value=>new Response(JSON.stringify(value),{headers:{'content-type':'application/json'}});
test('only the upload-board opening-post client admits the source 10 MiB ceiling',async()=>{
  const file=new File([new Uint8Array(10_485_760)],'owned.swf');let calls=0;
  const fetcher=async(url,options)=>{calls++;assert.equal(url,origin+'/f/upload');assert.equal(options.body.get('upfile').size,file.size);return json(receipt);};
  assert.deepEqual(await uploadQuickReplyFile({origin,board:'f',thread:'0',file,fetcher}),receipt);
  for(const [board,thread] of [['f','123'],['g','0'],['g','123']])await assert.rejects(uploadQuickReplyFile({origin,board,thread,file,fetcher}),/upload limit/);
  await assert.rejects(uploadQuickReplyFile({origin,board:'f',thread:'0',file:new File([new Uint8Array(10_485_761)],'too-large.swf'),fetcher}),/upload limit/);assert.equal(calls,1);
});
test('the upload-board post preserves source category and subject fields without resending bytes',async()=>{
  const fields={name:'Owned',sub:'Movie',com:'Comment',pwd:'owned-password',filetag:'4',upload_id:receipt.upload_id,upload_capability:receipt.upload_capability};
  const result=await sendUploadBoardPost({origin,board:'f',thread:'0',fields,fetcher:async(url,options)=>{
    assert.equal(url,origin+'/f/imgboard.php');assert.equal(options.body.get('filetag'),'4');assert.equal(options.body.get('sub'),'Movie');assert.equal(options.body.get('resto'),'0');assert.equal(options.body.has('upfile'),false);return json({tid:0,pid:123});
  }});assert.deepEqual(result,{thread:'123',post:'123'});
  for(const target of [{board:'g',thread:'0'},{board:'f',thread:'123'}])assert.throws(()=>sendUploadBoardPost({origin,...target,fields}),/Invalid upload-board target/);
});
