// Construct complete synthetic traces for adversarial validator tests. This is
// not captured Windows evidence and must never be used as a reproduction result.
import { requestBytes } from './validate-output.mjs';
export function syntheticEvidence(schedule = 'overlap', randomize = false) {
  const records = [{ type: 'header', schema: 1, profile: 'receive-connect-history', schedule, randomize, pools: 20, width: 6, exchanges: 6, interval_ms: 100, total_cap_ms: 30000, cleanup_cap_ms: 35000, response_cap: 8192, operation_cap: 30000, start_boot_ms: 100000 }], fixture = [];
  const exchanges = new Map(); let ms = 0, seq = 0;
  const op = (id, stage, result = 0, error = 0) => records.push({ type: 'operation', id, pool: Math.floor(id / 6), lane: id % 6, exchange: exchanges.get(id) ?? -1, stage, result, error, ms, boot_ms: 100000 + ms });
  const connect = id => {
    for (const stage of ['socket', 'nonblocking', 'nodelay', 'keepalive', 'event-create', 'write-event-create', 'event-select-connect', 'set-randomize', 'get-randomize']) op(id, stage);
    records.push({ type: 'option', id, value: randomize, length: 4, ms });
    op(id, 'binding-before', -1, 10022); op(id, 'connect-submit'); op(id, 'connect', -1, 10035); op(id, 'binding-after', 1);
  };
  const connected = id => { for (const stage of ['connect-wait', 'connect-enumerate', 'connect-async']) op(id, stage); };
  const begin = (id, ex) => {
    exchanges.set(id, ex); op(id, 'exchange-begin', ex);
    if (ex === 0) op(id, 'event-select-read-close');
    op(id, 'write-reset'); op(id, 'write-submit-bytes', requestBytes(id, ex)); op(id, 'write-submit'); op(id, 'write-complete-sync', requestBytes(id, ex));
  };
  const receive = (id, bytes, body) => { op(id, 'http-recv', bytes); op(id, 'body-consumed', body); };
  const prefix = id => { receive(id, 1024, 974); receive(id, 50, 1024); op(id, 'prefix-observed', 1024); };
  const finish = (id, ex, streaming = false) => {
    if (streaming) { receive(id, 1024, 2048); receive(id, 1024, 3072); receive(id, 1024, 4096); }
    else receive(id, 76, 26);
    op(id, 'http-complete', ex);
  };
  const fixtureEvent = (pool, event, stamp, bytes) => fixture.push({ schema_version: 1, scope: 'fixture-transport-overlap', sequence: ++seq, pool, request_id: pool + 1, lane: 0, event, monotonic_ns: stamp * 1000000, unix_ms: 1700000000000 + stamp, emitted_bytes: bytes, output_failed: false, emission_boundary: 'http_body_poll' });
  for (let pool = 0; pool <= 20; ++pool) {
    const base = pool * 6;
    if (pool > 0) { ms += 100; op(base, 'next-pool-start', 100); }
    if (pool === 20) { connect(base); connected(base); begin(base, 0); finish(base, 0); }
    else {
      const start = ms;
      fixtureEvent(pool, 'accepted', start, 0); fixtureEvent(pool, 'prefix_emitted', start, 1024);
      if (schedule === 'overlap') {
        connect(base); connected(base); begin(base, 0); prefix(base);
        for (let lane = 1; lane < 6; ++lane) { ++ms; connect(base + lane); op(base, 'receive-service', lane); }
        op(base, 'siblings-submitted', 5);
        for (let lane = 1; lane < 6; ++lane) { connected(base + lane); begin(base + lane, 0); finish(base + lane, 0); }
        ms = start + 250; finish(base, 0, true);
      } else {
        for (let lane = 0; lane < 6; ++lane) connect(base + lane);
        for (let lane = 0; lane < 6; ++lane) connected(base + lane);
        begin(base, 0); prefix(base); ms = start + 250; finish(base, 0, true);
        for (let lane = 1; lane < 6; ++lane) { begin(base + lane, 0); finish(base + lane, 0); }
      }
      fixtureEvent(pool, 'suffix_emitted', start + 250, 4096); fixtureEvent(pool, 'body_complete', start + 250, 4096);
      for (let ex = 1; ex < 6; ++ex) {
        if (schedule === 'overlap') {
          for (let lane = 0; lane < 6; ++lane) begin(base + lane, ex);
          for (let lane = 0; lane < 6; ++lane) finish(base + lane, ex);
        } else for (let lane = 0; lane < 6; ++lane) { begin(base + lane, ex); finish(base + lane, ex); }
      }
    }
    for (let lane = 0; lane < (pool === 20 ? 1 : 6); ++lane) { op(base + lane, 'pending-at-close'); op(base + lane, 'shutdown'); op(base + lane, 'closesocket'); }
    for (let lane = 0; lane < (pool === 20 ? 1 : 6); ++lane) { op(base + lane, 'event-close'); op(base + lane, 'write-event-close'); }
    op(base, 'pool-retired', pool);
  }
  op(120, 'wsa-cleanup');
  records.push({ type: 'summary', attempts: 121, successes: 721, completed_pools: 20, failures: 0, first_error: 0, sockets_opened: 121, sockets_closed: 121, events_opened: 242, events_closed: 242, max_live: 6, operation_records: 0, complete: true, elapsed_ms: ms, sync_writes: 721, async_writes: 0, pending_at_close: 0, post_close_signals: 0, retained_writes: 0, bytes_sent: 0, bytes_received: 0 });
  recount(records);
  return { records, fixture };
}
export function recount(records) {
  const summary = records.at(-1), ops = records.filter(r => r.type === 'operation');
  summary.operation_records = ops.length;
  summary.bytes_sent = ops.filter(r => /^write-complete-/.test(r.stage) && r.error === 0).reduce((n, r) => n + r.result, 0);
  summary.bytes_received = ops.filter(r => r.stage === 'http-recv' && r.result >= 0).reduce((n, r) => n + r.result, 0);
}
export const jsonl = records => records.map(row => JSON.stringify(row)).join('\n') + '\n';
