import { postId } from '../static/thread-watcher-core.v1.js';

export const POST_NUMBER_TITLES = Object.freeze(['Link to this post', 'Reply to this post']);
const exact = (attrs, expected) => Object.keys(attrs).sort().join(',') === Object.keys(expected).sort().join(',')
  && Object.entries(expected).every(([key, value]) => attrs[key] === value);
function require(value) { if (!value) throw new TypeError('invalid-snapshot'); }

// Mirror the released pure helper on text that this renderer HTML-escapes.
// Replace a split surrogate with valid Unicode before it crosses UTF-8 HTML.
export function mobileHeaderLabel(text) {
  const serialized = text.replace(/[&<>"']/g, character => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#039;' })[character]);
  const shortened = serialized.length > 30;
  let value = text;
  if (shortened) {
    value = serialized.replace('&#44;', ',').replaceAll('&amp;', '&').replaceAll('&quot;', '"')
      .replaceAll('&#039;', "'").replaceAll('&lt;', '<').replaceAll('&gt;', '>').slice(0, 30);
    value = new TextDecoder().decode(new TextEncoder().encode(value)) + '(...)';
  }
  return { text: value, shortened };
}

// The two header links are one finite recipe. A title or reply URL elsewhere
// in the tree does not grant an action, navigation target or DOM attribute.
export function validatePostNumbers(tree, { board, thread }, no) {
  const marked = [], all = [];
  function collect(node, parent) {
    if (typeof node === 'string') return;
    all.push(node);
    if ((node.attrs.class || '').split(' ').includes('postNum')
      || (node.tag === 'a' && Object.hasOwn(node.attrs, 'title') && parent?.attrs.class !== 'fileText')
      || (node.tag === 'span' && ['name', 'subject'].includes(node.attrs.class) && Object.hasOwn(node.attrs, 'title'))
      || (node.attrs.href?.startsWith('/') && node.attrs.href.includes('?quote='))) marked.push(node);
    node.children.forEach(child => collect(child, node));
  }
  collect(tree);
  if (!marked.length) {
    require(!all.some(node => node.attrs.class !== 'mFileInfo mobile' && (node.attrs.class || '').split(' ').some(token => ['postInfoM', 'mobile', 'dateTime'].includes(token))));
    return;
  }
  const blocks = all.filter(node => node.attrs.class === 'postNum' || node.attrs.class === 'dateTime postNum');
  const header = all.find(node => node.tag === 'div' && node.attrs.id === `pi${no}`);
  const post = all.find(node => node.tag === 'div' && node.attrs.id === `p${no}`);
  const mobile = all.filter(node => (node.attrs.class || '').split(' ').includes('postInfoM'));
  require(mobile.length <= 1 && blocks.length === 1 + mobile.length
    && tree.children.includes(post) && post?.children.includes(header));
  require(all.filter(node => node.attrs.class !== 'mFileInfo mobile' && (node.attrs.class || '').split(' ').some(token => ['postInfoM', 'mobile', 'dateTime'].includes(token))
    || node.attrs.id === `pim${no}`).every(node => node === mobile[0] || node === blocks.find(block => block.attrs.class === 'dateTime postNum')));
  const permitted = new Set();
  if (mobile.length) for (const label of validateMobileHeader(mobile[0], header, post, no, thread, all)) permitted.add(label);
  const permalink = `/${board}/thread/${thread}#p${no}`;
  const reply = `/${board}/thread/${thread}?quote=${no}#reply`;
  let replyHref;
  for (const block of blocks) {
    const isMobile = block.attrs.class === 'dateTime postNum';
    require(block.tag === 'span' && (isMobile ? mobile[0]?.children.includes(block)
      && block.children.length === 3 && typeof block.children[0] === 'string'
      : exact(block.attrs, { class: 'postNum' }) && header?.children.includes(block) && block.children.length === 2));
    const [link, quote] = block.children.slice(isMobile ? 1 : 0);
    for (const [node, href, title, text] of [[link, permalink, POST_NUMBER_TITLES[0], 'No.'],
      [quote, reply, POST_NUMBER_TITLES[1], no]]) {
      require(typeof node === 'object' && node.tag === 'a'
        && exact(node.attrs, { href: node === quote && node.attrs.href === permalink ? permalink : href, title })
        && node.children.length === 1 && node.children[0] === text);
    }
    replyHref ??= quote.attrs.href;
    require(replyHref === quote.attrs.href);
    for (const node of [block, link, quote]) permitted.add(node);
  }
  require(marked.length === permitted.size && marked.every(node => permitted.has(node)));
}

function validateMobileHeader(mobile, desktop, post, no, thread, all) {
  require(mobile.tag === 'div' && exact(mobile.attrs, { class: 'postInfoM mobile', id: `pim${no}` })
    && post.children.includes(mobile) && mobile.children.length === 2);
  const [nameBlock, numberDate] = mobile.children;
  require(nameBlock?.tag === 'span' && nameBlock.attrs.class?.split(' ')[0] === 'nameBlock'
    && Object.keys(nameBlock.attrs).length === 1);
  const own = (nodes, token) => nodes.find(node => typeof node === 'object' && (node.attrs.class || '').split(' ').includes(token));
  const desktopBadge = own(desktop.children, 'nameBlock');
  const name = desktopBadge ? desktopBadge.children[0] : own(desktop.children, 'name');
  const makeLabel = node => {
    require(node.tag === 'span' && exact(node.attrs, { class: node.attrs.class }) && node.children.every(child => typeof child === 'string'));
    const raw = node.children.join(''), value = mobileHeaderLabel(raw);
    return { tag: 'span', attrs: { class: node.attrs.class, ...(value.shortened ? { title: raw } : {}) }, children: value.text ? [value.text] : [] };
  };
  const prefix = desktopBadge ? [...desktopBadge.children] : [name];
  prefix[0] = makeLabel(name);
  if (!desktopBadge) for (const token of ['postertrip', 'posteruid', 'flag', 'bfl']) {
    const value = own(desktop.children, token);
    if (value) prefix.push(' ', value);
  }
  require(name && nameBlock.attrs.class === (desktopBadge?.attrs.class ?? 'nameBlock'));
  const subject = own(desktop.children, 'subject') ?? { tag: 'span', attrs: { class: 'subject' }, children: [] };
  const tail = [{ tag: 'br', attrs: {}, children: [] }, ...(no === thread ? [makeLabel(subject), ' '] : [])];
  require(JSON.stringify(nameBlock.children) === JSON.stringify([...prefix, ...tail]));
  const time = desktop.children.find(node => typeof node === 'object' && node.tag === 'time');
  require(time?.children.length === 1 && typeof time.children[0] === 'string'
    && exact(numberDate.attrs, { class: 'dateTime postNum', 'data-utc': String(Math.floor(Date.parse(time.attrs.datetime) / 1000)) })
    && numberDate.children[0] === `${time.children[0]} `);
  require(all.filter(node => node.attrs.class === 'nameBlock').every(node => node === nameBlock));
  return nameBlock.children.filter(node => typeof node === 'object' && ['name', 'subject'].includes(node.attrs.class) && Object.hasOwn(node.attrs, 'title'));
}

// Delegation accepts only the digits link in the original post's own header.
// Permalinks, preview copies and modified navigation retain normal behavior.
export function postNumberReply(link, board) {
  if (!link || link.localName !== 'a' || link.title !== POST_NUMBER_TITLES[1]) return null;
  const block = link.parentElement, header = block?.parentElement, section = header?.closest('.thread');
  const isMobile = header?.className === 'postInfoM mobile';
  const no = postId(header?.id?.slice(isMobile ? 3 : 2)), thread = postId(section?.id?.slice(1));
  if (!no || !thread || header.id !== `${isMobile ? 'pim' : 'pi'}${no}` || !isMobile && !header.classList.contains('postInfo')
    || header.parentElement?.id !== `p${no}` || header.parentElement?.parentElement?.id !== `pc${no}`
    || block?.className !== (isMobile ? 'dateTime postNum' : 'postNum') || block.children.length !== 2 || block.children[1] !== link
    || link.textContent !== no || link.getAttribute('href') !== `/${board}/thread/${thread}?quote=${no}#reply`
      && link.getAttribute('href') !== `/${board}/thread/${thread}#p${no}`) return null;
  const permalink = block.children[0];
  if (permalink.localName !== 'a' || permalink.title !== POST_NUMBER_TITLES[0]
    || permalink.textContent !== 'No.' || permalink.getAttribute('href') !== `/${board}/thread/${thread}#p${no}`) return null;
  return { thread, post: no };
}
