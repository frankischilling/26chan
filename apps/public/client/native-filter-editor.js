import { FILTER_LIMITS } from './native-filter-limits.js';
import { readFilterRules, filterColor } from './native-filter-rules.js';

const palette = ['#E0B0FF', '#F2F3F4', '#7DF9FF', '#FFFF00', '#FBCEB1', '#FFBF00',
  '#ADFF2F', '#0047AB', '#00A550', '#007FFF', '#AF0A0F', '#B5BD68'];
const types = [[0, 'Tripcode'], [1, 'Name'], [2, 'Comment'], [4, 'ID'], [5, 'Subject'], [6, 'Filename']];
function node(tag, text, className) {
  const element = document.createElement(tag);
  if (text !== undefined) element.textContent = text;
  if (className) element.className = className;
  return element;
}
function button(text, action, className) {
  const element = node('button', text, className);
  element.type = 'button';
  element.addEventListener('click', action);
  return element;
}

export function filterEditor({ board, read, save, match, changed }) {
  let active;
  return function open(opener) {
    if (active) { active.dialog.focus(); return; }
    let expected = read();
    const parsed = readFilterRules(expected, { migrateLegacy: true });
    const dialog = node('dialog', undefined, 'nativeSettings nativeFilters UIPanel');
    dialog.id = 'filtersMenu';
    dialog.setAttribute('aria-labelledby', 'filters-title');
    const panel = node('div', undefined, 'extPanel reply');
    const header = node('div', undefined, 'panelHeader');
    const title = node('span', 'Filters & Highlights');
    title.id = 'filters-title';
    const message = node('p', '', 'filterEditorMessage');
    message.setAttribute('role', 'status');
    let controller, popup, nextId = 0;
    const closePopup = () => { if (popup) { popup.close(); popup.remove(); popup = null; } };
    const close = () => {
      controller?.abort(); closePopup(); dialog.close(); dialog.remove();
      if (active?.dialog === dialog) active = null;
      opener?.focus();
    };
    const dismiss = button('\u00d7', close, 'panelCtrl');
    dismiss.setAttribute('aria-label', 'Close filters');
    const help = button('?', () => {
      closePopup();
      const detail = node('dialog', undefined, 'nativeSettings filterHelp');
      detail.id = 'filtersHelp';
      detail.setAttribute('aria-label', 'Filter help');
      detail.append(node('h3', 'Filters & Highlights'));
      for (const text of [
        'Tripcode, Name and ID match exact text. Other types accept words, a quoted phrase, or /regular expression/i.',
        'Separate boards with spaces or commas. Blank Boards applies a filter to posts on all boards; Auto requires explicit boards.',
        'The first matching active filter wins. Hide replaces content with a View control; a selected color highlights matching content.',
        'Auto adds matching threads when you manually refresh the enabled Thread Watcher. Subject filters apply on board indexes, not inside threads.',
        'Patterns run in bounded workers. A failed or over-budget match leaves posts visible. Save Settings separately to enable filtering.',
      ]) detail.append(node('p', text));
      detail.append(button('Close help', () => { closePopup(); help.focus(); }));
      detail.addEventListener('cancel', event => { event.preventDefault(); closePopup(); help.focus(); });
      dialog.append(detail); popup = detail; detail.showModal();
    }, 'filterHelpButton');
    help.setAttribute('aria-label', 'Filter help');
    header.append(title, help, dismiss);
    const form = node('form');
    const table = node('table');
    const thead = node('thead');
    const headings = node('tr');
    for (const label of ['', 'On', 'Pattern', 'Boards', 'Type', 'Color', 'Auto', 'Hide', 'Del']) headings.append(node('th', label));
    thead.append(headings);
    const list = node('tbody'); list.id = 'filter-list';
    const fields = new Map();
    const footer = node('tfoot'); const footerRow = node('tr'); const footerCell = node('td'); footerCell.colSpan = 9;
    const add = button('Add', () => addRow({ active: true, pattern: '', boards: '', type: 0, auto: false, hide: false }, true));
    add.dataset.cmd = 'filters-add';
    const submit = node('button', 'Save', 'right'); submit.type = 'submit'; submit.dataset.cmd = 'filters-save';
    footerCell.append(add, submit); footerRow.append(footerCell); footer.append(footerRow);
    table.append(thead, list, footer);
    form.append(table, message);
    panel.append(header, form); dialog.append(panel);

    function colorControl(field, color) {
      field.color = color;
      field.colorButton.style.backgroundColor = color;
      field.colorButton.textContent = color ? '' : '\u2215';
      field.colorButton.setAttribute('aria-label', color ? `Change color: ${color}` : 'Choose color');
      if (color) delete field.colorButton.dataset.nocolor; else field.colorButton.dataset.nocolor = '1';
    }
    function openPalette(field) {
      closePopup();
      const picker = node('dialog', undefined, 'nativeSettings nativeFilterPalette extPanel reply');
      picker.id = 'filter-palette'; picker.setAttribute('aria-label', 'Filter color');
      const grid = node('div', undefined, 'filterPaletteGrid'); grid.id = 'colorpicker';
      const choose = color => { colorControl(field, color); closePopup(); field.colorButton.focus(); };
      for (const color of palette) {
        const swatch = button('', () => choose(filterColor(color)), 'colorbox');
        swatch.style.backgroundColor = color; swatch.setAttribute('aria-label', color);
        grid.append(swatch);
      }
      const custom = node('input'); custom.id = 'palette-custom-input'; custom.type = 'text'; custom.maxLength = 128;
      const customLabel = node('label', 'Custom '); customLabel.htmlFor = custom.id;
      const chooseCustom = button('Select Color', () => { const color = filterColor(custom.value); if (color) choose(color); });
      chooseCustom.id = 'palette-custom-ok'; chooseCustom.disabled = true;
      custom.addEventListener('input', () => { chooseCustom.disabled = !filterColor(custom.value); });
      picker.append(grid, customLabel, custom, chooseCustom,
        button('Close', () => { closePopup(); field.colorButton.focus(); }), button('Clear', () => choose('')));
      picker.addEventListener('cancel', event => { event.preventDefault(); closePopup(); field.colorButton.focus(); });
      dialog.append(picker); popup = picker; picker.showModal();
    }
    function addRow(rule, focus = false) {
      if (list.children.length >= FILTER_LIMITS.filters) { message.textContent = `Filter limit: ${FILTER_LIMITS.filters}.`; return; }
      const index = nextId++;
      const row = node('tr'); row.id = `filter-${index}`;
      const field = {};
      fields.set(row, field);
      const cell = child => { const td = node('td'); td.append(child); row.append(td); };
      const up = button('\u2191', () => { if (row.previousElementSibling) list.insertBefore(row, row.previousElementSibling); });
      up.setAttribute('aria-label', `Move filter ${index + 1} up`); up.dataset.cmd = 'filters-up'; cell(up);
      const checkbox = (key, label) => {
        const input = node('input'); input.type = 'checkbox'; input.checked = rule[key] === true;
        input.setAttribute('aria-label', `${label} filter ${index + 1}`); field[key] = input; cell(input);
      };
      checkbox('active', 'Enable');
      for (const [key, className, limit] of [['pattern', 'fPattern', FILTER_LIMITS.patterns], ['boards', 'fBoards', FILTER_LIMITS.boardText]]) {
        const input = node('input', undefined, className); input.type = 'text'; input.value = rule[key]; input.maxLength = limit;
        input.setAttribute('aria-label', `${key === 'pattern' ? 'Pattern' : 'Boards'} for filter ${index + 1}`);
        field[key] = input; cell(input);
      }
      const type = node('select'); type.setAttribute('aria-label', `Type for filter ${index + 1}`);
      for (const [value, label] of types) { const option = node('option', label); option.value = String(value); type.append(option); }
      type.value = String(rule.type); field.type = type; cell(type);
      field.colorButton = button('', () => openPalette(field), 'colorbox fColor'); cell(field.colorButton);
      colorControl(field, filterColor(rule.color) ?? '');
      if (filterColor(rule.color) === null) message.textContent = 'An invalid stored color was cleared. Review the filters before saving.';
      checkbox('auto', 'Automatically watch'); checkbox('hide', 'Hide');
      const remove = button('\u00d7', () => { closePopup(); fields.delete(row); row.remove(); add.focus(); }, 'fDel');
      remove.setAttribute('aria-label', `Delete filter ${index + 1}`); remove.dataset.cmd = 'filters-del'; cell(remove);
      list.append(row); if (focus) field.pattern.focus();
    }
    if (parsed.status === 'ok') for (const rule of parsed.rules) addRow(rule);
    else message.textContent = 'Stored filters are invalid. Saving will replace them with the entries shown here.';
    form.addEventListener('submit', async event => {
      event.preventDefault(); if (submit.disabled) return;
      const rules = [...list.children].map(row => {
        const field = fields.get(row);
        return { active: field.active.checked, pattern: field.pattern.value, boards: field.boards.value,
          type: Number(field.type.value), auto: field.auto.checked, hide: field.hide.checked,
          ...(field.color ? { color: field.color } : {}) };
      });
      const raw = rules.length ? JSON.stringify(rules) : null;
      if (readFilterRules(raw).status !== 'ok') { message.textContent = 'Filters exceed their storage or field limits.'; return; }
      submit.disabled = true; controller = new AbortController();
      try {
        const checked = await match(rules, board, [], { mode: 'page', signal: controller.signal });
        if (controller.signal.aborted || !dialog.isConnected) return;
        if (checked.status !== 'ok') { message.textContent = 'A pattern is invalid or could not be checked. Filters were not saved.'; return; }
        const result = await save(raw, expected, controller.signal);
        if (controller.signal.aborted || !dialog.isConnected) return;
        if (result?.status !== 'ok') { message.textContent = 'Filters changed or could not be saved. Reopen the editor before saving again.'; return; }
        changed(result);
        if (result.persisted === false) {
          expected = raw;
          message.textContent = 'Filters are saved only in this tab. Browser storage or cross-tab locking is unavailable.';
          return;
        }
        close();
      } catch { if (dialog.isConnected) message.textContent = 'Filters could not be saved.'; }
      finally { submit.disabled = false; }
    });
    dialog.addEventListener('cancel', event => { event.preventDefault(); close(); });
    dialog.addEventListener('click', event => { if (event.target === dialog) close(); });
    document.body.append(dialog); active = { dialog }; dialog.showModal();
    (list.querySelector('.fPattern') ?? add).focus();
  };
}
