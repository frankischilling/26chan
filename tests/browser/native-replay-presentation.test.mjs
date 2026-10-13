import test from 'node:test';
import assert from 'node:assert/strict';
import { validReplayPresentation } from '../../apps/public/client/native-replay-presentation.js';
const receipt = () => ({color:'#123abc',tool:{id:1,size:64,alpha:.5,flow:1,tipId:0,
  sizeDynamicsEnabled:true,alphaDynamicsEnabled:false,flowDynamicsEnabled:false,preserveAlphaEnabled:true},
  layers:[{id:1,alpha:1,visible:true,active:true,selected:true,width:24,height:24,pixels:new Uint8ClampedArray(24*24*4)}]});
test('display receipt admits only bounded tool values and 24px layer previews', () => {
  assert.equal(validReplayPresentation(receipt()), true);
  const untouchedColor = receipt(); untouchedColor.color = null;
  assert.equal(validReplayPresentation(untouchedColor), true, 'The source has not updated its initial color control');
  for (const change of [v=>v.color=['#123abc'],v=>v.color='url(https://example.test)',v=>v.url='anything',
    v=>v.tool.size=65,v=>v.tool.alpha=NaN,v=>v.tool.flow=Infinity,v=>v.tool.id=9,v=>v.tool.tipId=3,
    v=>v.tool.sizeDynamicsEnabled=1,v=>v.tool.name='<svg>',v=>v.layers=[],v=>v.layers[0].active=false,
    v=>v.layers[0].width=25,v=>v.layers[0].pixels=new Uint8Array(24*24*4),
    v=>v.layers[0].pixels=new Uint8ClampedArray(24*24*4+1),v=>v.layers[0].id=16386,
    v=>v.layers.push({...v.layers[0]}),v=>v.layers[0].name='unexpected',v=>v.layers[0].visible=1]) {
    const value=receipt(); change(value); assert.equal(validReplayPresentation(value),false);
  }
});
test('the source layer cap and unique IDs bound the complete display receipt', () => {
  const value=receipt();
  value.layers=Array.from({length:25},(_,i)=>({...value.layers[0],id:i+1,active:i===0}));
  assert.equal(validReplayPresentation(value),true);
  value.layers.push({...value.layers[0],id:26,active:false}); assert.equal(validReplayPresentation(value),false);
});
