import { postId } from './native-post-numbers.js';

export const uploadFields = Object.freeze(['upload_id', 'upload_capability']);

export const QUICK_REPLY_UPLOAD_LIMITS = Object.freeze({
  bytes: 8_388_608,
  responseBytes: 4096,
  uploadMs: 30_000,
  statusMs: 10_000,
  cancelMs: 10_000,
});

const invalidUploadResponse = 'Invalid upload response.';
const invalidUploadOrigin = 'Invalid upload origin.';
const cancelFailure = 'Upload could not be canceled. Try again.';
const uploadStates = new Set(['queued', 'processing', 'approved', 'failed', 'incomplete']);
const lowerHex = (value, length) => typeof value === 'string' && value.length === length && /^[0-9a-f]+$/.test(value);
const exact = (value, keys) => value !== null && typeof value === 'object' && !Array.isArray(value)
  && Object.keys(value).sort().join(',') === [...keys].sort().join(',');

export const uploadTarget = thread => thread === '0' || !!postId(thread);

function uploadContext(board, thread, origin) {
  if (!/^[a-z0-9]{1,10}$/.test(board) || !uploadTarget(thread)) throw new Error('Invalid upload target.');
  let base;
  try { base = new URL(origin); } catch { throw new Error(invalidUploadOrigin); }
  if (!['http:', 'https:'].includes(base.protocol) || base.origin !== origin
    || base.username || base.password || base.pathname !== '/' || base.search || base.hash) throw new Error(invalidUploadOrigin);
  return { board, thread, origin };
}

export function parseQuickReplyUpload(text, thread) {
  if (typeof text !== 'string' || text.length > QUICK_REPLY_UPLOAD_LIMITS.responseBytes || !uploadTarget(thread)) {
    throw new Error(invalidUploadResponse);
  }
  let value;
  try { value = JSON.parse(text); } catch { throw new Error(invalidUploadResponse); }
  if (!exact(value, ['upload_id', 'upload_capability', 'resto', 'state'])
    || !lowerHex(value.upload_id, 32) || !lowerHex(value.upload_capability, 64)
    || value.resto !== thread || !uploadStates.has(value.state)) throw new Error(invalidUploadResponse);
  return Object.freeze({ ...value });
}

function safeError(text) {
  if (typeof text !== 'string' || text.length > QUICK_REPLY_UPLOAD_LIMITS.responseBytes) return null;
  try {
    const value = JSON.parse(text);
    return exact(value, ['error']) && typeof value.error === 'string' && value.error.length > 0
      && value.error.length <= 2000 && !/[\u0000-\u001f\u007f]/.test(value.error) ? value.error : null;
  } catch { return null; }
}

function trustedError(message) {
  const error = new Error(message);
  error.quickReplySafe = true;
  return error;
}

async function responseText(response, limit, signal) {
  if (response.headers?.get('content-type')?.split(';')[0].trim().toLowerCase() !== 'application/json') {
    try { await response.body?.cancel(); } catch { /* best effort */ }
    throw new Error('invalid-content-type');
  }
  const body = response.body, reader = body?.getReader?.();
  if (!reader) throw new Error('missing-body');
  const cancel = () => { try { reader.cancel().catch(() => {}); } catch { /* best effort */ } };
  signal?.addEventListener('abort', cancel, { once: true });
  const parts = [];
  let total = 0, reads = 0;
  try {
    for (;;) {
      const part = await reader.read();
      if (signal?.aborted) throw new Error('request-aborted');
      if (part.done) break;
      if (!(part.value instanceof Uint8Array) || ++reads > 4096) throw new Error('response-limit');
      total += part.value.byteLength;
      if (total > limit) throw new Error('response-limit');
      parts.push(part.value);
    }
    const bytes = new Uint8Array(total);
    let offset = 0;
    for (const part of parts) { bytes.set(part, offset); offset += part.byteLength; }
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } finally {
    signal?.removeEventListener('abort', cancel);
    try { await reader.cancel(); } catch { /* reader may already be closed */ }
  }
}

