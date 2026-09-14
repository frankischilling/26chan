import { postId, watchKey, splitWatchKey, readTrackedReplies, WATCH_LIMITS } from './thread-watcher-core.v1.js';

export const TRACK_LIMITS = Object.freeze({ threads: 128, indexChars: 8192, ageSeconds: 259200,
  receipts: 32, cookieChars: 8192 });

export function readTrackIndex(raw, now) {
  const result = new Map();
  if (typeof raw !== 'string' || raw.length > TRACK_LIMITS.indexChars) return result;
  try {
    const value = JSON.parse(raw);
    if (!value || typeof value !== 'object' || Array.isArray(value) || Object.keys(value).length > TRACK_LIMITS.threads) return result;
    for (const [id, time] of Object.entries(value)) {
      if (postId(id) && Number.isSafeInteger(time) && time >= 0 && time <= now) result.set(id, time);
    }
  } catch { /* Local hints are optional. */ }
  return result;
}

function updateTrackedPost(storage, board, thread, post, now) {
  if (!watchKey(board, thread) || !postId(post) || BigInt(post) < BigInt(thread)
    || !Number.isSafeInteger(now) || now < 0) throw new Error('Invalid tracked post');
  const prefix = `4chan-track-${board}-`;
  const indexKey = prefix + 'ts';
  const index = readTrackIndex(storage.getItem(indexKey), now);
  for (const [id, timestamp] of index) {
    if (now - timestamp >= TRACK_LIMITS.ageSeconds && id !== thread) {
      index.delete(id);
      storage.removeItem(prefix + id);
    }
  }
  const tracked = readTrackedReplies(storage.getItem(prefix + thread));
  tracked.add(post);
  const ids = [...tracked].sort((a, b) => BigInt(a) < BigInt(b) ? -1 : BigInt(a) > BigInt(b) ? 1 : 0)
    .slice(-WATCH_LIMITS.trackedPosts);
  index.set(thread, now);
  if (index.size > TRACK_LIMITS.threads) {
    const oldest = [...index].filter(([id]) => id !== thread).sort((a, b) => a[1] - b[1])[0][0];
    index.delete(oldest);
    storage.removeItem(prefix + oldest);
  }
  storage.setItem(prefix + thread, JSON.stringify(Object.fromEntries(ids.map(id => [`>>${id}`, 1]))));
  storage.setItem(indexKey, JSON.stringify(Object.fromEntries(index)));
}

export function recordTrackedPost(storage, board, thread, post, now = Math.floor(Date.now() / 1000)) {
  updateTrackedPost(storage, board, thread, post, now);
}

export function touchTrackedThread(storage, board, thread, now = Math.floor(Date.now() / 1000)) {
  if (!watchKey(board, thread)) return;
  const tracked = readTrackedReplies(storage.getItem(`4chan-track-${board}-${thread}`));
  const id = tracked.values().next().value;
  if (id) updateTrackedPost(storage, board, thread, id, now);
}

export function postReceipts(raw) {
  const result = [];
  if (typeof raw !== 'string' || raw.length > TRACK_LIMITS.cookieChars) return result;
  for (const part of raw.split(';')) {
    const match = /^board-posted-([1-9][0-9]{0,18})=([1-9][0-9]{0,18})\.([01])$/.exec(part.trim());
    if (match && postId(match[1]) && postId(match[2]) && BigInt(match[1]) >= BigInt(match[2])) {
      result.push({ name: `board-posted-${match[1]}`, thread: match[2], post: match[1], watch: match[3] === '1', track: true });
      if (result.length === TRACK_LIMITS.receipts) break;
    }
  }
  const native = /(?:^|;\s*)4chan_awt=([1-9][0-9]{0,18})(?:;|$)/.exec(raw);
  if (native && postId(native[1])) result.push({ name: '4chan_awt', thread: native[1], post: native[1], watch: true, track: false });
  return result;
}

export class PostTracking {
  constructor({ board, settings, locked, onPost }) {
    this.board = board;
    this.settings = settings;
    this.locked = locked;
    this.onPost = onPost;
    this.persistent = !!navigator.locks?.request;
    this.memory = new Map();
    this.storage = {
      getItem: key => {
        if (this.persistent) { try { return localStorage.getItem(key); } catch { this.persistent = false; } }
        return this.memory.get(key) ?? null;
      },
      setItem: (key, value) => {
        this.memory.set(key, value);
        if (this.persistent) { try { localStorage.setItem(key, value); } catch { this.persistent = false; } }
      },
      removeItem: key => {
        this.memory.delete(key);
        if (this.persistent) { try { localStorage.removeItem(key); } catch { this.persistent = false; } }
      },
    };
    document.addEventListener('submit', () => this.prepareForms(), true);
    this.prepareForms();
  }

  prepareForms() {
    const settings = this.settings();
    for (const form of document.querySelectorAll('form.postEditor')) {
      const url = new URL(form.action);
      if (form.method !== 'post' || url.origin !== location.origin || ![`/${this.board}/post`, `/${this.board}/imgboard.php`].includes(url.pathname)) continue;
      const fields = { track: settings.disableAll !== true,
        awt: settings.disableAll !== true && settings.threadWatcher === true && settings.threadAutoWatcher === true && form.elements.namedItem('resto')?.value === '0' };
      for (const [name, active] of Object.entries(fields)) {
        let input = form.querySelector(`input[data-post-tracking="${name}"]`);
        if (!active) { input?.remove(); continue; }
        if (!input) { input = document.createElement('input'); input.type = 'hidden'; input.name = name;
          input.dataset.postTracking = name; input.value = '1'; form.append(input); }
      }
    }
  }

  tracked(key) {
    const parts = splitWatchKey(key);
    return parts ? readTrackedReplies(this.storage.getItem(`4chan-track-${parts.board}-${parts.id}`)) : new Set();
  }

  async committed(thread, post) {
    await this.consume(thread);
    const saved = await this.locked(() => {
      if (this.settings().disableAll === true) return false;
      recordTrackedPost(this.storage, this.board, thread, post);
      return true;
    });
    if (saved) await this.onPost({ thread, post, track: true, watch: false });
    return saved;
  }

  async consume(thread) {
    const events = await this.locked(() => {
      const events = [];
      const disabled = this.settings().disableAll === true;
      for (const receipt of postReceipts(document.cookie)) {
        if (!disabled) {
          if (receipt.track) recordTrackedPost(this.storage, this.board, receipt.thread, receipt.post);
          events.push(receipt);
        }
        document.cookie = `${receipt.name}=; Max-Age=0; Path=/${this.board}/; SameSite=Strict${location.protocol === 'https:' ? '; Secure' : ''}`;
      }
      if (!disabled && thread) touchTrackedThread(this.storage, this.board, thread);
      return events;
    });
    for (const event of events || []) await this.onPost(event);
  }
}
