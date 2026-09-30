import { postId } from '../static/thread-watcher-core.v1.js';

export const POST_NUMBER_TITLES = Object.freeze(['Link to this post', 'Reply to this post']);
const exact = (attrs, expected) => Object.keys(attrs).sort().join(',') === Object.keys(expected).sort().join(',')
  && Object.entries(expected).every(([key, value]) => attrs[key] === value);
function require(value) { if (!value) throw new TypeError('invalid-snapshot'); }

// The two header links are one finite recipe. A title or reply URL elsewhere
// in the tree does not grant an action, navigation target or DOM attribute.
export function validatePostNumbers(tree, { board, thread }, no) {
  const marked = [], all = [];
  function collect(node) {
    if (typeof node === 'string') return;
    all.push(node);
    if ((node.attrs.class || '').split(' ').includes('postNum')
      || (node.tag === 'a' && Object.hasOwn(node.attrs, 'title'))
      || (node.attrs.href?.startsWith('/') && node.attrs.href.includes('?quote='))) marked.push(node);
    node.children.forEach(collect);
  }
  collect(tree);
  if (!marked.length) return;
  const blocks = all.filter(node => node.attrs.class === 'postNum');
  require(blocks.length === 1);
  const block = blocks[0], header = all.find(node => node.tag === 'div' && node.attrs.id === `pi${no}`);
  const post = all.find(node => node.tag === 'div' && node.attrs.id === `p${no}`);
  require(block.tag === 'span' && exact(block.attrs, { class: 'postNum' })
    && tree.children.includes(post) && post?.children.includes(header)
    && header?.children.includes(block) && block.children.length === 2);
  const permalink = `/${board}/thread/${thread}#p${no}`;
  const reply = `/${board}/thread/${thread}?quote=${no}#reply`;
  const [link, quote] = block.children;
  for (const [node, href, title, text] of [[link, permalink, POST_NUMBER_TITLES[0], 'No.'],
    [quote, reply, POST_NUMBER_TITLES[1], no]]) {
    require(typeof node === 'object' && node.tag === 'a'
      && exact(node.attrs, { href: node === quote && node.attrs.href === permalink ? permalink : href, title })
      && node.children.length === 1 && node.children[0] === text);
  }
  require(marked.length === 3 && marked.every(node => [block, link, quote].includes(node)));
}

// Delegation accepts only the digits link in the original post's own header.
// Permalinks, preview copies and modified navigation retain normal behavior.
export function postNumberReply(link, board) {
  if (!link || link.localName !== 'a' || link.title !== POST_NUMBER_TITLES[1]) return null;
  const block = link.parentElement, header = block?.parentElement, section = header?.closest('.thread');
  const no = postId(header?.id?.slice(2)), thread = postId(section?.id?.slice(1));
  if (!no || !thread || header.id !== `pi${no}` || !header.classList.contains('postInfo')
    || header.parentElement?.id !== `p${no}` || header.parentElement?.parentElement?.id !== `pc${no}`
    || block?.className !== 'postNum' || block.children.length !== 2 || block.children[1] !== link
    || link.textContent !== no || link.getAttribute('href') !== `/${board}/thread/${thread}?quote=${no}#reply`
      && link.getAttribute('href') !== `/${board}/thread/${thread}#p${no}`) return null;
  const permalink = block.children[0];
  if (permalink.localName !== 'a' || permalink.title !== POST_NUMBER_TITLES[0]
    || permalink.textContent !== 'No.' || permalink.getAttribute('href') !== `/${board}/thread/${thread}#p${no}`) return null;
  return { thread, post: no };
}
