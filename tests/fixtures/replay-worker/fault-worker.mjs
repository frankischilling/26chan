// Bounded synthetic CPU stall, solely to test actual parent termination. This
// is not raster performance evidence or a hard termination-time guarantee.
const mode = new URL(self.location.href).searchParams.get('mode');
let generation;
const stall = () => { const stop = performance.now() + 2000; while (performance.now() < stop) {} };
self.onmessage = ({ data }) => {
  generation = data.generation;
  if (data.type === 'init') {
    if (mode === 'startup') { self.postMessage({ type: 'stall-entered', generation }); stall(); }
    self.postMessage({ type: 'ready', generation, sequence: 0, eventCount: 1 });
  } else if (data.type === 'initial') {
    self.postMessage({ type: 'stall-entered', generation }); stall();
    self.postMessage({ type: 'error', generation, message: 'stall completed without parent termination' });
  }
};
