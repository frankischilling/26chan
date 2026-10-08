// Keep browser protocol failures useful without publishing response contents,
// locations, headers, cookies or one-use upload capabilities.
export function assertOwnedThreadResponse(response, report = console.error) {
  const status = response.status();
  if (!Number.isInteger(status) || status < 100 || status > 599) throw new Error('Invalid owned response status.');
  if (status === 303) return;
  const contentType = response.headers()['content-type'] || '';
  const type = contentType.startsWith('application/json') ? 'json'
    : contentType.startsWith('text/html') ? 'html'
      : contentType.startsWith('text/plain') ? 'plain' : 'other';
  report(`OWNED_UPLOAD_RESPONSE status=${status} type=${type} stage=owner-thread failure=http`);
  throw new Error('Owned thread response was not a creation redirect.');
}

export async function ownedUploadResponse(response, stage, report = console.error) {
  if (!['upload', 'post'].includes(stage)) throw new Error('Unknown owned response stage.');
  const status = response.status();
  const contentType = response.headers()['content-type'] || '';
  const type = contentType.startsWith('application/json') ? 'json'
    : contentType.startsWith('text/html') ? 'html'
      : contentType.startsWith('text/plain') ? 'plain' : 'other';
  const emit = failure => report(`OWNED_UPLOAD_RESPONSE status=${status} type=${type} stage=${stage} failure=${failure}`);
  if (status !== 200 || type !== 'json') emit('http');
  try {
    return { status, result: await response.json() };
  } catch (error) {
    emit(error instanceof SyntaxError ? 'json' : 'body');
    // Raw Playwright errors can contain the URL. The supervisor receives only
    // fixed classifications, and the original operation still fails.
    throw new Error('Owned upload response could not be read.');
  }
}

export async function ownedDeletionResponse(response, report = console.error) {
  const status = response.status();
  if (!Number.isInteger(status) || status < 100 || status > 599) throw new Error('Invalid owned response status.');
  const contentType = response.headers()['content-type'] || '';
  const type = contentType.startsWith('application/json') ? 'json'
    : contentType.startsWith('text/html') ? 'html'
      : contentType.startsWith('text/plain') ? 'plain' : 'other';
  const reject = failure => {
    report(`OWNED_UPLOAD_RESPONSE status=${status} type=${type} stage=deletion failure=${failure}`);
    throw new Error('Owned deletion response was not confirmed.');
  };
  if (status !== 200 || type !== 'html') reject('http');
  let text;
  try { text = await response.text(); } catch { reject('body'); }
  if (typeof text !== 'string' || text.length > 4096 || !text.includes('The deletion was completed.')) reject('content');
  return { status, text };
}

// Quick Reply and native deletion cancel their fetch controllers after consuming
// the body. Capture a clone before returning the original response to the client,
// so the observer does not depend on Chromium retaining its DevTools body copy.
async function captureOwnedResponse(page, url, maxBytes = 0) {
  await page.evaluate(({ url, maxBytes }) => {
    const original = window.fetch;
    let resolve;
    window.ownedUploadResponse = new Promise(done => { resolve = done; });
    window.fetch = async (...args) => {
      const response = await original(...args);
      if (String(args[0]) === url && args[1]?.method === 'POST') {
        window.fetch = original;
        const captured = { status: response.status, type: response.headers.get('content-type') || '' };
        try {
          const clone = response.clone();
          if (!maxBytes) captured.text = await clone.text();
          else {
            const reader = clone.body.getReader(), chunks = [];
            let bytes = 0, reads = 0;
            try {
              for (;;) {
                const part = await reader.read();
                if (part.done) break;
                if (++reads > maxBytes || (bytes += part.value.byteLength) > maxBytes) throw new Error('Owned response limit.');
                chunks.push(part.value);
              }
              const buffer = new Uint8Array(bytes);
              let offset = 0;
              for (const chunk of chunks) { buffer.set(chunk, offset); offset += chunk.byteLength; }
              captured.text = new TextDecoder('utf-8', { fatal: true }).decode(buffer);
            } finally { void reader.cancel().catch(() => {}); }
          }
        } catch {
          captured.failed = true;
        }
        resolve(captured);
      }
      return response;
    };
  }, { url, maxBytes });
  return () => page.evaluate(() => window.ownedUploadResponse);
}

export async function observeOwnedUploadResponse(page, url, stage, report = console.error) {
  if (!['upload', 'post'].includes(stage)) throw new Error('Unknown owned response stage.');
  const capture = await captureOwnedResponse(page, url);
  return async () => {
    const captured = await capture();
    return ownedUploadResponse({
      status: () => captured.status,
      headers: () => ({ 'content-type': captured.type }),
      json: async () => {
        if (captured.failed) throw new Error('Owned response body unavailable.');
        return JSON.parse(captured.text);
      },
    }, stage, report);
  };
}

export async function observeOwnedDeletionResponse(page, url, report = console.error) {
  const capture = await captureOwnedResponse(page, url, 4096);
  return async () => {
    const captured = await capture();
    return ownedDeletionResponse({
      status: () => captured.status,
      headers: () => ({ 'content-type': captured.type }),
      text: async () => {
        if (captured.failed) throw new Error('Owned response body unavailable.');
        return captured.text;
      },
    }, report);
  };
}
