// One instance belongs to the page and is injected into every content reader.
// A class or data attribute never grants ownership of a comment projection.
export function createCommentProjection() {
  const roots = new WeakMap();
  const annotations = new WeakMap();
  const has = node => !!node && roots.has(node);
  function owner(node) {
    for (let current = node; current; current = current.parentNode) {
      if (has(current)) return roots.get(current);
    }
    return null;
  }
  const within = node => owner(node) !== null;
  const queryAll = (root, selector) => Array.from(root?.querySelectorAll(selector) ?? []).filter(node => !within(node));
  const query = (root, selector) => queryAll(root, selector)[0] ?? null;
  function attributes(node) {
    const read = annotations.get(node);
    return Array.from(node.attributes, ({ name, value }) => ({ name, value: read ? read(name, value) : value }))
      .filter(attribute => attribute.value !== null);
  }
  const escape = value => value.replace(/[&<>"\u00a0]/g,
    ch => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', '\u00a0': '&nbsp;' })[ch]);
  const tags = new Set(['SPAN', 'S', 'PRE', 'BR', 'WBR', 'A']);

  function html(message) {
    let nodes = 0, size = 0;
    const output = [];
    function emit(value) {
      size += value.length;
      if (size > 65536) throw new RangeError('comment-html');
      output.push(value);
    }
    function visit(node, depth) {
      if (has(node)) return;
      if (++nodes > 32001 || depth > 32) throw new RangeError('comment-nodes');
      if (node.nodeType === 3) {
        if (node.data.length > 65536) throw new RangeError('comment-text');
        // Quotes are not escaped by the browser's text-node serialization.
        emit(escape(node.data).replace(/&quot;/g, '"')); return;
      }
      if (node.nodeType !== 1 || node.namespaceURI !== 'http://www.w3.org/1999/xhtml'
        || !tags.has(node.tagName) || node.attributes.length > 64) throw new TypeError('comment-node');
      emit(`<${node.localName}`);
      for (const { name, value } of attributes(node)) {
        if (name.length > 128 || value.length > 65536) throw new RangeError('comment-attribute');
        emit(` ${name}="${escape(value)}"`);
      }
      emit('>');
      for (const child of node.childNodes) visit(child, depth + 1);
      if (!['BR', 'WBR'].includes(node.tagName)) emit(`</${node.localName}>`);
    }
    for (const child of message.childNodes) visit(child, 1);
    return output.join('');
  }

  function text(element, lineBreak = '') {
    let nodes = 0, size = 0;
    const output = [];
    function visit(node, depth) {
      if (!node || has(node)) return;
      if (++nodes > 32768 || depth > 32) throw new RangeError('comment-nodes');
      const value = node.nodeType === 3 ? node.data : node.nodeName === 'BR' ? lineBreak : null;
      if (value !== null) {
        size += value.length;
        if (size > 262144) throw new RangeError('comment-text');
        output.push(value);
      } else for (const child of node.childNodes) visit(child, depth + 1);
    }
    visit(element, 0);
    return output.join('');
  }

  function clone(message) {
    // Validate the complete finite comment before allocating any copy. Its
    // grammar has no image, media, form or active resource-bearing element.
    html(message);
    function copy(node) {
      if (has(node)) return null;
      const result = node.cloneNode(false);
      if (node.nodeType === 1 && annotations.has(node)) {
        for (const { name } of Array.from(result.attributes)) result.removeAttribute(name);
        for (const { name, value } of attributes(node)) result.setAttribute(name, value);
      }
      for (const child of node.childNodes) {
        const copied = copy(child);
        if (copied) result.append(copied);
      }
      return result;
    }
    return copy(message);
  }

  function originalMutation(change) {
    if (within(change.target)) return false;
    if (change.type !== 'childList') return true;
    return [...change.addedNodes, ...change.removedNodes].some(node => !has(node));
  }
  return { has, owner, within, query, queryAll, html, text, clone, originalMutation, attributes,
    trackAttributes(node, read) {
      if (annotations.has(node) || typeof read !== 'function') throw new TypeError('annotation-owner');
      annotations.set(node, read);
      return () => { if (annotations.get(node) === read) annotations.delete(node); };
    },
    claim(node, value) {
      if (!node || node.nodeType !== 1 || !value || has(node)) throw new TypeError('projection-owner');
      // Retain weak ownership after removal so queued mutation records also
      // exclude the removed projection. The controller releases its own state.
      roots.set(node, value);
    } };
}
