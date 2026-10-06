import { openSync, readSync, closeSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

// Microsoft documents the event semantics, not a versioned Windows 2025
// manifest or its address byte order. No live manifest has been verified here.
// This phase inspects metadata only. Actual capture needs a reviewed profile
// covering every level 0..4 template AND a verified raw-event decoder.
export const LIMITS = Object.freeze({ metadataBytes: 262144, outputBytes: 65536 });
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
export function gateMetadata(metadata) {
  const result = unavailable('provider-schema-unverified');
  if (!metadata || metadata.provider !== result.provider || !Array.isArray(metadata.events)) {
    return unavailable('provider-metadata-unavailable');
  }
  if (metadata.events.length > 256) return unavailable('metadata-limit');
  // This list is review material only, never proof that a manifest is safe.
  // Unknown strings, blobs, arbitrary attributes, arrays and versions reject.
  const review = [];
  let rejected = false;
  for (const event of metadata.events) {
    if (!Number.isInteger(event.id) || event.id < 0 || event.id > 65535 ||
        (event.level !== null && (!Number.isInteger(event.level) || event.level < 0 || event.level > 4)) ||
        !Number.isInteger(event.version) || event.version < 0 || event.version > 255 ||
        !Array.isArray(event.fields) || event.fields.length > 16) {
      rejected = true; continue;
    }
    if (event.level === null || event.version !== 0 || event.unsupported !== false) rejected = true;
    const safeFields = [];
    for (const field of event.fields) {
      const name = label(field.name);
      const type = label(field.type, true);
      const outType = label(field.out_type, true);
      if (!fields.has(name) || !numeric.has(type) || field.scalar !== true) rejected = true;
      // Only bounded manifest identifiers, never rendered messages or values.
      safeFields.push({ name, type, out_type: outType, scalar: field.scalar === true });
    }
    review.push({ id: event.id, version: event.version, level: event.level, fields: safeFields });
  }
  const issues = Array.isArray(metadata.issues) ? metadata.issues.slice(0, 16).filter(issue =>
    ['metadata-limit', 'provider-level-unavailable', 'provider-template-unavailable',
      'provider-metadata-unavailable'].includes(issue)) : [];
  result.reason = issues[0] ?? (rejected ? 'provider-schema-rejected' : 'provider-schema-unverified');
  result.metadata_issues = [...new Set(issues)];
  result.metadata_partial = issues.length > 0;
  result.metadata = [];
  result.metadata_truncated = false;
  for (const event of review) {
    if (result.metadata.length >= 64 || Buffer.byteLength(JSON.stringify(result)) +
        Buffer.byteLength(JSON.stringify(event)) + 128 > LIMITS.outputBytes) {
      result.metadata_truncated = true; break;
    }
    result.metadata.push(event);
  }
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
