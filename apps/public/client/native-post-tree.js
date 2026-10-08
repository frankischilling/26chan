import { sourceSpoilerPath } from './native-spoilers.js';
import { isSpoilerAssetPath } from './native-spoiler-assets.js';

// Call only after the complete recipe and its live-page IDs have been checked.
export function buildPostTree(tree, document, context) {
  const path = context ? sourceSpoilerPath(document, context.board, Number(tree.attrs?.['data-custom-spoiler'] ?? 0)) : null;
  function build(node) {
    if (typeof node === 'string') return document.createTextNode(node);
    const element = document.createElement(node.tag);
    for (const [key, value] of Object.entries(node.attrs)) element.setAttribute(key,
      key === 'src' && node.tag === 'img' && path && isSpoilerAssetPath(value) ? path : value);
    for (const child of node.children) element.append(build(child));
    return element;
  }
  return build(tree);
}

export function checkPostTreeIds(trees, document) {
  const ids = new Set();
  function visit(tree) {
    if (typeof tree === 'string') return;
    if (tree.attrs.id) {
      if (ids.has(tree.attrs.id) || document.getElementById(tree.attrs.id)) throw new Error('duplicate-dom-id');
      ids.add(tree.attrs.id);
    }
    for (const child of tree.children) visit(child);
  }
  for (const tree of trees) visit(tree);
}
