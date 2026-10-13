import { test, expect } from '@playwright/test';
import { FULL_CASES } from '../fixtures/replay-worker/full-cases.mjs';
for (const row of FULL_CASES) test(`full source worker: ${row.id}`,async({page})=>{
  await page.goto('/');
  const result = await page.evaluate(async row=>{
    const {encodeOwnedCase} = await import('/cases.mjs');
    const bytes = encodeOwnedCase(row);
    const actual = await new Promise((resolve,reject)=>{
      const worker = new Worker('/full-worker.mjs',{type:'module'});
      const timer = setTimeout(()=>{worker.terminate();reject(new Error('Worker deadline exceeded'));},5000);
      worker.onerror = event=>{clearTimeout(timer);worker.terminate();reject(new Error(event.message));};
      worker.onmessage=({data})=>{
        clearTimeout(timer);worker.terminate();
        if(data.error){reject(new Error(data.error));return;}
        try {
          const canvas=document.createElement('canvas');canvas.width=data.bitmap.width;canvas.height=data.bitmap.height;
          const ctx=canvas.getContext('2d');ctx.drawImage(data.bitmap,0,0);
          resolve({pixels:ctx.getImageData(0,0,canvas.width,canvas.height).data,allocations:data.allocations});
        } finally {data.bitmap.close();}
      };
      worker.postMessage(bytes);
    });
    const {renderReference}=await import('/full-generated/reference.js');
    const expected = renderReference(bytes);
    let mismatch = actual.pixels.length === expected.length ? null : {length:actual.pixels.length,expected:expected.length};
    for(let i=0; mismatch===null && i<expected.length; i++) {
      if(actual.pixels[i]!==expected[i]) mismatch={index:i,actual:actual.pixels[i],expected:expected[i]};
    }
    return {mismatch,bytes:expected.length,allocations:actual.allocations};
  },row);
  expect(result.mismatch).toBeNull();
  expect(result.bytes).toBe(row.width*row.height*4);
  expect(result.allocations.chargedBytes).toBeLessThanOrEqual(result.allocations.maximumBytes);
});
