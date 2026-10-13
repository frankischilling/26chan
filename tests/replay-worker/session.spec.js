import { test, expect } from '@playwright/test';
import { FULL_CASES } from '../fixtures/replay-worker/full-cases.mjs';
for (const row of FULL_CASES) test(`owned replay session: ${row.id}`, async({page})=>{
  page.on('console', message => { if(message.type()==='error') console.error(message.text()); });
  await page.goto('/');
  const mismatch=await page.evaluate(async row=>{
    const {encodeOwnedCase}=await import('/cases.mjs');
    const {createReplaySession}=await import('/session.js');
    const bytes=encodeOwnedCase(row);
    const session=createReplaySession(bytes.buffer);
    let frame;
    try {
      frame=await session.ready;
      while(frame.next!==null) {
        frame.bitmap.close();frame=null;
        frame=await session.advance(0xffffffff);
      }
      const canvas=document.createElement('canvas');canvas.width=frame.bitmap.width;canvas.height=frame.bitmap.height;
      const ctx=canvas.getContext('2d');ctx.drawImage(frame.bitmap,0,0);
      const actual=ctx.getImageData(0,0,canvas.width,canvas.height).data;
      const {renderReference}=await import('/full-generated/reference.js');
      const expected=renderReference(bytes);
      if(actual.length!==expected.length) return {actual:actual.length,expected:expected.length};
      for(let i=0;i<actual.length;i++) if(actual[i]!==expected[i]) return {i,actual:actual[i],expected:expected[i]};
      return null;
    } finally {frame?.bitmap.close();session.close();}
  },row);
  expect(mismatch).toBeNull();
});
