import { postId } from '../static/thread-watcher-core.v1.js';

export function quoteInsertion(value, start, end, id, selected = '') {
  const quote = (postId(id) ? `>>${id}\n` : '')
    + (selected ? `>${selected.trim().replace(/[\r\n]+/g, '\n>')}\n` : '');
  return { value: value.slice(0, start) + quote + value.slice(end), caret: start + quote.length };
}

export function postingResult(text, thread) {
  if (!postId(thread) || typeof text !== 'string' || text.length > 8192) throw new Error('Invalid response');
  const value = JSON.parse(text);
  if (/^\s*\{\s*"error"\s*:\s*"(?:[^"\\]|\\.)*"\s*\}\s*$/.test(text)
    && value && typeof value.error === 'string' && value.error.length > 0 && value.error.length <= 2000) {
    return { error: value.error };
  }
  // Parse only the source's two integer tokens, never rounded Number values.
  const match = /^\s*\{\s*"tid"\s*:\s*([0-9]{1,19})\s*,\s*"pid"\s*:\s*([0-9]{1,19})\s*\}\s*$/.exec(text);
  if (!match || match[1] !== thread || !postId(match[2]) || BigInt(match[2]) <= BigInt(thread)) throw new Error('Invalid response');
  return { thread, post: match[2] };
}

export async function sendQuickReply({ board, thread, fields, signal, origin = location.origin, fetcher = fetch }) {
  if (!/^[a-z0-9]{1,10}$/.test(board) || !postId(thread)) throw new Error('Invalid posting target.');
  const base = new URL(origin);
  if (!['http:', 'https:'].includes(base.protocol) || base.origin !== origin || base.username || base.password) throw new Error('Invalid posting origin.');
  const form = new FormData();
  let bytes = 0;
  for (const name of ['name', 'email', 'com', 'pwd', 'upload_id', 'upload_capability', 'spoiler']) {
    const value = fields[name] ?? '';
    if (typeof value !== 'string') throw new Error('Invalid posting form.');
    bytes += new TextEncoder().encode(value).length;
    if (bytes > 90_000) throw new Error('Posting form is too large.');
    if (value || ['com', 'pwd'].includes(name)) form.append(name, value);
  }
  form.append('mode', 'regist'); form.append('resto', thread); form.append('track', '1');
  const controller = new AbortController();
  let reader, rejectAbort;
  const aborted = new Promise((_, reject) => { rejectAbort = reject; });
  const abort = () => {
    controller.abort(); reader?.cancel().catch(() => {});
    rejectAbort(new Error('Request stopped. Check the thread before posting again.'));
  };
  signal?.addEventListener('abort', abort, { once: true });
  const timer = setTimeout(abort, 15000);
  try {
    if (signal?.aborted) { abort(); return await aborted; }
    return await Promise.race([aborted, (async () => {
      const response = await fetcher(`${origin}/${board}/imgboard.php`, {
        method: 'POST', body: form, headers: { Accept: 'application/json' },
        credentials: 'same-origin', redirect: 'error', cache: 'no-store', signal: controller.signal,
      });
      if (controller.signal.aborted) { await response.body?.cancel(); throw new Error('Request stopped.'); }
      if (response.headers.get('content-type') !== 'application/json') {
        await response.body?.cancel(); throw new Error(`Posting failed (HTTP ${response.status}). Check the thread before retrying.`);
      }
      reader = response.body?.getReader(); if (!reader) throw new Error('Missing posting response.');
      let size = 0, text = ''; const decoder = new TextDecoder('utf-8', { fatal: true });
      while (true) {
        const chunk = await reader.read(); if (chunk.done) break;
        size += chunk.value.length;
        if (size > 8192) throw new Error('Posting response is too large. Check the thread before retrying.');
        text += decoder.decode(chunk.value, { stream: true });
      }
      text += decoder.decode();
      const result = postingResult(text, thread);
      if (response.status !== 200 && !result.error) throw new Error('Invalid posting status.');
      return result;
    })()]);
  } catch {
    // A lost or malformed response does not prove that the transaction failed.
    throw new Error('Posting response unavailable. Check the thread before posting again.');
  } finally {
    clearTimeout(timer); signal?.removeEventListener('abort', abort);
    controller.abort(); reader?.cancel().catch(() => {});
  }
}
