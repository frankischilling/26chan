// Isolated candidate preparation only. Nothing imports this module in a served
// asset. Neither result authorizes playback, storage, publication, or PNG claims.
// State, cost, provenance, acquisition limits/deadlines, and runtime isolation
// remain separate obligations. In particular, no Tegaki upload is parsed here.

export const REPLAY_WIRE_LIMITS = Object.freeze({
  headerBytes: 64, toolBytes: 24, toolCount: 8, eventBytes: 16,
  eventsOffset: 256, maxEvents: 16384, maxBytes: 262400,
});

const candidates = new WeakSet();
const typedArrayPrototype = Object.getPrototypeOf(Uint8Array.prototype);
const typedArrayTag = Object.getOwnPropertyDescriptor(typedArrayPrototype, Symbol.toStringTag).get;
const typedArrayLength = Object.getOwnPropertyDescriptor(typedArrayPrototype, 'byteLength').get;
const typedArrayBuffer = Object.getOwnPropertyDescriptor(typedArrayPrototype, 'buffer').get;
const arrayBufferLength = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength').get;
const arrayBufferResizable = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'resizable')?.get;
const copyBytes = Uint8Array.prototype.set;

export class ReplayCandidateError extends Error {
  constructor(code) {
    super(`Invalid untrusted replay candidate: ${code}`);
    this.name = 'ReplayCandidateError';
    this.code = code;
  }
}

function reject(code) { throw new ReplayCandidateError(code); }
function zero(bytes, from, to) {
  for (let i = from; i < to; i++) if (bytes[i] !== 0) reject('reserved');
}
function finite(view, offset) {
  const value = view.getFloat32(offset);
  if (!Number.isFinite(value)) reject('nonfinite-float');
  return value;
}

// Capture a bounded private snapshot using intrinsic typed-array accessors and
// copying, never an input iterator, slice/species, or caller-provided getters.
// Shared and resizable backing stores are deliberately outside this API.
function snapshot(input) {
  let length;
  try {
    if (typedArrayTag.call(input) !== 'Uint8Array') reject('input-type');
    length = typedArrayLength.call(input);
    const backing = typedArrayBuffer.call(input);
    arrayBufferLength.call(backing); // Reject SharedArrayBuffer.
    if (arrayBufferResizable?.call(backing)) reject('input-type');
  } catch { reject('input-type'); }
  if (length < 288 || length > REPLAY_WIRE_LIMITS.maxBytes) reject('input-size');
  const bytes = new Uint8Array(length);
  copyBytes.call(bytes, input);
  return bytes;
}

function payloadSize(tag) {
  switch (tag) {
    case 1: case 2: return 6;
    case 7: case 8: case 12: case 18: case 27: return 4;
    case 6: return 3;
    case 10: case 11: case 13: case 14: case 15: case 16: case 17:
    case 22: case 24: case 25: case 26: return 1;
    case 0: case 3: case 4: case 5: case 20: case 21: case 23:
    case 254: case 255: return 0;
    default: return reject('event-tag');
  }
}

function validate(bytes, view) {
  const magic = [73, 66, 82, 80, 76, 89, 48, 49]; // IBRPLY01
  if (magic.some((value, i) => bytes[i] !== value)
    || view.getUint16(8) !== 1 || view.getUint16(10) !== 64
    || view.getUint16(12) !== 1 || view.getUint16(14) !== 0
    || bytes[44] !== 0 || bytes[45] !== 9 || bytes[46] !== 4 || bytes[47] !== 1) reject('header');
  const count = view.getUint32(20);
  if (count < 2 || count > REPLAY_WIRE_LIMITS.maxEvents) reject('event-count');
  const expected = REPLAY_WIRE_LIMITS.eventsOffset + count * REPLAY_WIRE_LIMITS.eventBytes;
  if (view.getUint32(16) !== expected || bytes.length !== expected) reject('length');
  zero(bytes, 35, 36);
  zero(bytes, 48, 64);
  const width = view.getUint16(24), height = view.getUint16(26);
  if (width < 1 || width > 1024 || height < 1 || height > 1024
    || bytes[34] < 1 || bytes[34] > 8) reject('metadata');
  for (let id = 1; id <= 8; id++) {
    const offset = 64 + (id - 1) * 24;
    if (bytes[offset] !== id || (bytes[offset + 2] & 0xf0) !== 0) reject('tool');
    zero(bytes, offset + 16, offset + 24);
    finite(view, offset + 4); finite(view, offset + 8); finite(view, offset + 12);
  }
  // Complete structural preflight, including the last event, before any event
  // objects/array or source constructors are allocated. No semantic inference.
  for (let index = 0; index < count; index++) {
    const offset = 256 + index * 16, tag = bytes[offset], used = payloadSize(tag);
    zero(bytes, offset + 1, offset + 4);
    zero(bytes, offset + 8 + used, offset + 16);
    if ((index === 0) !== (tag === 0) || (index === count - 1) !== (tag === 255)) reject('markers');
    if (tag === 10 && (bytes[offset + 8] < 1 || bytes[offset + 8] > 8)) reject('tool');
    if ([13, 14, 16, 17].includes(tag) && bytes[offset + 8] > 1) reject('boolean');
    if (tag === 12 || tag === 18 || tag === 27) finite(view, offset + 8);
  }
  return count;
}

function rgb(bytes, offset) {
  return Object.freeze([bytes[offset], bytes[offset + 1], bytes[offset + 2]]);
}

