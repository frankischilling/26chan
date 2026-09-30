import { postId } from '../static/thread-watcher-core.v1.js';
import { cancelQuickReplyUpload, checkQuickReplyUpload, commentLengthWarning, quoteInsertion, sendQuickReply,
  uploadQuickReplyFile } from './native-quick-reply-transport.js';
import { mountNativePostForm } from './native-post-form.js';
import { quickReplyPosition } from './native-quick-reply-position.js';
import { restorePostPreferences } from './native-post-preferences.js';
import { postNumberReply } from './native-post-numbers.js';

export function mountNativeQuickReply({ board, thread, settings, savePosition, committed }) {
  const source = document.querySelector('form.postEditor');
  if (!/^[a-z0-9]{1,10}$/.test(board)) return null;
  restorePostPreferences(source);
  const uploadSource = document.querySelector(`form.postForm[action="/${board}/upload"]`);
  const uploadSourceInput = uploadSource?.querySelector('input[type=file][name=upfile]');
  const approvedThread = source?.elements.namedItem('upload_id') ? postId(source.elements.resto?.value) : null;
  let dialog, form, comment, error, submit, current, controller, opener, position, epoch = 0;
  let busy = false, commentTimer, uploadInput, uploadStatus, uploadCheck, uploadCancel, uploadSpoiler;
  let uploadReceipt = null, uploadOwned = false, uploadBusy = false, uploadController, uploadTimer;
  let uploadName = '', uploadPhase = 'empty', uploadCanCheck = false, uploadPolls = 0, uploadEpoch = 0, postingAttachment = false;
  const pollDelays = [1000, 2000, 4000];
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
    const next = quickReplyPosition(value, { width: innerWidth, height: innerHeight,
      panelWidth: dialog.offsetWidth, panelHeight: dialog.offsetHeight });
    if (!next) {
      dialog.style.left = 'auto'; dialog.style.right = '0px'; dialog.style.top = '10%'; return;
    }
    position = next;
    dialog.style.left = `${position.left}px`; dialog.style.top = `${position.top}px`; dialog.style.right = 'auto';
  }
  function removeSourceApproval(uncertain = false) {
    if (!source?.elements.namedItem('upload_id')) return;
    for (const key of ['upload_id', 'upload_capability']) source.elements.namedItem(key)?.remove();
    source.elements.namedItem('spoiler')?.closest('tr')?.remove();
    const nativeComment = source.elements.namedItem('com'); if (nativeComment) nativeComment.required = true;
    for (const button of source.querySelectorAll('button[type=submit], button:not([type])')) button.textContent = 'Post';
    const help = source.querySelector('#postHelp'); if (help) help.textContent = uncertain
      ? 'The image posting result is uncertain. Check the thread before posting again. Further replies require a comment.'
      : 'The approved image was posted. Further replies require a comment. Save your deletion password.';
  }
  function removeInlineCapability() {
    form?.querySelectorAll('[data-qr-upload-capability]').forEach(input => input.remove());
    if (uploadSpoiler) { uploadSpoiler.checked = false; uploadSpoiler.disabled = true; }
  }
  function addInlineCapability(receipt) {
    removeInlineCapability();
    for (const key of ['upload_id', 'upload_capability']) {
      const input = node('input'); input.type = 'hidden'; input.name = key; input.value = receipt[key];
      input.dataset.qrUploadCapability = 'true'; form.append(input);
    }
    if (uploadSpoiler) uploadSpoiler.disabled = false;
  }
  function stopUploadRequest() {
    clearTimeout(uploadTimer); uploadTimer = null;
    uploadController?.abort(); uploadController = null; uploadBusy = false;
  }
  function renderUpload() {
    if (!uploadStatus) return;
    const state = uploadReceipt?.state ?? uploadPhase;
    const labels = {
      empty: '', uploading: `Uploading ${uploadName}…`, queued: `${uploadName} queued for processing.`,
      processing: `${uploadName} is processing.`, approved: `${uploadName} is approved.`,
      failed: 'Processing failed. Cancel this upload or choose another file.',
      incomplete: 'The upload is incomplete. Cancel it or choose the file again.',
      checking: `Checking ${uploadName}…`,
    };
    uploadStatus.textContent = labels[state] ?? '';
    uploadCheck.hidden = !uploadCanCheck || uploadBusy || !uploadReceipt;
    uploadCancel.hidden = !(uploadBusy || (uploadOwned && uploadReceipt));
    uploadCancel.disabled = busy;
    uploadCheck.disabled = busy;
    uploadInput.disabled = uploadBusy || busy;
    uploadSpoiler.disabled = busy || state !== 'approved';
    sync();
  }
  function resetInlineUpload() {
    stopUploadRequest(); uploadEpoch++; uploadReceipt = null; uploadOwned = false; uploadName = '';
    uploadPhase = 'empty'; uploadCanCheck = false; uploadPolls = 0; removeInlineCapability();
    if (uploadInput) uploadInput.value = ''; renderUpload();
  }
  function bestEffortCancel() {
    const receipt = uploadReceipt, id = current;
    stopUploadRequest(); uploadEpoch++;
    if (receipt && uploadOwned && !postingAttachment && postId(id)) {
      void cancelQuickReplyUpload({ board, thread: id, receipt, keepalive: true }).catch(() => {});
    }
    uploadReceipt = null; uploadOwned = false; removeInlineCapability();
  }
  function retireAmbiguousAttachment() {
    if (source?.elements.namedItem('upload_id')) removeSourceApproval(true);
    stopUploadRequest(); uploadEpoch++; uploadReceipt = null; uploadOwned = false; uploadName = '';
    uploadPhase = 'empty'; uploadCanCheck = false; removeInlineCapability(); renderUpload();
  }
  function close() {
    if (!dialog) return;
    if (postingAttachment) retireAmbiguousAttachment(); else bestEffortCancel();
    epoch++; controller?.abort(); controller = null; busy = false;
    postingAttachment = false; uploadName = ''; uploadPhase = 'empty'; uploadCanCheck = false; uploadPolls = 0;
    uploadInput = uploadStatus = uploadCheck = uploadCancel = uploadSpoiler = null;
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
    submit.disabled = uploadBusy || (uploadOwned && uploadReceipt?.state !== 'approved') || (locked && !busy);
    if (locked) message('This thread is closed.');
    else if (error.textContent === 'This thread is closed.') message('');
  }
  function open(id = thread, quote = null, selected = '', quoting = false) {
    if (!source || disabled() || !postId(id) || closed(id) || busy) return false;
    if (dialog) {
      if (current !== id) {
        if (uploadOwned || uploadBusy) {
          void cancelInlineUpload().then(ok => { if (ok && dialog) open(id, quote, selected, quoting); });
          return true;
        }
        current = id; form.elements.resto.value = id; document.getElementById('qrTid').textContent = id; comment.value = '';
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
    if (source.elements.name?.type === 'hidden') {
      const name = node('input'); name.type = 'hidden'; name.name = 'name'; form.append(name);
    } else {
      const name = field('name', 'Name'); name.value = source.elements.name?.value ?? ''; name.autocomplete = 'off';
    }
    const options = field('email', 'Options'); options.id = 'qrEmail'; options.value = source.elements.email?.value ?? '';
    const sourceFlag = source.elements.namedItem('flag');
    if (sourceFlag?.tagName === 'SELECT') {
      const row = node('div'), flag = node('select'); flag.name = 'flag'; flag.id = 'qrFlag';
      flag.className = 'flagSelector'; flag.setAttribute('aria-label', 'Flag');
      for (const original of sourceFlag.options) {
        const option = node('option', original.textContent); option.value = original.value;
        flag.append(option);
      }
      flag.value = sourceFlag.value; row.append(flag); fields.append(row);
    }
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
    } else if (uploadSourceInput) {
      const row = node('div', undefined, 'qr-file-row');
      uploadInput = node('input'); uploadInput.type = 'file'; uploadInput.id = 'qrFile'; uploadInput.name = 'upfile';
      uploadInput.size = 19; uploadInput.accept = uploadSourceInput.accept; uploadInput.title = 'Choose one file; choose again to replace it. Shift-click to remove the current file.';
      const spoilerLabel = node('label'); uploadSpoiler = node('input'); uploadSpoiler.type = 'checkbox'; uploadSpoiler.name = 'spoiler';
      uploadSpoiler.value = 'on'; uploadSpoiler.disabled = true; const spoiler = node('span'); spoiler.id = 'qrSpoiler'; spoilerLabel.append(uploadSpoiler, 'Spoiler?'); spoiler.append(spoilerLabel);
      uploadStatus = node('span', '', 'qr-file-status'); uploadStatus.id = 'qrUploadStatus'; uploadStatus.setAttribute('role', 'status');
      uploadCheck = node('button', 'Check status'); uploadCheck.type = 'button'; uploadCheck.hidden = true;
      uploadCancel = node('button', 'Cancel file'); uploadCancel.type = 'button'; uploadCancel.hidden = true;
      row.append(uploadInput, spoiler, uploadStatus, uploadCheck, uploadCancel); fields.append(row);
      uploadInput.addEventListener('click', event => { if (event.shiftKey) { event.preventDefault(); void cancelInlineUpload(); } });
      uploadInput.addEventListener('change', () => { const file = uploadInput.files?.[0]; if (file) void selectUpload(file); });
      uploadCheck.addEventListener('click', () => { void checkUpload(false); });
      uploadCancel.addEventListener('click', () => { void cancelInlineUpload(); });
      row.addEventListener('dragover', event => { if ([...event.dataTransfer?.types ?? []].includes('Files')) event.preventDefault(); });
      row.addEventListener('drop', event => {
        event.preventDefault(); const files = [...event.dataTransfer?.files ?? []];
        if (files.length !== 1) { message('Choose one file.', 'upload'); return; }
        void selectUpload(files[0]);
      });
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
    renderUpload();
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
    if (uploadBusy || (uploadOwned && uploadReceipt?.state !== 'approved')) return;
    if (!dialog || disabled() || closed(current) || !form.reportValidity()) return;
    message(''); busy = true; submit.value = 'Sending';
    const active = ++epoch, id = current; controller = new AbortController();
    const fields = Object.fromEntries(new FormData(form));
    postingAttachment = typeof fields.upload_id === 'string' && fields.upload_id.length > 0;
    renderUpload();
    try {
      const result = await sendQuickReply({ board, thread: id, fields, signal: controller.signal });
      if (active !== epoch || disabled()) return;
      if (result.error) { postingAttachment = false; message(result.error); return; }
      if (source.elements.namedItem('upload_id')) {
        // Both editors refer to the same one-use approval. Once committed,
        // neither reopening QR nor submitting the ordinary form can reuse it.
        removeSourceApproval();
      }
      if (uploadOwned) resetInlineUpload();
      postingAttachment = false;
      const saved = Promise.resolve().then(() => committed?.(id, result.post)).catch(() => {});
      if (settings().persistentQR === true) {
        comment.value = ''; form.querySelector('.qr-approved-image')?.remove();
        for (const key of ['upload_id', 'upload_capability']) form.elements.namedItem(key)?.remove();
      } else close();
      await saved;
      const event = new Event('4chanQRPostSuccess'); event.detail = { threadId: id, postId: result.post }; document.dispatchEvent(event);
    } catch (failure) {
      if (active === epoch) {
        if (postingAttachment) retireAmbiguousAttachment();
        postingAttachment = false;
        message(failure instanceof Error ? failure.message : 'Connection error. Check the thread before retrying.');
      }
    } finally { if (active === epoch) { busy = false; controller = null; submit.value = 'Post'; renderUpload(); sync(); } }
  }
  function scheduleUploadCheck() {
    clearTimeout(uploadTimer);
    if (!dialog || !uploadReceipt || !['queued', 'processing'].includes(uploadReceipt.state)) return;
    if (uploadPolls >= pollDelays.length) { uploadCanCheck = true; renderUpload(); return; }
    const delay = pollDelays[uploadPolls++];
    uploadTimer = setTimeout(() => { uploadTimer = null; void checkUpload(true); }, delay);
  }
  async function checkUpload(automatic) {
    if (busy) return false;
    if (disabled()) { close(); return false; }
    if (!dialog || uploadBusy || !uploadReceipt || !postId(current)) return false;
    const active = ++uploadEpoch, id = current; uploadController = new AbortController(); uploadBusy = true;
    uploadPhase = 'checking'; uploadCanCheck = false; renderUpload();
    try {
      const next = await checkQuickReplyUpload({ board, thread: id, receipt: uploadReceipt, signal: uploadController.signal });
      if (active !== uploadEpoch || !dialog || current !== id) return false;
      uploadReceipt = next; uploadPhase = next.state; uploadBusy = false; uploadController = null;
      if (next.state === 'approved') { addInlineCapability(next); message(''); }
      else if (next.state === 'failed') message('Processing failed. Cancel this upload or choose another file.', 'upload');
      else if (next.state === 'incomplete') message('The upload is incomplete. Cancel it or choose the file again.', 'upload');
      else if (automatic) scheduleUploadCheck();
      else uploadCanCheck = true;
      renderUpload(); return true;
    } catch (failure) {
      if (active !== uploadEpoch || !dialog) return false;
      uploadBusy = false; uploadController = null; uploadCanCheck = true; uploadPhase = uploadReceipt?.state ?? 'empty';
      message(failure instanceof Error ? failure.message : 'Upload status unavailable. Try again.', 'upload'); renderUpload(); return false;
    }
  }
  async function cancelInlineUpload() {
    if (busy) return false;
    if (disabled()) { close(); return false; }
    if (!uploadOwned && !uploadBusy) return true;
    stopUploadRequest();
    if (!uploadReceipt) { resetInlineUpload(); message(''); return true; }
    const receipt = uploadReceipt, active = ++uploadEpoch, id = current; uploadController = new AbortController(); uploadBusy = true; renderUpload();
    try {
      await cancelQuickReplyUpload({ board, thread: id, receipt, signal: uploadController.signal });
      if (active !== uploadEpoch || !dialog) return false;
      uploadBusy = false; uploadController = null; resetInlineUpload(); message(''); return true;
    } catch (failure) {
      if (active !== uploadEpoch || !dialog) return false;
      uploadBusy = false; uploadController = null;
      message(failure instanceof Error ? failure.message : 'Upload could not be canceled. Try again.', 'upload'); renderUpload(); return false;
    }
  }
  async function selectUpload(file) {
    if (busy) return;
    if (disabled()) { close(); return; }
    if (!dialog || !postId(current)) return;
    if (uploadReceipt && !await cancelInlineUpload()) return;
    else if (uploadBusy) { stopUploadRequest(); uploadEpoch++; }
    const active = ++uploadEpoch, id = current; uploadName = file.name; uploadPhase = 'uploading'; uploadCanCheck = false; uploadPolls = 0;
    removeInlineCapability(); uploadController = new AbortController(); uploadBusy = true; message(''); renderUpload();
    try {
      const receipt = await uploadQuickReplyFile({ board, thread: id, file, signal: uploadController.signal });
      if (active !== uploadEpoch || !dialog || current !== id) return;
      uploadReceipt = receipt; uploadOwned = true; uploadBusy = false; uploadController = null; uploadPhase = receipt.state; renderUpload(); scheduleUploadCheck();
    } catch (failure) {
      if (active !== uploadEpoch || !dialog) return;
      uploadReceipt = null; uploadOwned = false; uploadBusy = false; uploadController = null; uploadPhase = 'empty'; uploadCanCheck = false;
      if (uploadInput) uploadInput.value = '';
      message(failure instanceof Error ? failure.message : 'Upload failed. Choose the file again.', 'upload'); renderUpload();
    }
  }
  const nav = document.querySelector('.threadNav.desktop'); let entry;
  if (source && (nav || source.elements.namedItem('upload_id')) && thread && !closed(thread)) {
    entry = node('div', undefined, 'open-qr-wrap'); const link = node('a', 'Post a Reply', 'open-qr-link'); link.href = '#postForm'; link.dataset.cmd = 'open-qr';
    link.addEventListener('click', event => { if (!disabled()) { event.preventDefault(); open(thread); } }); entry.append('[', link, ']'); if (nav) nav.prepend(entry); else source.before(entry);
  }
  document.addEventListener('click', event => {
    if (disabled() || event.button !== 0 || event.metaKey || event.shiftKey || event.altKey) return;
    const target = postNumberReply(event.target.closest?.('.postInfo > .postNum > a,.postInfoM > .postNum > a'), board);
    if (!target || (!source && !closed(target.thread))) return;
    event.preventDefault(); quote(target.thread, event.ctrlKey ? null : target.post, getSelection()?.toString() ?? '');
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
