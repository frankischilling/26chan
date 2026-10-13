import { installReplayAllocationGuard } from './full-generated/native-replay-worker-allocations.js';
import { installReplayWorkerBoundary } from './full-generated/native-replay-worker-boundary.js';
const allocations = installReplayAllocationGuard();
const boundary = installReplayWorkerBoundary();
self.onmessage = async ({data}) => {
  let bitmap;
  try {
    self.onmessage = null;
    const { createReplayRuntime } = await import('./full-generated/runtime.js');
    const runtime = createReplayRuntime(data, boundary);
    for (let i=0; i<runtime.eventCount; i++) runtime.step();
    bitmap = runtime.frame();
    self.postMessage({bitmap, allocations:allocations.audit(), boundary:boundary.audit()}, [bitmap]);
    bitmap = null;
  } catch (error) { bitmap?.close(); self.postMessage({error:error.message}); }
};