/** Decode one already-acquired Uint8Array (Buffer works), with exact framing.
 * All 28 structural tags are recognized. Raw float32 values become exact JS
 * Numbers, including -0; signed coordinates and absent pressure stay distinct.
 * Success says nothing about chronology, state, tool ranges, cost, or safety.
 * The private brand proves only that this module made the immutable snapshot.
 */
export function decodeReplayCandidateWire(input) {
  const bytes = snapshot(input), view = new DataView(bytes.buffer);
  const count = validate(bytes, view);
  const metadata = Object.freeze({
    startedAtSeconds: view.getUint32(36), endedAtSeconds: view.getUint32(40),
    width: view.getUint16(24), height: view.getUint16(26),
    background: rgb(bytes, 28), color: rgb(bytes, 31), toolId: bytes[34],
  });
  const tools = [];
  for (let id = 1; id <= 8; id++) {
    const offset = 64 + (id - 1) * 24, flags = bytes[offset + 2];
    tools.push(Object.freeze({
      id, size: bytes[offset + 1], alpha: view.getFloat32(offset + 4),
      step: view.getFloat32(offset + 8), flow: view.getFloat32(offset + 12),
      sizeDynamics: !!(flags & 1), alphaDynamics: !!(flags & 2),
      usePreserveAlpha: !!(flags & 4), flowDynamics: !!(flags & 8),
      tipId: view.getInt8(offset + 3),
    }));
  }
  const events = [];
  for (let index = 0; index < count; index++) {
    const offset = 256 + index * 16, tag = bytes[offset];
    const event = { tag, timestampMs: view.getUint32(offset + 4) };
    switch (tag) {
      case 1: case 2:
        event.x = view.getInt16(offset + 8); event.y = view.getInt16(offset + 10);
        event.pressure = view.getUint16(offset + 12); break;
      case 7: case 8:
        event.x = view.getInt16(offset + 8); event.y = view.getInt16(offset + 10); break;
      case 6: event.color = rgb(bytes, offset + 8); break;
      case 12: case 18: case 27: event.value = view.getFloat32(offset + 8); break;
      default: if (payloadSize(tag) === 1) event.value = bytes[offset + 8];
    }
    events.push(Object.freeze(event));
  }
  const result = Object.freeze({
    kind: 'untrusted-replay-candidate', candidateProfile: 1,
    metadata, tools: Object.freeze(tools), events: Object.freeze(events),
  });
  candidates.add(result);
  return result;
}

/** Prepare source-shaped own data, without installing it on the supplied viewer.
 * The provider MUST be a trusted, unmodified Tegaki 0.9.4 replay viewer from
 * production SHA-256 daea182c52df0c032eadbecb4de8f91f634a61bf82aaf35dda077fab50e68744.
 * This function cannot authenticate an arbitrary executable provider. Its only
 * runtime calls are getEventIdMap and the real constructors returned by it.
 *
 * Dynamics-change and delete/move/merge tags are outside this subset. Core state
 * restrictions are NOT enforced here. Frozen results are still untrusted and
 * intentionally omit loaded/playing/duration and any playback authorization.
 * Event own fields are frozen; source constructor prototypes belong to the
 * trusted realm and are not modified/frozen here. Their methods are never run.
 */
export function prepareReplayCandidate(candidate, replayViewer) {
  if (!candidates.has(candidate)) reject('candidate-brand');
  for (const event of candidate.events) {
    if ([13, 14, 17, 21, 22, 23].includes(event.tag)) reject('unsupported-core-tag');
  }
  if (!replayViewer || typeof replayViewer.getEventIdMap !== 'function') reject('constructor-provider');
  const constructors = replayViewer.getEventIdMap();
  if (!constructors || typeof constructors !== 'object') reject('constructor-map');
  // Snapshot and check all used constructors before constructing even Prelude.
  const selected = new Map();
  for (const event of candidate.events) {
    if (selected.has(event.tag)) continue;
    const descriptor = Object.getOwnPropertyDescriptor(constructors, event.tag);
    if (!descriptor || typeof descriptor.value !== 'function') reject('constructor-map');
    selected.set(event.tag, descriptor.value);
  }
  const source = candidate.metadata;
  const metadata = Object.freeze({
    canvasWidth: source.width, canvasHeight: source.height,
    startTimeStamp: source.startedAtSeconds * 1000, endTimeStamp: source.endedAtSeconds * 1000,
    bgColor: source.background, toolColor: source.color, toolId: source.toolId,
  });
  const toolMap = {};
  for (const tool of candidate.tools) {
    toolMap[tool.id] = Object.freeze({
      id: tool.id, size: tool.size, alpha: tool.alpha, step: tool.step,
      sizeDynamicsEnabled: Number(tool.sizeDynamics), alphaDynamicsEnabled: Number(tool.alphaDynamics),
      usePreserveAlpha: Number(tool.usePreserveAlpha), tipId: tool.tipId,
      flow: tool.flow, flowDynamicsEnabled: Number(tool.flowDynamics),
    });
  }
  const events = candidate.events.map(event => {
    const Constructor = selected.get(event.tag), time = event.timestampMs;
    let result;
    switch (event.tag) {
      case 1: case 2: result = new Constructor(time, event.x, event.y, event.pressure); break;
      case 7: case 8: result = new Constructor(time, event.x, event.y); break;
      case 6: result = new Constructor(time, event.color); break;
      default: result = Object.hasOwn(event, 'value')
        ? new Constructor(time, event.value) : new Constructor(time);
    }
    return Object.freeze(result);
  });
  return Object.freeze({
    kind: 'untrusted-tegaki-preparation', candidateProfile: 1,
    metadata, toolMap: Object.freeze(toolMap), events: Object.freeze(events),
  });
}
