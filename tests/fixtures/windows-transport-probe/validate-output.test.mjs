import test from 'node:test';
import assert from 'node:assert/strict';
import { validateOutput, MAX_BYTES } from './validate-output.mjs';
function sample(failed = false) {
  const records = [{ type:'header',schema:1,mode:'plain',batches:1,width:6,interval_ms:50,total_cap_ms:30000,request:'owned-readyz' }];
  const op = (id,stage,result=0,error=0) => records.push({type:'operation',id,stage,result,error,ms:0});
  for (let id=0; id<6; id++) {
    for(const stage of ['socket','nonblocking','nodelay','keepalive','event-create','write-event-create','event-select-connect','set-randomize','get-randomize']) op(id,stage);
    records.push({type:'option',id,value:false,length:4,ms:0});op(id,'binding-before');
    if (failed && id===0) { op(id,'connect',-1,10055); op(id,'binding-after',-1,10022); records.push({type:'failure',id,stage:'connect-sync',error:10055,ms:0}); }
    else { op(id,'connect',-1,10035); op(id,'binding-after',1); for(const stage of ['connect-wait','connect-enumerate','connect-async']) op(id,stage); op(id,'http-send',80); op(id,'http-recv',70); op(id,'http-complete'); }
    op(id,'shutdown'); op(id,'closesocket'); op(id,'event-close'); op(id,'write-event-close');
  }
  op(6,'wsa-cleanup');
  records.push({type:'summary',attempts:6,successes:failed?5:6,failures:failed?1:0,sockets_opened:6,sockets_closed:6,events_opened:12,events_closed:12,operation_records:records.filter(row=>row.type==='operation').length,complete:true,elapsed_ms:0});
  return records;
}
const encode = records => records.map(row=>JSON.stringify(row)).join('\n')+'\n';
test('valid complete aggregate is admitted',()=>assert.equal(validateOutput(encode(sample())).passed,true));
test('10055 remains a failing result',()=>{ const result=validateOutput(encode(sample(true))); assert.equal(result.passed,false); assert.equal(result.error10055,1); });
for (const [name, mutate] of [
  ['unknown exported field',r=>r[0].url='private'],
  ['unknown stage',r=>r[1].stage='unknown'],
  ['unsupported mode',r=>r[0].mode='retry'],
  ['oversized workload',r=>r[0].batches=129],
  ['counter mismatch',r=>r.at(-1).failures=1],
  ['inconsistent completeness',r=>r[0].batches=2],
  ['duplicate ownership',r=>r.find(row=>row.stage==='socket'&&row.id===1).id=0],
  ['unowned close',r=>r.find(row=>row.stage==='closesocket').id=99],
  ['missing cleanup',r=>r.splice(-2,1)],
  ['extra payload',r=>r[1].payload='secret'],
  ['negative time',r=>r[1].ms=-1],
  ['hidden option failure',r=>r.find(row=>row.stage==='set-randomize').error=10022],
  ['missing variant observation',r=>r.find(row=>row.stage==='get-randomize').stage='io-select'],
  ['hidden connect failure',r=>r.find(row=>row.stage==='connect').error=10055],
  ['hidden cleanup failure',r=>r.find(row=>row.stage==='wsa-cleanup').result=-1],
  ['wrong observed option',r=>r.find(row=>row.type==='option').value=true],
  ['wrong option length',r=>r.find(row=>row.type==='option').length=8],
  ['missing actual option value',r=>r.find(row=>row.type==='option').value=null],
  ['reported success after deadline',r=>r.at(-1).elapsed_ms=40000],
  ['close before create',r=>{const index=r.findIndex(row=>row.stage==='closesocket'); const [row]=r.splice(index,1);r.splice(1,0,row);}],
  ['close after global cleanup',r=>{const index=r.findIndex(row=>row.stage==='closesocket');const [row]=r.splice(index,1);r.splice(-1,0,row);}],
  ['extra io-select 10055 operation',r=>{r.splice(-2,0,{type:'operation',id:0,stage:'io-select',result:-1,error:10055,ms:0});r.at(-1).operation_records++;}],
  ['io-select hidden in valid HTTP phase',r=>{const index=r.findIndex(row=>row.stage==='http-send');r.splice(index,0,{type:'operation',id:0,stage:'io-select',result:-1,error:10055,ms:0});r.at(-1).operation_records++;}],
  ['missing get-randomize readback row',r=>{r.splice(r.findIndex(row=>row.type==='option'),1);}],
  ['connect phase exceeds one second',r=>{const index=r.findIndex(row=>row.stage==='connect-wait');for(const row of r.slice(index,-1))row.ms=1001;r.at(-1).elapsed_ms=1001;}],
  ['HTTP phase exceeds one second',r=>{const index=r.findIndex(row=>row.stage==='http-recv');for(const row of r.slice(index,-1))row.ms=1001;r.at(-1).elapsed_ms=1001;}],
  ['receive before request send',r=>{const a=r.findIndex(row=>row.stage==='http-send'),b=r.findIndex(row=>row.stage==='http-recv');[r[a],r[b]]=[r[b],r[a]];}],
  ['option observed before getsockopt',r=>{const a=r.findIndex(row=>row.type==='option');[r[a-1],r[a]]=[r[a],r[a-1]];}],
  ['wrong result/error pairing',r=>{r.find(row=>row.stage==='nonblocking').result=-1;}],
]) test(name,()=>{const records=sample();mutate(records);assert.throws(()=>validateOutput(encode(records)));});
test('verified randomized mode passes',()=>{const r=sample();r[0].mode='randomized';for(const row of r.filter(row=>row.type==='option'))row.value=true;assert.equal(validateOutput(encode(r)).passed,true);});
test('actual select 10055 with failure record never passes',()=>{
  const r=sample().filter(row=>!(row.id===0 && ['http-send','http-recv','http-complete'].includes(row.stage)));
  const index=r.findIndex(row=>row.id===0 && row.stage==='shutdown');
  r.splice(index,0,{type:'operation',id:0,stage:'io-select',result:-1,error:10055,ms:0},{type:'failure',id:0,stage:'io-select',error:10055,ms:0});
  r.at(-1).operation_records=r.filter(row=>row.type==='operation').length;r.at(-1).failures=1;r.at(-1).successes=5;
  const result=validateOutput(encode(r));assert.equal(result.passed,false);assert.equal(result.error10055,1);
});
test('incomplete output rejected',()=>assert.throws(()=>validateOutput(encode(sample()).trimEnd())));
test('size cap enforced',()=>assert.throws(()=>validateOutput('x'.repeat(MAX_BYTES+1)+'\n')));
test('missing summary rejected',()=>assert.throws(()=>validateOutput(encode(sample().slice(0,-1)))));
test('valid incomplete schedule is not a pass',()=>{const r=sample();r[0].batches=2;r.at(-1).complete=false;assert.equal(validateOutput(encode(r)).passed,false);});
