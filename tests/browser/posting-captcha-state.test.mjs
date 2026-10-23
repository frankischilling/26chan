import test from 'node:test';
import assert from 'node:assert/strict';
import { createPostingCaptcha,mountPostingCaptcha } from '../../apps/public/client/posting-captcha-core.js';
import { environment,settle } from '../helpers/captcha-dom.mjs';
const token='c1_'+'ab'.repeat(32),nonce='cd'.repeat(32),ticket='t1_'+'ef'.repeat(32),png='data:image/png;base64,iVBORw0KGgo=';
const reply=(challenge,extra={})=>({csrf:nonce,ticket_challenge:false,report_exempt:false,challenge,...extra});
const ready=(value=token)=>reply({state:'ready',token:value,image:png,background:png,slider:true,offset_max:80,ttl_seconds:120,cooldown_seconds:5});
const response=value=>new Response(JSON.stringify(value),{headers:{'content-type':'application/json'}});
function fixture(fetcher,purpose='reply') {const e=environment(fetcher),form=e.doc.createElement('form');const root=createPostingCaptcha(form,{board:'b',target:purpose==='thread'?'0':'10',purpose},e.doc);const api=mountPostingCaptcha(root,{win:e.win});return {...e,form,root,api,field:name=>root.querySelector(`[name="${name}"]`)};}
test('loading is on demand and expiry clears all successful proof controls',async()=>{
  const calls=[];const f=fixture(async(url,init)=>{calls.push([url,new URLSearchParams(init.body)]);return response(ready());});
  assert.equal(calls.length,0);assert.equal(f.api.canSubmit(),false);await f.api.load();assert.equal(calls.length,1);assert.equal(calls[0][0],'/captcha');assert.equal(calls[0][1].get('action'),'init');assert.equal(calls[0][1].get('target'),'10');
  assert.equal(f.field('captcha-csrf').value,nonce);assert.equal(f.field('t-challenge').value,token);assert.equal(f.api.canSubmit(),false);f.field('t-response').value='ABC123';assert.equal(f.api.canSubmit(),true);
  f.advance(117000);assert.equal(f.field('t-challenge').value,'');assert.equal(f.field('t-response').value,'');assert.equal(f.api.canSubmit(),false);f.api.dispose();assert.equal(f.timers.size,0);
});
test('saved tickets resume on init and waiting resumes only challenge issuance',async()=>{
  const calls=[];const f=fixture(async(url,init)=>{calls.push(new URLSearchParams(init.body));return response(calls.length===1?reply({state:'wait',seconds:2,ticket}):ready());});
  // Mount reads the per-board ticket from storage, so remount after populating it.
  f.api.dispose();f.storage.set('26chan:captcha-ticket:b',ticket);const api=mountPostingCaptcha(f.root,{win:f.win});
  await api.load();assert.equal(calls[0].get('action'),'init');assert.equal(calls[0].get('ticket'),ticket);assert.equal(api.canSubmit(),false);
  f.advance(2000);await settle();assert.equal(calls.length,2);assert.equal(calls[1].get('action'),'challenge');assert.equal(calls[1].get('ticket'),ticket);assert.equal(calls[1].get('csrf'),nonce);assert.equal(f.field('t-challenge').value,token);api.dispose();
});
test('ticket answers never become posting proofs or automatic post submissions',async()=>{
  const calls=[];const f=fixture(async(url,init)=>{calls.push([url,new URLSearchParams(init.body)]);return response(calls.length===1?{...ready(),ticket_challenge:true}:reply({state:'wait',seconds:900,ticket}));},'thread');
  await f.api.load();f.field('t-response').value='ABCDEFGH';assert.equal(f.api.canSubmit(),false);f.advance(5000);await f.api.load();
  assert.equal(calls.length,2);assert.ok(calls.every(([url])=>url==='/captcha'));assert.equal(calls[1][1].get('ticket_challenge'),token);assert.equal(calls[1][1].get('ticket_answer'),'ABCDEFGH');assert.equal(f.field('t-challenge').value,'');assert.equal(f.field('t-response').value,'');assert.equal(f.storage.get('26chan:captcha-ticket:b'),ticket);f.api.dispose();
});
test('credit and report-exempt states need no answer but do not survive expiry or page restore',async()=>{
  const f=fixture(async()=>response(reply({state:'credits',remaining:1,ttl_seconds:20})));await f.api.load();assert.equal(f.field('captcha-credit').value,'1');assert.equal(f.api.canSubmit(),true);f.advance(17000);assert.equal(f.field('captcha-credit').value,'');assert.equal(f.api.canSubmit(),false);f.api.dispose();
  const report=fixture(async()=>response(reply({state:'disabled'},{report_exempt:true})),'report');await report.api.load();assert.equal(report.root.hidden,true);assert.equal(report.api.canSubmit(),true);report.win.emit('pageshow',{persisted:true});assert.equal(report.root.hidden,false);assert.equal(report.field('captcha-csrf').value,'');assert.equal(report.api.canSubmit(),false);report.api.dispose();
});
test('retargeting and disposal reject late responses without touching the current form',async()=>{
  const pending=[],calls=[];const f=fixture((url,init)=>{calls.push(init);return new Promise(resolve=>pending.push(resolve));});
  const first=f.api.load();await settle();assert.equal(f.api.setTarget('11'),true);assert.equal(calls[0].signal.aborted,true);const second=f.api.load();await settle();const next='c1_'+'12'.repeat(32);
  pending[1](response(ready(next)));await second;pending[0](response(ready()));await first;assert.equal(f.field('t-challenge').value,next);assert.equal(new URLSearchParams(calls[1].body).get('target'),'11');
  f.api.dispose();assert.equal(f.field('t-challenge').value,'');assert.equal(f.timers.size,0);assert.equal(f.form.emit('submit').defaultPrevented,false);
});
test('failures clear proof and enforce source retry delay without retrying automatically',async()=>{
  let count=0;const f=fixture(async()=>{count++;throw new Error('private network detail');});await f.api.load();assert.equal(f.field('t-challenge').value,'');assert.equal(f.api.canSubmit(),false);await f.api.load();assert.equal(count,1);f.advance(59999);await f.api.load();assert.equal(count,1);f.advance(1);assert.equal(count,1);await f.api.load();assert.equal(count,2);assert.ok(!f.root.querySelector('.captchaMessage').textContent.includes('private network'));f.api.dispose();
});

