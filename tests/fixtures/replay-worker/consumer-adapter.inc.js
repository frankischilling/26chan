// TEST-ONLY SAME-MODULE ADAPTER v1. Appended after the unchanged source and
// license notices. Never a production export or an arbitrary method dispatcher.
import { decodeReplayCandidateWire, prepareReplayCandidate } from './canonical-data.mjs';
import { loadOwnedCase } from '../load-case.mjs';

const PROBE_UI_SINKS = Object.freeze([
  'onToolChanged', 'updateToolSize', 'updateToolAlpha', 'updateToolFlow',
  'updateToolDynamics', 'updateToolShape', 'updateToolPreserveAlpha',
  'updateLayersGridAdd', 'updateLayersGridActive', 'updateLayersGridVisibility',
  'updateLayersGridSelectedClear', 'updateLayersGridSelectedSet',
  'updateLayerAlphaOpt', 'updateLayerPreview', 'updateZoomLevel',
]);
let probeCreated = false;
export async function createQualificationProbe(caseId, mode, boundary) {
  if (probeCreated) throw new Error('A fresh realm is required for every replay');
  probeCreated = true;
  if (mode !== 'original-dom' && mode !== 'worker') throw new Error('Unknown probe mode');
  const { bytes, row } = await loadOwnedCase(caseId);
  const candidate = decodeReplayCandidateWire(bytes);
  if (candidate.metadata.width > 24 || candidate.metadata.height > 24 || candidate.events.length > 96) throw new Error('Probe envelope exceeded');
  const forbid = name => () => { throw new Error(`Forbidden source path: ${name}`); };
  // No source URL/TGKR loading or scheduler in either realm. Keep the exact
  // constructors and dispatch bodies; only the viewer lifecycle is blocked.
  for (const name of Object.getOwnPropertyNames(TegakiReplayViewer.prototype)) {
    if (name !== 'constructor' && name !== 'getEventIdMap') TegakiReplayViewer.prototype[name] = forbid(`viewer.${name}`);
  }
  if (mode === 'worker') {
    if (!boundary || typeof OffscreenCanvas !== 'function') throw new Error('Missing native worker boundary');
    for (const [name, value] of Object.entries(TegakiUI)) {
      if (typeof value !== 'function' || name === 'updateUndoRedo') continue;
      TegakiUI[name] = PROBE_UI_SINKS.includes(name)
        ? () => boundary.record(`UI:${name}`) : forbid(`UI.${name}`);
    }
    for (const name of PROBE_UI_SINKS) if (typeof TegakiUI[name] !== 'function') throw new Error(`Missing audited sink ${name}`);
    for (const [name, value] of Object.entries(TegakiCursor)) {
      if (typeof value === 'function') TegakiCursor[name] = name === 'init'
        ? () => boundary.record('cursor:init-omitted') : forbid(`cursor.${name}`);
    }
    for (const name of ['updatePosOffset', 'bindGlobalEvents', 'updateCursorStatus']) {
      Tegaki[name] = () => boundary.record(`presentation:${name}-omitted`);
    }
    // Audited core-only counterpart of open({replayMode:true}), with no buildUI,
    // body insertion, listeners or pointer/viewport state. All eight tools exist.
    Tegaki.replayMode = true; Tegaki.saveReplay = false; Tegaki.replayRecorder = null;
    Tegaki.createTools();
    Tegaki.bg = boundary.containers.bg;
    Tegaki.canvasCnt = boundary.containers.canvasContainer;
    Tegaki.layersCnt = boundary.containers.layers;
    Tegaki.replayViewer = new TegakiReplayViewer();
  } else {
    // Genuine reference DOM, UI, cursor, previews, and HTML canvas methods.
    Tegaki.open({ replayMode: true, saveReplay: false });
  }
  const prepared = prepareReplayCandidate(candidate, Tegaki.replayViewer);
  const viewer = Tegaki.replayViewer;
  for (const name of ['canvasWidth', 'canvasHeight', 'startTimeStamp', 'endTimeStamp', 'bgColor', 'toolColor', 'toolId']) {
    viewer[name] = prepared.metadata[name];
  }
  viewer.toolMap = prepared.toolMap;
  Tegaki.initFromReplay(); Tegaki.init(); Tegaki.setTool(Tegaki.defaultTool);
  if (Tegaki.replayRecorder !== null) throw new Error('Recorder unexpectedly activated');
  let index = 0, presentationCount = 0;
  // This surface alone may be transferred to an ImageBitmap. Never a live layer.
  const presentation = mode === 'worker' ? new OffscreenCanvas(1, 1) : document.createElement('canvas');
  const presentationContext = presentation.getContext('2d');
  if (!presentationContext) throw new Error('Native presentation context unavailable');

  function data(value) {
    if (value === null || value === undefined || typeof value !== 'object') return typeof value === 'function' ? '<function>' : value;
    if (value instanceof ImageData) return { width: value.width, height: value.height, data: Array.from(value.data) };
    if (ArrayBuffer.isView(value)) return Array.from(value);
    if (Array.isArray(value)) return value.map(data);
    if (value instanceof Map) return Array.from(value, ([key, item]) => [key, data(item)]);
    return Object.fromEntries(Object.entries(value).filter(([, item]) => typeof item !== 'function').map(([key, item]) => [key, data(item)]));
  }
  function action(value) {
    if (!value) return null;
    const kind = Object.keys(TegakiHistoryActions).find(name => value instanceof TegakiHistoryActions[name]);
    const result = { kind };
    for (const key of ['layerId', 'aLayerIdBefore', 'aLayerIdAfter', 'newAlpha', 'layerAlphas', 'imageDataBefore', 'imageDataAfter']) {
      if (key in value) result[key] = data(value[key]);
    }
    if (value.layer) result.storedLayer = { id: value.layer.id, isLive: Tegaki.layers.includes(value.layer) };
    return result;
  }
  function inspect() {
    const H = TegakiHistory;
    return {
      eventIndex: index, dimensions: [Tegaki.baseWidth, Tegaki.baseHeight],
      toolId: Tegaki.tool.id, color: Tegaki.toolColor,
      tools: Object.fromEntries(Object.entries(Tegaki.tools).map(([name, tool]) => [name, data(tool)])),
      pressure: [TegakiPressure.pressureThen, TegakiPressure.pressureNow],
      ghost: Array.from(Tegaki.ghostBuffer.data), blend: Array.from(Tegaki.blendBuffer.data),
      active: Tegaki.activeLayer.id, selected: Array.from(Tegaki.selectedLayers), layerCounter: Tegaki.layerCounter,
      domOrder: Array.from(Tegaki.layersCnt.children).slice(1).map(canvas => +canvas.getAttribute('data-id')),
      isPainting: Tegaki.isPainting,
      layers: Tegaki.layers.map(layer => ({ id: layer.id, alpha: layer.alpha, visible: layer.visible,
        authoritative: Array.from(layer.imageData.data),
        nativeReadback: Array.from(layer.ctx.getImageData(0, 0, Tegaki.baseWidth, Tegaki.baseHeight).data),
        context: Object.fromEntries(['lineCap', 'lineJoin', 'strokeStyle', 'fillStyle', 'globalAlpha', 'lineWidth', 'globalCompositeOperation'].map(key => [key, layer.ctx[key]])),
      })),
      undo: H.undoStack.map(action), redo: H.redoStack.map(action), pending: action(H.pendingAction),
      pendingUndoIndexes: H.undoStack.flatMap((item, i) => item === H.pendingAction ? [i] : []),
      pendingRedoIndexes: H.redoStack.flatMap((item, i) => item === H.pendingAction ? [i] : []),
    };
  }
  return Object.freeze({
    eventCount: prepared.events.length,
    dispatchNext() {
      if (index >= prepared.events.length) throw new Error('Replay already complete');
      const event = prepared.events[index];
      // Closed admitted tag vocabulary, with dispatch still on the exact source class.
      switch (event.type) {
        case 0: case 1: case 2: case 3: case 4: case 5: case 6: case 7: case 8:
        case 10: case 11: case 12: case 15: case 16: case 18: case 20:
        case 24: case 25: case 26: case 27: case 254: case 255: event.dispatch(); break;
        default: throw new Error('Unsupported probe event');
      }
      index++;
    },
    inspect,
    flatten() {
      if (++presentationCount > prepared.events.length + 1) throw new Error('Presentation count exceeded');
      Tegaki.flatten(presentationContext);
      return { canvas: presentation, bytes: Array.from(presentationContext.getImageData(0, 0, Tegaki.baseWidth, Tegaki.baseHeight).data),
        presentationCount, repeatedPresentationOutsideOneTimeCostModel: true };
    },
    identity: Object.freeze({ consumer: 'tegaki-worker-probe-v1', sourceVersion: Tegaki.VERSION, caseId, bytes: row.bytes }),
  });
}
