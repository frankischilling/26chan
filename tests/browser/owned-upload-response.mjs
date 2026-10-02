// Keep browser protocol failures useful without publishing response contents,
// locations, headers, cookies or one-use upload capabilities.
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
