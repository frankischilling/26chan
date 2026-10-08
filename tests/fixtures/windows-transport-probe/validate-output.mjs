import fs from 'node:fs';
import { pathToFileURL } from 'node:url';
export const MAX_BYTES = 4 * 1024 * 1024;
const MAX_ELAPSED = 40000;
const ordered = ['socket','nonblocking','nodelay','keepalive','event-create','write-event-create','event-select-connect','set-randomize','get-randomize','option','binding-before','connect','binding-after','connect-wait','connect-enumerate','connect-async','http-send','http-recv','http-complete','shutdown','closesocket','event-close','write-event-close'];
const stages = new Set(['socket','nonblocking','nodelay','keepalive','event-create','write-event-create','event-select-connect','set-randomize','get-randomize','binding-before','binding-after','connect','connect-wait','connect-enumerate','connect-async','http-send','http-recv','http-complete','shutdown','closesocket','event-close','write-event-close','io-select','wsa-cleanup']);
const failureStages = new Set([...stages, 'verify-randomize','connect-sync','connect-event-missing','http-deadline','fixture-response','response-cap','total-deadline','ownership','output-cap']);
function check(condition, message) { if (!condition) throw new Error(message); }
function exact(record, keys) { check(Object.keys(record).sort().join() === [...keys].sort().join(), 'Unexpected fields'); }
function natural(value, cap = 1_000_000) { return Number.isSafeInteger(value) && value >= 0 && value <= cap; }
export function validateOutput(text) {
  check(typeof text === 'string' && Buffer.byteLength(text) <= MAX_BYTES && text.endsWith('\n'), 'Incomplete/oversized output');
  const lines = text.trimEnd().split(/\r?\n/);
  check(lines.length >= 2 && lines.length <= 100000, 'Record count');
  const records = lines.map(line => { check(line.length <= 1024, 'Line length'); return JSON.parse(line); });
  const header = records.shift(), summary = records.pop();
  exact(header, ['type','schema','mode','batches','width','interval_ms','total_cap_ms','request']);
  check(header.type === 'header' && header.schema === 1 && ['plain','randomized'].includes(header.mode) && natural(header.batches,128) && header.batches > 0 && header.width === 6 && header.interval_ms === 50 && header.total_cap_ms === 30000 && header.request === 'owned-readyz', 'Header invalid');
  exact(summary, ['type','attempts','successes','failures','sockets_opened','sockets_closed','events_opened','events_closed','operation_records','complete','elapsed_ms']);
  check(summary.type === 'summary' && typeof summary.complete === 'boolean', 'Summary invalid');
  for (const key of Object.keys(summary).filter(key => !['type','complete'].includes(key))) check(natural(summary[key]), 'Summary number');
  check(summary.attempts <= header.batches * header.width, 'Attempt cap');
  let operations = 0, failures = 0, previousMs = 0;
  const byId = new Map(), state = new Map(), observations = new Map();
  let cleanupSeen = false, liveSockets = 0, liveEvents = 0;
  const counts = new Map(), socketIds = new Set(), eventIds = new Set(), closedIds = new Set(), eventClosedIds = new Set(), successfulIds = new Set();
  for (const row of records) {
    exact(row, row.type === 'operation' ? ['type','id','stage','result','error','ms'] : row.type === 'option' ? ['type','id','value','length','ms'] : ['type','id','stage','error','ms']);
    check(['operation','failure','option'].includes(row.type) && natural(row.id, 768) && natural(row.ms, MAX_ELAPSED) && row.ms >= previousMs, 'Record invalid'); previousMs = row.ms;
    if (row.type === 'option') {
      const current=state.get(row.id);
      check(!cleanupSeen && current?.last==='get-randomize' && !observations.has(row.id), 'Option observation out of order');
      check((typeof row.value==='boolean' && Number.isInteger(row.length) && row.length>=-1 && row.length<=65535) || (row.value===null && row.length===null),'Invalid option observation');
      observations.set(row.id,row); current.last='option'; current.rank=ordered.indexOf('option'); continue;
    }
    check(natural(row.error,65535),'Invalid error code');
    if (row.type === 'failure') { check(failureStages.has(row.stage), 'Unknown failure stage'); ++failures; continue; }
    check(stages.has(row.stage) && Number.isSafeInteger(row.result) && row.result >= -2147483648 && row.result <= 2147483647, 'Operation invalid'); ++operations;
    if (row.stage==='wsa-cleanup') {
      check(!cleanupSeen && row.id===summary.attempts,'Duplicate or invalid global cleanup'); cleanupSeen=true;
    } else {
      check(!cleanupSeen && row.id<summary.attempts,'Operation outside owned lifetime');
      if (row.stage==='socket') {check(row.id===state.size && !state.has(row.id),'Invalid creation order');check(row.ms>=Math.floor(row.id/6)*header.interval_ms,'Attempt precedes fixed schedule');state.set(row.id,{rank:-1,last:null,socket:false,event:false,write:false,closed:false,connected:false});}
      const current=state.get(row.id);check(current,'Operation before creation');
      if(row.stage==='io-select') check(current.connected && current.rank<ordered.indexOf('http-complete'),'I/O select outside HTTP phase');
      else {
        const rank=ordered.indexOf(row.stage);
        check(rank>current.rank || (rank===current.rank && ['http-send','http-recv'].includes(row.stage)),'Lifecycle stage out of order');current.rank=rank;current.last=row.stage;
      }
      if(row.stage==='socket' && row.result===0) {current.socket=true;liveSockets++;check(liveSockets<=6,'Socket concurrency cap');}
      if(['connect','connect-async'].includes(row.stage) && row.result===0 && row.error===0) current.connected=true;
      if(row.stage==='event-create' || row.stage==='write-event-create') {check(current.socket && !current.closed,'Event before live socket');if(row.result===0){current[row.stage==='event-create'?'event':'write']=true;liveEvents++;check(liveEvents<=12,'Event concurrency cap');}}
      if(row.stage==='shutdown') check(current.socket && !current.closed,'Shutdown of unowned socket');
      if(row.stage==='closesocket') {check(current.socket && !current.closed && byId.get(row.id)?.some(item=>item.stage==='shutdown'),'Close before owned shutdown');current.closed=true;if(row.result===0)liveSockets--;}
      if(row.stage==='event-close' || row.stage==='write-event-close') {const key=row.stage==='event-close'?'event':'write';check(current.closed && current[key],'Close before event creation/socket close');current[key]=false;if(row.result===0)liveEvents--;}
    }
    if (!byId.has(row.id)) byId.set(row.id, []); byId.get(row.id).push(row);
    if (['http-send','http-recv'].includes(row.stage)) check((row.result>0 && row.result<=8192 && row.error===0) || (row.result===0 && row.error===0) || (row.result===-1 && row.error>0),'Invalid I/O result/error');
    else if(['binding-before','binding-after'].includes(row.stage)) check(([0,1].includes(row.result) && row.error===0) || (row.result===-1 && row.error>0),'Invalid binding result/error');
    else if(row.stage==='connect-wait') check((row.result===0 && row.error===0) || (row.result===258 && row.error===0) || (row.result===-1 && row.error>0),'Invalid wait result/error');
    else if(row.stage==='io-select') check(row.result===-1 && row.error>0,'Invalid select failure');
    else check((row.result===0 && row.error===0) || (row.result===-1 && row.error>0),'Invalid result/error combination');
    counts.set(row.stage, (counts.get(row.stage) ?? 0) + 1);
    const destinations = row.stage === 'socket' ? socketIds : ['event-create','write-event-create'].includes(row.stage) ? eventIds : row.stage === 'closesocket' ? closedIds : ['event-close','write-event-close'].includes(row.stage) ? eventClosedIds : row.stage === 'http-complete' ? successfulIds : null;
    const identity = row.stage.includes('event-') ? `${row.id}:${row.stage.startsWith('write-') ? 'write' : 'connect'}` : row.id;
    if (destinations && row.result === 0 && row.error === 0) { check(!destinations.has(identity), 'Repeated ownership/result identity'); destinations.add(identity); }
  }
  check(summary.operation_records === operations && summary.failures === failures && summary.attempts === (counts.get('socket') ?? 0), 'Counter mismatch');
  for (const [key, ids] of [['sockets_opened',socketIds],['sockets_closed',closedIds],['events_opened',eventIds],['events_closed',eventClosedIds],['successes',successfulIds]]) check(summary[key] === ids.size, 'Ownership counter mismatch');
  check([...closedIds].every(id => socketIds.has(id)) && [...eventClosedIds].every(id => eventIds.has(id)), 'Unowned close');
  check((counts.get('wsa-cleanup') ?? 0) === 1, 'Missing Winsock cleanup');
  check(summary.elapsed_ms<=MAX_ELAPSED,'Hard deadline exceeded');
  check(summary.elapsed_ms >= previousMs, 'Summary time precedes observations');
  if (summary.complete) check(summary.attempts === header.batches * 6, 'False completeness');
  const error10055 = records.filter(row=>row.type==='operation' && row.error===10055).length;
  if(error10055) check(records.some(row=>row.type==='failure' && row.error===10055),'Unreported operation 10055');
  const passed = error10055===0 && summary.complete && summary.failures === 0 && summary.successes === summary.attempts && summary.sockets_opened === summary.sockets_closed && summary.events_opened === summary.events_closed;
  if (passed) {
    check(summary.elapsed_ms<=header.total_cap_ms,'Successful run exceeds application deadline');
    check(operations<=20000,'Operation output cap');
    const zeroStages = ['socket','nonblocking','nodelay','keepalive','event-create','write-event-create','event-select-connect','set-randomize','get-randomize','http-complete','shutdown','closesocket','event-close','write-event-close'];
    const one = (rows, stage) => { const matches=rows.filter(row=>row.stage===stage); check(matches.length===1, 'Required stage missing/duplicated'); return matches[0]; };
    for (let id=0;id<summary.attempts;id++) {
      const rows=byId.get(id) ?? [];
      const observation=observations.get(id);check(observation && observation.length===4 && observation.value===(header.mode==='randomized'),'Variant readback does not prove requested mode');
      check(!rows.some(row=>row.stage==='io-select'),'Failed I/O select cannot pass');
      const connect=rows.find(row=>row.stage==='connect');
      const expected=ordered.filter(stage=>stage!=='option' && (connect?.result!==0 || !['connect-wait','connect-enumerate','connect-async'].includes(stage)));
      const actual=rows.map(row=>row.stage).filter((stage,index,array)=>index===0 || stage!==array[index-1]);
      check(actual.join()===expected.join(),'Incomplete successful lifecycle');
      for (const stage of zeroStages) { const row=one(rows,stage); check(row.result===0 && row.error===0,'Successful result contains API failure'); }
      const before=one(rows,'binding-before'), after=one(rows,'binding-after');
      check((before.result===0 && before.error===0) || (before.result===-1 && before.error===10022),'Invalid unbound state');
      check(after.result===1 && after.error===0,'No assigned binding in successful connect');
      const connected=one(rows,'connect');
      if (connected.result===-1 && connected.error===10035) for (const stage of ['connect-wait','connect-enumerate','connect-async']) {const row=one(rows,stage);check(row.result===0 && row.error===0,'Pending connect failed');}
      else check(connected.result===0 && connected.error===0,'Connect failure cannot pass');
      for(const row of rows.filter(row=>['connect-wait','connect-enumerate','connect-async'].includes(row.stage))) check(row.ms-connected.ms<=1000,'Connect phase deadline exceeded');
      for (const stage of ['http-send','http-recv']) {const io=rows.filter(row=>row.stage===stage);check(io.length>0 && io.every(row=>row.result>0 && row.error===0),'Missing/failed HTTP I/O');}
      check(one(rows,'http-complete').ms-rows.find(row=>row.stage==='http-send').ms<=1000,'HTTP phase deadline exceeded');
    }
    const cleanup=one(records.filter(row=>row.type==='operation'),'wsa-cleanup');check(cleanup.result===0 && cleanup.error===0,'Cleanup failure cannot pass');
  }
  return { header, summary, passed, error10055 };
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    if (process.argv.length !== 3) throw new Error('Usage: node validate-output.mjs output.jsonl');
    const file = fs.statSync(process.argv[2]); if (file.size > MAX_BYTES) throw new Error('Oversized output');
    const result = validateOutput(fs.readFileSync(process.argv[2], 'utf8'));
    console.log(JSON.stringify(result)); process.exitCode = result.passed ? 0 : 1;
  } catch (error) { console.error(error.message); process.exitCode = 2; }
}
