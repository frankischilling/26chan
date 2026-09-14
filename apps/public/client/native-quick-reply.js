import { postId } from '../static/thread-watcher-core.v1.js';
import { quoteInsertion, sendQuickReply } from './native-quick-reply-transport.js';

export function mountNativeQuickReply({ board, thread, settings, savePosition, committed }) {
  const source = document.querySelector('form.postEditor');
  if (!source || !/^[a-z0-9]{1,10}$/.test(board)) return null;
  let dialog, form, comment, error, submit, current, controller, opener, position, epoch = 0;
  let busy = false;
  const disabled = () => settings().disableAll === true || settings().quickReply === false;
  const section = id => document.getElementById(`t${id}`);
  const closed = id => section(id) ? ['closed', 'archived'].some(key => section(id).dataset[key] === 'true')
    : !(source.elements.namedItem('upload_id') && source.elements.resto?.value === id);
  const node = (tag, text, className) => {
    const element = document.createElement(tag);
    if (text !== undefined) element.textContent = text;
    if (className) element.className = className;
    return element;
  };
  const mobile = () => matchMedia('(max-width: 480px)').matches;
  function place(value) {
    if (!dialog) return;
    if (mobile()) { dialog.style.left = '5px'; dialog.style.right = 'auto'; dialog.style.top = `${scrollY + 28}px`; return; }
    if (!value || !Number.isFinite(value.left) || !Number.isFinite(value.top)) {
      dialog.style.left = 'auto'; dialog.style.right = '0px'; dialog.style.top = '10%'; return;
    }
    position = { left: Math.max(0, Math.min(innerWidth - dialog.offsetWidth, value.left)),
      top: Math.max(0, Math.min(innerHeight - dialog.offsetHeight, value.top)) };
    dialog.style.left = `${position.left}px`; dialog.style.top = `${position.top}px`; dialog.style.right = 'auto';
  }
  function close() {
    if (!dialog) return;
    epoch++; controller?.abort(); controller = null; busy = false;
    dialog?.remove(); dialog = form = comment = error = submit = null; current = null;
    opener?.focus();
  }
  function message(text) { if (error) { error.textContent = text; error.hidden = !text; } }
  function sync() {
    if (disabled()) close();
    if (entry) entry.hidden = disabled() || closed(thread) || mobile();
    if (!dialog) return;
    const locked = closed(current);
    submit.disabled = locked && !busy;
    if (locked) message('This thread is closed.');
    else if (error.textContent === 'This thread is closed.') message('');
  }
  function open(id = thread, quote = null, selected = '') {
    if (disabled() || !postId(id) || closed(id) || busy) return false;
    if (dialog) {
      if (current !== id) { current = id; form.elements.resto.value = id; document.getElementById('qrTid').textContent = id; comment.value = '';
        const upload = form.querySelector('.qr-upload-link'); if (upload) upload.href = `/${board}/thread/${id}#upfile`;
      }
      if (quote || selected) insert(quote, selected);
      else comment.focus();
      if (mobile()) place();
      return true;
    }
    current = id; opener = document.activeElement;
    dialog = node('dialog', undefined, 'extPanel reply nativeQuickReply'); dialog.id = 'quickReply';
    dialog.setAttribute('aria-labelledby', 'qrHeader');
    const header = node('div', undefined, 'drag postblock'); header.id = 'qrHeader';
    const title = node('span', 'Reply to Thread No.'); const number = node('span', id); number.id = 'qrTid'; title.append(number);
    const dismiss = node('button', '\u00d7', 'extButton'); dismiss.id = 'qrClose'; dismiss.type = 'button';
    dismiss.setAttribute('aria-label', 'Close Quick Reply'); dismiss.addEventListener('click', close); header.append(title, dismiss);
    form = node('form', undefined, 'postEditor'); form.name = 'qrPost'; form.method = 'post';
    form.action = `/${board}/imgboard.php`; form.enctype = 'multipart/form-data';
    for (const [name, value] of [['mode', 'regist'], ['resto', id]]) {
      const input = node('input'); input.type = 'hidden'; input.name = name; input.value = value; form.append(input);
    }
    const fields = node('div'); fields.id = 'qrForm'; form.append(fields);
    function field(name, label, type = 'text') {
      const row = node('div'); const input = node(type === 'textarea' ? 'textarea' : 'input');
      if (type !== 'textarea') input.type = type;
      input.id = name === 'com' ? 'qrCom' : `qr-${name}`; input.name = name;
      input.setAttribute('aria-label', label); input.placeholder = label;
      row.append(input); fields.append(row); return input;
    }
    const name = field('name', 'Name'); name.value = source.elements.name?.value ?? ''; name.autocomplete = 'off';
    const options = field('email', 'Options'); options.id = 'qrEmail'; options.value = source.elements.email?.value ?? '';
    comment = field('com', 'Comment', 'textarea'); comment.rows = 4;
    const password = field('pwd', 'Deletion password', 'password'); password.minLength = 8; password.maxLength = 128;
    password.required = true; password.autocomplete = 'new-password'; password.value = source.elements.pwd?.value ?? '';
    // Approved capabilities are available only on the isolated upload result page.
    for (const key of ['upload_id', 'upload_capability']) {
      const value = source.elements.namedItem(key)?.value;
      if (value) { const input = node('input'); input.type = 'hidden'; input.name = key; input.value = value; form.append(input); }
    }
    if (source.elements.namedItem('upload_id')) {
      const row = node('div', undefined, 'qr-approved-image'), label = node('label'), spoiler = node('input'); spoiler.type = 'checkbox'; spoiler.name = 'spoiler'; spoiler.value = 'on';
      label.append(spoiler, 'Spoiler?'); row.append(label); fields.append(row);
    } else {
      const upload = document.querySelector('form.postForm[action$="/upload"]');
      if (upload) { const link = node('a', 'Post with an image', 'qr-upload-link'); link.href = `/${board}/thread/${id}#upfile`; const row = node('div'); row.append(link); fields.append(row); }
    }
    const actions = node('div'); submit = node('input'); submit.type = 'submit'; submit.value = 'Post'; actions.append(submit); fields.append(actions);
    error = node('div'); error.id = 'qrError'; error.hidden = true; error.setAttribute('role', 'alert');
    dialog.append(header, form, error); document.body.append(dialog); dialog.show();
    dialog.addEventListener('cancel', event => { event.preventDefault(); close(); });
    form.addEventListener('submit', event => { event.preventDefault(); void send(); });
    comment.addEventListener('keydown', event => {
      if (event.key === 'Escape' && !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey) { event.preventDefault(); close(); }
      else if (event.ctrlKey && event.key.toLowerCase() === 's') {
        event.preventDefault(); event.stopPropagation();
        const start = comment.selectionStart, end = comment.selectionEnd, empty = comment.value.length === 0;
        const value = `[spoiler]${comment.value.slice(start, end)}[/spoiler]`;
        comment.setRangeText(value, start, end, 'end'); if (empty) comment.setSelectionRange(9, 9);
      }
    });
    let drag;
    header.addEventListener('pointerdown', event => {
      if (mobile() || event.button !== 0 || event.target.closest('button')) return;
      const box = dialog.getBoundingClientRect(); drag = { x: event.clientX - box.left, y: event.clientY - box.top };
      header.setPointerCapture(event.pointerId); event.preventDefault();
    });
    header.addEventListener('pointermove', event => { if (drag) place({ left: event.clientX - drag.x, top: event.clientY - drag.y }); });
    header.addEventListener('pointerup', () => { if (drag && position) void savePosition?.({ ...position }); drag = null; });
    header.addEventListener('pointercancel', () => { drag = null; });
    place(position ?? settings()['QR-position']); sync();
    if (quote || selected) insert(quote, selected); else comment.focus();
    return true;
  }
  function insert(id, selected) {
    const result = quoteInsertion(comment.value, comment.selectionStart, comment.selectionEnd, id, selected.slice(0, 65536));
    comment.value = result.value; comment.setSelectionRange(result.caret, result.caret); comment.focus(); comment.scrollTop = comment.scrollHeight;
  }
  async function send() {
    if (busy) { controller?.abort(); return; }
    if (!dialog || disabled() || closed(current) || !form.reportValidity()) return;
    message(''); busy = true; submit.value = 'Sending';
    const active = ++epoch, id = current; controller = new AbortController();
    const fields = Object.fromEntries(new FormData(form));
    try {
      const result = await sendQuickReply({ board, thread: id, fields, signal: controller.signal });
      if (active !== epoch || disabled()) return;
      if (result.error) { message(result.error); return; }
      const saved = Promise.resolve().then(() => committed?.(id, result.post)).catch(() => {});
      if (settings().persistentQR === true) {
        comment.value = ''; form.querySelector('.qr-approved-image')?.remove();
        for (const key of ['upload_id', 'upload_capability']) form.elements.namedItem(key)?.remove();
      } else close();
      await saved;
      const event = new Event('4chanQRPostSuccess'); event.detail = { threadId: id, postId: result.post }; document.dispatchEvent(event);
    } catch (failure) {
      if (active === epoch) message(failure instanceof Error ? failure.message : 'Connection error. Check the thread before retrying.');
    } finally { if (active === epoch) { busy = false; controller = null; submit.value = 'Post'; sync(); } }
  }
  const nav = document.querySelector('.threadNav.desktop'); let entry;
  if ((nav || source.elements.namedItem('upload_id')) && thread && !closed(thread)) {
    entry = node('div', undefined, 'open-qr-wrap'); const link = node('a', 'Post a Reply', 'open-qr-link'); link.href = '#postForm'; link.dataset.cmd = 'open-qr';
    link.addEventListener('click', event => { if (!disabled()) { event.preventDefault(); open(thread); } }); entry.append('[', link, ']'); if (nav) nav.prepend(entry); else source.before(entry);
  }
  document.addEventListener('click', event => {
    if (disabled() || event.button !== 0 || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return;
    const link = event.target.closest?.('.postInfo > .postNum'); if (!link) return;
    const target = link.closest('.thread'), id = postId(target?.id.slice(1)), post = postId(link.closest('.postInfo')?.id.slice(2));
    if (id && post && open(id, post, getSelection()?.toString() ?? '')) event.preventDefault();
  });
  window.addEventListener('pagehide', close); window.addEventListener('resize', () => { place(position); sync(); });
  document.addEventListener('4chanThreadUpdated', sync);
  document.addEventListener('boardThreadStateChanged', sync);
  sync(); return { open: () => open(thread || postId([...document.querySelectorAll('.thread')].find(item => item.getBoundingClientRect().bottom > 0)?.id.slice(1))), sync, close };
}
