// Re-record only from the pinned reference checkout, never from application output.
import { createHash } from 'node:crypto';
import { readFile, readdir, writeFile } from 'node:fs/promises';
import path from 'node:path';

const directory = process.argv[2];
if (!directory) throw new Error('Usage: node tests/fixtures/source-board-subtitles-record.mjs <reference-directory>');
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const sourceRevision = '545b7812d1849f7958d914950c91fdbbe38f6b22';
const files = new Map();
async function source(file) {
  if (!files.has(file)) files.set(file, await readFile(path.join(directory, file)));
  return files.get(file);
}
async function excerpt(file, pattern) {
  const bytes = await source(file), text = bytes.toString('utf8'), match = text.match(pattern);
  if (!match || text.indexOf(match[0], match.index + 1) !== -1) throw Error(`Missing or ambiguous source excerpt: ${file}`);
  const byteStart = Buffer.byteLength(text.slice(0, match.index));
  return { path: file, source_sha256: digest(bytes), byte_start: byteStart,
    byte_end: byteStart + Buffer.byteLength(match[0]), sha256: digest(match[0]), text: match[0] };
}
const boards = [];
for (const file of (await readdir(path.join(directory, 'config/boards'))).sort()) {
  if (!file.endsWith('.config.ini')) continue;
  const pin = await source(`config/boards/${file}`);
  const lines = [...pin.toString().matchAll(/^SUBTITLE\s*=\s*(.*)\r?$/gm)];
  if (!lines.length) continue;
  if (lines.length !== 1) throw Error(`Repeated subtitle: ${file}`);
  boards.push({ board: file.slice(0, -11), html: lines[0][1].trimEnd(),
    source: await excerpt(`config/boards/${file}`, /^SUBTITLE[^\r\n]*\r?\n/m) });
}
if (boards.map(row => row.board).join(',') !== 'b,gif,trash') throw Error('Pinned subtitle inventory changed');
const excerpts = {
  board_builder: await excerpt('imgboard.php', /\tif\( defined\( 'SUBTITLE' \) \) \{\r?\n[\s\S]*?\r?\n\t\}/),
  catalog_builder: await excerpt('catalog.php', /  if\( defined\( 'SUBTITLE' \) \) \{\r?\n[\s\S]*?\r?\n  \}/),
  board_banner: await excerpt('imgboard.php', /<div class="boardBanner">\r?\n\t\$titlepart[\s\S]*?<\/div>\r?\n\$abovePostForm/),
  catalog_banner: await excerpt('catalog.php', /<div class="boardBanner">\r?\n  \$titlepart[\s\S]*?<\/div>\r?\n<hr class="abovePostForm">/),
  board_modes: await excerpt('imgboard.php', /\tif \(!\$res\) \{\r?\n\t  if \(\$is_arclist\)[\s\S]*?\tif \(!\$is_arclist\) \{[\s\S]*?\r?\n\t\}/),
  catalog_text_mode: await excerpt('catalog.php', /  if \(TEXT_ONLY\) \{\r?\n    \$text_only[\s\S]*?\r?\n  \}/),
  archive_header: await excerpt('imgboard.php', /  head\(\$html, 0, 0, 0, 0, true\);/),
};
const styles = [];
for (const [theme, stem] of [['yotsuba', 'yotsuba'], ['yotsuba-b', 'yotsublue'], ['futaba', 'futaba'],
  ['burichan', 'burichan'], ['photon', 'photon'], ['tomorrow', 'tomorrow']]) {
  const board = ['photon', 'tomorrow'].includes(stem) ? stem : `${stem}new`;
  const catalog = { yotsuba: 'yotsuba_new', 'yotsuba-b': 'yotsuba_b_new', futaba: 'futaba_new', burichan: 'burichan_new', photon: 'photon', tomorrow: 'tomorrow' }[theme];
  for (const [mode, file] of [['index', `css/${board}.css`], ['catalog', `css/catalog_${catalog}.css`]]) {
    styles.push({ theme, mode, source: await excerpt(file, /div\.boardBanner\s*>\s*div\.boardSubtitle\s*\{[^}]*\}/) });
  }
}
for (const file of ['css/yotsubamobile.css', 'css/yotsubluemobile.css', 'css/0ch.css']) {
  styles.push({ mode: file.includes('mobile') ? 'mobile' : 'text', source: await excerpt(file, /div\.boardBanner\s*>\s*div\.boardSubtitle\s*\{[^}]*\}/) });
}
const result = { source_revision: sourceRevision,
  scope: 'Only b/trash/gif define SUBTITLE. The common header includes it on index, live/archived thread and archive index; catalog has its own equivalent header. TEXT_ONLY changes body/control classes, not subtitle emission. Fixed source HTML is preserved except the GIF destination is mapped to local /wsg/.',
  boards, excerpts, styles };
await writeFile(new URL('./source-board-subtitles.json', import.meta.url), JSON.stringify(result, null, 2) + '\n');
