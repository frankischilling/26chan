import {renderMath} from './renderer.mjs';

self.onmessage = event => {
  const request = event.data;
  if (!request || !Number.isSafeInteger(request.id) || request.id < 0) return;
  try {
    const geometry = renderMath(request.tex, request.display);
    self.postMessage({id: request.id, ok: true, geometry});
  } catch {
    self.postMessage({id: request.id, ok: false});
  }
};
