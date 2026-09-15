import { FILTER_LIMITS } from './native-filter-limits.js';

// Public extension v1191 Linkify lexical rules. Offsets address a serialized text
// run with <wbr> represented as ZWS; this module never parses strings as HTML.
export const LINKIFY_LIMITS = Object.freeze({ characters: 192000, nodes: 32001, depth: 32 });
const probe = /(?:^|[^\B"])https?:\/\/[-.a-z0-9]+\.[a-z]{2,4}/;

export function linkificationEnabled(settings, mobileLayout, neverMobile) {
  return settings?.disableAll !== true
    && ((mobileLayout === true && neverMobile !== 'true') || settings?.linkify === true);
}

export function hasSourceLink(serialized) {
  return typeof serialized === 'string' && serialized.length <= LINKIFY_LIMITS.characters
    && probe.test(serialized);
}

export function sourceLinkSpans(serialized, probed = hasSourceLink(serialized)) {
  if (!probed || typeof serialized !== 'string' || serialized.length > LINKIFY_LIMITS.characters) return [];
  const links = /(^|[^\B"])(https?:\/\/[-.a-z0-9\u200b]+\.[a-z\u200b]{2,15}(?:\/[^\s<>]*)?)/ig;
  const output = [];
  for (const match of serialized.matchAll(links)) {
    const original = match[2], start = match.index + match[1].length;
    if (serialized.slice(start + original.length, start + original.length + 4) === '</a>') continue;
    let length = original.length - (original.match(/[:!?,.'"]+$/)?.[0].length ?? 0);
    // The source checks parentheses against the original, not punctuation-trimmed URL.
    const closing = original.match(/\)+$/)?.[0].length ?? 0;
    if (closing) length -= Math.max(0, closing - (original.match(/\(/g)?.length ?? 0));
    const label = original.slice(0, length), destination = label.replace(/\u200b/g, '');
    try {
      // Preserve source spelling in the parameter; URL is validation only.
      const url = new URL(destination);
      if (!['http:', 'https:'].includes(url.protocol) || !url.hostname || url.username || url.password
        || /[\u0000-\u0020\u007f\\]/.test(destination)) continue;
      output.push({ start, end: start + length, parameter: encodeURIComponent(destination) });
    } catch { /* Invalid URLs and lone surrogates remain escaped text. */ }
  }
  return output;
}

const tags = new Set(['SPAN', 'S', 'PRE', 'BR', 'WBR', 'A']);
const escapeText = ch => ch === '&' ? '&amp;' : ch === '<' ? '&lt;' : ch === '>' ? '&gt;' : ch;
const generated = 'data-native-linkified';

// Runs cannot cross an element boundary except a soft break. Build a complete
// plan before mutation so the live message can remain unchanged on any bound.
function planMessage(message) {
  if (!message || message.nodeType !== 1) return null;
  let nodes = 0, characters = 0;
  const runs = [];
  function attributes(node) {
    for (const attribute of node.attributes) characters += attribute.name.length + attribute.value.length;
    if (characters > LINKIFY_LIMITS.characters) throw new RangeError('link-characters');
  }
  function visit(parent, depth) {
    if (depth > LINKIFY_LIMITS.depth) throw new RangeError('link-depth');
    if (parent.childNodes.length + nodes > LINKIFY_LIMITS.nodes) throw new RangeError('link-nodes');
    let run = null;
    for (const [index, node] of Array.from(parent.childNodes).entries()) {
      if (++nodes > LINKIFY_LIMITS.nodes) throw new RangeError('link-nodes');
      if (node.nodeType === 1) attributes(node);
      if (node.nodeType === 3 || (node.nodeType === 1 && node.tagName === 'WBR')) {
        if (!run) {
          // A preceding opening/closing element ends in > in innerHTML.
          const prefix = parent !== message || node.previousSibling ? '>' : '';
          run = { text: prefix, points: Array(prefix.length + 1).fill(null) };
          runs.push(run);
        }
        if (node.nodeType === 3) {
          if (node.data.length + characters > LINKIFY_LIMITS.characters) throw new RangeError('link-characters');
          for (let offset = 0; offset < node.data.length;) {
            const ch = String.fromCodePoint(node.data.codePointAt(offset)), encoded = escapeText(ch);
            run.points[run.text.length] = [node, offset];
            run.text += encoded;
            for (let i = 1; i <= encoded.length; i++) run.points.push(null);
            offset += ch.length;
            run.points[run.text.length] = [node, offset];
            characters += encoded.length;
          }
        } else {
          run.points[run.text.length] = [parent, index];
          run.text += '\u200b'; run.points.push([parent, index + 1]); characters++;
        }
        if (characters > LINKIFY_LIMITS.characters) throw new RangeError('link-characters');
      } else if (node.nodeType === 1 && tags.has(node.tagName)) {
        run = null;
        if (node.tagName !== 'A') visit(node, depth + 1);
        else {
          // Existing anchors participate in the source's probe but never in a
          // replacement range. Charge their descendants before serialization.
          charge(node, depth + 1);
        }
      } else throw new TypeError('link-node');
    }
  }
  function charge(parent, depth) {
    if (depth > LINKIFY_LIMITS.depth) throw new RangeError('link-depth');
    for (const node of parent.childNodes) {
      if (++nodes > LINKIFY_LIMITS.nodes) throw new RangeError('link-nodes');
      if (node.nodeType === 3) characters += node.data.length * 5;
      else if (node.nodeType === 1 && tags.has(node.tagName)) { attributes(node); charge(node, depth + 1); }
      else throw new TypeError('link-node');
      if (characters > LINKIFY_LIMITS.characters) throw new RangeError('link-characters');
    }
  }
  try { visit(message, 0); } catch { return null; }
  // Read-only serialization retains the source's global, case-sensitive probe.
  if (!hasSourceLink(message.innerHTML)) return null;
  return runs.flatMap(run => sourceLinkSpans(run.text, true).flatMap(span => {
    const start = run.points[span.start], end = run.points[span.end];
    return start && end ? [{ start, end, parameter: span.parameter }] : [];
  }));
}

function decorateMessage(message, planned) {
  const document = message.ownerDocument;
  for (const { start, end, parameter } of planned.reverse()) {
    const range = document.createRange();
    range.setStart(...start); range.setEnd(...end);
    const anchor = document.createElement('a');
    anchor.setAttribute('href', `/derefer?url=${parameter}`);
    anchor.target = '_blank'; anchor.className = 'linkified';
    anchor.rel = 'noreferrer ugc noopener';
    anchor.setAttribute(generated, 'true');
    anchor.append(range.extractContents()); range.insertNode(anchor);
  }
  // The source's final global ZWS replacement also affects literal text and
  // existing anchor labels. Attributes remain untouched for browser safety.
  const walker = document.createTreeWalker(message, 4), replacements = [];
  while (walker.nextNode()) if (walker.currentNode.data.includes('\u200b')) replacements.push(walker.currentNode);
  for (const node of replacements) {
    const fragment = document.createDocumentFragment();
    node.data.split('\u200b').forEach((part, index) => {
      if (index) fragment.append(document.createElement('wbr'));
      fragment.append(document.createTextNode(part));
    });
    node.replaceWith(fragment);
  }
}

export function linkifyMessage(message) {
  const planned = planMessage(message);
  if (planned === null) return 0;

  // Page filters consume the live postMessage.innerHTML under FILTER_LIMITS.html.
  // Decorate an exact detached clone first, including generated anchor markup and
  // the source's global ZWS-to-WBR pass. Crossing that existing ceiling leaves
  // the entire live message untouched, including server-created anchor identity.
  const preview = message.cloneNode(true);
  const previewPlan = planMessage(preview);
  if (previewPlan === null) return 0;
  decorateMessage(preview, previewPlan);
  if (preview.innerHTML.length > FILTER_LIMITS.html) return 0;

  decorateMessage(message, planned);
  return planned.length;
}

function unlinkMessage(message) {
  for (const anchor of message.querySelectorAll(`a.linkified[${generated}="true"]`)) {
    anchor.replaceWith(...Array.from(anchor.childNodes));
  }
}

// Mount once on a board page. The observer only schedules post messages from
// the board or a quote preview; each message still passes the finite DOM checks
// in linkifyMessage before any mutation occurs.
export function mountNativeLinkification({ root, settings, mobile,
  readNeverMobile = () => {
    try { return localStorage.getItem('4chan_never_show_mobile'); }
    catch { return null; }
  } } = {}) {
  if (!root || typeof settings !== 'function' || !document.body) return null;
  let stopped = false, scheduled = false;
  const pending = new Set();
  const eligible = message => message?.nodeType === 1 && message.classList.contains('postMessage')
    && (root.contains(message) || message.closest('#quote-preview')?.isConnected);
  function enabled() {
    let value = {}, neverMobile = null;
    try { value = settings() ?? {}; } catch { /* Unavailable settings use defaults. */ }
    try { neverMobile = readNeverMobile(); } catch { /* Unavailable storage does not disable the mobile default. */ }
    return linkificationEnabled(value, mobile?.matches === true, neverMobile);
  }
  function apply(message, active) {
    if (!eligible(message)) return;
    if (active) linkifyMessage(message);
    else unlinkMessage(message);
  }
  function refresh() {
    if (stopped) return;
    const active = enabled();
    for (const message of root.querySelectorAll('.postMessage')) apply(message, active);
    for (const message of document.querySelectorAll('#quote-preview .postMessage')) apply(message, active);
  }
  function queue(message) {
    if (!eligible(message)) return;
    pending.add(message);
    if (scheduled) return;
    scheduled = true;
    queueMicrotask(() => {
      scheduled = false;
      if (stopped) { pending.clear(); return; }
      const active = enabled(), work = Array.from(pending); pending.clear();
      for (const item of work) apply(item, active);
    });
  }
  function added(node) {
    if (node.nodeType !== 1) return;
    if (node.classList.contains('postMessage')) queue(node);
    for (const message of node.querySelectorAll?.('.postMessage') ?? []) queue(message);
  }
  const observer = new MutationObserver(changes => {
    for (const change of changes) for (const node of change.addedNodes) added(node);
  });
  observer.observe(document.body, { childList: true, subtree: true });
  const onStorage = event => {
    if (event.key === null || event.key === '4chan-settings' || event.key === '4chan_never_show_mobile') refresh();
  };
  const onSettings = () => refresh();
  const onMobile = () => refresh();
  const onPageHide = event => { if (!event.persisted) disconnect(); };
  function disconnect() {
    if (stopped) return;
    stopped = true; pending.clear(); observer.disconnect();
    window.removeEventListener('storage', onStorage);
    window.removeEventListener('pagehide', onPageHide);
    document.removeEventListener('4chanSettingsSaved', onSettings);
    mobile?.removeEventListener?.('change', onMobile);
  }
  window.addEventListener('storage', onStorage);
  window.addEventListener('pagehide', onPageHide);
  document.addEventListener('4chanSettingsSaved', onSettings);
  mobile?.addEventListener?.('change', onMobile);
  refresh();
  return { refresh, disconnect };
}