async function uploadRequest({ url, body, signal, fetcher, timeout, failure, parse, keepalive = false }) {
  const controller = new AbortController();
  let rejectAbort;
  const aborted = new Promise((_, reject) => { rejectAbort = reject; });
  const abort = () => {
    controller.abort();
    rejectAbort(new Error(failure));
  };
  signal?.addEventListener('abort', abort, { once: true });
  const timer = setTimeout(abort, timeout);
  try {
    if (signal?.aborted) throw new Error(failure);
    return await Promise.race([aborted, (async () => {
      const response = await fetcher(url, {
        method: 'POST', body, headers: { Accept: 'application/json' },
        credentials: 'same-origin', mode: 'same-origin', redirect: 'error', cache: 'no-store',
        keepalive, signal: controller.signal,
      });
      if (controller.signal.aborted) { try { await response.body?.cancel(); } catch { /* best effort */ } throw new Error(failure); }
      const text = await responseText(response, QUICK_REPLY_UPLOAD_LIMITS.responseBytes, controller.signal);
      if (controller.signal.aborted) throw new Error(failure);
      if (!response.ok) {
        const message = safeError(text);
        if (message) throw trustedError(message);
        throw new Error(failure);
      }
      return parse(text);
    })()]);
  } catch (error) {
    if (error?.quickReplySafe === true && !controller.signal.aborted) throw error;
    throw new Error(failure);
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener('abort', abort);
    controller.abort();
  }
}

export async function uploadQuickReplyFile({
  board, thread, file, badged = false, signal, origin = location.origin, fetcher = fetch,
}) {
  uploadContext(board, thread, origin);
  if (!(file instanceof Blob) || typeof file.name !== 'string' || !file.name || file.name.length > 255
    || [...file.name].some(char => /[\u0000-\u001f\u007f]/.test(char))) throw new Error('Choose one file.');
  if (file.size < 1) throw new Error('Choose one non-empty file.');
  const maximum = board === 'f' && thread === '0' ? 10_485_760 : QUICK_REPLY_UPLOAD_LIMITS.bytes;
  if (file.size > maximum) throw new Error(`File exceeds ${maximum / 1_048_576} MiB upload limit.`);
  const form = new FormData();
  form.append('resto', thread);
  if (badged) form.append('vip_image', '1');
  form.append('upfile', file, file.name);
  return uploadRequest({
    url: `${origin}/${board}/upload`, body: form, signal, fetcher,
    timeout: QUICK_REPLY_UPLOAD_LIMITS.uploadMs,
    failure: 'Upload failed. Choose the file again.',
    parse: text => parseQuickReplyUpload(text, thread),
  });
}

// Ordered fields match the streaming drawing endpoint. A recording never falls
// back to the file-only route, including after cancellation or a server error.
export async function uploadDrawingFile({
  board, thread, file, replay = null, signal, origin = location.origin, fetcher = fetch,
}) {
  const context = uploadContext(board, thread, origin);
  if (!(file instanceof Blob) || file.name !== 'tegaki.png' || file.type !== 'image/png'
    || file.size < 33 || file.size > QUICK_REPLY_UPLOAD_LIMITS.bytes) throw new Error('Invalid drawing PNG.');
  if (replay !== null && (!(replay instanceof Blob) || replay.name !== 'tegaki.tgkr'
    || replay.size < 13 || replay.size > QUICK_REPLY_UPLOAD_LIMITS.bytes)) throw new Error('Invalid drawing recording.');
  const form = new FormData();
  form.append('resto', context.thread);
  form.append('png_bytes', String(file.size));
  form.append('replay_bytes', String(replay?.size ?? 0));
  form.append('upfile', file, 'tegaki.png');
  if (replay !== null) form.append('replay', replay, 'tegaki.tgkr');
  return uploadRequest({
    url: `${context.origin}/${context.board}/drawing`, body: form, signal, fetcher,
    timeout: QUICK_REPLY_UPLOAD_LIMITS.uploadMs,
    failure: 'Drawing upload failed. Keep the drawing and try again.',
    parse: text => parseQuickReplyUpload(text, context.thread),
  });
}

