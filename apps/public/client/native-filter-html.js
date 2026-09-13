import { defaultTreeAdapter, parseFragment } from 'parse5';
import { FILTER_LIMITS } from './native-filter-limits.js';

// A data-only HTML5 tree, not DOM nodes: parsing cannot load URLs or execute scripts.
// scriptingEnabled selects the native noscript grammar; it does not execute JavaScript.
export function nativeCommentText(raw) {
  if (typeof raw !== 'string' || raw.length > FILTER_LIMITS.html) throw new RangeError('html-limit');
  let elements = 0;
  const treeAdapter = {
    ...defaultTreeAdapter,
    createElement(...args) {
      if (++elements > FILTER_LIMITS.htmlNodes) throw new RangeError('html-node-limit');
      return defaultTreeAdapter.createElement(...args);
    },
  };
  const context = treeAdapter.createElement('div', 'http://www.w3.org/1999/xhtml', []);
  // Preserve both exact replacements from the pinned Filter.match, including its
  // unusual character class. This is deliberately not a generic tag-strip regex.
  const input = raw.replace(/<br>/g, '\n').replace(/[<[^>]+>/g, '');
  const root = parseFragment(context, input, { scriptingEnabled: true, treeAdapter });
  const pending = [root], parts = [];
  let length = 0, nodes = 0;
  while (pending.length) {
    const node = pending.pop();
    if (++nodes > FILTER_LIMITS.htmlNodes) throw new RangeError('html-node-limit');
    if (node.nodeName === '#text') {
      length += node.value.length;
      if (length > FILTER_LIMITS.field) throw new RangeError('text-limit');
      parts.push(node.value);
    }
    // A template's separate content fragment is not a textContent descendant.
    if (node.childNodes) {
      for (let i = node.childNodes.length - 1; i >= 0; i--) pending.push(node.childNodes[i]);
    }
  }
  return parts.join('');
}

// generateLabel slices the encoded subject, or its tag-stripped comment, before
// the native watcher inserts it into a link. Decode that small result as data;
// the caller receives only text and never inserts the original HTML string.
export function nativeWatchLabel(post) {
  const raw = post.sub ? post.sub.slice(0, 45)
    : post.com ? post.com.replace(/(?:<br>)+/g, ' ').replace(/<[^>]*?>/g, '').slice(0, 45)
      : `No.${post.no}`;
  const context = defaultTreeAdapter.createElement('a', 'http://www.w3.org/1999/xhtml', []);
  const root = parseFragment(context, raw, { scriptingEnabled: true });
  const pending = [root], parts = [];
  while (pending.length) {
    const node = pending.pop();
    if (node.nodeName === '#text') parts.push(node.value);
    if (node.childNodes) {
      for (let i = node.childNodes.length - 1; i >= 0; i--) pending.push(node.childNodes[i]);
    }
  }
  return parts.join('').replace(/[\u0000-\u001f\u007f]/g, ' ').slice(0, 45);
}