test('first main and Quick Reply initialization share the cookie-establishing sequence',async()=>{
  const pending=[],calls=[];const e=environment((url,init)=>{calls.push(new URLSearchParams(init.body));return new Promise(resolve=>pending.push(resolve));});
  const firstForm=e.doc.createElement('form'), secondForm=e.doc.createElement('form');
  const firstRoot=createPostingCaptcha(firstForm,{board:'b',target:'0',purpose:'thread'},e.doc),secondRoot=createPostingCaptcha(secondForm,{board:'b',target:'10',purpose:'reply',prefix:'qr-'},e.doc);
  const first=mountPostingCaptcha(firstRoot,{win:e.win}),second=mountPostingCaptcha(secondRoot,{win:e.win,interceptSubmit:false});
  const a=first.load();await settle();const b=second.load();await settle();assert.equal(calls.length,1);
  pending[0](response(ready()));await a;await settle();assert.equal(calls.length,2);assert.equal(calls[1].get('target'),'10');
  pending[1](response(ready()));await b;second.setBusy(true);assert.equal(secondForm.emit('submit').defaultPrevented,false,'QR owns its busy/cancel submit action');
  first.dispose();second.dispose();assert.equal(e.timers.size,0);
});
test('a fetch implementation ignoring abort cannot leave initialization or disposal pending',async()=>{
  const f=fixture(()=>new Promise(()=>{}));const request=f.api.load();await settle();f.advance(12000);await request;assert.equal(f.api.canSubmit(),false);assert.equal(f.field('t-challenge').value,'');f.api.dispose();assert.equal(f.timers.size,0);
});

