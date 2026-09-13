// Pinned extension v1191 intervals, in seconds. No catch-up burst after a
// throttled tab resumes: every request completion starts one fresh countdown.
export const UPDATE_DELAYS = Object.freeze([10, 15, 20, 30, 60, 90, 120, 180, 240, 300]);

export class NativeUpdaterSchedule {
  constructor({ poll, tick, hidden = () => document.hidden,
    later = (fn, ms) => setTimeout(fn, ms), clear = timer => clearTimeout(timer) }) {
    this.poll = poll; this.tick = tick; this.hidden = hidden;
    this.later = later; this.clear = clear;
    this.auto = false; this.busy = false; this.delay = 0; this.remaining = 0;
    this.timer = null; this.generation = 0;
  }
  cancelTimer() {
    this.generation++;
    if (this.timer !== null) this.clear(this.timer);
    this.timer = null;
  }
  pulse() {
    this.cancelTimer();
    if (!this.auto || this.busy) return;
    if (this.remaining === 0) { this.poll(); return; }
    this.tick(this.remaining--);
    const generation = this.generation;
    this.timer = this.later(() => {
      if (generation !== this.generation) return;
      this.timer = null; this.pulse();
    }, 1000);
  }
  start() {
    if (this.auto || this.busy) return false;
    this.auto = true; this.delay = 0; this.remaining = UPDATE_DELAYS[0];
    this.pulse(); return true;
  }
  stop() { this.auto = false; this.cancelTimer(); }
  begin() {
    if (this.busy) return false;
    this.cancelTimer(); this.busy = true; return true;
  }
  finish(count, forced) {
    if (!this.busy) return;
    this.busy = false;
    if (count > 0) this.delay = this.hidden() ? 4 : 0;
    else if (!forced) this.delay = Math.min(this.delay + 1, UPDATE_DELAYS.length - 1);
    this.remaining = UPDATE_DELAYS[this.delay];
    this.pulse();
  }
  suspend() { this.stop(); this.busy = false; }
  visibility() {
    if (!this.auto) return;
    this.delay = this.hidden() && this.delay < 4 ? 4 : 0;
    this.remaining = UPDATE_DELAYS[0];
    // A running response owns the next countdown. Visibility events cannot
    // schedule another request or leave two timers when that response finishes.
    this.pulse();
  }
}
