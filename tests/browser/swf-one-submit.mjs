// Actual browser form with page scripts disabled. The Rust supervisor alone
// owns intake, publication and database credentials.
import assert from 'node:assert/strict';
import path from 'node:path';
import { chromium,expect } from '@playwright/test';
const origin=new URL(process.argv[2]),source=process.argv[3];
assert.equal(origin.hostname,'127.0.0.1');assert.equal(origin.protocol,'http:');assert.ok(path.isAbsolute(source));assert.match(path.basename(source),/^owned-browser-[0-9a-f]{32}\.swf$/);
for(const name of Object.keys(process.env))assert.ok(!name.endsWith('DATABASE_URL')&&!['POSTER_ID_KEY','PUBLIC_INTAKE_TOKEN'].includes(name));
const browser=await chromium.launch({timeout:15000});
try {
  const context=await browser.newContext({javaScriptEnabled:false,viewport:{width:1280,height:900}});
  const page=await context.newPage();page.setDefaultTimeout(15000);
  const writes=[];page.on('request',request=>{if(request.method()==='POST')writes.push(new URL(request.url()).pathname);});
  await page.goto(origin.origin+'/f/');
  await expect(page.locator('#postFile')).toBeVisible();
  await expect(page.locator('form[action="/f/upload"]')).toHaveCount(0);
  await page.locator('#name').fill('Owned no-script browser');await page.locator('#sub').fill('Owned no-script movie');await page.locator('#com').fill('Owned one-submit browser without page scripts');
  await page.locator('#filetag').selectOption('4');await page.locator('#postFile').setInputFiles(source);
  await page.locator('#reply button[type=submit]').click();
  await page.waitForURL(/\/f\/thread\/[1-9][0-9]*(?:\/[^#]*)?#p[1-9][0-9]*$/);
  assert.deepEqual(writes,['/f/imgboard.php']);
  await expect(page.locator('.post.op .postMessage')).toContainText('Owned one-submit browser without page scripts');
  await expect(page.locator('.post.op .fileText')).toContainText(path.basename(source));
  await expect(page.locator('form[action="/f/upload"]')).toHaveCount(0);
  console.log('Source SWF form posted through actual intake/publication in one request with page scripts disabled.');
} finally {await browser.close();}
