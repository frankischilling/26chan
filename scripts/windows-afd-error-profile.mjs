import { AFD_PROVIDER_GUID, LIMITS } from './windows-visual-afd.mjs';

// Offline review only. Nothing in this module starts ETW or qualifies a live
// EVENT_RECORD layout. Packed little-endian bytes are a synthetic test contract.
export const ERROR_PROFILE_LIMITS = Object.freeze({ records: 128, identities: 32,
  payloadBytes: 36, outputBytes: 32768 });
export const ERROR_PROFILE_IDS = Object.freeze([1, 6, 13, 40]);
const keyword = '0x8000000000000000';
const pointer = name => [name, 'win:Pointer', 'win:HexInt64'];
const uint = name => [name, 'win:UInt32', 'xs:unsignedInt'];
const schemas = new Map([
  [1, [pointer('Process'), pointer('Endpoint'), uint('AddressFamily'),
    uint('SocketType'), uint('Protocol'), pointer('UserModePid')]],
  ...[6, 13, 40].map(id => [id, [pointer('Process'), pointer('Endpoint'),
    ['Error', 'win:UInt32', 'win:NTStatus']]]),
]);
const empty = value => Array.isArray(value) && value.length === 0;
const index = (value, array) => Number.isInteger(value) && value >= 0 && value < array.length;
const base = () => ({ schema: 1, mode: 'offline-synthetic-only', capture: 'unavailable',
  capture_ready: false, complete: false, live_layout_qualified: false,
  native_role_correlation: 'unverified' });
const profileResult = reason => ({ ...base(), metadata_profile_valid: reason === null,
  reason, event_ids: reason === null ? [...ERROR_PROFILE_IDS] : [] });

// A string boundary caps work before parsing. Never echo input or parse errors.
export function validateErrorProfile(inventoryJson) {
  if (typeof inventoryJson !== 'string' || Buffer.byteLength(inventoryJson) > LIMITS.outputBytes) {
    return profileResult('inventory-limit');
  }
  let inventory;
  try { inventory = JSON.parse(inventoryJson); } catch { return profileResult('inventory-invalid'); }
  const x = inventory;
  if (!x || x.schema !== 1 || x.inventory_schema !== 3 ||
      x.provider !== 'Microsoft-Windows-Winsock-AFD' || x.provider_guid !== AFD_PROVIDER_GUID ||
      x.provider_guid_available !== true || x.capture !== 'unavailable' || x.complete !== false ||
      !empty(x.events) || x.metadata_partial !== false || x.metadata_truncated !== false ||
      x.parser_limited !== false || x.failures_truncated !== false ||
      x.raw_descriptor_status !== 'ok' || x.raw_descriptor_api_status !== null ||
      x.raw_descriptor_complete !== true || !empty(x.raw_descriptor_issues) || !empty(x.metadata_issues) ||
      !Array.isArray(x.metadata) || x.metadata.length > LIMITS.events ||
      !Array.isArray(x.metadata_fields) || x.metadata_fields.length > LIMITS.events * LIMITS.fields ||
      !Array.isArray(x.metadata_descriptors) || x.metadata_descriptors.length > LIMITS.rawDescriptors) {
    return profileResult('inventory-unqualified');
  }
  const counts = x.metadata_counts;
  if (!counts || counts.events_total_exact !== true ||
      !['events_total', 'events_received', 'events_retained'].every(k => counts[k] === x.metadata.length) ||
      counts.unique_fields_retained !== x.metadata_fields.length) return profileResult('inventory-counts');
  let fields = 0;
  const selected = new Map();
  for (const event of x.metadata) {
    if (!event || !Number.isInteger(event.id) || event.id < 0 || event.id > 65535 ||
        !Number.isInteger(event.version) || event.version < 0 || event.version > 255 ||
        !index(event.descriptor, x.metadata_descriptors) || !Array.isArray(event.fields) ||
        event.fields.length > LIMITS.fields || !event.fields.every(i => index(i, x.metadata_fields))) {
      return profileResult('inventory-reference');
    }
    fields += event.fields.length;
    if (!schemas.has(event.id)) continue;
    if (selected.has(event.id) || event.version !== 0 || event.level !== 4 || !empty(event.rejections)) {
      return profileResult('candidate-identity');
    }
    const d = x.metadata_descriptors[event.descriptor];
    if (!d || d.channel_id !== 16 || d.opcode !== 0 || d.task !== 0 || d.keyword_mask !== keyword ||
        d.raw_descriptor_complete !== true ||
        d.channel_name !== 'Microsoft-Windows-Winsock-AFD/Operational' ||
        !d.available || !['opcode', 'task', 'channel_name', 'keyword_values'].every(k => d.available[k] === true) ||
        !Array.isArray(d.keyword_values) || d.keyword_values.length !== 1 || d.keyword_values[0] !== keyword) {
      return profileResult('candidate-descriptor');
    }
    const expected = schemas.get(event.id);
    if (event.fields.length !== expected.length) return profileResult('candidate-schema');
    for (let i = 0; i < expected.length; i++) {
      const f = x.metadata_fields[event.fields[i]];
      const [name, type, outType] = expected[i];
      // The inventory's broader legacy gate has not reviewed AddressFamily.
      // This narrow profile reviews its exact UInt32 shape independently.
      const allowedRejections = name === 'AddressFamily' ? ['field-name-unreviewed'] : [];
      if (!f || f.name !== name || f.type !== type || f.out_type !== outType || f.scalar !== true ||
          f.count !== null || f.length !== null || !Array.isArray(f.attributes) ||
          f.attributes.length !== 3 || !f.attributes.every(a => typeof a === 'string') ||
          [...f.attributes].sort().join(',') !== 'inType,name,outType' ||
          !Array.isArray(f.rejections) || f.rejections.length > allowedRejections.length ||
          !f.rejections.every(r => allowedRejections.includes(r))) return profileResult('candidate-schema');
    }
    selected.set(event.id, true);
  }
  if (counts.fields_received !== fields || counts.fields_retained !== fields) return profileResult('inventory-counts');
  if (selected.size !== ERROR_PROFILE_IDS.length) return profileResult('candidate-missing');
  return profileResult(null);
}

