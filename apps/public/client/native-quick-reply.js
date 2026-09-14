import { postId } from '../static/thread-watcher-core.v1.js';
import { commentLengthWarning, quoteInsertion, sendQuickReply } from './native-quick-reply-transport.js';
import { mountNativePostForm } from './native-post-form.js';

export function mountNativeQuickReply({ board, thread, settings, savePosition, committed }) {
  const source = document.querySelector('form.postEditor');
  if (!/^[a-z0-9]{1,10}$/.test(board)) return null;
  const approvedThread = source?.elements.namedItem('upload_id') ? postId(source.elements.resto?.value) : null;
  let dialog, form, comment, error, submit, current, controller, opener, position, epoch = 0;
  let busy = false, commentTimer;
  const disabled = () => settings().disableAll === true || settings().quickReply === false;
  const section = id => document.getElementById(`t${id}`);
  const closed = id => section(id) ? ['closed', 'archived'].some(key => section(id).dataset[key] === 'true')
    : approvedThread !== id;
  const node = (tag, text, className) => {
    const element = document.createElement(tag);
    if (text !== undefined) element.textContent = text;
    if (className) element.className = className;
    return element;
  };
  const mobile = () => matchMedia('(max-width: 480px)').matches;
  function place(value, reopening = false) {
    if (!dialog) return;
    if (mobile()) { dialog.style.left = '5px'; dialog.style.right = 'auto'; dialog.style.top = `${scrollY + (reopening ? 25 : 28)}px`; return; }
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
    clearTimeout(commentTimer); commentTimer = null;
    dialog?.remove(); dialog = form = comment = error = submit = null; current = null;
    opener?.focus();
  }
  function message(text, type = '') { if (error) { error.textContent = text; error.hidden = !text; error.dataset.type = type; } }
  function checkComment() {
    if (!dialog || closed(current)) return;
    const warning = commentLengthWarning(comment.value, source.dataset.commentLimit);
    if (warning) message(warning, 'length');
    else if (error.dataset.type === 'length') message('');
  }
  function sync() {
    if (disabled()) close();
    if (entry) entry.hidden = disabled() || closed(thread) || mobile();
    if (!dialog) return;
    const locked = closed(current);
    submit.disabled = locked && !busy;
    if (locked) message('This thread is closed.');
    else if (error.textContent === 'This thread is closed.') message('');
  }
  function open(id = thread, quote = null, selected = '', quoting = false) {
    if (!source || disabled() || !postId(id) || closed(id) || busy) return false;
    if (dialog) {
      if (current !== id) { current = id; form.elements.resto.value = id; document.getElementById('qrTid').textContent = id; comment.value = '';
        const upload = form.querySelector('.qr-upload-link'); if (upload) upload.href = `/${board}/thread/${id}#upfile`;
      }
      if (quoting || quote || selected) insert(quote, selected);
      else comment.focus();
      if (mobile()) place(null, true);
      return true;
    }
    current = id; opener = document.activeElement;
    dialog = node('dialog', undefined, 'extPanel reply nativeQuickReply'); dialog.id = 'quickReply';
    dialog.dataset.trackpos = 'QR-position';
    dialog.setAttribute('aria-labelledby', 'qrHeader');
    const header = node('div', undefined, 'drag postblock'); header.id = 'qrHeader';
    const title = node('span', 'Reply to Thread No.'); const number = node('span', id); number.id = 'qrTid'; title.append(number);
    const dismiss = node('button', '\u00d7', 'extButton'); dismiss.id = 'qrClose'; dismiss.type = 'button';
    dismiss.setAttribute('aria-label', 'Close Quick Reply'); dismiss.addEventListener('click', close); header.append(title, dismiss);
    form = node('form', undefined, 'postEditor'); form.name = 'qrPost'; form.method = 'post';
    form.action = `/${board}/imgboard.php`; form.enctype = 'multipart/form-data';
    for (const [name, value] of [['mode', 'regist'], ['resto', id]]) {
      const input = node('input'); input.type = 'hidden'; input.name = name; input.value = value; form.append(input);
      if (name === 'resto') input.id = 'qrResto';
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
    const onEdit = event => {
      if (event.key === 'Escape' && !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey) { event.preventDefault(); close(); return; }
      else if (event.ctrlKey && event.key?.toLowerCase() === 's') {
        event.preventDefault(); event.stopPropagation();
        const start = comment.selectionStart, end = comment.selectionEnd, empty = comment.value.length === 0;
        const value = `[spoiler]${comment.value.slice(start, end)}[/spoiler]`;
        comment.setRangeText(value, start, end, 'end'); if (empty) comment.setSelectionRange(9, 9);
      }
      clearTimeout(commentTimer); commentTimer = setTimeout(checkComment, 500);
    };
    for (const type of ['keydown', 'paste', 'cut']) comment.addEventListener(type, onEdit);
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
    if (quoting || quote || selected) insert(quote, selected); else comment.focus();
    return true;
  }
  function insert(id, selected) {
    const result = quoteInsertion(comment.value, comment.selectionStart, comment.selectionEnd, id, selected.slice(0, 65536));
    comment.value = result.value; comment.setSelectionRange(result.caret, result.caret);
    if (result.caret === comment.value.length) comment.scrollTop = comment.scrollHeight;
    comment.focus();
  }
  function quote(id, post, selected) {
    if (disabled() || !postId(id)) return false;
    if (closed(id)) { alert('This thread is closed'); return false; }
    return open(id, post, selected, true);
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
      if (form.elements.namedItem('upload_id')) {
        // Both editors refer to the same one-use approval. Once committed,
        // neither reopening QR nor submitting the ordinary form can reuse it.
        for (const key of ['upload_id', 'upload_capability']) source.elements.namedItem(key)?.remove();
        source.elements.namedItem('spoiler')?.closest('tr')?.remove();
        const nativeComment = source.elements.namedItem('com'); if (nativeComment) nativeComment.required = true;
        for (const button of source.querySelectorAll('button[type=submit], button:not([type])')) button.textContent = 'Post';
        const help = source.querySelector('#postHelp'); if (help) help.textContent = 'The approved image was posted. Further replies require a comment. Save your deletion password.';
      }
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
  if (source && (nav || source.elements.namedItem('upload_id')) && thread && !closed(thread)) {
    entry = node('div', undefined, 'open-qr-wrap'); const link = node('a', 'Post a Reply', 'open-qr-link'); link.href = '#postForm'; link.dataset.cmd = 'open-qr';
    link.addEventListener('click', event => { if (!disabled()) { event.preventDefault(); open(thread); } }); entry.append('[', link, ']'); if (nav) nav.prepend(entry); else source.before(entry);
  }
  document.addEventListener('click', event => {
    if (disabled() || event.button !== 0) return;
    const link = event.target.closest?.('.postInfo > .postNum'); if (!link) return;
    const target = link.closest('.thread'), id = postId(target?.id.slice(1)), post = postId(link.closest('.postInfo')?.id.slice(2));
    if (id && post) {
      event.preventDefault(); quote(id, event.ctrlKey ? null : post, getSelection()?.toString() ?? '');
    }
  });
  window.addEventListener('pagehide', close); window.addEventListener('resize', () => { place(position); sync(); });
  document.addEventListener('4chanThreadUpdated', sync);
  document.addEventListener('boardThreadStateChanged', sync);
  mountNativePostForm({ source, thread, openQuickReply: () => {
    if (!thread || disabled()) return false;
    open(thread); return true;
  } });
  sync(); return { open: () => !!thread && quote(thread, null, getSelection()?.toString() ?? ''), sync, close };
}
