import sourceControls from '../../../fixtures/source-video-controls.json';
// The fallback uses only pinned same-origin assets and an already-approved
// WebM URL. Codec workers receive bytes, never arbitrary script locations.
const loads = new WeakMap();
const SCRIPT = '/static/ogv/ogv.js';
const INTEGRITY = 'sha256-dmgkH+OeoiybVav2XatL9ay5PjK6/sSaA4ZUuChfXs8=';
function library(document) {
  if (loads.has(document)) return loads.get(document);
  const pending = new Promise((resolve, reject) => {
    const script = document.createElement('script');
    script.src = SCRIPT; script.integrity = INTEGRITY; script.crossOrigin = 'anonymous';
    let timer;
    const done = error => {
      clearTimeout(timer); script.onload = script.onerror = null;
      if (error) { script.remove(); reject(new Error('video-fallback-unavailable')); }
      else {
        const window = document.defaultView;
        if (typeof window.OGVPlayer !== 'function' || !window.OGVLoader) { reject(new Error('video-fallback-unavailable')); return; }
        // Upstream's dynamic stylesheet is blocked by our style policy. The
        // owned renderer sets dimensions directly after bounded metadata checks.
        window.OGVPlayer.styleManager = { appendRule() {} };
        window.OGVLoader.base = '/static/ogv'; resolve(window.OGVPlayer);
      }
    };
    script.onload = () => done(false); script.onerror = () => done(true);
    timer = setTimeout(() => done(true), 10000);
    document.head.append(script);
  });
  loads.set(document, pending);
  pending.catch(() => loads.delete(document));
  return pending;
}

export async function createFallback({ document, url, mediaOrigin, active }) {
  const parsed = new URL(url), prefix = `${mediaOrigin}/`;
  if (!['http:', 'https:'].includes(parsed.protocol) || parsed.origin !== mediaOrigin
    || parsed.href !== url || parsed.search || parsed.hash || parsed.username || parsed.password
    || !url.startsWith(prefix) || !/^[a-z0-9]{1,10}\/[1-9][0-9]{0,18}\.webm$/.test(url.slice(prefix.length))) {
    throw new TypeError('invalid-fallback-video');
  }
  if (BigInt(parsed.pathname.split('/').at(-1).split('.')[0]) > 9223372036854775807n) throw new TypeError('invalid-fallback-video');
  const Player = await library(document);
  if (!active()) return null;
  const player = new Player({ wasm: true, threading: false, simd: false, worker: true });
  const container = document.createElement('div'); container.className = 'ogv-cnt';
  const controls = document.createElement('div'); controls.className = 'nativeVideoControls ogv-ctrl'; controls.hidden = true;
  container.append(player, controls);
  const listeners = [];
  function on(node, event, callback) { node.addEventListener(event, callback); listeners.push(() => node.removeEventListener(event, callback)); }
  function button(label, icon, callback) {
    const node = document.createElement('button'); node.type = 'button'; node.className = 'ogv-btn'; node.setAttribute('aria-label', label);
    for (const shape of sourceControls.icons[icon]) {
      const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
      for (const [key, value] of Object.entries(shape.attributes)) svg.setAttribute(key, value);
      svg.setAttribute('aria-hidden', 'true');
      for (const attributes of shape.paths) {
        const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
        for (const [key, value] of Object.entries(attributes)) path.setAttribute(key, value);
        svg.append(path);
      }
      node.append(svg);
    }
    on(node, 'click', callback); controls.append(node); return node;
  }
  const play = button('Play', 'play', () => { if (player.paused) player.play(); else player.pause(); refresh(); });
  const mute = button('Mute', 'mute', () => { player.muted = !player.muted; refresh(); });
  const seek = document.createElement('input'); seek.className = 'ogv-seek'; seek.type = 'range'; seek.min = '0'; seek.max = '100'; seek.step = '0.1'; seek.value = '0'; seek.setAttribute('aria-label', 'Video position');
  on(seek, 'change', () => {
    if (Number.isFinite(player.duration) && player.duration > 0) player.currentTime = Number(seek.value) * player.duration / 100;
  });
  controls.append(seek);
  const volume = document.createElement('input'); volume.className = 'ogv-vol'; volume.type = 'range'; volume.min = '0'; volume.max = '100'; volume.step = '0.1'; volume.setAttribute('aria-label', 'Video volume');
  on(volume, 'input', () => { player.volume = Number(volume.value) / 100; refresh(); }); controls.append(volume);
  const timestamp = document.createElement('span'); timestamp.className = 'nativeVideoTimestamp ogv-ts'; controls.append(timestamp);
  const fullscreen = button('Fullscreen', 'fullscreen', () => {
    const request = document.fullscreenElement ? document.exitFullscreen?.() : container.requestFullscreen?.();
    if (request && typeof request.catch === 'function') request.catch(() => {});
  });
  const time = value => { const seconds = Number.isFinite(value) ? Math.max(0, Math.floor(value)) : 0; return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`; };
  function refresh() {
    play.setAttribute('aria-label', player.paused ? 'Play' : 'Pause'); play.classList.toggle('ogv-toggled', player.paused);
    mute.setAttribute('aria-label', player.muted ? 'Unmute' : 'Mute'); mute.classList.toggle('ogv-toggled', player.muted); mute.setAttribute('aria-pressed', String(player.muted));
    volume.value = String(player.volume * 100);
    fullscreen.classList.toggle('ogv-toggled', document.fullscreenElement === container);
    timestamp.textContent = `${time(player.currentTime)} / ${time(player.duration)}`;
    if (document.activeElement !== seek) seek.value = Number.isFinite(player.duration) && player.duration > 0 ? String(100 * player.currentTime / player.duration) : '0';
  }
  controls.replaceChildren(play, seek, timestamp, mute, volume, fullscreen);
  on(player, 'mouseup', () => { controls.hidden = !controls.hidden; refresh(); });
  on(player, 'loadedmetadata', () => container.classList.add('ogv-loaded'));
  on(document, 'fullscreenchange', refresh);
  let disposed = false;
  on(player, 'ended', () => { if (!disposed && active()) { if (player.seekable.length) player.currentTime = 0; else player.stop(); player.play(); } });
  for (const event of ['loadedmetadata', 'volumechange', 'play', 'pause', 'timeupdate']) on(player, event, refresh);
  const timer = setInterval(() => { if (!controls.hidden) refresh(); }, 500);
  return { player, controls, container, dispose() {
    if (disposed) return; disposed = true; clearInterval(timer);
    for (const remove of listeners) remove();
    // stop() aborts the stream, closes audio, terminates codec workers and clears
    // decode/render timers. Removing only the DOM node would leak that work.
    player.stop(); controls.remove(); player.remove(); container.remove();
  } };
}