// Call only with constructed test records. The origin marker is an assertion by
// the caller, not provenance verification or permission to ingest runtime data.
// No raw-file entry point is provided. All identity keys stay in this invocation.
export function qualifySyntheticErrorRun(inventoryJson, records) {
  const result = { ...base(), synthetic_qualified: false, reason: null,
    records_truncated: false, events: [] };
  const fail = (reason, truncated = false) => ({ ...result, reason, records_truncated: truncated, events: [] });
  if (!validateErrorProfile(inventoryJson).metadata_profile_valid) return fail('profile-unqualified');
  if (!Array.isArray(records) || records.length === 0) return fail('synthetic-records-invalid');
  if (records.length > ERROR_PROFILE_LIMITS.records) return fail('record-limit', true);
  const identities = new Map();
  let ordinal = 0;
  for (const record of records) {
    if (!record || record.origin !== 'synthetic' || record.provider_guid !== AFD_PROVIDER_GUID ||
        !schemas.has(record.id) || record.version !== 0 || record.channel !== 16 || record.level !== 4 ||
        record.task !== 0 || record.opcode !== 0 || record.keyword_mask !== keyword ||
        ![4, 8].includes(record.pointer_bytes) || !Buffer.isBuffer(record.payload)) return fail('synthetic-record-invalid');
    const width = record.pointer_bytes;
    const expected = record.id === 1 ? 3 * width + 12 : 2 * width + 4;
    if (record.payload.length !== expected || record.payload.length > ERROR_PROFILE_LIMITS.payloadBytes) {
      return fail('synthetic-length-invalid');
    }
    const b = record.payload;
    const readPointer = offset => width === 4 ? BigInt(b.readUInt32LE(offset)) : b.readBigUInt64LE(offset);
    const process = readPointer(0);
    const endpoint = readPointer(width);
    if (process === 0n || endpoint === 0n) return fail('synthetic-identity-invalid');
    const key = `${width}:${process}:${endpoint}`;
    if (record.id === 1) {
      // Repeated creation always starts a fresh generation. PID is validated
      // as a pointer-sized field but never used to assign native process roles.
      if (readPointer(2 * width + 12) === 0n) return fail('synthetic-identity-invalid');
      if (!identities.has(key) && identities.size >= ERROR_PROFILE_LIMITS.identities) return fail('identity-limit', true);
      identities.set(key, ++ordinal);
      result.events.push({ id: 1, socket_ordinal: ordinal, correlation: 'synthetic-candidate' });
    } else {
      const status = b.readUInt32LE(2 * width);
      const socket = identities.get(key) ?? null;
      result.events.push({ id: record.id, socket_ordinal: socket,
        correlation: socket === null ? 'unmatched' : 'synthetic-candidate',
        ntstatus: `0x${status.toString(16).padStart(8, '0')}`,
        ntstatus_failure: status >= 0x80000000 });
      // Close retires the identity even when the status has its high bit set.
      // Missing lifecycle evidence cannot be repaired by timestamp proximity.
      if (record.id === 13) identities.delete(key);
    }
    if (Buffer.byteLength(JSON.stringify(result)) > ERROR_PROFILE_LIMITS.outputBytes) return fail('output-limit', true);
  }
  identities.clear();
  return { ...result, synthetic_qualified: true };
}
