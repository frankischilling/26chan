import { postId } from '../static/thread-watcher-core.v1.js';

// Source href/DOM spellings stay lexical. Only lookup and cycle authority use
// this exact decimal identity; protocol envelopes must still be canonical.
export function quotePostId(value) {
  if (typeof value !== 'string' || value.length > 512 || !/^[0-9]+$/.test(value) || /\D/.test(value)) return null;
  const canonical = value.replace(/^0+/, '');
  return canonical && postId(canonical) === canonical ? canonical : null;
}
