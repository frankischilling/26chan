import { postId } from '../static/thread-watcher-core.v1.js';

// Preserve the source href/DOM spelling. Only lookup and cycle guards use the
// 64-bit PHP string/%d identity; protocol envelopes still require postId().
export function quotePostId(value) {
  if (typeof value !== 'string' || value.length > 512
    || !/^[+-]?[0-9]+(?:e[+-]?[0-9]+)?$/.test(value) || /[^0-9e+-]/.test(value)) return null;
  let integer;
  if (value.includes('e')) {
    const number = Number(value);
    if (!Number.isFinite(number)) return null;
    integer = BigInt(Math.trunc(number));
  } else integer = BigInt(value);
  if (integer <= 0n) return null;
  const canonical = String(integer > 9223372036854775807n ? 9223372036854775807n : integer);
  return postId(canonical) === canonical ? canonical : null;
}
