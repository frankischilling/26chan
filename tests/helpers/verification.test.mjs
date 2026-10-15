import test from 'node:test';
import assert from 'node:assert/strict';
import { mountVerification, validChallenge } from '../../apps/public/client/verification.js';
const ready = () => ({ state: 'ready', token: `c1_${'ab'.repeat(32)}`, image: 'data:image/png;base64,YQ==', background: 'data:image/png;base64,Yg==', slider: true, offset_max: 80, ttl_seconds: 120, cooldown_seconds: 5 });
function fixture(mode = 'request') {
  class Element extends EventTarget {
    value = ''; textContent = ''; hidden = false; disabled = false; style = {}; previousElementSibling = {};
    removeAttribute(name) { delete this[name]; } focus() { this.focused = true; }
  }
  const ids = ['auth-form', 't-frame', 't-fg', 't-bg', 't-slider', 't-load', 't-msg', 't-resp', 't-challenge', 'verification-submit', 't-help'];
  const nodes = Object.fromEntries(ids.map(id => [id, new Element()]));
  const form = nodes['auth-form']; form.dataset = { mode };
  form.elements = { namedItem: name => name === 'csrf' ? { value: 'a'.repeat(64) } : mode === 'verify' && name === 'tkn' ? { value: `e1_${'cd'.repeat(32)}` } : null };
  const win = new EventTarget(), timers = new Map(); let next = 1; const requests = [];
  win.setTimeout = (fn, delay) => { const id = next++; timers.set(id, { fn, delay }); return id; };
  win.clearTimeout = id => timers.delete(id);
  win.setInterval = () => next++; win.clearInterval = () => {}; win.alert = () => {};
  win.fetch = (url, options) => new Promise((resolve,reject) => requests.push({url,options,resolve,reject}));
  const mounted = mountVerification({ getElementById: id => nodes[id] }, win);
  const click = () => nodes['t-load'].dispatchEvent(new Event('click'));
  const resolve = async (value) => { requests.at(-1).resolve({ ok:true,headers:new Headers({'content-type':'application/json'}),text:async()=>JSON.stringify(value) }); await new Promise(setImmediate); };
  return {nodes, form, win, timers, requests, mounted, click, resolve};
}
test('accept only bounded canonical self-contained PNG challenge responses', () => {
  assert.equal(validChallenge(ready()),true);
  for (const patch of [{token:'e1_'+'ab'.repeat(32)},{image:'https://elsewhere.test/image.png'},{background:'data:image/svg+xml;base64,YQ=='},{image:'data:image/png;base64,'+'A'.repeat(65536)},{ttl_seconds:3},{ttl_seconds:121},{offset_max:81},{slider:'true'},{cooldown_seconds:301}]) {
    assert.equal(Boolean(validChallenge({...ready(),...patch})),false,JSON.stringify(patch).slice(0,100));
  }
  assert.equal(validChallenge({...ready(),slider:false,background:null}),true);
});
test('request form loads only on click and sends no email or browser authority', async () => {
  const f=fixture(); assert.equal(f.requests.length,0); assert.equal(f.nodes['verification-submit'].disabled,true);
  f.click(); f.click(); assert.equal(f.requests.length,1);
  const r=f.requests[0]; assert.equal(r.url,'/signin/challenge'); assert.equal(r.options.redirect,'error');
  assert.deepEqual([...r.options.body.keys()],['csrf']);
  await f.resolve(ready()); assert.equal(f.nodes['t-challenge'].value,ready().token); assert.equal(f.nodes['verification-submit'].disabled,false);
  f.nodes['t-slider'].value='50'; f.nodes['t-slider'].dispatchEvent(new Event('input')); assert.equal(f.nodes['t-bg'].style.transform,'translateX(-40px)');
  const expiry=[...f.timers.values()].find(v=>v.delay===117000); assert.ok(expiry); expiry.fn();
  assert.equal(f.nodes['t-challenge'].value,''); assert.equal(f.nodes['t-fg'].src,undefined); assert.equal(f.requests.length,1);
});
test('verification auto-loads once and late replies cannot restore a departed page', async () => {
  const f=fixture('verify'); assert.equal(f.requests.length,1); assert.ok(f.requests[0].options.body.get('tkn'));
  f.win.dispatchEvent(new Event('pagehide')); assert.equal(f.requests[0].options.signal.aborted,true);
  await f.resolve(ready()); assert.equal(f.nodes['t-challenge'].value,''); assert.equal(f.nodes['verification-submit'].disabled,true);
  const event=new Event('pageshow'); Object.defineProperty(event,'persisted',{value:true}); f.win.dispatchEvent(event); assert.equal(f.requests.length,1);
});
test('wait, malformed payload and failed fetch leave no usable proof', async () => {
  for(const result of [{state:'wait',seconds:30},{state:'disabled'}, {...ready(),token:'bad'}]) {
    const f=fixture(); f.click(); await f.resolve(result);
    assert.equal(f.nodes['t-challenge'].value,''); assert.equal(f.nodes['verification-submit'].disabled,true);
    assert.equal(f.requests.length,1);
    if(result.state==='wait')assert.equal(f.nodes['t-load'].textContent,'Wait 30s');
  }
});
test('native submit requires proof and never triggers a mutation fetch',async()=>{
  const f=fixture(); const absent=new Event('submit',{cancelable:true}); f.form.dispatchEvent(absent); assert.equal(absent.defaultPrevented,true);
  f.click(); await f.resolve(ready()); f.nodes['t-resp'].value='ADGH';
  const submit=new Event('submit',{cancelable:true}); f.form.dispatchEvent(submit); assert.equal(submit.defaultPrevented,false);
  assert.equal(f.nodes['verification-submit'].disabled,true); assert.equal(f.nodes['t-resp'].disabled,false); assert.equal(f.nodes['t-challenge'].disabled,false);
  assert.equal(f.requests.length,1);
});
test('an abandoned fetch timeout cannot abort the next lifecycle request',()=>{
  const f=fixture();f.click();const oldTimer=[...f.timers.values()].find(v=>v.delay===12000).fn;
  f.mounted.reset();f.click();assert.equal(f.requests.length,2);oldTimer();
  assert.equal(f.requests[0].options.signal.aborted,true);assert.equal(f.requests[1].options.signal.aborted,false);
});
