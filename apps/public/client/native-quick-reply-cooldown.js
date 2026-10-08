// Advisory only. The server independently enforces ordinary posting delays.
export function cooldownSeconds(timestamp, delaySeconds, now = Date.now()) {
  if (!/^[0-9]+$/.test(String(timestamp)) || !/^[0-9]+$/.test(String(delaySeconds))) return 0;
  const posted = Number(timestamp), delay = Number(delaySeconds);
  if (!Number.isSafeInteger(posted) || !Number.isSafeInteger(delay) || delay > 86400 || posted > now) return 0;
  return Math.max(0, Math.ceil((posted + delay * 1000 - now) / 1000));
}

export function createQuickReplyCooldown({ board, replySeconds, imageSeconds, storage,
  now = Date.now, schedule = setTimeout, cancel = clearTimeout, changed, expired }) {
  const key = `4chan-cd-${board}`;
  let timestamp = null, timer = null, image = false, armed = false, active = false, generation = 0;
  try { timestamp = storage?.getItem(key); } catch { /* Storage is optional. */ }
  const seconds = () => cooldownSeconds(timestamp, image ? imageSeconds : replySeconds, now());
  const clear = () => { generation++; cancel(timer); timer = null; };
  function render() {
    const remaining = seconds();
    changed?.({ seconds: remaining, armed, label: remaining ? `${remaining}s${armed ? ' (auto)' : ''}` : 'Post' });
    return remaining;
  }
  function pulse() {
    clear();
    const remaining = render();
    if (!active || (!remaining && !armed)) return;
    const token = generation;
    timer = schedule(() => {
      if (!active || token !== generation) return;
      if (!seconds()) {
        // Clear the intent before calling back, including on failure or reentry.
        const submit = armed && Number(timestamp) <= now();
        armed = false; clear(); render();
        if (submit) expired?.();
      } else pulse();
    }, remaining ? 1000 : 0);
  }
  return {
    refresh(hasImage = false) { image = hasImage; active = true; pulse(); return seconds(); },
    toggle() { armed = seconds() > 0 && !armed; pulse(); return armed; },
    disarm() { armed = false; render(); },
    success() { timestamp = String(now()); armed = false; try { storage?.setItem(key, timestamp); } catch { /* Keep the local advisory. */ } pulse(); },
    stop() { active = false; armed = false; clear(); },
    storageEvent(event) {
      if (event.key !== key || !event.newValue || (event.storageArea && event.storageArea !== storage)) return;
      timestamp = event.newValue;
      // A changed timestamp refreshes the advisory, never submits synchronously.
      if (!seconds()) armed = false;
      if (active) pulse();
    },
  };
}