test('server-rendered classic hydration preserves typed answers and focus',()=>{
  const e=environment(()=>{throw new Error('Classic hydration must not fetch');});const form=e.doc.createElement('form');
  const root=createPostingCaptcha(form,{board:'f',target:'0',purpose:'thread',classic:true},e.doc);root.dataset.ttl='120';
  root.querySelector('[name="captcha-csrf"]').value=nonce;root.querySelector('[name="t-challenge"]').value=token;root.querySelector('[name="t-response"]').value='ABC123';root.querySelector('.captchaForeground').src=png;
  const comment=e.doc.createElement('textarea');comment.focus();const api=mountPostingCaptcha(root,{win:e.win});assert.equal(api.canSubmit(),true);assert.equal(root.querySelector('[name="t-response"]').value,'ABC123');assert.equal(e.doc.activeElement,comment);api.dispose();
  root.dataset.noscriptOnly='true';assert.equal(mountPostingCaptcha(root,{win:e.win}),null);
});
test('refresh cooldown ticks do not report proof changes that would cancel armed Quick Reply',async()=>{
  const e=environment(async()=>response(ready())),form=e.doc.createElement('form');
  const root=createPostingCaptcha(form,{board:'b',target:'10',purpose:'reply'},e.doc),events=[];
  const api=mountPostingCaptcha(root,{win:e.win,changed:proofChanged=>events.push(proofChanged)});
  await api.load();assert.ok(events.includes(true));events.length=0;
  e.advance(4000);assert.ok(events.length>0);assert.ok(events.every(value=>value===false));
  events.length=0;api.invalidate();assert.ok(events.includes(true));api.dispose();
});

test('only current disabled CAPTCHA advice selects the presentation-only cooldown bypass', async () => {
  const f=fixture(async()=>response(reply({state:'disabled'})));
  assert.equal(f.api.cooldownBypass(),false); await f.api.load(); assert.equal(f.api.cooldownBypass(),true);
  f.api.invalidate(); assert.equal(f.api.cooldownBypass(),false); await f.api.load(); assert.equal(f.api.cooldownBypass(),true);
  f.api.dispose(); assert.equal(f.api.cooldownBypass(),false);
  const report=fixture(async()=>response(reply({state:'disabled'},{report_exempt:true})),'report');
  await report.api.load(); assert.equal(report.api.cooldownBypass(),false); report.api.dispose();
  const credit=fixture(async()=>response(reply({state:'credits',remaining:1,ttl_seconds:60})));
  await credit.api.load(); assert.equal(credit.api.cooldownBypass(),false); credit.api.dispose();
});

test('a Pass presentation cookie requests verified advice but never grants its own bypass', async () => {
  for (const accepted of [false,true]) {
    let calls=0;
    const e=environment(async()=>{calls++;return response(accepted?reply({state:'disabled'}):ready());});
    e.win.document=e.doc; e.doc.cookie='pass_enabled=1';
    const form=e.doc.createElement('form'); const root=createPostingCaptcha(form,{board:'b',target:'10',purpose:'reply'},e.doc);
    const api=mountPostingCaptcha(root,{win:e.win});
    assert.equal(api.cooldownBypass(),false); await settle();
    assert.equal(calls,1); assert.equal(api.cooldownBypass(),accepted);
    if (accepted) { api.afterSubmit(); await settle(); assert.equal(calls,2); assert.equal(api.cooldownBypass(),true); }
    api.dispose();
  }
});


test('native Pass submission waits for delayed issuer advice even with an existing form nonce', async () => {
  let finish;
  const e = environment(() => new Promise(resolve => { finish = resolve; }));
  e.win.document = e.doc; e.doc.cookie = 'pass_enabled=1';
  const form = e.doc.createElement('form');
  const root = createPostingCaptcha(form, { board: 'b', target: '0', purpose: 'thread' }, e.doc);
  root.querySelector('[name="captcha-csrf"]').value = nonce;
  const api = mountPostingCaptcha(root, { win: e.win });
  await settle();
  assert.equal(root.querySelector('[name="captcha-csrf"]').value, nonce);
  assert.equal(root.querySelector('button[id$="t-load"]').disabled, true);
  assert.equal(form.emit('submit').defaultPrevented, true);
  finish(response(reply({ state: 'disabled' })));
  await settle();
  assert.equal(root.querySelector('.captchaMessage').textContent, '');
  assert.equal(root.querySelector('button[id$="t-load"]').disabled, false);
  assert.equal(root.hidden, true);
  assert.equal(form.emit('submit').defaultPrevented, false);
  api.dispose();
});
