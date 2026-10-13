import { installReplayAllocationGuard } from './native-replay-worker-allocations.js';
import { installReplayWorkerBoundary } from './native-replay-worker-boundary.js';

// Fixed release-owned module only. Uploaded recordings never select code URLs.
let runtime = null, index = 0, sequence = 0, busy = false, stopped = false;
function stop() { stopped = true; self.onmessage = null; self.close(); }
self.onmessage = async ({ data }) => {
  let bitmap, stage = 'command';
  try {
    if (stopped || busy || !data || typeof data !== 'object' || Array.isArray(data)
      || !Number.isSafeInteger(data.sequence) || data.sequence !== sequence + 1) throw new Error('Invalid replay command');
    busy = true; sequence = data.sequence;
    if (data.type === 'init' && runtime === null && sequence === 1
      && Object.keys(data).sort().join(',') === 'bytes,sequence,type') {
      if (!(data.bytes instanceof ArrayBuffer) || data.bytes.byteLength < 288 || data.bytes.byteLength > 262400) throw new Error('Invalid replay bytes');
      stage = 'boundary';
      installReplayAllocationGuard();
      const boundary = installReplayWorkerBoundary();
      stage = 'import';
      const { createReplayRuntime } = await import('./runtime.v1.js');
      stage = 'init';
      runtime = createReplayRuntime(new Uint8Array(data.bytes), boundary);
    } else if (data.type === 'advance' && runtime !== null
      && Object.keys(data).sort().join(',') === 'sequence,type,until'
      && Number.isSafeInteger(data.until) && data.until >= 0 && data.until <= 0xffffffff) {
      // One command processes at most 50 events. Parent deadlines also cover a
      // slow individual native canvas operation that cannot yield here.
      for (let n = 0; n < 50 && index < runtime.eventCount && runtime.nextTimestamp() <= data.until; n++) {
        index = runtime.step();
      }
    } else throw new Error('Invalid replay command');
    stage = 'frame';
    bitmap = runtime.frame();
    self.postMessage({ type: 'frame', sequence, index, count: runtime.eventCount,
      next: runtime.nextTimestamp(), bitmap }, [bitmap]);
    bitmap = null; busy = false;
  } catch {
    bitmap?.close();
    self.postMessage({ type: 'error', sequence, stage });
    stop();
  }
};
