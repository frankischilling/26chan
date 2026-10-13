import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
const pins = {
  "yotsuba": [
    "css/yotsubanew.css",
    "05d61b9a886aca60bd418b63a5b86d2ef3050d1d90c640da64f2206dda740db9",
    "34df7f82bbf620645ecbd9f24d8179404b638760f20b69bbbf8295ea0007ae14"
  ],
  "yotsuba-b": [
    "css/yotsubluenew.css",
    "d6abc7c9fb8c2d9c168554b39a54b4569f68243aa4bcb972b8452264cbdd1a65",
    "2e11ac0939e14d7e85dc6b72dbfbd9653f31beaa599619977287828d369113dc"
  ],
  "futaba": [
    "css/futabanew.css",
    "8beb159cfcf83d042a68b1037997511c7b0bb959289eb1b5757036cdd7aa0945",
    "117e2b7f2e523ba437d9954808fc988551c4071ad36d323f74cd04291c26e7fa"
  ],
  "burichan": [
    "css/burichannew.css",
    "71a5a5b8362aa78269c19e89cacc249cda48f2af2393fda7d3638fa518307b18",
    "914f7994bb0b24e10a284848799cca5291cfc7c175749e6492f95df212833a3e"
  ],
  "photon": [
    "css/photon.css",
    "a4741759760d959b4a2e29d9622ab63a91d1cdc49f10679335b71f5b16e9575d",
    "3bf4d47382b3a713847bf2f35bca5fbdef4bd2b16724ea840c3d960422376027"
  ],
  "tomorrow": [
    "css/tomorrow.css",
    "40b4c474a93e87737811c77a50cce9af4fadc9b025a3863e126ca418a8f33a4a",
    "bb0004aada4517015e5936657e85de1cc2f2a4d929f6d67613084f1b9a65e256"
  ]
};
const fixture = JSON.parse(await readFile(new URL('../fixtures/source-quote-styles.json', import.meta.url)));
const args = process.argv.slice(2);
assert.ok(args.length === 0 || args.length === 2 && args[0] === '--source');
const sha = text => createHash('sha256').update(text).digest('hex');
assert.equal(fixture.source_revision, '545b7812d1849f7958d914950c91fdbbe38f6b22');
assert.deepEqual(Object.keys(fixture.themes), Object.keys(pins));
for (const [theme, [file, full, excerpt]] of Object.entries(pins)) {
  const row = fixture.themes[theme];
  assert.equal(row.file, file);
  assert.equal(row.source_sha256, full);
  assert.equal(row.excerpt_sha256, excerpt);
  assert.equal(sha(row.excerpt), excerpt);
  if (args.length) {
    const raw = await readFile(resolve(args[1], file));
    assert.equal(sha(raw), full);
    for (const rule of row.excerpt.split('}\n').map((value, index, all) => index < all.length - 1 ? value + '}' : value)) {
      assert.ok(raw.toString('utf8').includes(rule), 'Captured stylesheet rule drift');
    }
  }
}
console.log('Six source quote/deadlink stylesheet excerpts match their independent pins.');
