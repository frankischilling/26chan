import { CONTROL_TRIGGER } from '../tests/helpers/browser-control-trigger.js';

export const TARGET = 'http://127.0.0.1:3000/readyz';
export const SUMMARY_BYTES = 8192;
export const STARTUP_MS = 30_000;
export const NAVIGATION_MS = 5_000;
export const CLOSE_MS = 5_000;

export function diagnosticPlan(env, platform = process.platform) {
  if (platform !== 'win32' || env.WINDOWS_BROWSER_CONTROL !== '1' ||
      env.WINDOWS_BROWSER_CONTROL_OWNED !== '1' || env.THEME_SHARD !== '7' ||
      env.WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS !== '1' || env.WINDOWS_VISUAL_NETLOG !== '1' ||
      env.VISUAL_FIXTURE_SERVER !== '1') throw new Error('Owned browser control configuration rejected');
  return { args: ['--shard', '7/8', '--output', 'test-results/windows-themes-7'],
    output: 'test-results/windows-browser-control-7' };
}

// Bounded ASCII framing. Invalid/oversized lines stay poisoned until LF.
export function createTriggerCollector(trigger) {
  let line = '', poisoned = false, fired = false;
  return chunk => {
    if (fired || !Buffer.isBuffer(chunk)) return;
    for (const byte of chunk) {
      if (byte === 10) {
        if (!poisoned && line === CONTROL_TRIGGER) {
          fired = true;
          try { trigger(); } catch { }
          return;
        }
        line = ''; poisoned = false;
      } else if (byte < 32 || byte > 126 || line.length >= 128) {
        line = ''; poisoned = true;
      } else if (!poisoned) line += String.fromCharCode(byte);
    }
  };
}

export function bounded(promise, milliseconds, fallback) {
  let timer;
  return Promise.race([Promise.resolve(promise).catch(() => fallback),
    new Promise(resolve => { timer = setTimeout(() => resolve(fallback), milliseconds); })])
    .finally(() => clearTimeout(timer));
}

export function encodeSummary(value) {
  const body = JSON.stringify(value);
  if (Buffer.byteLength(body) > SUMMARY_BYTES) throw new Error('Control summary exceeds limit');
  return body;
}

// Only the winning HTTP job/socket bindings qualify; controller/alternate-job
// edges are not proof that the response used a particular connection. Chromium
// tcp_socket_win.cc emits an empty successful END; failure ENDs carry os_error.
// The two bindings and empty END were also checked against a pinned Windows log.
// A readiness-control capture still needs its own complete evidence chain.
export function inspectControlNetlog(log) {
  const none = { fresh_tcp: 'unverified', successful_tcp: 'unverified', attempts: 0,
    request_time: null, connect_time: null, connect_end_time: null, clock_domain: 'control-netlog-only' };
  if (!Array.isArray(log?.events) || !log.constants?.logEventTypes) return none;
  const types = log.constants.logEventTypes;
  const sources = log.constants.logSourceType;
  if (!sources || !['URL_REQUEST', 'HTTP_STREAM_JOB', 'SOCKET'].every(name => Number.isSafeInteger(sources[name]))) return none;
  const phases = log.constants.logEventPhase;
  if (!phases || !['PHASE_BEGIN', 'PHASE_END', 'PHASE_NONE'].every(name => Number.isSafeInteger(phases[name]))) return none;
  const begin = phases.PHASE_BEGIN, end = phases.PHASE_END;
  const key = source => source && Number.isSafeInteger(source.id) && Number.isSafeInteger(source.type)
    ? `${source.type}:${source.id}` : null;
  const numericTime = time => /^(?:0|[1-9][0-9]{0,19})(?:\.[0-9]{1,6})?$/.test(String(time)) ? Number(time) : NaN;
  const requests = log.events.filter(event => event.type === types.URL_REQUEST_START_JOB && event.phase === begin &&
    event.params?.url === TARGET && event.source?.type === sources.URL_REQUEST && key(event.source) && Number.isFinite(numericTime(event.time)));
  if (requests.length !== 1) return none;
  const request = requests[0], requestKey = key(request.source), requestTime = numericTime(request.time);
  const requestIndex = log.events.indexOf(request);
  const bindings = (source, type) => log.events.filter(event => type !== undefined && event.type === type &&
    event.phase === phases.PHASE_NONE && key(event.source) === source && key(event.params?.source_dependency) && numericTime(event.time) >= requestTime);
  const jobs = bindings(requestKey, types.HTTP_STREAM_REQUEST_BOUND_TO_JOB);
  if (jobs.length !== 1 || jobs[0].params.source_dependency.type !== sources.HTTP_STREAM_JOB) return { ...none, request_time: String(request.time) };
  const sockets = bindings(key(jobs[0].params.source_dependency), types.SOCKET_POOL_BOUND_TO_SOCKET);
  if (sockets.length !== 1 || sockets[0].params.source_dependency.type !== sources.SOCKET) return { ...none, request_time: String(request.time) };
  const socketKey = key(sockets[0].params.source_dependency), boundTime = numericTime(sockets[0].time);
  const boundIndex = log.events.indexOf(sockets[0]);
  let pending = null, attempts = 0, first = null, successful = null;
  for (let index = requestIndex + 1; index < boundIndex; index++) {
    const event = log.events[index];
    if (event.type !== types.TCP_CONNECT_ATTEMPT || key(event.source) !== socketKey) continue;
    if (event.phase === begin) {
      successful = null;
      pending = event.params?.address === '127.0.0.1:3000' && numericTime(event.time) >= requestTime &&
        numericTime(event.time) <= boundTime ? event : null;
      if (pending) { attempts++; first ??= pending; }
    } else if (event.phase === end) {
      if (pending && numericTime(event.time) >= numericTime(pending.time) && numericTime(event.time) <= boundTime &&
          (event.params === undefined || event.params === null || typeof event.params === 'object' && !Array.isArray(event.params) && Object.keys(event.params).length === 0)) {
        successful = { start: pending, end: event };
      }
      pending = null;
    }
  }
  return { ...none, fresh_tcp: attempts ? 'observed' : 'unverified', successful_tcp: successful ? 'observed' : 'unverified',
    attempts: Math.min(attempts, 65535), request_time: String(request.time),
    connect_time: first ? String((successful?.start ?? first).time) : null,
    connect_end_time: successful ? String(successful.end.time) : null };
}
