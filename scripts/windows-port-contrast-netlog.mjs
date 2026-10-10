// Diagnostic-only admitted copies. Required CI's original NetLogs are untouched.
import { readFileSync, readdirSync, writeFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { demand, sha256 } from './windows-port-contrast-core.mjs';
import { checkedFile } from '../tests/helpers/windows-port-contrast.js';
export const NETLOG_LIMIT = 12 * 1024 * 1024;
const clientFields = new Set(['name', 'version', 'cl', 'version_mod', 'official', 'os_type', 'command_line']);
export function admittedNetlog(parsed) {
  demand(parsed && typeof parsed === 'object' && !Array.isArray(parsed) && Array.isArray(parsed.events));
  demand(parsed.constants && typeof parsed.constants === 'object' && !Array.isArray(parsed.constants));
  const client = parsed.constants.clientInfo;
  demand(client && typeof client === 'object' && !Array.isArray(client));
  demand(Object.keys(client).every(key => clientFields.has(key)));
  demand(Object.entries(client).every(([key, value]) => typeof value === 'string' && Buffer.byteLength(value) <= (key === 'command_line' ? 131072 : 1024)));
  const { command_line, ...retainedClient } = client;
  // Only this reviewed metadata field is removed. Events, source dependencies,
  // clocks, enum dictionaries and polledData retain their exact parsed values.
  const copy = { ...parsed, constants: { ...parsed.constants, clientInfo: retainedClient } };
  const body = JSON.stringify(copy);
  demand(Buffer.byteLength(body) <= NETLOG_LIMIT);
  return { body, command_line_redacted: command_line !== undefined };
}
export function copyContrastNetlogs(source, destination) {
  let files;
  try { files = readdirSync(source); }
  catch (error) {
    if (error.code === 'ENOENT') return { schema: 1, status: 'absent', files: 0, command_line_redactions: 0, event_chains_preserved: true, captures: [] };
    throw new Error('Port contrast NetLog unavailable');
  }
  demand(files.length <= 4 && files.every(file => /^worker-[0-9]+-[a-zA-Z0-9-]+\.json$/.test(file)));
  const copies = files.sort().map(file => {
    const input = path.join(source, file);
    checkedFile(input, NETLOG_LIMIT);
    return admittedNetlog(JSON.parse(readFileSync(input, 'utf8')));
  });
  if (copies.length) mkdirSync(destination);
  for (const [index, copy] of copies.entries()) writeFileSync(path.join(destination, `netlog-${index + 1}.json`), copy.body, { flag: 'wx' });
  return { schema: 1, status: copies.length ? 'complete' : 'absent', files: copies.length,
    command_line_redactions: copies.filter(copy => copy.command_line_redacted).length, event_chains_preserved: true,
    captures: copies.map((copy, index) => ({ ordinal: index + 1, bytes: Buffer.byteLength(copy.body), sha256: sha256(copy.body), command_line_redacted: copy.command_line_redacted })) };
}

export function verifyAdmittedCopies(directory, status) {
  demand(Array.isArray(status?.captures) && status.captures.length <= 4 && status.files === status.captures.length);
  let names = [];
  try { names = readdirSync(directory); } catch (error) { demand(error.code === 'ENOENT'); }
  demand(names.length === status.captures.length);
  for (const [index, capture] of status.captures.entries()) {
    demand(capture.ordinal === index + 1);
    const name = `netlog-${capture.ordinal}.json`;
    demand(names.includes(name));
    const file = path.join(directory, name);
    const actual = checkedFile(file, NETLOG_LIMIT);
    demand(actual.sha256 === capture.sha256 && actual.bytes === capture.bytes);
    demand(admittedNetlog(JSON.parse(readFileSync(file, 'utf8'))).command_line_redacted === false);
  }
  return true;
}
