// Independently written, literal source-grounded expectations. This module
// imports neither the corpus encoder, decoder, generated source nor adapter.
// Event positions are intentional frozen transcript checkpoints, not computed
// by replaying the same preparation/dispatch implementation under comparison.
import { equal } from './comparison.mjs';
const INITIAL_TOOL = Object.freeze({ 'tone-initial-and-settings': 5, 'tiny-pen': 2 });
const pixel = (bytes, width, x, y) => bytes.slice(4 * (y * width + x), 4 * (y * width + x) + 4);
export function checkSemanticCheckpoint(caseId, state, flattened, realm) {
  let assertions = 0;
  const check = (actual, expected, name) => { equal(actual, expected, `${realm} semantic ${caseId}@${state.eventIndex}: ${name}`); assertions++; };
  const truth = (condition, name) => check(condition, true, name);
  const index = state.eventIndex, layer = id => state.layers.find(item => item.id === id);
  const kinds = stack => stack.map(action => action.kind);
  if (index === 0) {
    check(state.toolId, INITIAL_TOOL[caseId] || 1, 'initial tool');
    check(state.pressure, [0, 0], 'initial source pressure');
    check(state.layers.map(item => [item.id, item.alpha, item.visible]), [[1, 1, true]], 'initial layer');
    check(state.active, 1, 'initial active layer'); check(state.selected, [1], 'initial selection');
    check(state.undo, [], 'fresh undo'); check(state.redo, [], 'fresh redo'); check(state.pending, null, 'fresh pending');
    truth(layer(1).authoritative.every(value => value === 0), 'transparent authoritative layer');
    truth(layer(1).nativeReadback.every(value => value === 0), 'transparent native layer');
    check(pixel(flattened, state.dimensions[0], 0, 0), [240, 246, 251, 255], 'literal background');
    for (const tool of Object.values(state.tools)) {
      check([tool.sizeDynamicsEnabled, tool.alphaDynamicsEnabled, tool.flowDynamicsEnabled], [false, false, false], `disabled dynamics ${tool.id}`);
    }
  }
  if (caseId === 'pencil-pressure') {
    if (index === 0) check(state.dimensions, [24, 24], 'literal pencil geometry');
    if (index === 2) {
      check(state.pressure, [49151 / 65535, 49151 / 65535], 'encoded start pressure');
      check(state.pending.kind, 'Draw', 'source pending Draw'); check(state.pending.layerId, 1, 'pending target');
      check(state.undo.length, 0, 'start is not committed'); check(state.isPainting, false, 'replay start does not set isPainting');
      truth(state.pending.imageDataBefore.data.every(value => value === 0), 'Draw before snapshot');
    }
    if (index >= 2) {
      // Source size-one pencil kernel has alpha=255. Disabled dynamics and
      // alpha/flow=1 make the start pixel exactly the recorded RGB, opaque.
      check(pixel(layer(1).authoritative, 24, 3, 4), [43, 61, 79, 255], 'literal pencil start authoritative');
      check(pixel(layer(1).nativeReadback, 24, 3, 4), [43, 61, 79, 255], 'literal pencil start native');
      check(pixel(flattened, 24, 3, 4), [43, 61, 79, 255], 'literal pencil start flattened');
      check(pixel(flattened, 24, 0, 0), [240, 246, 251, 255], 'untouched background');
    }
    if (index >= 3) check(state.pressure, [49151 / 65535, 32768 / 65535], 'encoded draw pressure');
    if (index >= 4) {
      check(kinds(state.undo), ['Draw'], 'committed history'); check(state.pendingUndoIndexes, [0], 'pending Draw alias');
      truth(state.ghost.every(value => value === 0) && state.blend.every(value => value === 0), 'commit clears both buffers');
    }
  }
  if (caseId === 'tool-1' && index === 11) {
    check(state.pressure, [0.5 / 65535, 0.5 / 65535], 'source NoP divisor, not normalized one-half');
    check(state.tools.pencil.alpha, 0.625, 'literal alpha setter'); check(state.tools.pencil.flow, 0.375, 'literal flow setter');
  }
  if (caseId === 'tool-2' && index === 10) {
    check(state.tools.pen.flow, 0.375, 'pen raw flow');
    check(state.tools.pen.brushFlow, 1 - Math.sqrt(1 - Math.pow(0.375, 3)), 'source pen flow easing');
  }
  if (caseId === 'tone-initial-and-settings' && index === 0) {
    check([state.tools.tone.mapWidth, state.tools.tone.mapHeight, state.tools.tone.mapCache.length], [24, 24, 16], 'initial tone cache');
  }
  if (caseId === 'layer-history-order') {
    if (index === 15) {
      check(state.layers.map(item => item.id), [1, 4, 2, 3], 'insert after selected middle layer');
      check(state.active, 4, 'new middle active'); check(state.selected, [4], 'new middle selected');
      check(state.undo.length, 6, 'source creation/history count');
    }
    if (index === 23) {
      check(state.selected, [3, 2], 'selection insertion order'); check(state.active, 3, 'zero selects top layer ID');
      check([layer(3).alpha, layer(2).alpha], [0.75, 0.75], 'coalesced alpha');
      check(state.undo.length, 8, 'coalescing does not push another action');
      check(state.pendingUndoIndexes, [6], 'earlier Draw remains pending');
    }
    if (index === 24) check([layer(3).alpha, layer(2).alpha], [1, 0.375], 'Alpha undo preserves earliest values');
    if (index === 26) check(layer(2).visible, false, 'visibility tag hides the named layer');
    if (index === 34) {
      check(state.layers.map(item => item.id), [1, 4, 2, 3], 'final insertion order'); check(state.active, 2, 'final active');
      check(state.selected, [2], 'final selection'); truth(state.layers.every(item => item.visible), 'visibility restored');
    }
  }
  if (caseId === 'alpha-coalesce-redo') {
    if (index === 9 || index === 10) {
      check(kinds(state.undo), ['Draw', 'SetLayersAlpha'], 'undo stack before retained redo');
      check(kinds(state.redo), ['Draw'], 'Draw redo survives Alpha coalescing'); check(state.pendingRedoIndexes, [0], 'pending aliases redo');
    }
    if (index === 10) {
      check(layer(1).alpha, 0.75, 'coalesced current alpha'); check(state.undo[1].layerAlphas, [[1, 1]], 'original alpha before preserved');
    }
    if (index === 13) { check(layer(1).alpha, 1, 'undo coalesced Alpha'); check(kinds(state.redo), ['Draw', 'SetLayersAlpha'], 'redo ordering'); }
  }
  if (caseId === 'eight-layers') {
    if (index === 8) { check(state.layers.map(item => item.id), [1, 2, 3, 4, 5, 6, 7, 8], 'eight creation-only IDs'); check(state.active, 8, 'eighth active'); }
    if (index === 14) { check([layer(4).alpha, layer(4).visible], [0.5, false], 'hidden fractional-alpha middle layer'); check(state.active, 4, 'visibility does not change active'); }
    if (index === 15) { check(state.active, 8, 'zero activates top ID'); check(state.selected, [8], 'top-only selection'); }
  }
  if (caseId === 'history-eviction' && index === 56) {
    check(state.undo.length, 50, 'maximum retained history'); truth(state.undo.every(action => action.kind === 'Dummy'), 'Draw was evicted');
    check(state.pending.kind, 'Draw', 'eviction does not clear pending Draw');
    check(state.pendingUndoIndexes, [], 'pending no longer in undo'); check(state.pendingRedoIndexes, [], 'pending not in redo');
    check(pixel(state.pending.imageDataAfter.data, 24, 3, 4), [43, 61, 79, 255], 'evicted pending Draw keeps its after snapshot');
  }
  return assertions;
}
