import { test, expect } from '@playwright/test';
import { readFile, mkdir } from 'node:fs/promises';
test.skip(process.env.VERIFICATION_BROWSER_FIXTURE !== '1', 'Uses the explicit verification UI fixture.');
const images = await Promise.all(['foreground','background'].map(async name => 'data:image/png;base64,' + (await readFile(new URL(`../fixtures/verification-render/${name}.png`,import.meta.url))).toString('base64')));
const challenge = () => ({state:'ready',token:`c1_${'ab'.repeat(32)}`,image:images[0],background:images[1],ttl_seconds:120,cooldown_seconds:5,slider:true,offset_max:80});
async function mockChallenge(page, response = challenge()) {
  const calls=[];
  await page.route('**/signin/challenge',async route=>{calls.push(route.request());await route.fulfill({contentType:'application/json',body:JSON.stringify(response)});});
  return calls;
}
async function screenshot(page,name) {
  await mkdir('.local/verification-review',{recursive:true});
  await page.screenshot({path:`.local/verification-review/${name}.png`,fullPage:true});
}
for(const mobile of [false,true]) {
  test(`production sign-in controls, native keyboard slider and headers ${mobile?'mobile':'desktop'}`,async({page})=>{
    await page.setViewportSize({width:mobile?390:1280,height:900});
    await page.addInitScript(()=>{window.violations=[];document.addEventListener('securitypolicyviolation',e=>violations.push(e.violatedDirective));});
    const calls=await mockChallenge(page);const response=await page.goto('/signin');
    expect(response.headers()['referrer-policy']).toBe('strict-origin');
    expect(response.headers()['content-security-policy']).toContain('connect-src http://127.0.0.1:3000/signin/challenge;');
    await expect(page.locator('#verification-submit')).toBeDisabled();expect(calls.length).toBe(0);
    await screenshot(page,`request-${mobile?'mobile':'desktop'}`);
    await page.getByRole('button',{name:'Get Captcha',exact:true}).click();
    await expect(page.locator('#t-resp')).toBeVisible();await expect(page.locator('#t-fg')).toHaveJSProperty('naturalWidth',240);
    await expect(page.locator('#t-bg')).toHaveJSProperty('naturalWidth',320);
    expect(calls.length).toBe(1);const sent=new URLSearchParams(calls[0].postData());expect([...sent.keys()]).toEqual(['csrf']);
    expect(calls[0].headers().origin).toBe('http://127.0.0.1:3000');
    const slider=page.getByRole('slider',{name:'Align the image'});await slider.focus();await page.keyboard.press('ArrowRight');
    await expect(slider).toHaveValue('1');await expect(page.locator('#t-bg')).toHaveCSS('transform','matrix(1, 0, 0, 1, -0.8, 0)');
    await screenshot(page,`challenge-${mobile?'mobile':'desktop'}`);
    expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
    expect(await page.evaluate(()=>violations)).toEqual([]);
  });
}
test('expiry and persisted restoration clear proof without reloading',async({page})=>{
  await page.clock.install();const calls=await mockChallenge(page);await page.goto('/fixture/verify');
  await expect(page.locator('#t-challenge')).toHaveValue(challenge().token);expect(calls.length).toBe(1);
  await page.clock.fastForward(117000);await expect(page.locator('#t-challenge')).toHaveValue('');await expect(page.locator('#verification-submit')).toBeDisabled();
  await expect(page.locator('#t-msg')).toContainText('expired');expect(calls.length).toBe(1);
  await page.getByRole('button',{name:'Get Captcha',exact:true}).click();await expect(page.locator('#t-challenge')).toHaveValue(challenge().token);
  await page.evaluate(()=>window.dispatchEvent(new PageTransitionEvent('pageshow',{persisted:true})));
  await expect(page.locator('#t-challenge')).toHaveValue('');await expect(page.locator('#t-frame')).toBeHidden();expect(calls.length).toBe(2);
});
test('native verification submission retains proof and sends same-origin headers',async({page})=>{
  await mockChallenge(page);await page.goto('/fixture/verify');await expect(page.locator('#t-resp')).toBeVisible();await page.locator('#t-resp').fill('JP22XR');
  let submitted;
  await page.route('**/signin',async route=>{submitted=route.request();await route.fulfill({contentType:'text/html',body:'<p>Owned submission received.</p>'});});
  await page.getByRole('button',{name:'Verify',exact:true}).click();await expect(page.getByText('Owned submission received.')).toBeVisible();
  expect(submitted.method()).toBe('POST');expect(submitted.headers().origin).toBe('http://127.0.0.1:3000');
  const form=new URLSearchParams(submitted.postData());expect(form.get('action')).toBe('verify');expect(form.get('t-challenge')).toBe(challenge().token);expect(form.get('t-response')).toBe('JP22XR');
});
test('preview, logout and completion are readable native forms',async({page})=>{
  for(const mode of ['preview','logout','message']) {
    await page.goto(`/fixture/${mode}`);await screenshot(page,mode);
    await expect(page.locator('h1')).toHaveText('Email Verification');
    if(mode==='preview')await expect(page.getByRole('button',{name:'Continue verification'})).toBeVisible();
    if(mode==='logout')await expect(page.getByRole('button',{name:'Logout'})).toBeVisible();
    if(mode==='message')await expect(page.getByRole('status')).toHaveText('This session is now verified.');
  }
});
