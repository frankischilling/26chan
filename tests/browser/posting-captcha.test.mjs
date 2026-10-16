import test from 'node:test';
import assert from 'node:assert/strict';
import { validReply, validScope } from '../../apps/public/client/posting-captcha-core.js';
import { sendQuickReply } from '../../apps/public/client/native-quick-reply-transport.js';
const png = 'data:image/png;base64,iVBORw0KGgo=';
const ready = () => ({ csrf: 'ab'.repeat(32), ticket_challenge: false, report_exempt: false,
  challenge: { state: 'ready', token: `c1_${'cd'.repeat(32)}`, image: png, background: png, slider: true, offset_max: 80, ttl_seconds: 120, cooldown_seconds: 5 } });
test('CAPTCHA scope preserves large canonical IDs and rejects internal purposes', () => {
  for (const [board, target, purpose, valid] of [['b','0','thread',true],['b','9223372036854775807','reply',true],['f','1','report',true],['b','01','reply',false],['b','9223372036854775808','reply',false],['B','1','reply',false],['b','1','ticket',false],['b','0','email_request',false],['b','0','report',false],['b',1,'reply',false],[['b'],'1','reply',false]]) assert.equal(validScope(board,target,purpose),valid);
});
test('ordinary and ticket challenges accept only bounded raster images and lifetimes', () => {
  assert.ok(validReply(ready()));
  const ticket=ready(); ticket.ticket_challenge=true; assert.ok(validReply(ticket));
  for (const patch of [{token:['c1_'+ 'ab'.repeat(32)]},{image:'data:image/svg+xml;base64,abcd'},{image:'https://example.test/image.png'},{background:null},{ttl_seconds:121},{ttl_seconds:3},{cooldown_seconds:301},{offset_max:81},{slider:'true'}]) assert.equal(validReply({...ready(),challenge:{...ready().challenge,...patch}}),false);
  const classic=ready();classic.challenge.slider=false;classic.challenge.background=null;assert.ok(validReply(classic));
  assert.equal(validReply({...ready(),csrf:['ab'.repeat(32)]}),false);
  assert.equal(validReply({...ready(),report_exempt:true}),false);
  const large=ready();large.challenge.image='data:image/png;base64,'+'A'.repeat(87384);assert.equal(validReply(large),false);
});
test('waiting, credit and explicit exemption states remain distinct', () => {
  const base={csrf:'ab'.repeat(32),ticket_challenge:false,report_exempt:false};
  assert.ok(validReply({...base,challenge:{state:'wait',seconds:900,ticket:'t1_'+'ef'.repeat(32)}}));
  assert.ok(validReply({...base,challenge:{state:'wait',seconds:1,ticket:null}}));
  for(const seconds of [0,901,-1,1.5,'1']) assert.equal(validReply({...base,challenge:{state:'wait',seconds,ticket:null}}),false);
  assert.equal(validReply({...base,challenge:{state:'wait',seconds:1,ticket:['t1_'+'ef'.repeat(32)]}}),false);
  assert.ok(validReply({...base,challenge:{state:'credits',remaining:3,ttl_seconds:120}}));
  for(const patch of [{remaining:0},{remaining:4},{ttl_seconds:0},{ttl_seconds:121}]) assert.equal(validReply({...base,challenge:{state:'credits',remaining:1,ttl_seconds:60,...patch}}),false);
  assert.ok(validReply({...base,report_exempt:true,challenge:{state:'disabled'}}));
  assert.equal(validReply({...base,ticket_challenge:true,challenge:{state:'credits',remaining:1,ttl_seconds:60}}),false);
});
test('Quick Reply sends only bounded CAPTCHA fields alongside the ordinary form', async () => {
  let sent;
  const fetcher=async(url,init)=>{sent=init.body;return new Response(JSON.stringify({tid:10,pid:11}),{headers:{'content-type':'application/json'}});};
  await sendQuickReply({board:'b',thread:'10',origin:'https://board.example',fields:{com:'Owned post',pwd:'owned-password','captcha-csrf':'ab'.repeat(32),'t-challenge':'c1_'+'cd'.repeat(32),'t-response':'ABC123','captcha-credit':'',peer_hash:'not-authority'},fetcher});
  assert.equal(sent.get('captcha-csrf'),'ab'.repeat(32));assert.equal(sent.get('t-challenge'),'c1_'+'cd'.repeat(32));assert.equal(sent.get('t-response'),'ABC123');assert.equal(sent.has('peer_hash'),false);
  for(const [field,length] of [['captcha-csrf',65],['t-challenge',68],['t-response',65],['captcha-credit',2]]) await assert.rejects(()=>sendQuickReply({board:'b',thread:'10',origin:'https://board.example',fields:{com:'Owned',pwd:'owned-password',[field]:'a'.repeat(length)},fetcher}),/Invalid CAPTCHA/);
});
