import { WATCH_LIMITS, watchKey, splitWatchKey, readWatches, writeWatches } from '../static/thread-watcher-core.v1.js';
import { FILTER_LIMITS } from './native-filter-limits.js';

export const BLACKLIST_LIMITS = Object.freeze({ entries: 4096, storageChars: 196608 });

export function readBlacklist(raw) {
  if (raw === null) return { status: 'ok', keys: new Set() };
  if (typeof raw !== 'string' || raw.length > BLACKLIST_LIMITS.storageChars) return { status: 'invalid-blacklist' };
  try {
    const value = JSON.parse(raw);
    if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('invalid-blacklist');
    const keys = Object.keys(value);
    if (keys.length > BLACKLIST_LIMITS.entries
      || keys.some(key => !splitWatchKey(key) || value[key] !== 1)) throw new Error('invalid-blacklist');
    return { status: 'ok', keys: new Set(keys) };
  } catch { return { status: 'invalid-blacklist' }; }
}

export function writeBlacklist(keys) {
  if (!(keys instanceof Set) || keys.size > BLACKLIST_LIMITS.entries) throw new Error('blacklist-limit');
  const value = Object.create(null);
  for (const key of keys) {
    if (!splitWatchKey(key)) throw new Error('invalid-blacklist');
    value[key] = 1;
  }
  const raw = JSON.stringify(value);
  if (raw.length > BLACKLIST_LIMITS.storageChars) throw new Error('blacklist-limit');
  return raw;
}

// Matching remains in disposable workers. An unsuccessful board supplies no
// evidence for pruning its blacklist, even if another board completed normally.
export async function collectAutoWatches({ filters, boards, transport, matcher, signal }) {
  const cycle = await transport.refresh(boards, { signal });
  if (cycle.status !== 'complete') return { ...cycle, results: [] };
  const results = [];
  for (const row of cycle.results) {
    if (signal?.aborted) return { status: 'cancelled', results: [], bytes: cycle.bytes };
    if (row.status !== 'ok') { results.push({ board: row.board, status: row.status }); continue; }
    const matched = await matcher.match(filters, row.board, row.posts, { signal, labels: true });
    results.push(matched.status === 'ok'
      ? { board: row.board, status: 'ok', present: row.posts.map(post => post.no), matches: matched.matches }
      : { board: row.board, status: matched.status });
  }
  return { status: signal?.aborted ? 'cancelled' : 'complete', results, bytes: cycle.bytes };
}

// The integration calls this with freshly loaded state inside its Web Lock,
// after checking the cycle's settings, watch and blacklist snapshots.
export function planAutoWatches(entries, blacklist, cycle) {
  const next = readWatches(writeWatches(entries));
  const blocked = readBlacklist(writeBlacklist(blacklist)).keys;
  if (cycle.status !== 'complete' || !Array.isArray(cycle.results)
    || cycle.results.length > FILTER_LIMITS.boards) throw new Error('invalid-cycle');
  const seenBoards = new Set();
  let added = 0, limited = 0, failed = 0;
  for (const row of cycle.results) {
    if (typeof row.board !== 'string' || seenBoards.has(row.board)) throw new Error('invalid-board');
    seenBoards.add(row.board);
    if (row.status !== 'ok') { failed++; continue; }
    if (!watchKey(row.board, '1') || !Array.isArray(row.present) || row.present.length > FILTER_LIMITS.posts
      || new Set(row.present).size !== row.present.length || row.present.some(id => !watchKey(row.board, id))
      || !Array.isArray(row.matches) || row.matches.length > row.present.length) throw new Error('invalid-board');
    const positions = new Map(row.present.map((id, index) => [id, index]));
    let previous = -1;
    for (const match of row.matches) {
      const index = positions.get(match.id);
      if (index === undefined || index <= previous || typeof match.label !== 'string'
        || match.label.length > WATCH_LIMITS.labelChars || /[\u0000-\u001f\u007f]/.test(match.label)) throw new Error('invalid-match');
      previous = index;
      const key = watchKey(row.board, match.id);
      if (next.has(key) || blocked.has(key)) continue;
      if (next.size >= WATCH_LIMITS.entries) { limited++; continue; }
      next.set(key, { label: match.label, read: '0', unread: 0, archived: false, ownReply: false });
      added++;
    }
    for (const key of blocked) {
      const parts = splitWatchKey(key);
      if (parts.board === row.board && !positions.has(parts.id)) blocked.delete(key);
    }
  }
  return { entries: next, blacklist: blocked, added, limited, failed };
}
