import { createReplaySession } from './native-replay-session.js';

// Scheduling owns no source state: rewind always starts a fresh worker. Frames
// are closed after the display callback, including stale or cancelled replies.
export function createReplayPlayback(bytes, { frame, changed = () => {},
  sessionFactory = createReplaySession, now = () => performance.now(),
  schedule = setTimeout, unschedule = clearTimeout } = {}) {
  if (typeof frame !== 'function') throw new Error('Missing replay display.');
  let session, generation = 0, timer, closed = false, busy = false, autoPaused = false;
  let phase = 'loading', speed = 1, gapless = true, cursor = 0, wall = 0, next = null, index = 0, count = 0, start = 0, end = 0;
  const notify = () => changed({ phase, speed, gapless, index, count,
    elapsed: phase === 'done' ? end - start : Math.max(0, Math.min(end, time()) - start), duration: end - start });
  const time = () => Math.min(0xffffffff, cursor + (phase === 'playing' ? (now() - wall) * speed : 0));
  function stopTimer() { unschedule(timer); timer = null; }
  function fail(epoch) {
    if (closed || epoch !== generation) return;
    stopTimer(); session?.close(); session = null; busy = false; phase = 'failed'; notify();
  }
  function display(value, epoch) {
    try {
      if (closed || epoch !== generation) return false;
      start = value.start; end = value.end;
      frame(value.bitmap, value.presentation); index = value.index; count = value.count; next = value.next;
      if (next === null) { phase = 'done'; autoPaused = false; }
      return true;
    } finally { value.bitmap.close(); }
  }
  async function reset(play = false) {
    if (closed) return;
    autoPaused = false;
    const epoch = ++generation;
    stopTimer(); session?.close(); session = null; busy = true; phase = 'loading'; index = count = start = end = cursor = 0; next = null; notify();
    try {
      session = sessionFactory(bytes);
      const value = await session.ready;
      if (!display(value, epoch)) return;
      busy = false; cursor = next ?? 0; wall = now(); phase = next === null ? 'done' : (play ? 'playing' : 'paused');
      notify(); if (play && next !== null) queue();
    } catch { fail(epoch); }
  }
  function queue() {
    stopTimer(); if (!closed && phase === 'playing' && !busy) timer = schedule(tick, 16);
  }
  async function tick() {
    timer = null;
    if (closed || phase !== 'playing' || busy || !session) return;
    const epoch = generation;
    let until = Math.floor(time());
    if (gapless && next !== null && next > until + 3000) { cursor = until = next; wall = now(); }
    if (next !== null && next > until) { notify(); queue(); return; }
    busy = true;
    try {
      const value = await session.advance(until);
      if (!display(value, epoch)) return;
      // The source freezes its timeline at the next due event when the
      // per-frame event cap is reached, rather than racing through a backlog.
      if (phase === 'playing' && next !== null && next <= until) { cursor = next; wall = now(); }
      busy = false; notify(); queue();
    } catch { fail(epoch); }
  }
  const ready = reset();
  return { ready,
    play() {
      autoPaused = false;
      if (closed || phase === 'loading' || phase === 'failed') return;
      if (phase === 'done') { void reset(true); return; }
      if (phase === 'playing') return;
      wall = now(); phase = 'playing'; notify(); queue();
    },
    pause() { autoPaused = false; if (phase !== 'playing') return; cursor = time(); phase = 'paused'; stopTimer(); notify(); },
    visibility(visible) {
      if (closed) return;
      if (!visible && phase === 'playing') {
        this.pause(); autoPaused = true;
      } else if (visible && autoPaused) {
        this.play();
      }
    },
    speed(value) {
      if (![0.5,1,2,5,10,25].includes(value) || closed) return;
      cursor = time(); wall = now(); speed = value; notify();
    },
    gapless(value) { if (!closed) { gapless = value === true; notify(); } },
    rewind() { return reset(false); },
    close() { if (closed) return; closed = true; generation++; stopTimer(); session?.close(); session = null; },
  };
}