function receiptBody(receipt, context) {
  const checked = parseQuickReplyUpload(JSON.stringify(receipt), context.thread);
  return new URLSearchParams({
    upload_id: checked.upload_id,
    upload_capability: checked.upload_capability,
    resto: checked.resto,
  });
}

export async function checkQuickReplyUpload({
  board, thread, receipt, signal, origin = location.origin, fetcher = fetch,
}) {
  const context = uploadContext(board, thread, origin);
  const expected = parseQuickReplyUpload(JSON.stringify(receipt), context.thread);
  return uploadRequest({
    url: `${context.origin}/${context.board}/upload/status`,
    body: receiptBody(expected, context), signal, fetcher,
    timeout: QUICK_REPLY_UPLOAD_LIMITS.statusMs,
    failure: 'Upload status unavailable. Try again.',
    parse: text => {
      const next = parseQuickReplyUpload(text, context.thread);
      if (next.upload_id !== expected.upload_id || next.upload_capability !== expected.upload_capability) {
        throw new Error(invalidUploadResponse);
      }
      return next;
    },
  });
}

export async function cancelQuickReplyUpload({
  board, thread, receipt, signal, origin = location.origin, fetcher = fetch, keepalive = false,
}) {
  const context = uploadContext(board, thread, origin);
  return uploadRequest({
    url: `${context.origin}/${context.board}/upload/cancel`,
    body: receiptBody(receipt, context), signal, fetcher,
    timeout: QUICK_REPLY_UPLOAD_LIMITS.cancelMs,
    failure: cancelFailure,
    keepalive,
    parse: text => {
      let value;
      try { value = JSON.parse(text); } catch { throw new Error(cancelFailure); }
      if (!exact(value, ['cancelled']) || value.cancelled !== true) throw new Error(cancelFailure);
      return Object.freeze({ cancelled: true });
    },
  });
}

export function commentLengthWarning(value, limit) {
  if (typeof value !== 'string' || !/^[1-9][0-9]{0,4}$/.test(limit) || Number(limit) > 16000) return '';
  const bytes = new TextEncoder().encode(value).length;
  return bytes > Number(limit) ? `Error: Comment too long (${bytes}/${limit}).` : '';
}

export function quoteInsertion(value, start, end, id, selected = '') {
  const quote = (postId(id) ? `>>${id}\n` : '')
    + (selected ? `>${selected.trim().replace(/[\r\n]+/g, '\n>')}\n` : '');
  return { value: value.slice(0, start) + quote + value.slice(end), caret: start + quote.length };
}

export function postingResult(text, thread) {
  return parsePostingResult(text, thread, false);
}

export function drawingPostingResult(text, thread) {
  return parsePostingResult(text, thread, true);
}

function parsePostingResult(text, thread, allowNew) {
  if (!(allowNew ? uploadTarget(thread) : postId(thread)) || typeof text !== 'string' || text.length > 8192) throw new Error('Invalid response');
  const value = JSON.parse(text);
  if (/^\s*\{\s*"error"\s*:\s*"(?:[^"\\]|\\.)*"\s*\}\s*$/.test(text)
    && value && typeof value.error === 'string' && value.error.length > 0 && value.error.length <= 2000) {
    return { error: value.error };
  }
  // Parse only the source's two integer tokens, never rounded Number values.
  const match = /^\s*\{\s*"tid"\s*:\s*([0-9]{1,19})\s*,\s*"pid"\s*:\s*([0-9]{1,19})\s*\}\s*$/.exec(text);
  if (!match || match[1] !== thread || !postId(match[2]) || BigInt(match[2]) <= BigInt(thread)) throw new Error('Invalid response');
  return { thread: thread === '0' ? match[2] : thread, post: match[2] };
}

// A server-owned token, never a response-supplied URL or executable markup.
export function schedulePostingRedirect(result, board, { navigate = path => location.assign(path), schedule = setTimeout } = {}) {
  if (!/^[a-z0-9]{1,10}$/.test(board) || result?.redirect !== 'banned' || result.error !== 'Error: You are banned.') return false;
  schedule(() => navigate(`/banned?board=${board}`), 3000);
  return true;
}

