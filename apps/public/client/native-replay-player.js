import { createReplayUI } from './native-replay-source-ui.js';
import { acquireReplay } from './native-replay-acquire.js';
import { createReplayPlayback } from './native-replay-playback.js';

export function mountReplayPlayer({ root = document, board, mediaOrigin,
  acquire = acquireReplay, playbackFactory = createReplayPlayback } = {}) {
  if (!/^[a-z0-9]{1,10}$/.test(board) || !mediaOrigin || typeof root.createElement !== 'function') return null;
  let generation = 0, request, playback, dialog, opener, disposed = false, speed = 1, phase = 'loading';
  const speeds = [.5,1,2,5,10,25];
  const node = (tag, text) => { const result = root.createElement(tag); if (text) result.textContent = text; return result; };
  function close() {
    generation++; request?.abort(); request = null; playback?.close(); playback = null;
    const old = dialog; dialog = null;
    if (old) { old.close(); old.remove(); }
    if (opener?.isConnected) opener.focus(); opener = null;
  }
  async function open(link, tim) {
    close(); if (disposed) return;
    // A replay must never destroy the painter's retained, unsaved canvas.
    if (root.documentElement.dataset.nativeDrawingActive === 'true') return;
    opener = link;
    const epoch = generation;
    const panel = node('dialog'); panel.id = 'native-replay-player'; panel.setAttribute('aria-label', 'Oekaki replay');
    const shadow = panel.attachShadow({mode: 'open'});
    const stylesheet = node('link'); stylesheet.rel = 'stylesheet'; stylesheet.href = '/static/tegaki/tegaki-0.9.4.v1.css';
    shadow.append(stylesheet);
    const zooms = [.5, 1, 2, 4, 8, 16]; let zoomIndex = 1, gapless = true;
    const ui = createReplayUI(shadow, root, {
      close,
      play: () => { if (phase === 'playing') playback?.pause(); else playback?.play(); },
      rewind: () => { void playback?.rewind(); },
      slower: () => playback?.speed(speeds[Math.max(0, speeds.indexOf(speed) - 1)]),
      faster: () => playback?.speed(speeds[Math.min(speeds.length - 1, speeds.indexOf(speed) + 1)]),
      gapless: () => { gapless = !gapless; playback?.gapless(gapless); },
      zoom: direction => { zoomIndex = Math.max(0, Math.min(zooms.length - 1, zoomIndex + direction)); zoom(); },
    });
    function zoom() { ui.zoom(zooms[zoomIndex], zoomIndex, zooms.length); }
    function changed(state) {
      if (disposed || epoch !== generation || dialog !== panel) return;
      phase = state.phase; speed = state.speed; ui.changed(state);
    }
    panel.addEventListener('cancel', event => { event.preventDefault(); close(); });
    root.body.append(panel); dialog = panel;
    changed({phase:'loading',speed:1}); zoom(); panel.showModal(); ui.focus();
    request = new AbortController();
    try {
      const bytes = await acquire({board,mediaOrigin,tim,signal:request.signal});
      if(disposed || epoch !== generation || dialog !== panel) return;
      request = null;
      playback = playbackFactory(bytes,{ changed, frame(bitmap, receipt) {
        if(epoch !== generation || dialog !== panel) return;
        ui.frame(bitmap, receipt); zoom();
      }});
      await playback.ready;
      if(epoch === generation && !disposed && dialog === panel) {
        playback.play(); playback.visibility(root.visibilityState === 'visible');
      }
    } catch {
      if(epoch === generation && !disposed && dialog === panel) changed({phase:'failed',speed:1});
    }
  }
  function click(event) {
    const link=event.target?.closest?.('a[data-drawing-replay]');
    if(!link || !root.contains(link) || !link.closest('.postMessage') || !link.closest('.postContainer')) return;
    const tim=link.dataset.drawingReplay;
    if(!/^[1-9][0-9]{0,15}$/.test(tim) || BigInt(tim)>9007199254740991n || link.getAttribute('href')!==`#replay-${tim}`) return;
    event.preventDefault(); void open(link,tim);
  }
  const hide = () => close(), visibility = () => { playback?.visibility(root.visibilityState === 'visible'); };
  root.addEventListener('click',click); root.addEventListener('visibilitychange',visibility);
  root.defaultView?.addEventListener('pagehide',hide); root.defaultView?.addEventListener('popstate',hide);
  return { close, dispose() {
    if(disposed) return; disposed=true; close(); root.removeEventListener('click',click); root.removeEventListener('visibilitychange',visibility);
    root.defaultView?.removeEventListener('pagehide',hide); root.defaultView?.removeEventListener('popstate',hide);
  }};
}

const context = document.getElementById('watcher-context');
if (context && context.dataset.catalog !== 'true') mountReplayPlayer({board:context.dataset.board,mediaOrigin:context.dataset.mediaOrigin});
