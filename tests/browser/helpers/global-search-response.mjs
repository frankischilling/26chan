// Install before navigation. The client cancels its fetch controller after
// reading, so a later DevTools body lookup is not a reliable observation.
export function captureSearchResponses() {
  const original = window.fetch;
  window.ownedSearchResponses = [];
  window.fetch = async (...args) => {
    if (new URL(String(args[0]), location.href).pathname !== '/search/api'
        || (args[1]?.method || 'GET') !== 'GET') return original(...args);
    let resolve;
    window.ownedSearchResponses.push(new Promise(done => { resolve = done; }));
    let response;
    try { response = await original(...args); }
    catch (error) { resolve({ failed: true }); throw error; }
    const captured = { status: response.status, type: response.headers.get('content-type') };
    try {
      const reader = response.clone().body.getReader(), parts = [];
      let bytes = 0, reads = 0;
      try {
        for (;;) {
          const part = await reader.read();
          if (part.done) break;
          // These direct-hash cases deliberately return no matching posts.
          if (++reads > 4096 || (bytes += part.value.byteLength) > 4096) throw new Error('Search fixture response limit.');
          parts.push(part.value);
        }
        const buffer = new Uint8Array(bytes);
        let offset = 0;
        for (const part of parts) { buffer.set(part, offset); offset += part.byteLength; }
        captured.result = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(buffer));
      } finally { void reader.cancel().catch(() => {}); }
    } catch { captured.failed = true; }
    resolve(captured);
    return response;
  };
}
