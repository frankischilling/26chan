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

// Quick Reply cancels its fetch controller after consuming the body. Chromium
// can then discard the DevTools copy before Playwright's response.json() runs.
// Capture a clone in the page before handing the same response to the client.
export async function observeOwnedUploadResponse(page, url, stage, report = console.error) {
  if (!['upload', 'post'].includes(stage)) throw new Error('Unknown owned response stage.');
  await page.evaluate(url => {
    const original = window.fetch;
    let resolve;
    window.ownedUploadResponse = new Promise(done => { resolve = done; });
    window.fetch = async (...args) => {
      const response = await original(...args);
      if (String(args[0]) === url && args[1]?.method === 'POST') {
        window.fetch = original;
        const captured = { status: response.status, type: response.headers.get('content-type') || '' };
        try {
          captured.text = await response.clone().text();
        } catch {
          captured.failed = true;
        }
        resolve(captured);
      }
      return response;
    };
  }, url);
  return async () => {
    const captured = await page.evaluate(() => window.ownedUploadResponse);
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
