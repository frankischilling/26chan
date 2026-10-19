import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
const records=JSON.parse(readFileSync('fixtures/janitor-preference-source.json'));
assert(process.argv[2], 'Supply the pinned source directory');
for(const record of Object.values(records)) {
  const raw=readFileSync(`${process.argv[2]}/${record.source}`);
  assert.equal(createHash('sha256').update(raw).digest('hex'),record.sha256);
  assert(raw.toString().replaceAll('\r\n','\n').includes(record.fragment));
}
console.log('Pinned menu, key alphabet and amendment search fragments match.');
