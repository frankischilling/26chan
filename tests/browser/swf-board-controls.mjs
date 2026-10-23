// Real rendered pages and client code. Upload/post responses are owned mocks;
// actual storage, intake and VM behavior have separate qualification tests.
import assert from 'node:assert/strict';
import { chromium,expect } from '@playwright/test';
import {assertUploadTableStyles} from './swf-source-styles.mjs';
const origin=new URL(process.argv[2]),id=process.argv[3];
assert.equal(origin.hostname,'127.0.0.1');assert.equal(origin.protocol,'http:');assert.match(id,/^[1-9][0-9]{0,18}$/);
for(const key of Object.keys(process.env))assert.ok(!key.endsWith('DATABASE_URL')&&!['POSTER_ID_KEY','PUBLIC_INTAKE_TOKEN'].includes(key),'Browser must not inherit service credentials');
const browser=await chromium.launch({timeout:15000});
try {
  const context=await browser.newContext({viewport:{width:1280,height:900}});
  await context.route('http://localhost:3002/player/**',route=>route.fulfill({status:200,contentType:'text/html',body:'<!doctype html><title>Owned controller fixture</title>'}));
  const page=await context.newPage();page.setDefaultTimeout(10000);
  let uploaded=0,posted=0;
  await page.route(origin.origin+'/f/upload',async route=>{
    uploaded++;assert.equal(route.request().method(),'POST');
    const body=route.request().postDataBuffer().toString();assert.ok(body.includes('name="resto"'));assert.ok(body.includes('filename="owned-form.swf"'));
    await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify({upload_id:'a'.repeat(32),upload_capability:'b'.repeat(64),resto:'0',state:'approved'})});
  });
  await page.route(origin.origin+'/f/imgboard.php',async route=>{
    posted++;const body=route.request().postDataBuffer().toString();
    for(const [key,value] of [['sub','Owned one-submit form'],['filetag','4'],['com','Owned upload-board body'],['upload_id','a'.repeat(32)],['upload_capability','b'.repeat(64)]])assert.ok(body.includes(`name="${key}"\r\n\r\n${value}\r\n`),key);
    assert.ok(!body.includes('filename='));
    await route.fulfill({status:200,contentType:'application/json',body:`{"tid":0,"pid":${id}}`});
  });
  for(const [theme,name,subject,size] of [
    ['yotsuba','rgb(17, 119, 67)','rgb(204, 17, 5)','12px'],
    ['yotsuba-b','rgb(17, 119, 67)','rgb(204, 17, 5)','12px'],
    ['futaba','rgb(17, 119, 67)','rgb(204, 17, 5)','16px'],
    ['burichan','rgb(17, 119, 67)','rgb(204, 17, 5)','16px'],
    ['photon','rgb(0, 74, 153)','rgb(17, 17, 17)','12px'],
    ['tomorrow','rgb(197, 200, 198)','rgb(178, 148, 187)','12px'],
  ]) {
    await context.addCookies([{name:'board-theme',value:theme,url:origin.origin}]);
    await page.goto(origin.origin+'/f/');
    const row=page.locator(`table.flashListing tr[data-thread="${id}"]`);
    await expect(row.locator('.name')).toHaveCSS('color',name);
    await expect(row.locator('.subject')).toHaveCSS('color',subject);
    await expect(row.locator('td').first()).toHaveCSS('font-size',size);
    await assertUploadTableStyles(page,context,theme);
  }
  await context.addCookies([{name:'board-theme',value:'yotsuba',url:origin.origin}]);
  await page.goto(origin.origin+'/f/');
  const row=page.locator(`table.flashListing tr[data-thread="${id}"]`);
  await expect(row.getByRole('link',{name:'Embed',exact:true})).toBeVisible();
  await expect(page.locator('#swf-embed')).toHaveCount(0);
  await row.getByRole('link',{name:'Embed',exact:true}).click();
  await expect(page.locator('#swf-embed iframe')).toHaveAttribute('sandbox','allow-scripts');
  await page.locator('#swf-embed-close').click();await expect(page.locator('#swf-embed')).toHaveCount(0);
  await page.locator('#togglePostFormLink a').click();
  await expect(page.locator('#postFile')).toBeVisible();
  await expect(page.locator('form.postForm[action="/f/upload"]')).toBeHidden();
  await page.locator('#sub').fill('Owned one-submit form');await page.locator('#com').fill('Owned upload-board body');
  await page.locator('#filetag').selectOption('4');await page.locator('#postFile').setInputFiles({name:'owned-form.swf',mimeType:'application/x-shockwave-flash',buffer:Buffer.from('FWS-owned-fixture')});
  assert.equal(uploaded,0);assert.equal(posted,0);
  await page.locator('#reply button[type=submit]').click();
  await page.waitForURL(origin.origin+`/f/thread/${id}#p${id}`);assert.equal(uploaded,1);assert.equal(posted,1);
  await page.locator('[data-swf-toggle]').click();await expect(page.locator('#swf-embed iframe')).toHaveCount(1);
  await page.locator('[data-swf-toggle]').click();await expect(page.locator('#swf-embed')).toHaveCount(0);
  await expect(page.locator('form.postForm[action="/f/upload"]')).toHaveCount(0);
  const mobile=await browser.newContext({viewport:{width:390,height:844},isMobile:true,hasTouch:true});
  await mobile.route('http://localhost:3002/player/**',route=>route.fulfill({status:200,contentType:'text/html',body:'<!doctype html><title>Owned mobile fixture</title>'}));
  const phone=await mobile.newPage();await phone.goto(origin.origin+'/f/');
  await expect(phone.locator('table.flashListing a').filter({hasText:/^Embed$/})).toHaveCount(0);
  await assertUploadTableStyles(phone,mobile,'yotsuba','mobile');
  await phone.goto(origin.origin+`/f/thread/${id}`);await phone.locator('[data-swf-toggle]').click();
  const frame=phone.locator('#swf-embed iframe');await expect(frame).toHaveCount(1);assert.ok(Number(await frame.getAttribute('width'))<=360);
  await phone.locator('[data-swf-toggle]').click();await expect(phone.locator('#swf-embed')).toHaveCount(0);
  console.log('Upload-board desktop/mobile controls and mocked one-submit orchestration passed.');
} finally {await browser.close();}
