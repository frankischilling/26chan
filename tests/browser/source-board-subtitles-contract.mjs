import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { parse, parseFragment } from 'parse5';

export const subtitleSource = JSON.parse(await readFile(new URL('../fixtures/source-board-subtitles.json', import.meta.url)));
const attributes = node => Object.fromEntries((node.attrs ?? []).map(({ name, value }) => [name, value]));
const hasClass = (node, name) => (attributes(node).class ?? '').split(/\s+/).includes(name);
function descendants(node) { return [node, ...(node.childNodes ?? []).flatMap(descendants)]; }
function tree(node) {
  if (node.nodeName === '#text') return node.value;
  return { tag: node.tagName, attrs: attributes(node), children: (node.childNodes ?? []).map(tree) };
}
export function expectedSubtitle(kind) {
  if (kind === 'none') return '';
  const board = kind === 'fiction' ? 'b' : kind === 'worksafe_gif' ? 'gif' : null;
  assert.ok(board, `Unknown test subtitle ${kind}`);
  return subtitleSource.boards.find(row => row.board === board).html.replace('//boards.4chan.org/wsg/', '/wsg/');
}
export function assertSubtitleDocument(html, board, mode) {
  const document = parse(html), nodes = descendants(document);
  const banners = nodes.filter(node => hasClass(node, 'boardBanner'));
  assert.equal(banners.length, 1, 'A public page has one board banner');
  const banner = banners[0], children = (banner.childNodes ?? []).filter(node => node.tagName);
  const titles = children.filter(node => hasClass(node, 'boardTitle'));
  assert.equal(titles.length, 1);
  const subtitles = nodes.filter(node => hasClass(node, 'boardSubtitle'));
  if (board.kind === 'none') assert.equal(subtitles.length, 0, 'An unconfigured board must not substitute its description or archive title');
  else {
    assert.equal(subtitles.length, 1, 'Configured subtitle appears once');
    assert.equal(children[children.indexOf(titles[0]) + 1], subtitles[0], 'Subtitle directly follows title inside banner');
    assert.equal(subtitles[0].tagName, 'div');
    const expected = parseFragment(expectedSubtitle(board.kind));
    assert.deepEqual(subtitles[0].childNodes.map(tree), expected.childNodes.map(tree), 'Exact fixed source markup; only GIF destination becomes local');
  }
  assert.equal(descendants(banner).filter(node => ['script', 'img', 'svg', 'iframe'].includes(node.tagName)).length, 0, 'Board title/description cannot inject elements');
  const text = node => node.nodeName === '#text' ? node.value : (node.childNodes ?? []).map(text).join('');
  assert.ok(['index', 'thread', 'archived-thread', 'catalog', 'archive-index'].includes(mode), 'Known public page mode');
  const prefix = board.slug === 's4s' ? '[s4s]' : `/${board.slug}/`;
  assert.equal(text(titles[0]), `${prefix} - ${board.title}`, 'Archive and ordinary pages retain the source board heading');
  assert.ok(!text(banner).includes(board.description), 'Board description is not banner content');
  const body = nodes.find(node => node.tagName === 'body');
  assert.equal(hasClass(body, 'text_only'), board.textOnly, 'Text layout does not change subtitle policy');
}
