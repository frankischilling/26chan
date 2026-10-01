import { CATALOG_FILTER_LIMITS, CatalogFilterMatcher, readCatalogFilters, writeCatalogFilters } from './catalog-filter-core.v1.js';
import { NativeWatchLock, filterColor } from './native-filter.v1.js';

const palette = ['#E0B0FF', '#F2F3F4', '#7DF9FF', '#FFFF00', '#FBCEB1', '#FFBF00',
  '#ADFF2F', '#0047AB', '#00A550', '#007FFF', '#AF0A0F', '#B5BD68'];
const storageKey = 'catalog-filters';
function node(tag, text, className) {
  const element = document.createElement(tag);
  if (text !== undefined) element.textContent = text;
  if (className) element.className = className;
  return element;
}
function button(text, action, className) {
  const element = node('button', text, className);
  element.type = 'button'; element.addEventListener('click', action);
  return element;
}

export function mountCatalogFilters({ board, cards, form, container, opener, changed }) {
  const matcher = new CatalogFilterMatcher();
  const notice = node('p', '', 'catalogFilterNotice'); notice.hidden = true; notice.setAttribute('role', 'status');
  form.after(notice);
  let snapshot = { rules: [], matches: new Map() }, hits = [], controller, generation = 0;
  let retired = false, suspended = false, dialog, help, picker, fields, list, search, message, submit, nextId = 0;
  let expectedRaw = null, cachedRaw = null, pendingSave = null;
  let persistent = typeof navigator.locks?.request === 'function';
  const live = () => !retired && !suspended && form.isConnected && container.isConnected
    && document.getElementById('ctrl') === form && document.getElementById('threads') === container;
  const announce = text => { notice.textContent = text; notice.hidden = !text; };
  const read = () => {
    if (!persistent) return cachedRaw;
    try { cachedRaw = localStorage.getItem(storageKey); return cachedRaw; }
    catch { persistent = false; return cachedRaw; }
  };
  // Read the initial value even when this browser cannot persist through Web Locks.
  try { cachedRaw = localStorage.getItem(storageKey); } catch { persistent = false; }
  const lock = new NativeWatchLock({
    acquire: persistent ? async (action, signal) => {
      let entered = false;
      try { return await navigator.locks.request('paperboard-thread-watcher', { signal }, () => { entered = true; return action(); }); }
      catch (error) {
        if (entered || signal.aborted || error?.name !== 'SecurityError') throw error;
        persistent = false;
        return { status: 'unavailable' };
      }
    } : null,
    warn: () => { if (live()) announce('Catalog filter storage is busy or unavailable.'); },
  });
  const parsedRules = raw => {
    const parsed = readCatalogFilters(raw);
    if (parsed.status !== 'ok' || parsed.rules.some(rule => filterColor(rule.color) === null)) return { status: 'invalid-settings' };
    return parsed;
  };
  const eligible = rule => rule.active && rule.pattern !== '' && (!rule.boards || rule.boards.split(' ').includes(board));
  const updateHits = () => {
    if (!list) return;
    for (const [row, field] of fields) {
      field.hits.textContent = eligible(snapshot.rules[field.savedIndex] ?? {}) ? `x${hits[field.savedIndex] ?? 0}` : '';
    }
  };
  async function refresh() {
    controller?.abort(); controller = new AbortController();
    const signal = controller.signal, current = ++generation;
    if (!live()) return;
    const parsed = parsedRules(read());
    const next = { rules: [], matches: new Map() };
    if (parsed.status !== 'ok') {
      snapshot = next; changed(); announce('Stored catalog filters are invalid. Threads remain visible.'); return;
    }
    if (parsed.rules.length) {
      // Bound each worker packet and the complete page cycle independently.
      const deadline = setTimeout(() => {
        if (controller?.signal !== signal || current !== generation || !live()) return;
        controller.abort(); snapshot = { rules: [], matches: new Map() }; changed();
        announce('Catalog filters exceeded their time limit. Threads remain visible.');
      }, 60000);
      try {
        for (let offset = 0; offset < cards.length;) {
          const batch = []; let size = JSON.stringify(parsed.rules).length + 128;
          while (offset < cards.length && batch.length < CATALOG_FILTER_LIMITS.cards) {
            const card = cards[offset], length = JSON.stringify(card).length + 1;
            if (size + length > CATALOG_FILTER_LIMITS.request) break;
            batch.push(card); size += length; offset++;
          }
          if (!batch.length) throw new Error('request-limit');
          const result = await matcher.match(parsed.rules, board, batch, { signal });
          if (signal.aborted || current !== generation || !live()) return;
          if (result.status !== 'ok') throw new Error('match-failed');
          for (const match of result.matches) next.matches.set(match.id, match.filter);
        }
        next.rules = parsed.rules;
      } catch {
        if (!signal.aborted && current === generation && live()) {
          snapshot = { rules: [], matches: new Map() }; changed(); announce('Catalog filters could not be checked. Threads remain visible.');
        }
        return;
      } finally { clearTimeout(deadline); }
    }
    if (signal.aborted || current !== generation || !live()) return;
    snapshot = next; changed(); updateHits(); announce('');
  }
  const closePicker = () => { if (picker?.open) picker.close(); picker?.classList.add('hidden'); };
  const closeHelp = () => { if (help?.open) help.close(); help?.classList.add('hidden'); };
  const close = () => {
    pendingSave?.abort(); pendingSave = null; closePicker(); closeHelp();
    if (dialog?.open) dialog.close(); dialog?.classList.add('hidden');
    if (list) { list.replaceChildren(); fields.clear(); }
    if (live()) opener.focus();
  };
  function setColor(field, color) {
    field.color = color;
    field.colorButton.style.backgroundColor = color;
    field.colorButton.textContent = color ? '' : '\u2215';
    if (color) delete field.colorButton.dataset.nocolor; else field.colorButton.dataset.nocolor = '1';
    field.colorButton.setAttribute('aria-label', color ? `Change color: ${color}` : 'Change Color');
  }
  function paletteFor(field) {
    closePicker(); picker.replaceChildren();
    const panel = node('div', undefined, 'panel catalogFilterPanel'); panel.id = 'colorpicker';
    const table = node('table'); table.id = 'filter-color-table';
    const body = node('tbody');
    const choose = raw => {
      const color = filterColor(raw);
      if (color === null) return;
      setColor(field, color); closePicker(); field.colorButton.focus();
    };
    for (let offset = 0; offset < palette.length; offset += 4) {
      const row = node('tr');
      for (const color of palette.slice(offset, offset + 4)) {
        const cell = node('td'), swatch = button('', () => choose(color), 'button clickbox');
        swatch.style.backgroundColor = color; swatch.setAttribute('aria-label', color); cell.append(swatch); row.append(cell);
      }
      body.append(row);
    }
    const footer = node('tfoot');
    const addFooter = child => { const row = node('tr'), cell = node('td'); cell.colSpan = 4; cell.append(child); row.append(cell); footer.append(row); };
    addFooter(node('label', 'Custom'));
    const customRow = node('span'), input = node('input'); input.type = 'text'; input.id = 'filter-rgb'; input.maxLength = 128;
    input.setAttribute('aria-label', 'Custom filter color');
    const select = button('', () => { if (!select.disabled) choose(input.value); }, 'button clickbox');
    select.id = 'filter-rgb-ok'; select.setAttribute('aria-label', 'Select Color'); select.disabled = true;
    input.addEventListener('keyup', () => { const color = filterColor(input.value); select.disabled = !color; select.style.backgroundColor = color ?? ''; });
    customRow.append(input, select); addFooter(customRow);
    const actions = node('span');
    const dismiss = button('Close', () => { closePicker(); field.colorButton.focus(); }, 'button'); dismiss.id = 'filter-palette-close';
    const clear = button('Clear', () => choose(''), 'button'); clear.id = 'filter-palette-clear'; actions.append(dismiss, ' ', clear); addFooter(actions);
    table.append(body, footer); panel.append(table); picker.append(panel);
    const rect = field.colorButton.getBoundingClientRect();
    panel.style.left = `${Math.min(Math.max(0, rect.left), Math.max(0, innerWidth - 160))}px`;
    panel.style.top = `${Math.min(rect.bottom + 3, Math.max(0, innerHeight - 200))}px`;
    picker.classList.remove('hidden'); picker.showModal();
  }
  function addRow(rule, savedIndex = null) {
    if (fields.size >= CATALOG_FILTER_LIMITS.rules) { message.textContent = `Filter limit: ${CATALOG_FILTER_LIMITS.rules}.`; return; }
    const index = nextId++, row = node('tr'); row.id = `filter-${index}`;
    const field = { savedIndex }; fields.set(row, field);
    const cell = child => { const td = node('td'); td.append(child); row.append(td); };
    const up = button('\u2191', () => { if (row.previousElementSibling) list.insertBefore(row, row.previousElementSibling); }, 'pointer');
    up.dataset.up = String(index); up.setAttribute('aria-label', `Move filter ${index + 1} up`); cell(up);
    const checkbox = (key, className, label) => {
      const input = node('input', undefined, className); input.type = 'checkbox'; input.checked = !!rule[key];
      input.setAttribute('aria-label', `${label} filter ${index + 1}`); field[key] = input; cell(input);
    };
    checkbox('active', 'filter-active', 'Enable');
    for (const [key, className, maximum] of [['pattern', 'filter-pattern', CATALOG_FILTER_LIMITS.pattern], ['boards', 'filter-boards', CATALOG_FILTER_LIMITS.boards]]) {
      const input = node('input', undefined, className); input.type = 'text'; input.value = rule[key]; input.maxLength = maximum;
      input.setAttribute('aria-label', `${key === 'pattern' ? 'Pattern' : 'Boards'} for filter ${index + 1}`); field[key] = input; cell(input);
    }
    field.colorButton = button('', () => paletteFor(field), 'button clickbox filter-color');
    field.colorButton.id = `filter-color-${index}`; cell(field.colorButton); setColor(field, filterColor(rule.color) ?? '');
    checkbox('hidden', 'filter-hide', 'Hide'); checkbox('top', 'filter-top', 'Move to top');
    const remove = button('\u00d7', () => { closePicker(); fields.delete(row); row.remove(); }, 'pointer');
    remove.dataset.target = String(index); remove.setAttribute('aria-label', `Delete filter ${index + 1}`); cell(remove);
    field.hits = node('td', '', 'filter-hits'); field.hits.id = `fhc-${index}`; row.append(field.hits);
    list.append(row); updateHits();
  }
  async function save() {
    if (pendingSave || !live() || !dialog.open) return;
    const rules = [...list.children].map(row => {
      const field = fields.get(row);
      return { active: Number(field.active.checked), pattern: field.pattern.value, boards: field.boards.value,
        hidden: Number(field.hidden.checked), top: Number(field.top.checked), ...(field.color ? { color: field.color } : {}) };
    });
    const checked = writeCatalogFilters(rules);
    if (checked.status !== 'ok') { message.textContent = 'Filters exceed their field or storage limits.'; return; }
    const request = new AbortController(); pendingSave = request; submit.disabled = true;
    try {
      const validated = await matcher.match(rules.map(rule => ({ ...rule, boards: '' })), board, [], { signal: request.signal });
      if (request.signal.aborted || !live() || pendingSave !== request) return;
      if (validated.status !== 'ok') { message.textContent = 'A pattern is invalid or could not be checked. Filters were not saved.'; return; }
      let result;
      if (persistent) result = await lock.run(() => {
        if (request.signal.aborted || !live() || pendingSave !== request || !dialog.open) return { status: 'cancelled' };
        if (read() !== expectedRaw) return { status: 'conflict' };
        if (!persistent) return { status: 'unavailable' };
        try {
          if (checked.raw === null) localStorage.removeItem(storageKey); else localStorage.setItem(storageKey, checked.raw);
          cachedRaw = checked.raw; return { status: 'ok' };
        } catch { persistent = false; return { status: 'unavailable' }; }
      }, request.signal);
      else result = { status: 'unavailable' };
      if (request.signal.aborted || !live() || pendingSave !== request) return;
      if (result?.status === 'unavailable' && !persistent) {
        cachedRaw = checked.raw; expectedRaw = checked.raw; await refresh();
        message.textContent = 'Filters are saved only in this tab. Browser storage or cross-tab locking is unavailable.';
      } else if (result?.status === 'ok') { await refresh(); if (!request.signal.aborted && live()) close(); }
      else message.textContent = result?.status === 'conflict'
        ? 'Catalog filters changed in another tab. Reopen the editor before saving.' : 'Catalog filters could not be saved.';
    } catch { if (live() && dialog.open && !request.signal.aborted) message.textContent = 'Catalog filters could not be saved.'; }
    finally { if (pendingSave === request) pendingSave = null; submit.disabled = false; }
  }
  function build() {
    fields = new Map();
    dialog = node('dialog', undefined, 'panel catalogFilterPanel hidden'); dialog.id = 'filters'; dialog.setAttribute('aria-label', 'Catalog Filters & Highlights');
    const header = node('div', 'Filters & Highlights', 'panelHeader');
    search = node('input'); search.id = 'filters-search'; search.type = 'text'; search.placeholder = 'Search'; search.maxLength = CATALOG_FILTER_LIMITS.pattern;
    search.setAttribute('aria-label', 'Search catalog filters');
    search.addEventListener('keyup', event => {
      if (event.key === 'Escape') search.value = '';
      for (const [row, field] of fields) row.style.display = field.pattern.value.toLowerCase().includes(search.value.toLowerCase()) ? '' : 'none';
    });
    const helpButton = button('', () => { help.classList.remove('hidden'); help.showModal(); }, 'icon helpIcon');
    helpButton.id = 'filters-help-open'; helpButton.setAttribute('aria-label', 'Filter help');
    const dismiss = button('', close, 'icon closeIcon'); dismiss.id = 'filters-close'; dismiss.setAttribute('aria-label', 'Close catalog filters');
    header.prepend(search); header.append(helpButton, dismiss);
    const table = node('table'); table.id = 'filter-table'; const head = node('thead'), headings = node('tr');
    for (const label of ['Order', 'On', 'Pattern', 'Boards', 'Color', 'Hide', 'Top', 'Del', '']) headings.append(node('th', label));
    head.append(headings); list = node('tbody'); list.id = 'filter-list';
    const foot = node('tfoot'), footerRow = node('tr'), footerCell = node('td'); footerCell.colSpan = 9;
    const add = button('Add', () => addRow({ active: 1, pattern: '', boards: '', hidden: 0, top: 0 }), 'left'); add.id = 'filters-add';
    const saveWrapper = node('span', undefined, 'right');
    message = node('span'); message.id = 'filters-msg'; message.setAttribute('role', 'status');
    submit = button('Save', save); submit.id = 'filters-save'; saveWrapper.append(message, submit);
    footerCell.append(add, saveWrapper); footerRow.append(footerCell); foot.append(footerRow); table.append(head, list, foot); dialog.append(header, table);
    picker = node('dialog', undefined, 'catalogFilterPalette hidden'); picker.id = 'filter-palette'; picker.setAttribute('aria-label', 'Filter color');
    help = node('dialog', undefined, 'panel catalogFilterPanel hidden'); help.id = 'filters-protip'; help.setAttribute('aria-label', 'Filters & Highlights Help');
    const helpHeader = node('div', 'Filters & Highlights Help', 'panelHeader');
    const helpClose = button('', closeHelp, 'icon closeIcon'); helpClose.id = 'filters-help-close'; helpClose.setAttribute('aria-label', 'Close filter help'); helpHeader.append(helpClose); help.append(helpHeader);
    for (const [title, text] of [
      ['Patterns', 'Words match case-insensitively at word boundaries. Spaces mean AND; | means OR. * matches within a word. Double quotes search a case-sensitive string; /pattern/i uses a regular expression.'],
      ['Names and tripcodes', 'Prefix a pattern with # for tripcodes or ## for names. Names and tripcodes match case-sensitively. A capcode follows the tripcode as !#mod, !#admin or another published capcode.'],
      ['Boards and order', 'Blank Boards applies on every board. Separate board names with spaces. The first matching active rule wins. Hide removes a thread; Top moves it before ordinary threads; Color highlights it. Pinned threads and active catalog searches bypass filters.'],
    ]) help.append(node('h4', title), node('p', text));
    dialog.append(picker, help); document.body.append(dialog);
    dialog.addEventListener('cancel', event => {
      event.preventDefault(); if (event.target !== dialog) return;
      if (document.activeElement === search) { search.value = ''; for (const row of fields.keys()) row.style.display = ''; }
      else close();
    });
    picker.addEventListener('cancel', event => { event.preventDefault(); closePicker(); });
    help.addEventListener('cancel', event => { event.preventDefault(); closeHelp(); });
  }
  function open() {
    if (!live()) return;
    if (!dialog) build();
    if (dialog.open) return;
    expectedRaw = read(); const parsed = parsedRules(expectedRaw);
    search.value = ''; message.textContent = ''; nextId = 0;
    list.replaceChildren(); fields.clear();
    if (parsed.status === 'ok') parsed.rules.forEach((rule, index) => addRow(rule, index));
    else message.textContent = 'Stored catalog filters are invalid. Review the new entries before replacing them.';
    dialog.style.top = `${scrollY + 60}px`; dialog.classList.remove('hidden'); dialog.showModal();
    (list.querySelector('.filter-active') ?? dialog).focus();
  }
  opener.hidden = false; opener.addEventListener('click', open);
  window.addEventListener('storage', event => {
    if (!persistent || event.storageArea && event.storageArea !== localStorage || event.key !== null && event.key !== storageKey) return;
    if (pendingSave) {
      pendingSave.abort();
      if (dialog?.open) message.textContent = 'Catalog filters changed in another tab. Reopen the editor before saving.';
    }
    void refresh();
  });
  window.addEventListener('pagehide', () => { suspended = true; controller?.abort(); close(); });
  window.addEventListener('pageshow', event => { if (event.persisted && !retired) { suspended = false; void refresh(); } });
  const observer = new MutationObserver(() => {
    if (!form.isConnected || !container.isConnected || document.getElementById('ctrl') !== form || document.getElementById('threads') !== container) {
      retired = true; controller?.abort(); close(); observer.disconnect();
    }
  });
  observer.observe(document.body, { childList: true, subtree: true });
  void refresh();
  return { snapshot: () => snapshot, setHits: value => { hits = value; updateHits(); }, open, refresh };
}
