import test from 'node:test';
import assert from 'node:assert/strict';
import { createReplayPlayback } from '../../apps/public/client/native-replay-playback.js';
function deferred() {let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};}
function setup() {
  let clock=0, callback;
  const sessions=[], states=[], frames=[];
  const controller=createReplayPlayback(new ArrayBuffer(288),{
    frame:value=>frames.push(value),changed:value=>states.push(value),now:()=>clock,
    schedule:fn=>{callback=fn;return 1;},unschedule:()=>{callback=null;},
    sessionFactory:()=>{
      const init=deferred(),calls=[];
      const session={ready:init.promise,init,calls,closed:0,close(){this.closed++;},advance(until){const value=deferred();calls.push({until,...value});return value.promise;}};
      sessions.push(session);return session;
    },
  });
  const response=(index=0,next=100)=>({index,count:3,next,bitmap:{closed:0,close(){this.closed++;}}});
  return {controller,sessions,states,frames,response,clock:value=>{clock=value;},tick:()=>callback?.()};
}
test('pause, speed and gapless controls preserve source event scheduling',async()=>{
  const f=setup();const init=f.response();f.sessions[0].init.resolve(init);await f.controller.ready;
  assert.equal(init.bitmap.closed,1);assert.equal(f.states.at(-1).phase,'paused');
  f.controller.play();const tick=f.tick();assert.equal(f.sessions[0].calls[0].until,100);
  const first=f.response(1,300);f.sessions[0].calls[0].resolve(first);await tick;
  f.clock(50);f.controller.pause();f.clock(999);f.controller.speed(2);f.controller.gapless(false);f.controller.play();
  f.clock(1024);await f.tick();assert.equal(f.sessions[0].calls.length,1,'No worker command before the next source event');
  f.clock(1100);const finish=f.tick();assert.equal(f.sessions[0].calls[1].until,352);
  f.sessions[0].calls[1].resolve(f.response(3,null));await finish;
  assert.equal(f.states.at(-1).phase,'done');f.controller.close();
});
test('rewind uses a fresh worker and closes late frames without displaying them',async()=>{
  const f=setup();f.sessions[0].init.resolve(f.response());await f.controller.ready;f.controller.play();
  const running=f.tick();const reset=f.controller.rewind();assert.equal(f.sessions[0].closed,1);
  const late=f.response(2,1000);f.sessions[0].calls[0].resolve(late);await running;
  assert.equal(late.bitmap.closed,1);assert.equal(f.frames.length,1);
  f.sessions[1].init.resolve(f.response());await reset;assert.equal(f.frames.length,2);
  assert.equal(f.states.at(-1).phase,'paused');f.controller.close();
});
test('close during initialization discards the bitmap and suppresses later state updates',async()=>{
  const f=setup();f.controller.close();const count=f.states.length,late=f.response();
  f.sessions[0].init.resolve(late);await f.controller.ready;
  assert.equal(late.bitmap.closed,1);assert.equal(f.frames.length,0);assert.equal(f.states.length,count);
});
test('worker failure enters a terminal visible state and cannot resume',async()=>{
  const f=setup();f.sessions[0].init.reject(new Error('deadline'));await f.controller.ready;
  assert.equal(f.states.at(-1).phase,'failed');f.controller.play();assert.equal(f.sessions.length,1);f.controller.close();
});
