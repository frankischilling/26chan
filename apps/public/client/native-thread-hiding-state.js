import { postId } from '../static/thread-watcher-core.v1.js';
import { CATALOG_LIMITS } from './native-catalog.js';
import { HIDDEN_REPLY_LIMITS, readHiddenReplies } from './native-reply-hiding.js';

export const HIDDEN_THREAD_LIMITS = Object.freeze({
  entries: HIDDEN_REPLY_LIMITS.entries, storage: HIDDEN_REPLY_LIMITS.storage,
  purgeMs: 43200000,
});

const validTime = now => Number.isSafeInteger(now) && now >= 0;
const validId = id => typeof id === 'string' && postId(id) === id;
const serialize = entries => entries.size ? JSON.stringify(Object.fromEntries(entries)) : null;

export function threadHidingKeys(board) {
  if (typeof board !== 'string' || !/^[a-z0-9]{1,10}$/.test(board)) return null;
  return { hidden: `4chan-hide-t-${board}`, purge: `4chan-purge-t-${board}` };
}

// Thread and reply records have the same native shape, but not the same lifetime.
// Never use the reply-hiding seven-day expiry for thread records.
export function readHiddenThreads(raw, now = Date.now()) {
  return validTime(now) ? readHiddenReplies(raw, now) : { status: 'invalid' };
}

export function changeHiddenThread(raw, id, hidden, now = Date.now()) {
  const parsed = readHiddenThreads(raw, now);
  if (parsed.status !== 'ok' || !validId(id) || typeof hidden !== 'boolean') {
    return { status: 'invalid' };
  }
  const entries = parsed.entries;
  if (hidden) {
    if (!entries.has(id) && entries.size >= HIDDEN_THREAD_LIMITS.entries) return { status: 'limit' };
    entries.set(id, now);
  } else entries.delete(id);
  return { status: 'ok', entries, raw: serialize(entries) };
}

// Only visible, manually restored hides are renewed. The caller excludes threads
// suppressed by Hide filters; a Highlight filter does not exclude them.
export function renewHiddenThreads(raw, visible, now = Date.now()) {
  const parsed = readHiddenThreads(raw, now);
  if (parsed.status !== 'ok' || !(visible instanceof Set)
    || visible.size > HIDDEN_THREAD_LIMITS.entries
    || [...visible].some(id => !validId(id))) return { status: 'invalid' };
  for (const id of visible) if (parsed.entries.has(id)) parsed.entries.set(id, now);
  return { status: 'ok', entries: parsed.entries, raw: serialize(parsed.entries) };
}

export function planHiddenThreadPurge(board, raw, purgeRaw, now = Date.now()) {
  const parsed = readHiddenThreads(raw, now);
  if (!threadHidingKeys(board) || parsed.status !== 'ok') return { status: 'invalid' };
  let last = null;
  if (purgeRaw !== null) {
    if (typeof purgeRaw !== 'string' || !/^(0|[1-9][0-9]{0,15})$/.test(purgeRaw)) {
      return { status: 'invalid' };
    }
    last = Number(purgeRaw);
    if (!validTime(last) || last > now) return { status: 'invalid' };
  }
  if (!parsed.entries.size) return { status: 'empty' };
  // The reference uses a strict less-than comparison at the twelve-hour edge.
  if (last !== null && last >= now - HIDDEN_THREAD_LIMITS.purgeMs) return { status: 'cooldown' };
  return Object.freeze({ status: 'due', board, raw, purgeRaw, started: now });
}

// This is a write plan, not a storage write. Apply it under the board's Web Lock,
// after checking settings/page lifetime, and write the hidden record before the
// purge timestamp. Read both current values inside that lock. No failed, partial,
// stale or malformed transport result may become an authoritative empty list.
export function completeHiddenThreadPurge(plan, cycle, currentRaw, currentPurgeRaw, now = Date.now()) {
  if (!plan || plan.status !== 'due' || !validTime(now) || now < plan.started
    || planHiddenThreadPurge(plan.board, plan.raw, plan.purgeRaw, plan.started).status !== 'due') {
    return { status: 'invalid' };
  }
  if (currentRaw !== plan.raw || currentPurgeRaw !== plan.purgeRaw) return { status: 'stale' };
  if (cycle?.status !== 'complete' || !Array.isArray(cycle.results) || cycle.results.length !== 1) {
    return { status: 'unavailable' };
  }
  const result = cycle.results[0];
  if (result?.board !== plan.board || result.status !== 'ok' || !Array.isArray(result.posts)
    || result.posts.length > CATALOG_LIMITS.posts) return { status: 'unavailable' };
  const live = new Set();
  for (const post of result.posts) {
    if (!post || typeof post !== 'object' || Array.isArray(post)
      || !validId(post.no) || live.has(post.no)) return { status: 'unavailable' };
    live.add(post.no);
  }
  const entries = new Map();
  for (const id of readHiddenThreads(plan.raw, now).entries.keys()) {
    if (live.has(id)) entries.set(id, 1);
  }
  return { status: 'ready', entries, raw: serialize(entries), purgeRaw: String(now) };
}
