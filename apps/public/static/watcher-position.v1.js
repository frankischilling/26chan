// The reference stores CSS text. Accept only its finite coordinate subset.
const sides = ['left', 'right', 'top', 'bottom'];
export function readWatcherPosition(raw) {
  if (typeof raw !== 'string' || raw.length > 256) return null;
  const result = {};
  const seen = new Set();
  for (const declaration of raw.split(';')) {
    if (!declaration.trim()) continue;
    const match = declaration.trim().match(/^(left|right|top|bottom|position)\s*:\s*(.+)$/);
    if (!match || seen.has(match[1])) return null;
    const [, key, value] = match;
    seen.add(key);
    if (key === 'position') {
      if (!['absolute', 'fixed'].includes(value)) return null;
      continue; // The separate fixedThreadWatcher preference owns this choice.
    }
    const coordinate = value.match(/^(\d+(?:\.\d+)?|\.\d+)(px|%)?$/);
    if (!coordinate) return null;
    const amount = Number(coordinate[1]);
    const unit = coordinate[2] || 'px';
    if ((!coordinate[2] && amount !== 0) || !Number.isFinite(amount)
      || amount > (unit === '%' ? 10000 : 1000000)) return null;
    result[key] = `${amount}${unit}`;
  }
  if (Number('left' in result) + Number('right' in result) !== 1
    || Number('top' in result) + Number('bottom' in result) !== 1) return null;
  return result;
}

export function writeWatcherPosition(position, fixed = false) {
  const raw = sides.filter(side => side in position).map(side => `${side}: ${position[side]};`).join(' ');
  if (!readWatcherPosition(raw)) return null;
  return raw + (fixed ? ' position: fixed;' : '');
}

export function dragWatcherPosition(clientX, clientY, state) {
  if (![clientX, clientY, ...['width', 'height', 'panelWidth', 'panelHeight', 'dx', 'dy', 'scrollX', 'scrollY', 'offsetTop'].map(key => state[key])].every(Number.isFinite)
    || state.width <= 0 || state.height <= 0) return null;
  const x = clientX - state.dx + state.scrollX;
  const y = clientY - state.dy + state.scrollY;
  const percent = (value, span) => `${Number((value / span * 100).toFixed(6))}%`;
  const position = x < 1 ? { left: '0px' } : x > state.width - state.panelWidth
    ? { right: '0px' } : { left: percent(x, state.width) };
  Object.assign(position, y <= state.offsetTop ? { top: `${state.offsetTop}px` }
    : y > state.height - state.panelHeight && state.panelHeight < state.height
      ? { bottom: '0px' } : { top: percent(y, state.height) });
  return readWatcherPosition(writeWatcherPosition(position));
}

export function mountWatcherPosition({ panel, heading, catalog, mobile, read, save }) {
  const defaults = () => ({ left: '10px', top: catalog ? '75px' : '380px' });
  let raw;
  let position = defaults();
  let fixed = false;
  let wasMobile = mobile.matches;
  let drag = null;
  let saving = false;
  panel.dataset.trackpos = 'TW-position';
  heading.tabIndex = 0;
  heading.setAttribute('aria-label', 'Move Thread Watcher');

  function apply() {
    for (const side of sides) panel.style[side] = '';
    panel.style.position = fixed ? 'fixed' : 'absolute';
    const current = mobile.matches ? { left: '0px', top: `${window.scrollY + 30}px` } : position;
    for (const side of sides) if (current[side]) panel.style[side] = current[side];
  }
  function cancel() {
    if (!drag) return;
    const previous = drag;
    drag = null;
    position = previous.position;
    if (heading.hasPointerCapture(previous.id)) heading.releasePointerCapture(previous.id);
    apply();
  }
  function sync() {
    const settings = read();
    const nextRaw = JSON.stringify(settings['TW-position']);
    const nextFixed = !catalog && !mobile.matches && settings.fixedThreadWatcher === true;
    if (nextRaw !== raw || nextFixed !== fixed || mobile.matches !== wasMobile || panel.hidden) cancel();
    if (nextRaw !== raw) position = readWatcherPosition(settings['TW-position']) || defaults();
    raw = nextRaw;
    fixed = nextFixed;
    wasMobile = mobile.matches;
    apply();
  }
  function geometry(rect, dx, dy) {
    return { width: document.documentElement.clientWidth, height: document.documentElement.clientHeight,
      panelWidth: rect.width, panelHeight: rect.height, dx, dy,
      scrollX: fixed ? 0 : window.scrollX, scrollY: fixed ? 0 : window.scrollY, offsetTop: 0 };
  }
  async function commit(expected, expectedFixed) {
    const value = writeWatcherPosition(position, fixed);
    if (!value) return;
    saving = true;
    try {
      if (await save(value, expected, expectedFixed)) raw = JSON.stringify(value);
    } finally { saving = false; sync(); }
  }
  heading.addEventListener('pointerdown', event => {
    if (event.button !== 0 || event.isPrimary === false || mobile.matches || panel.hidden || saving
      || event.target.closest('button, a, input, select, textarea')) return;
    sync();
    const rect = panel.getBoundingClientRect();
    drag = { id: event.pointerId, position: { ...position }, expected: raw,
      expectedFixed: read().fixedThreadWatcher === true,
      geometry: geometry(rect, event.clientX - rect.left, event.clientY - rect.top) };
    heading.setPointerCapture(event.pointerId);
    event.preventDefault();
  });
  heading.addEventListener('pointermove', event => {
    if (!drag || drag.id !== event.pointerId) return;
    const next = dragWatcherPosition(event.clientX, event.clientY, drag.geometry);
    if (next) { position = next; apply(); }
  });
  heading.addEventListener('pointerup', event => {
    if (!drag || drag.id !== event.pointerId) return;
    const ended = drag;
    drag = null;
    if (heading.hasPointerCapture(event.pointerId)) heading.releasePointerCapture(event.pointerId);
    void commit(ended.expected, ended.expectedFixed);
  });
  heading.addEventListener('pointercancel', cancel);
  heading.addEventListener('lostpointercapture', cancel);
  heading.addEventListener('keydown', event => {
    if (event.target !== heading || event.altKey || event.ctrlKey || event.metaKey
      || !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key)) return;
    event.preventDefault();
    if (mobile.matches || panel.hidden || saving) return;
    sync();
    const rect = panel.getBoundingClientRect();
    const step = event.shiftKey ? 10 : 1;
    const x = rect.left + (event.key === 'ArrowLeft' ? -step : event.key === 'ArrowRight' ? step : 0);
    const y = rect.top + (event.key === 'ArrowUp' ? -step : event.key === 'ArrowDown' ? step : 0);
    const next = dragWatcherPosition(x, y, geometry(rect, 0, 0));
    if (next) {
      position = next;
      apply();
      void commit(raw, read().fixedThreadWatcher === true);
    }
  });
  window.addEventListener('pagehide', cancel);
  return { sync };
}
