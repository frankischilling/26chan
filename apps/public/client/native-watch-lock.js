// The protected callbacks are synchronous bounded storage mutations. Expiry
// revokes a queued callback, even if a lock provider settles after its deadline.
export class NativeWatchLock {
  constructor({ acquire = null, warn = () => {}, later = (fn, ms) => setTimeout(fn, ms),
    clear = timer => clearTimeout(timer) } = {}) {
    this.acquire = acquire; this.warn = warn; this.later = later; this.clear = clear;
    this.waiting = new Set(); this.active = true;
  }
  suspend() { this.active = false; for (const controller of this.waiting) controller.abort(); }
  resume() { this.active = true; }
  run(action, signal) {
    if (!this.active || signal?.aborted) return Promise.resolve(false);
    if (this.waiting.size >= 16) {
      this.warn('Too many pending watch changes. Try again.'); return Promise.resolve(false);
    }
    const controller = new AbortController(); this.waiting.add(controller);
    return new Promise(resolve => {
      let settled = false, timer;
      const cancel = () => controller.abort();
      const finish = value => {
        if (settled) return;
        settled = true; this.clear(timer); this.waiting.delete(controller);
        signal?.removeEventListener('abort', cancel);
        controller.signal.removeEventListener('abort', aborted);
        resolve(value);
      };
      const aborted = () => finish(false);
      controller.signal.addEventListener('abort', aborted, { once: true });
      signal?.addEventListener('abort', cancel, { once: true });
      timer = this.later(() => { this.warn('Watch storage is busy. Try again.'); cancel(); }, 5000);
      const enter = () => {
        if (settled || controller.signal.aborted || !this.active) return false;
        const value = action(); finish(value); return value;
      };
      const failed = () => {
        if (settled) return;
        this.warn('Watch change could not be saved. Try again.'); finish(false);
      };
      try {
        if (this.acquire) Promise.resolve(this.acquire(enter, controller.signal)).then(finish, failed);
        else enter();
      } catch { failed(); }
    });
  }
}
