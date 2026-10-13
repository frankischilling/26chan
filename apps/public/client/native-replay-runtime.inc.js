// Appended to the unchanged pinned Tegaki source by the replay build. This
// module runs only inside the disposable replay worker.
import { decodeReplayCandidateWire } from './native-replay-data.js';
import { prepareBoundedReplayCandidate } from './native-replay-envelope.js';

let replayCreated = false;
export function createReplayRuntime(bytes, boundary) {
  if (replayCreated) throw new Error('A fresh worker is required for each replay');
  replayCreated = true;
  if (!boundary || typeof OffscreenCanvas !== 'function') throw new Error('Missing replay worker boundary');
  const candidate = decodeReplayCandidateWire(bytes);
  const viewer = new TegakiReplayViewer();
  const prepared = prepareBoundedReplayCandidate(candidate, viewer);
  const [previewWidth, previewHeight] = $T.calcThumbSize(candidate.metadata.width, candidate.metadata.height, 24);
  const preview = new OffscreenCanvas(previewWidth, previewHeight), previewContext = preview.getContext('2d');
  if (!previewContext) throw new Error('Native layer preview unavailable');
  previewContext.imageSmoothingEnabled = false;
  const previews = new Map();
  const forbid = name => () => { throw new Error(`Unavailable replay source path: ${name}`); };
  for (const name of Object.getOwnPropertyNames(TegakiReplayViewer.prototype)) {
    if (name !== 'constructor' && name !== 'getEventIdMap') TegakiReplayViewer.prototype[name] = forbid(name);
  }
  const sinks = new Set([
    'onToolChanged', 'updateToolSize', 'updateToolAlpha', 'updateToolFlow',
    'updateToolDynamics', 'updateToolShape', 'updateToolPreserveAlpha',
    'updateLayersGridAdd', 'updateLayersGridActive', 'updateLayersGridVisibility',
    'updateLayersGridSelectedClear', 'updateLayersGridSelectedSet',
    'updateLayerAlphaOpt', 'updateLayerPreview', 'updateZoomLevel',
    'updateLayersGridRemove', 'deleteLayerPreviewCtx', 'updayeLayersGridOrder',
  ]);
  for (const name of sinks) if (typeof TegakiUI[name] !== 'function') throw new Error('Pinned source UI changed');
  for (const [name, value] of Object.entries(TegakiUI)) {
    if (typeof value !== 'function' || name === 'updateUndoRedo') continue;
    TegakiUI[name] = sinks.has(name) ? () => boundary.record(`UI:${name}`) : forbid(`UI.${name}`);
  }
  TegakiUI.updateLayersGridAdd = layer => {
    boundary.record('UI:updateLayersGridAdd');
    previews.set(layer.id, new Uint8ClampedArray(previewWidth * previewHeight * 4));
  };
  TegakiUI.updateLayerPreview = layer => {
    boundary.record('UI:updateLayerPreview');
    previewContext.clearRect(0, 0, previewWidth, previewHeight);
    previewContext.drawImage(layer.canvas, 0, 0, previewWidth, previewHeight);
    previews.set(layer.id, previewContext.getImageData(0, 0, previewWidth, previewHeight).data);
  };
  for (const [name, value] of Object.entries(TegakiCursor)) {
    if (typeof value === 'function') TegakiCursor[name] = name === 'init'
      ? () => boundary.record('cursor:init-omitted') : forbid(`cursor.${name}`);
  }
  for (const name of ['updatePosOffset', 'bindGlobalEvents', 'updateCursorStatus']) {
    Tegaki[name] = () => boundary.record(`presentation:${name}-omitted`);
  }
  Tegaki.replayMode = true; Tegaki.saveReplay = false; Tegaki.replayRecorder = null;
  Tegaki.createTools(); Tegaki.bg = boundary.containers.bg;
  Tegaki.canvasCnt = boundary.containers.canvasContainer; Tegaki.layersCnt = boundary.containers.layers;
  Tegaki.replayViewer = viewer;
  for (const name of ['canvasWidth', 'canvasHeight', 'startTimeStamp', 'endTimeStamp', 'bgColor', 'toolColor', 'toolId']) {
    viewer[name] = prepared.metadata[name];
  }
  viewer.toolMap = prepared.toolMap;
  Tegaki.initFromReplay(); Tegaki.init(); Tegaki.setTool(Tegaki.defaultTool);
  if (Tegaki.replayRecorder !== null) throw new Error('Unexpected replay recording');
  const presentation = new OffscreenCanvas(1, 1), context = presentation.getContext('2d');
  if (!context) throw new Error('Native replay presentation unavailable');
  let index = 0;
  return Object.freeze({
    eventCount: prepared.events.length,
    start: prepared.events[0].timeStamp, end: prepared.events.at(-1).timeStamp,
    width: candidate.metadata.width, height: candidate.metadata.height,
    nextTimestamp: () => prepared.events[index]?.timeStamp ?? null,
    step() {
      if (index >= prepared.events.length) throw new Error('Replay is complete');
      const event = prepared.events[index];
      // The decoder has already admitted this closed source event vocabulary.
      switch (event.type) {
        case 0: case 1: case 2: case 3: case 4: case 5: case 6: case 7: case 8:
        case 10: case 11: case 12: case 13: case 14: case 15: case 16: case 17:
        case 18: case 20: case 21: case 22: case 23: case 24: case 25: case 26:
        case 27: case 254: case 255: event.dispatch(); break;
        default: throw new Error('Unsupported replay event');
      }
      index++;
      if (Tegaki.layers.length > 25) throw new Error('Replay layer limit exceeded');
      return index;
    },
    presentation() {
      const tool = Tegaki.tool;
      return { color: $T.id('tegaki-colorpicker').value || null, tool: {
        id: tool.id, size: tool.size, alpha: tool.alpha, flow: tool.flow, tipId: tool.tipId ?? 0,
        sizeDynamicsEnabled: tool.sizeDynamicsEnabled, alphaDynamicsEnabled: tool.alphaDynamicsEnabled,
        flowDynamicsEnabled: tool.flowDynamicsEnabled, preserveAlphaEnabled: tool.preserveAlphaEnabled,
      }, layers: Tegaki.layers.map(layer => {
        return { id: layer.id, alpha: layer.alpha, visible: layer.visible,
          active: Tegaki.activeLayer === layer, selected: Tegaki.selectedLayers.has(layer.id),
          width: previewWidth, height: previewHeight,
          pixels: previews.get(layer.id) };
      }) };
    },
    frame() {
      Tegaki.flatten(context);
      return presentation.transferToImageBitmap();
    },
  });
}
