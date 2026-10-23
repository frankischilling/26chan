import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {threadSize,indexSize} from '../../apps/public/static/native-swf-core.v1.js';
const fixture=JSON.parse(fs.readFileSync(new URL('../../fixtures/swf-embed-reference.json',import.meta.url),'utf8'));

test('embed sizing matches the original desktop and mobile arithmetic',()=>{
  for(const row of fixture.cases){
    const expected=value=>value.width>0&&value.height>0?value:null;
    assert.deepEqual(threadSize(row.width,row.height,row.viewportWidth,row.mobile),expected(row.thread));
    assert.deepEqual(indexSize(row.width,row.height,row.viewportWidth,row.viewportHeight),expected(row.index));
  }
});
test('invalid dimensions and unusable viewport sizes do not create frames',()=>{
  for(const value of [0,-1,NaN,Infinity,1.5,'640',null,Number.MAX_SAFE_INTEGER+1]){
    assert.equal(threadSize(value,480,1280,false),null);
    assert.equal(indexSize(640,value,1280,900),null);
  }
  assert.equal(threadSize(640,480,100,false),null);
  assert.equal(threadSize(640,480,30,true),null);
  assert.equal(indexSize(640,480,10,900),null);
  assert.equal(indexSize(640,480,1280,30),null);
});
