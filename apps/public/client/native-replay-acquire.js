export function replayUrl(mediaOrigin, board, tim) {
  if (typeof mediaOrigin !== 'string' || !/^[a-z0-9]{1,10}$/.test(board)
    || typeof tim !== 'string' || !/^[1-9][0-9]{0,15}$/.test(tim) || BigInt(tim) > 9007199254740991n) throw new Error('Invalid replay address.');
  const base = new URL(mediaOrigin);
  if (!['https:', 'http:'].includes(base.protocol) || base.origin !== mediaOrigin
    || base.username || base.password || base.pathname !== '/' || base.search || base.hash) throw new Error('Invalid replay origin.');
  return `${base.origin}/replay/${board}/${tim}.ibr`;
}

export async function acquireReplay({ mediaOrigin, board, tim, signal, fetcher = fetch,
  timeoutMs = 10000, schedule = setTimeout, unschedule = clearTimeout }) {
  const url = replayUrl(mediaOrigin, board, tim), controller = new AbortController();
  let reader, rejectAbort;
  const cancelled = new Promise((_, reject) => { rejectAbort = reject; });
  const abort = () => {
    controller.abort();
    try { reader?.cancel().catch(() => {}); } catch { /* already released */ }
    rejectAbort(new Error('Replay loading was cancelled or timed out.'));
  };
  signal?.addEventListener('abort', abort, { once: true });
  const timer = schedule(abort, timeoutMs);
  try {
    if (signal?.aborted) throw new Error('Replay loading was cancelled.');
    return await Promise.race([cancelled, (async () => {
      const response = await fetcher(url, { credentials: 'omit', mode: 'cors', redirect: 'error', cache: 'no-store', signal: controller.signal });
      if (controller.signal.aborted || !response.ok || response.status !== 200
        || response.headers.get('content-type')?.split(';')[0].trim().toLowerCase() !== 'application/octet-stream') {
        try { response.body?.cancel().catch(() => {}); } catch { /* best effort */ }
        throw new Error('Replay is unavailable.');
      }
      reader = response.body?.getReader();
      if (!reader) throw new Error('Replay has no body.');
      const chunks = []; let length = 0, reads = 0;
      for (;;) {
        const part = await reader.read();
        if (controller.signal.aborted) throw new Error('Replay loading was cancelled.');
        if (part.done) break;
        if (!(part.value instanceof Uint8Array) || ++reads > 2048 || length + part.value.length > 262400) throw new Error('Replay exceeds its limits.');
        length += part.value.length; chunks.push(part.value.slice());
      }
      if (length < 288 || (length - 256) % 16 !== 0) throw new Error('Replay is incomplete.');
      const result = new Uint8Array(length); let offset = 0;
      for (const chunk of chunks) { result.set(chunk, offset); offset += chunk.length; }
      return result.buffer;
    })()]);
  } finally {
    unschedule(timer); signal?.removeEventListener('abort', abort); controller.abort();
    try { reader?.cancel().catch(() => {}); } catch { /* best effort */ }
  }
}
