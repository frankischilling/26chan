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
    const toolbar = node('div'); toolbar.className = 'nativeReplayToolbar';
    const status = node('span', 'Loading replay…'); status.setAttribute('role','status'); status.setAttribute('aria-live','polite');
    const canvas = node('canvas'); canvas.width = canvas.height = 1; canvas.setAttribute('aria-label','Drawing replay');
    const viewport = node('div'); viewport.className = 'nativeReplayViewport'; viewport.append(canvas);
    const zooms = [.5, 1, 2, 4, 8, 16]; let zoomIndex = 1;
    const zoomOut = node('button', 'Zoom out'), zoomIn = node('button', 'Zoom in'), zoomLabel = node('span', '100%');
    zoomLabel.className = 'nativeReplayZoom';
    function zoom() {
      canvas.style.width = `${Math.ceil(canvas.width * zooms[zoomIndex])}px`;
      canvas.style.height = `${Math.ceil(canvas.height * zooms[zoomIndex])}px`;
      zoomLabel.textContent = `${zooms[zoomIndex] * 100}%`;
      zoomOut.disabled = zoomIndex === 0; zoomIn.disabled = zoomIndex === zooms.length - 1;
    }
    zoomOut.addEventListener('click', () => { zoomIndex = Math.max(0, zoomIndex - 1); zoom(); });
    zoomIn.addEventListener('click', () => { zoomIndex = Math.min(zooms.length - 1, zoomIndex + 1); zoom(); });
    const play = node('button', 'Play'), rewind = node('button','Rewind'), slower = node('button','Slower'), faster = node('button','Faster');
    const rate = node('span','1×'), gapless = node('input'), gapLabel = node('label'), exit = node('button','Close');
    gapless.type = 'checkbox'; gapless.checked = true; gapLabel.append(gapless,'Gapless');
    for (const button of [play,rewind,slower,faster,zoomOut,zoomIn,exit]) button.type = 'button';
    function changed(state) {
      if (disposed || epoch !== generation || dialog !== panel) return;
      phase = state.phase; speed = state.speed;
      const disabled = phase === 'loading' || phase === 'failed';
      play.disabled = rewind.disabled = gapless.disabled = disabled;
      slower.disabled = disabled || speed === speeds[0]; faster.disabled = disabled || speed === speeds.at(-1);
      play.textContent = phase === 'playing' ? 'Pause' : 'Play'; rate.textContent = `${speed}×`;
      status.textContent = phase === 'failed' ? 'Could not load the replay or it exceeded playback limits.'
        : phase === 'loading' ? 'Loading replay…' : phase === 'done' ? 'Replay finished.' : '';
    }
    play.addEventListener('click',()=>{ if(phase === 'playing') playback?.pause(); else playback?.play(); });
    rewind.addEventListener('click',()=>{ void playback?.rewind(); });
    slower.addEventListener('click',()=>playback?.speed(speeds[Math.max(0,speeds.indexOf(speed)-1)]));
    faster.addEventListener('click',()=>playback?.speed(speeds[Math.min(speeds.length-1,speeds.indexOf(speed)+1)]));
    gapless.addEventListener('change',()=>playback?.gapless(gapless.checked));
    exit.addEventListener('click',close);
    panel.addEventListener('cancel',event=>{ event.preventDefault(); close(); });
    toolbar.append(play,rewind,slower,rate,faster,gapLabel,zoomOut,zoomLabel,zoomIn,exit);
    panel.append(toolbar,status,viewport); root.body.append(panel); dialog = panel;
    changed({phase:'loading',speed:1}); panel.showModal(); exit.focus();
    request = new AbortController();
    try {
      const bytes = await acquire({board,mediaOrigin,tim,signal:request.signal});
      if(disposed || epoch !== generation || dialog !== panel) return;
      request = null;
      playback = playbackFactory(bytes,{ changed, frame(bitmap) {
        if(epoch !== generation || dialog !== panel) return;
        canvas.width=bitmap.width; canvas.height=bitmap.height; zoom();
        const context=canvas.getContext('2d'); if(!context) throw new Error('Canvas unavailable');
        context.drawImage(bitmap,0,0);
      }});
      await playback.ready;
      if(epoch === generation && !disposed && dialog === panel) playback.play();
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
  const hide = () => close(), visibility = () => { if(root.visibilityState !== 'visible') playback?.pause(); };
  root.addEventListener('click',click); root.addEventListener('visibilitychange',visibility);
  root.defaultView?.addEventListener('pagehide',hide); root.defaultView?.addEventListener('popstate',hide);
  return { close, dispose() {
    if(disposed) return; disposed=true; close(); root.removeEventListener('click',click); root.removeEventListener('visibilitychange',visibility);
    root.defaultView?.removeEventListener('pagehide',hide); root.defaultView?.removeEventListener('popstate',hide);
  }};
}

const context = document.getElementById('watcher-context');
if (context && context.dataset.catalog !== 'true') mountReplayPlayer({board:context.dataset.board,mediaOrigin:context.dataset.mediaOrigin});
