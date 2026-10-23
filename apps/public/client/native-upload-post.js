import { mountPostingCaptcha } from '../static/posting-captcha.v1.js';
import { createUploadPostSession } from './native-upload-post-session.js';
import { uploadQuickReplyFile, checkQuickReplyUpload, cancelQuickReplyUpload, sendUploadBoardPost, renderQuickReplyError, schedulePostingRedirect } from './native-quick-reply-transport.js';

export function mountUploadPost(doc = document, win = window) {
  const context = doc.getElementById('watcher-context');
  if (context?.dataset.uploadBoard !== 'true' || context.dataset.board !== 'f' || context.dataset.thread !== '0') return null;
  try { const settings=win.localStorage.getItem('4chan-settings');if(settings && settings.length <= 32768 && JSON.parse(settings)?.disableAll===true)return null; } catch { /* Storage is optional. */ }
  const form = doc.getElementById('reply'), row = form?.querySelector('[data-upload-file-row]');
  const input = row?.querySelector('input[type=file]'), fallback = doc.querySelector('form.postForm[action="/f/upload"]');
  if (!form || !input || !fallback) return null;
  const captchaRoot = form.querySelector('[data-posting-captcha]');
  if (captchaRoot?.dataset.noscriptOnly === 'true') return null;
  const captcha = captchaRoot ? mountPostingCaptcha(captchaRoot) : null;
  if (captchaRoot && !captcha) return null;
  const status = doc.createElement('span');status.setAttribute('role', 'status');status.setAttribute('aria-live', 'polite');
  const cancel = doc.createElement('button');cancel.type = 'button';cancel.textContent = 'Cancel file';cancel.hidden = true;
  input.parentElement.append(' ', cancel, ' ', status);
  const origin = win.location.origin, target = { board: 'f', thread: '0', origin };
  let controller = null;
  const notify = text => { status.textContent = text; };
  const session = createUploadPostSession({
    upload: (file, signal) => uploadQuickReplyFile({ ...target, file, signal }),
    check: (receipt, signal) => checkQuickReplyUpload({ ...target, receipt, signal }),
    cancel: (receipt, signal) => cancelQuickReplyUpload({ ...target, receipt, signal }),
    send: (fields, signal) => sendUploadBoardPost({ ...target, fields, signal }), notify,
    wait: signal => new Promise((resolve, reject) => {
      const stop = () => { win.clearTimeout(timer);reject(new Error('Upload stopped.')); };
      const timer = win.setTimeout(() => { signal.removeEventListener('abort', stop);resolve(); }, 1500);
      signal.addEventListener('abort', stop, { once: true });if (signal.aborted) stop();
    }),
  });
  const fields = () => {
    const values = {};
    for (const [key,value] of new FormData(form)) if (typeof value === 'string') values[key] = value;
    return values;
  };
  function sync(active) {
    for (const button of form.querySelectorAll('button[type=submit]')) button.disabled = active || session.state().uncertain;
    input.disabled = active || session.state().hasFile;
    cancel.hidden = !session.state().hasFile;cancel.disabled = active || session.state().uncertain;
  }
  async function submit(event) {
    if (event.defaultPrevented) return;
    event.preventDefault();
    if (controller || (captcha && !captcha.canSubmit())) return;
    const selected = input.files?.[0];
    if (!selected) { notify('Choose one SWF file.');return; }
    const category = form.elements.namedItem('filetag')?.value;
    if (!['0','6','1','2','3','5','4'].includes(category)) { notify('Error: Invalid file tag.');return; }
    controller = new AbortController();sync(true);
    try {
      const result = await session.submit(selected, fields, () => !captcha || captcha.canSubmit(), controller.signal);
      if (result) {
        form.dispatchEvent(new Event('captcha:submitted'));
        if (result.error) { renderQuickReplyError(status,result.error);schedulePostingRedirect(result,'f'); }
        else { win.location.assign(`/f/thread/${result.thread}#p${result.post}`); }
      }
    } catch (error) { notify(error.message); }
    finally { controller = null;sync(false); }
  }
  async function discard() {
    if (controller) return;
    controller = new AbortController();sync(true);
    try { await session.discard(controller.signal);input.value = ''; }
    catch (error) { notify(error.message); }
    finally { controller = null;sync(false); }
  }
  const stop = () => controller?.abort();
  form.addEventListener('submit',submit);cancel.addEventListener('click',discard);win.addEventListener('pagehide',stop);
  row.hidden = false;input.required = true;fallback.hidden = true;
  const help = doc.getElementById('postHelp');if(help) help.textContent = 'Choose an SWF file and category, then post.';
  return { destroy() { stop();form.removeEventListener('submit',submit);cancel.removeEventListener('click',discard);win.removeEventListener('pagehide',stop);row.hidden=true;input.required=false;fallback.hidden=false;status.remove();cancel.remove(); } };
}
if(typeof document !== 'undefined' && typeof window !== 'undefined') mountUploadPost();
