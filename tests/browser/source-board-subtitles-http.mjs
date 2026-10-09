// Run against an already-started real development public server; no browser or
// hand-built response fixtures are used by this qualification.
import assert from 'node:assert/strict';
import { assertSubtitleDocument } from './source-board-subtitles-contract.mjs';
import { canonicalSubtitleBoards, subtitlePaths, withOwnedSubtitles } from './source-board-subtitles-fixture.mjs';

const origin = new URL(process.env.SUBTITLE_HTTP_ORIGIN || 'http://127.0.0.1:3000');
if (origin.protocol !== 'http:' || origin.hostname !== '127.0.0.1'
  || origin.username || origin.password || origin.pathname !== '/' || origin.search || origin.hash) {
  throw Error('Subtitle HTTP qualification requires a loopback development origin');
}
let checked = 0;
async function check(board, mode, path) {
  const response = await fetch(new URL(path, origin), { signal: AbortSignal.timeout(15000) });
  assert.equal(response.status, 200, `${mode}: ${board.slug}`);
  assert.match(response.headers.get('content-type'), /text\/html/);
  assert.equal(new URL(response.url).origin, origin.origin);
  assertSubtitleDocument(await response.text(), board, mode); checked++;
}
for (const board of canonicalSubtitleBoards()) {
  await check(board, 'index', `/${board.slug}/`);
  await check(board, 'catalog', `/${board.slug}/catalog`);
}
await withOwnedSubtitles(async ({ boards }) => {
  for (const board of boards) for (const [mode, path] of subtitlePaths(board)) await check(board, mode, path);
});
console.log(`Verified ${checked} real HTTP pages: b/trash/gif/g index/catalog and owned none/fiction/GIF/text-only boards in all five modes; owned rows cleaned up.`);
