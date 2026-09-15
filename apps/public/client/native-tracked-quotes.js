import { postId } from '../static/thread-watcher-core.v1.js';

// Native Parser initializes tracked replies only on a thread page. Local
// receipt hints decorate text; they never change the link's destination.
export function markNativeTrackedQuotes(section, tracked, enabled, { readLabel, writeLabel } = {}) {
  if (!section) return;
  for (const link of section.querySelectorAll('.postMessage .quotelink')) {
    const previous = link.dataset.nativeTracked;
    const label = readLabel ? readLabel(link) : link.textContent;
    const write = value => { if (writeLabel) writeLabel(link, value); else link.textContent = value; };
    const text = previous && label === `>>${previous} (You)` ? `>>${previous}` : label;
    const id = text.startsWith('>>') ? postId(text.slice(2)) : null;
    if (enabled && id && tracked.has(id)) {
      if (label !== `${text} (You)`) write(`${text} (You)`);
      link.classList.add('ql-tracked'); link.dataset.nativeTracked = id;
    } else if (previous) {
      if (label === `>>${previous} (You)`) write(`>>${previous}`);
      link.classList.remove('ql-tracked'); delete link.dataset.nativeTracked;
    }
  }
}

export function notificationKind(current, { you, highlighted, unread }) {
  if (you) return 'rep';
  if (highlighted && current !== 'rep') return 'hl';
  return unread === 0 ? 'new' : current;
}

export function notificationIcon(worksafe, kind) {
  const names = { new: 'newposts', rep: 'newreplies', hl: 'newfilters', dead: 'deadthread' };
  if (kind === null) return `/static/notifications/favicon${worksafe ? '-ws' : ''}.ico`;
  return Object.hasOwn(names, kind) ? `/static/notifications/favicon-${worksafe ? 'ws' : 'nws'}-${names[kind]}.ico` : null;
}