export function sendQuickReply(options) { return sendPost(options, false); }
export function sendDrawingPost(options) { return sendPost(options, true); }
export function sendUploadBoardPost(options) {
  if (options.board !== 'f' || options.thread !== '0' || !options.fields.upload_id || !options.fields.upload_capability) throw new Error('Invalid upload-board target.');
  return sendPost(options, true);
}

async function sendPost({ board, thread, fields, signal, origin = location.origin, fetcher = fetch }, allowNew) {
  if (!/^[a-z0-9]{1,10}$/.test(board) || !(allowNew ? uploadTarget(thread) : postId(thread))) throw new Error('Invalid posting target.');
  const base = new URL(origin);
  if (!['http:', 'https:'].includes(base.protocol) || base.origin !== origin || base.username || base.password) throw new Error('Invalid posting origin.');
  const annotated = fields.oe_time !== undefined || fields.oe_src !== undefined;
  if (annotated && (board !== 'i' || typeof fields.oe_time !== 'string'
    || !/^(0|[1-9][0-9]*)$/.test(fields.oe_time) || !Number.isSafeInteger(Number(fields.oe_time))
    || (fields.oe_src !== undefined && !postId(fields.oe_src))
    || !fields.upload_id || !fields.upload_capability)) throw new Error('Invalid drawing annotation.');
  const form = new FormData();
  let bytes = 0;
  for (const name of ['name', 'email', ...(allowNew ? ['sub', 'filetag'] : []), 'com', 'pwd', 'upload_id', 'upload_capability', 'spoiler', 'flag', 'captcha-csrf', 't-challenge', 't-response', 'captcha-credit',
    ...(annotated ? ['oe_time', 'oe_src'] : [])]) {
    const value = fields[name] ?? '';
    if (typeof value !== 'string') throw new Error('Invalid posting form.');
    const captchaLimit = { 'captcha-csrf': 64, 't-challenge': 67, 't-response': 64, 'captcha-credit': 1 }[name];
    if (captchaLimit && value.length > captchaLimit) throw new Error('Invalid CAPTCHA form.');
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
      const result = parsePostingResult(text, thread, allowNew);
      if (response.status !== 200 && !result.error) throw new Error('Invalid posting status.');
      const redirect = response.headers.get('x-board-posting-redirect');
      if (redirect !== null) {
        if (redirect !== 'banned' || response.status !== 200 || result.error !== 'Error: You are banned.') throw new Error('Invalid posting redirect.');
        return { ...result, redirect: 'banned' };
      }
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

// Match the one fixed source suffix without accepting general HTML in errors.
const passInfoUrl = 'https://www.4chan.org/pass', passInfoText = '4chan Pass users have lower cooldowns. [';
export const PASS_COOLDOWN_HINT = `<br>${passInfoText}<a href="${passInfoUrl}" target="_blank">Learn More</a>]`;
export function renderQuickReplyError(element, text) {
  const duplicate = /^Error: Duplicate file exists\. <a href="(\/[a-z0-9]{1,10}\/thread\/(\d+)#p(\d+))">here<\/a>\.$/.exec(text);
  if (duplicate && postId(duplicate[2]) && postId(duplicate[3])) {
    element.textContent = 'Error: Duplicate file exists. ';
    element.append(Object.assign(element.ownerDocument.createElement('a'), { href: duplicate[1], textContent: 'here' }), '.');
    return;
  }
  const offered = text.endsWith(PASS_COOLDOWN_HINT);
  element.textContent = offered ? text.slice(0, -PASS_COOLDOWN_HINT.length) : text;
  if (!offered) return;
  const document = element.ownerDocument;
  const link = Object.assign(document.createElement('a'), {
    href: passInfoUrl, target: '_blank', rel: 'noopener noreferrer', textContent: 'Learn More',
  });
  element.append(document.createElement('br'), passInfoText, link, ']');
}
