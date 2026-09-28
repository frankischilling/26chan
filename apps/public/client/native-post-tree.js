// Call only after the complete recipe and its live-page IDs have been checked.
export function buildPostTree(tree, document) {
  if (typeof tree === 'string') return document.createTextNode(tree);
  const element = document.createElement(tree.tag);
  for (const [key, value] of Object.entries(tree.attrs)) element.setAttribute(key, value);
  for (const child of tree.children) element.append(buildPostTree(child, document));
  return element;
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
