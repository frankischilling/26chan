// Release-owned settings controls. Stored strings never become HTML or CSS.
export function installSettings({ catalog, read, save, toggleWatcher, openFilters, clearThreads }) {
  const navigation = document.querySelector('.boardList');
  let active = null;
  let opener = null;
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
  function link(id, text, action) {
    const element = node('a', text);
    element.id = id;
    element.href = '#settings';
    element.addEventListener('click', event => { event.preventDefault(); action(element); });
    return element;
  }
  function close() {
    if (!active) return;
    active.close();
    active.remove();
    active = null;
    opener?.focus();
  }
  function open(source) {
    if (active) { close(); return; }
    opener = source;
    const initial = read();
    const dialog = node('dialog', undefined, `nativeSettings ${catalog ? 'catalogSettings panel' : 'extensionSettings UIPanel'}`);
    dialog.id = catalog ? 'theme' : 'settingsMenu';
    dialog.setAttribute('aria-labelledby', 'native-settings-title');
    const content = catalog ? dialog : node('div', undefined, 'extPanel nativeSettingsBody');
    const header = node('h2', undefined, 'panelHeader');
    const title = node('span', 'Settings');
    title.id = 'native-settings-title';
    header.append(title);
    const dismiss = button('\u00d7', close, 'panelCtrl');
    dismiss.id = catalog ? 'theme-close' : 'settings-close';
    dismiss.setAttribute('aria-label', 'Close settings');
    header.append(dismiss);
    content.append(header);
    const form = node('form');
    const fields = new Map();
    function option(parent, key, label, tip, className) {
      const row = node('li', undefined, className);
      const caption = node('label');
      const input = node('input', undefined, 'menuOption');
      input.type = 'checkbox';
      input.dataset.option = key;
      input.id = catalog && key === 'threadWatcher' ? 'theme-tw' : `setting-${key}`;
      input.checked = (key === 'threadHiding' ? initial[key] !== false : initial[key] === true) && (!catalog || initial.disableAll !== true);
      fields.set(key, { input, initial: input.checked });
      caption.append(input, document.createTextNode(` ${label}`));
      row.append(caption);
      parent.append(row);
      if (tip) parent.append(node('li', tip, `settings-tip ${className || ''}`));
      return input;
    }
    let category;
    let expand;
    let filterCategory, filterExpand;
    if (catalog) {
      form.append(node('h4', 'Options'));
      const options = node('ul', undefined, 'clickset');
      option(options, 'threadWatcher', 'Thread Watcher');
      form.append(options);
    } else {
      const all = node('p', undefined, 'settingsExpandAll');
      all.id = 'settings-exp-all';
      all.append('[', link('settings-expand-all', 'Expand All Settings', () => {
        category.hidden = false; expand.setAttribute('aria-expanded', 'true');
        filterCategory.hidden = false; filterExpand.setAttribute('aria-expanded', 'true');
      }), ']');
      form.append(all);
      const heading = node('h3', undefined, 'settings-cat-lbl');
      category = node('ul', undefined, 'settings-cat');
      category.id = 'settings-monitoring';
      category.hidden = Object.keys(initial).length !== 0;
      expand = button('Monitoring', () => {
        category.hidden = !category.hidden;
        expand.setAttribute('aria-expanded', String(!category.hidden));
      }, 'settings-expand');
      expand.setAttribute('aria-controls', category.id);
      expand.setAttribute('aria-label', 'Monitoring');
      expand.setAttribute('aria-expanded', String(!category.hidden));
      heading.append(expand);
      option(category, 'threadWatcher', 'Thread Watcher', "Keep track of threads you're watching and see when they receive new posts");
      option(category, 'threadAutoWatcher', 'Automatically watch threads you create', '', 'settings-sub');
      option(category, 'fixedThreadWatcher', 'Pin Thread Watcher to the page', 'Thread Watcher will scroll with you', 'settingsDesktop');
      const filterHeading = node('h3', undefined, 'settings-cat-lbl');
      filterCategory = node('ul', undefined, 'settings-cat');
      filterCategory.id = 'settings-filters';
      filterCategory.hidden = Object.keys(initial).length !== 0;
      filterExpand = button('Filters & Post Hiding', () => {
        filterCategory.hidden = !filterCategory.hidden;
        filterExpand.setAttribute('aria-expanded', String(!filterCategory.hidden));
      }, 'settings-expand');
      filterExpand.setAttribute('aria-controls', filterCategory.id);
      filterExpand.setAttribute('aria-label', 'Filters & Post Hiding');
      filterExpand.setAttribute('aria-expanded', String(!filterCategory.hidden));
      filterHeading.append(filterExpand);
      const filter = option(filterCategory, 'filter', 'Filter and highlight specific threads/posts', 'Enable pattern-based filters');
      filter.parentElement.parentElement.append(' [', link('filters-edit', 'Edit', source => openFilters?.(source)), ']');
      const hiding = option(filterCategory, 'threadHiding', 'Thread hiding', 'Hide entire threads by clicking the minus button');
      hiding.parentElement.parentElement.append(' [', link('thread-hiding-clear', 'Clear History', () => clearThreads?.()), ']');
      option(filterCategory, 'hideStubs', 'Hide thread stubs', "Don't display stubs of hidden threads");
      const global = node('ul');
      option(global, 'disableAll', 'Disable the native extension', '', 'settings-off');
      form.append(filterHeading, filterCategory, heading, category, global);
    }
    const message = node('p', '', 'settingsMessage');
    message.setAttribute('role', 'status');
    const actions = node('div', undefined, 'center');
    const submit = node('button', 'Save Settings');
    submit.type = 'submit';
    submit.id = catalog ? 'theme-save' : 'settings-save';
    actions.append(submit);
    form.append(message, actions);
    form.addEventListener('submit', async event => {
      event.preventDefault();
      if (submit.disabled) return;
      submit.disabled = true;
      const changes = {};
      for (const [key, field] of fields) {
        if (catalog || field.input.checked !== field.initial) changes[key] = field.input.checked;
      }
      try {
        const result = await save(changes);
        if (result === false) {
          message.textContent = 'Settings could not be saved. Try again.';
          return;
        }
        document.dispatchEvent(new CustomEvent(catalog ? '4chanCatalogThemeApplied' : '4chanSettingsSaved'));
        close();
        // The extension applies settings through navigation. A volatile fallback
        // stays on this page so unavailable storage cannot discard the changes.
        if (!catalog && result.persisted) location.assign(location.pathname + location.search);
      } catch {
        message.textContent = 'Settings could not be saved. Try again.';
      } finally { submit.disabled = false; }
    });
    content.append(form);
    if (!catalog) dialog.append(content);
    dialog.addEventListener('cancel', event => { event.preventDefault(); close(); });
    dialog.addEventListener('click', event => {
      if (event.target !== dialog) return;
      const bounds = dialog.getBoundingClientRect();
      if (!catalog || event.clientX < bounds.left || event.clientX > bounds.right
        || event.clientY < bounds.top || event.clientY > bounds.bottom) close();
    });
    document.body.append(dialog);
    if (catalog) dialog.style.top = `${window.scrollY + 60}px`;
    active = dialog;
    dialog.showModal();
    if (catalog || !category.hidden) fields.get('threadWatcher').input.focus();
    else expand.focus();
  }
  const desktop = node('span', undefined, 'settingsDesktop');
  const desktopLink = link('settingsWindowLink', 'Settings', open);
  desktopLink.setAttribute('aria-haspopup', 'dialog');
  desktop.append('[', desktopLink, ']');
  const mobile = node('span', undefined, 'settingsMobile');
  const watcher = link('watcher-open-mobile', 'TW', toggleWatcher);
  watcher.setAttribute('aria-controls', 'threadWatcher');
  watcher.hidden = true;
  const mobileLink = link('settingsWindowLinkMobile', 'Settings', open);
  mobileLink.setAttribute('aria-haspopup', 'dialog');
  mobile.append(watcher, ' ', mobileLink);
  const navigationLinks = node('span');
  navigationLinks.id = 'navtopright';
  navigationLinks.append(desktop, mobile);
  navigation?.append(navigationLinks);
  return {
    setWatcherEnabled(enabled, visible) {
      watcher.hidden = !enabled;
      watcher.setAttribute('aria-expanded', String(visible));
    },
  };
}
