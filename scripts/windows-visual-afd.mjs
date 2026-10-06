import { openSync, readSync, closeSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

// Microsoft documents the event semantics, not a versioned Windows 2025
// manifest or its address byte order. No live manifest has been verified here.
// This phase inspects metadata only. Actual capture needs a reviewed profile
// covering every level 0..4 template AND a verified raw-event decoder.
export const LIMITS = Object.freeze({ metadataBytes: 262144, outputBytes: 65536, events: 256, fields: 16, attributes: 16 });
const fields = new Set(['Process', 'Endpoint', 'SocketType', 'Protocol', 'UserModePid',
  'Address', 'Port', 'Status', 'Error', 'Reason']);
const numeric = new Set(['win:UInt8', 'win:UInt16', 'win:UInt32', 'win:UInt64',
  'win:Int8', 'win:Int16', 'win:Int32', 'win:Int64', 'win:HexInt32', 'win:HexInt64',
  'win:Pointer', 'win:GUID', 'win:Boolean']);
const label = (value, type = false) => typeof value === 'string' &&
  (type ? /^[A-Za-z][A-Za-z0-9]{0,15}:[A-Za-z][A-Za-z0-9]{0,47}$/ : /^[A-Za-z_][A-Za-z0-9_]{0,63}$/).test(value) ? value : 'unknown';
const reasons = new Set(['provider-metadata-unavailable', 'provider-schema-unverified',
  'provider-schema-rejected', 'provider-level-unavailable', 'provider-template-unavailable',
  'metadata-limit', 'diagnostic-unavailable']);
export function unavailable(reason = 'diagnostic-unavailable') {
  return { schema: 1, provider: 'Microsoft-Windows-Winsock-AFD', capture: 'unavailable',
    reason: reasons.has(reason) ? reason : 'diagnostic-unavailable',
    events: [], counts: { scanned: 0, correlated: 0, failures: 0 },
    events_lost: null, buffers_lost: null, circular_overwrite: null,
    complete: false, parser_limited: false, failures_truncated: false };
}
// Only manifest dimensions: constants or references to other manifest fields.
// Arbitrary attribute values (maps, messages, paths, etc.) are never retained.
const dimension = value => value === undefined || value === null || value === '' ? null :
  typeof value === 'string' && /^(0|[1-9][0-9]{0,4})$/.test(value) && Number(value) <= 65535
    ? value : label(value);
const templateReasons = new Set(['template-unavailable', 'template-limit',
  'template-node-unsupported', 'template-field-limit', 'field-attribute-limit']);
const issueReasons = new Set(['metadata-limit', 'provider-level-unavailable',
  'provider-template-unavailable', 'provider-metadata-unavailable']);
const boundedCount = (value, max) => Number.isInteger(value) && value >= 0 && value <= max;
export function gateMetadata(metadata) {
  const result = unavailable('provider-schema-unverified');
  if (!metadata || metadata.provider !== result.provider || !Array.isArray(metadata.events)) {
    return unavailable('provider-metadata-unavailable');
  }
  if (metadata.events.length > LIMITS.events) return unavailable('metadata-limit');
  const issues = Array.isArray(metadata.issues) ? metadata.issues.slice(0, 16).filter(issue =>
    issueReasons.has(issue)) : [];
  result.inventory_schema = 1;
  result.metadata_issues = [...new Set(issues)];
  result.metadata_partial = issues.length > 0;
  // Event fields reference this deduplicated table by zero-based index. This
  // preserves every bounded descriptor without repeating long names per event.
  result.metadata_fields = [];
  result.metadata = [];
  result.metadata_truncated = false;
  result.rejection_counts = {};
  const suppliedTotal = boundedCount(metadata.events_total, 4096) &&
    metadata.events_total >= metadata.events.length;
  result.metadata_counts = { events_total: suppliedTotal ? metadata.events_total : metadata.events.length,
    events_total_exact: suppliedTotal ? metadata.events_total_exact === true : !result.metadata_partial,
    events_received: metadata.events.length, events_retained: 0,
    fields_received: 0, fields_retained: 0, unique_fields_retained: 0 };
  const review = [];
  const reject = (list, reason) => {
    if (!list.includes(reason)) list.push(reason);
    result.rejection_counts[reason] = (result.rejection_counts[reason] ?? 0) + 1;
  };
  for (const event of metadata.events) {
    const rejected = [];
    if (!event || !boundedCount(event.id, 65535) ||
        (event.level !== null && !boundedCount(event.level, 4)) ||
        !boundedCount(event.version, 255) || !Array.isArray(event.fields) ||
        event.fields.length > LIMITS.fields) {
      reject(rejected, 'event-shape-invalid'); result.metadata_partial = true; continue;
    }
    if (event.level === null) reject(rejected, 'level-unavailable');
    if (event.version !== 0) reject(rejected, 'version-unreviewed');
    if (event.unsupported !== false) reject(rejected, 'template-unsupported');
    if (Array.isArray(event.rejections)) {
      for (const reason of new Set(event.rejections.slice(0, 16))) {
        if (templateReasons.has(reason)) {
          reject(rejected, reason); result.metadata_partial = true;
        }
      }
    }
    const safeFields = [];
    for (const field of event.fields) {
      result.metadata_counts.fields_received++;
      const name = label(field?.name);
      const type = label(field?.type, true);
      const outType = field?.out_type === '' || field?.out_type === undefined ? null : label(field?.out_type, true);
      const fieldRejections = [];
      if (!fields.has(name)) reject(fieldRejections, 'field-name-unreviewed');
      if (!numeric.has(type)) reject(fieldRejections, 'field-type-unreviewed');
      if (field?.scalar !== true) reject(fieldRejections, 'field-nonscalar');
      const attributes = Array.isArray(field?.attributes)
        ? [...new Set(field.attributes.slice(0, LIMITS.attributes).map(value => label(value)))].sort() : [];
      if (Array.isArray(field?.attributes) && field.attributes.length > LIMITS.attributes) {
        reject(fieldRejections, 'field-attribute-limit'); result.metadata_partial = true;
      }
      if (attributes.some(value => !['name', 'inType', 'outType'].includes(value))) {
        reject(fieldRejections, 'field-attributes-unreviewed');
      }
      const count = dimension(field?.count);
      const length = dimension(field?.length);
      if (count !== null || length !== null) reject(fieldRejections, 'field-dimensions-unreviewed');
      if (name === 'unknown' || type === 'unknown' || outType === 'unknown' ||
          attributes.includes('unknown') || count === 'unknown' || length === 'unknown') {
        reject(fieldRejections, 'field-identifier-invalid');
      }
      safeFields.push({ name, type, out_type: outType, scalar: field?.scalar === true,
        attributes, count, length, rejections: fieldRejections });
    }
    review.push({ id: event.id, version: event.version, level: event.level,
      fields: safeFields, rejections: rejected });
  }
  result.reason = issues[0] ?? (Object.keys(result.rejection_counts).length
    ? 'provider-schema-rejected' : 'provider-schema-unverified');
  if (result.metadata_counts.events_total > metadata.events.length ||
      !result.metadata_counts.events_total_exact) result.metadata_partial = true;
  // Reserve nothing heuristically: serialize the actual complete envelope after
  // each addition, and roll back that event and its new descriptors if too large.
  const indices = new Map();
  for (const event of review) {
    const previousFields = result.metadata_fields.length;
    const added = [];
    const references = event.fields.map(field => {
      const key = JSON.stringify(field);
      if (!indices.has(key)) {
        indices.set(key, result.metadata_fields.length);
        result.metadata_fields.push(field); added.push(key);
      }
      return indices.get(key);
    });
    result.metadata.push({ ...event, fields: references });
    result.metadata_counts.events_retained++;
    result.metadata_counts.fields_retained += references.length;
    result.metadata_counts.unique_fields_retained = result.metadata_fields.length;
    if (Buffer.byteLength(JSON.stringify(result)) > LIMITS.outputBytes) {
      result.metadata.pop();
      result.metadata_fields.length = previousFields;
      for (const key of added) indices.delete(key);
      result.metadata_counts.events_retained--;
      result.metadata_counts.fields_retained -= references.length;
      result.metadata_counts.unique_fields_retained = previousFields;
      result.metadata_truncated = true;
      break;
    }
  }
  result.metadata_truncated ||= result.metadata.length !== metadata.events.length ||
    result.metadata_counts.events_total > metadata.events.length;
  return result;
}

export function encode(result) {
  const output = JSON.stringify(result);
  return Buffer.byteLength(output) <= LIMITS.outputBytes ? output : JSON.stringify(unavailable('metadata-limit'));
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [input, output] = process.argv.slice(2);
  let result;
  try {
    const fd = openSync(input, 'r');
    const buffer = Buffer.alloc(LIMITS.metadataBytes + 1);
    let bytes = 0;
    try {
      while (bytes < buffer.length) {
        const read = readSync(fd, buffer, bytes, buffer.length - bytes, null);
        if (!read) break;
        bytes += read;
      }
    } finally { closeSync(fd); }
    result = bytes <= LIMITS.metadataBytes
      ? gateMetadata(JSON.parse(buffer.subarray(0, bytes).toString('utf8')))
      : unavailable('metadata-limit');
  } catch { result = unavailable('provider-metadata-unavailable'); }
  writeFileSync(output, encode(result), { encoding: 'utf8', flag: 'wx' });
}
