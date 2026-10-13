// Appended inside a function-scoped copy of the pinned source. Only its UI
// builders run here. The painter, parser, event dispatcher and renderer remain
// inaccessible; display values come from the checked disposable-worker receipt.
const createTools = Tegaki.createTools;
for (const [key, value] of Object.entries(Tegaki)) {
  if (typeof value === 'function') Tegaki[key] = () => {};
}
for (const [key, value] of Object.entries(TegakiUI)) {
  if (key.startsWith('on') && typeof value === 'function') TegakiUI[key] = () => {};
}
Tegaki.replayMode = true;
createTools();
Tegaki.onCloseViewerClick = callbacks.close;
Tegaki.onReplayPlayPauseClick = callbacks.play;
Tegaki.onReplayRewindClick = callbacks.rewind;
Tegaki.onReplaySlowDownClick = callbacks.slower;
Tegaki.onReplaySpeedUpClick = callbacks.faster;
Tegaki.onReplayGaplessClick = callbacks.gapless;
Tegaki.onZoomChange = event => callbacks.zoom(event.currentTarget.hasAttribute('data-in') ? 1 : -1);
const [shell, viewport, layers] = TegakiUI.buildUI();
Tegaki.bg = shell;
shadow.append(shell);
TegakiUI.setReplayMode(true);
document.getElementById('tegaki-filepicker').remove();
for (const input of shell.querySelectorAll('input')) {
  input.inert = true; input.tabIndex = -1; input.setAttribute('aria-disabled', 'true');
}
const canvas = owner.createElement('canvas'); canvas.id = 'tegaki-canvas';
canvas.width = canvas.height = 1; canvas.setAttribute('aria-label', 'Drawing replay'); layers.append(canvas);
const status = document.getElementById('tegaki-status-output');
status.setAttribute('role', 'status'); status.setAttribute('aria-live', 'polite');
const controls = {
  play: document.getElementById('tegaki-replay-play-btn'),
  rewind: shell.querySelector('.tegaki-to-start'),
  slower: document.getElementById('tegaki-replay-slower-btn'),
  faster: document.getElementById('tegaki-replay-faster-btn'),
  close: document.getElementById('tegaki-finish-btn'),
  zoomIn: document.getElementById('tegaki-zoomin-btn'),
  zoomOut: document.getElementById('tegaki-zoomout-btn'),
  gapless: document.getElementById('tegaki-replay-gapless-btn'),
};
for (const [key, button] of Object.entries(controls)) {
  button.setAttribute('role', key === 'gapless' ? 'checkbox' : 'button'); button.tabIndex = 0;
  button.setAttribute('aria-label', {play:'Play',rewind:'Rewind',slower:'Slower',faster:'Faster',close:'Close',zoomIn:'Zoom in',zoomOut:'Zoom out',gapless:'Gapless'}[key]);
  button.addEventListener('click', event => {
    if (button.getAttribute('aria-disabled') === 'true') { event.preventDefault(); event.stopImmediatePropagation(); }
  }, true);
  button.addEventListener('keydown', event => {
    if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); button.click(); }
  });
}
const disable = (button, disabled) => {
  button.setAttribute('aria-disabled', String(disabled)); button.classList.toggle('tegaki-disabled', disabled);
};
const tools = Object.values(Tegaki.tools);

return {
  canvas, focus: () => controls.close.focus(),
  zoom(value, index, count) {
    canvas.style.width = `${Math.ceil(canvas.width * value)}px`; canvas.style.height = `${Math.ceil(canvas.height * value)}px`;
    layers.style.width = canvas.style.width; layers.style.height = canvas.style.height;
    document.getElementById('tegaki-zoom-lbl').textContent = `${value * 100}%`;
    disable(controls.zoomOut, index === 0); disable(controls.zoomIn, index === count - 1);
  },
  changed(state) {
    const disabled = state.phase === 'loading' || state.phase === 'failed';
    TegakiUI.enableReplayControls(!disabled);
    for (const key of ['rewind','gapless']) disable(controls[key], disabled);
    disable(controls.play, disabled || state.phase === 'done');
    disable(controls.slower, disabled || state.speed === .5); disable(controls.faster, disabled || state.speed === 25);
    controls.play.classList.toggle('tegaki-play', state.phase !== 'playing');
    controls.play.classList.toggle('tegaki-pause', state.phase === 'playing');
    const label = state.phase === 'playing' ? 'Pause' : 'Play';
    controls.play.title = label; controls.play.setAttribute('aria-label', label);
    controls.gapless.setAttribute('aria-checked', String(state.gapless !== false));
    document.getElementById('tegaki-replay-gapless-cb').classList.toggle('tegaki-ui-cb-a', state.gapless !== false);
    document.getElementById('tegaki-replay-speed-lbl').textContent = state.speed.toFixed(1);
    document.getElementById('tegaki-replay-now-lbl').textContent = $T.msToHms(state.elapsed || 0);
    document.getElementById('tegaki-replay-end-lbl').textContent = $T.msToHms(state.duration || 0);
    const message = state.phase === 'failed' ? 'Could not load the replay or it exceeded playback limits.'
      : state.phase === 'loading' ? 'Loading replay…' : state.phase === 'done' ? 'Replay finished.' : '';
    if (status.textContent !== message) status.textContent = message;
  },
  frame(bitmap, receipt) {
    canvas.width = bitmap.width; canvas.height = bitmap.height;
    const context = canvas.getContext('2d'); if (!context) throw new Error('Canvas unavailable');
    context.drawImage(bitmap, 0, 0);
    Tegaki.baseWidth = bitmap.width; Tegaki.baseHeight = bitmap.height;
    const tool = tools.find(tool => tool.id === receipt.tool.id);
    Object.assign(tool, receipt.tool); Tegaki.tool = tool; Tegaki.toolColor = receipt.color;
    for (const item of tools) document.getElementById(`tegaki-tool-btn-${item.name}`).classList.toggle('tegaki-tool-active', item === tool);
    document.getElementById('tegaki-color').style.backgroundColor = receipt.color;
    document.getElementById('tegaki-colorpicker').value = receipt.color;
    // This source callback only updates read-only controls. No source event runs.
    TegakiUI.updateToolSize(); TegakiUI.updateToolAlpha(); TegakiUI.updateToolFlow(); TegakiUI.updateToolModes();
    document.getElementById('tegaki-toolmode-bar').classList.remove('tegaki-hidden');
    const grid = document.getElementById('tegaki-layers-grid'); grid.replaceChildren();
    for (const layer of receipt.layers) {
      const row = TegakiUI.buildLayerGridCell({id: layer.id, name: `Layer ${layer.id}`, visible: layer.visible});
      row.draggable = false; row.classList.toggle('tegaki-layers-cell-a', layer.active); row.classList.toggle('tegaki-layers-cell-s', layer.selected);
      const preview = row.querySelector('canvas'); preview.width = layer.width; preview.height = layer.height;
      preview.getContext('2d').putImageData(new ImageData(layer.pixels, layer.width, layer.height), 0, 0);
      grid.prepend(row);
      if (layer.active) document.getElementById('tegaki-layer-alpha-opt').value = Math.round(layer.alpha * 100);
    }
  },
};
