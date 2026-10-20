const TOKEN = /^c1_[a-f0-9]{64}$/;
const TICKET = /^t1_[a-f0-9]{64}$/;
const NONCE = /^[a-f0-9]{64}$/;
const mounted = new WeakMap();
const initialRequests = new WeakMap();
export function validScope(board, target, purpose) {
  return typeof board === 'string' && typeof target === 'string' && typeof purpose === 'string' && /^[a-z0-9]{1,10}$/.test(board) && /^(0|[1-9][0-9]{0,18})$/.test(target)
    && BigInt(target) <= 9223372036854775807n
    && (purpose === 'thread' ? target === '0' : ['reply', 'report'].includes(purpose) && target !== '0');
}
function png(image) {
  if (typeof image !== 'string' || !image.startsWith('data:image/png;base64,') || image.length > 87406) return false;
  const data = image.slice(22), padding = data.endsWith('==') ? 2 : data.endsWith('=') ? 1 : 0;
  return /^[A-Za-z0-9+/]+={0,2}$/.test(data) && data.length % 4 === 0 && data.length / 4 * 3 - padding <= 65536;
}
export function validReply(reply) {
  if (!reply || typeof reply.csrf !== 'string' || !NONCE.test(reply.csrf) || typeof reply.ticket_challenge !== 'boolean' || typeof reply.report_exempt !== 'boolean') return false;
  const c = reply.challenge;
  if (!c || (reply.report_exempt && (c.state !== 'disabled' || reply.ticket_challenge))) return false;
  if (c.state === 'disabled') return !reply.ticket_challenge;
  if (c.state === 'credits') return !reply.ticket_challenge && Number.isInteger(c.remaining) && c.remaining >= 1 && c.remaining <= 3
    && Number.isInteger(c.ttl_seconds) && c.ttl_seconds >= 1 && c.ttl_seconds <= 120;
  if (c.state === 'wait') return !reply.ticket_challenge && Number.isInteger(c.seconds) && c.seconds >= 1 && c.seconds <= 900
    && (c.ticket === null || (typeof c.ticket === 'string' && TICKET.test(c.ticket)));
  return c.state === 'ready' && typeof c.token === 'string' && TOKEN.test(c.token) && png(c.image) && typeof c.slider === 'boolean'
    && (c.slider ? png(c.background) : c.background === null)
    && Number.isInteger(c.offset_max) && c.offset_max >= 0 && c.offset_max <= 80
    && Number.isInteger(c.ttl_seconds) && c.ttl_seconds >= 4 && c.ttl_seconds <= 120
    && Number.isInteger(c.cooldown_seconds) && c.cooldown_seconds >= 0 && c.cooldown_seconds <= 300;
}

export function createPostingCaptcha(form, { board, target, purpose, classic = false, prefix = '' }, doc = document) {
  if (!validScope(board, String(target), purpose)) return null;
  const node = (tag, id, className) => {
    const value = doc.createElement(tag);
    if (id) value.id = `${prefix}${id}`;
    if (className) value.className = className;
    return value;
  };
  const root = node('div', 't-root', 'postingCaptcha');
  Object.assign(root.dataset, { postingCaptcha: '', board, target: String(target), purpose, classic: String(classic), exempt: 'false', noscriptOnly: 'false', ttl: '0' });
  for (const [name, id] of [['captcha-csrf', null], ['t-challenge', 't-challenge'], ['captcha-credit', null]]) {
    const input = node('input', id); input.type = 'hidden'; input.name = name; root.append(input);
  }
  const frame = node('div', 't-frame', 'captchaFrame'); frame.hidden = true;
  const images = node('div', 't-cnt', 'captchaImage');
  for (const [id, className, alt] of [['t-bg', 'captchaBackground', 'CAPTCHA background'], ['t-fg', 'captchaForeground', 'CAPTCHA text']]) {
    const image = node('img', id, className); image.alt = alt; image.draggable = false; if (id === 't-bg') image.hidden = true; images.append(image);
  }
  const sliderLabel = node('label', null, 'captchaSliderLabel'); sliderLabel.htmlFor = `${prefix}t-slider`; sliderLabel.textContent = 'Align the image'; sliderLabel.hidden = true;
  const slider = node('input', 't-slider', 'captchaSlider'); slider.type = 'range'; slider.min = '0'; slider.max = '100'; slider.value = '0'; slider.hidden = true;
  const label = node('label'); label.htmlFor = `${prefix}t-resp`; label.textContent = 'CAPTCHA text';
  const answer = node('input', 't-resp'); answer.type = 'text'; answer.name = 't-response'; answer.maxLength = 64; answer.autocomplete = 'off'; answer.autocapitalize = 'characters'; answer.spellcheck = false;
  frame.append(images, sliderLabel, slider, label, answer);
  const msg = node('p', 't-msg', 'captchaMessage'); msg.setAttribute('role', 'status'); msg.setAttribute('aria-live', 'polite');
  const load = node('button', 't-load'); load.type = 'button'; load.textContent = 'Get Captcha';
  const help = node('button', 't-help'); help.type = 'button'; help.textContent = '?'; help.setAttribute('aria-label', 'CAPTCHA help');
  root.append(frame, msg, load, help); form.append(root); return root;
}

export function mountPostingCaptcha(root, { win = window, changed = () => {}, interceptSubmit = true } = {}) {
  if (!root || root.dataset.noscriptOnly === 'true') return null;
  if (mounted.has(root)) return mounted.get(root);
  const form = root.closest('form'), board = root.dataset.board, purpose = root.dataset.purpose;
  let target = root.dataset.target;
  if (!form || !validScope(board, target, purpose)) return null;
  const find = name => root.querySelector(`[name="${name}"]`);
  const csrf = find('captcha-csrf'), token = find('t-challenge'), credit = find('captcha-credit'), answer = find('t-response');
  const frame = root.querySelector('.captchaFrame'), fg = root.querySelector('.captchaForeground'), bg = root.querySelector('.captchaBackground');
  const slider = root.querySelector('.captchaSlider'), sliderLabel = root.querySelector('.captchaSliderLabel'), msg = root.querySelector('.captchaMessage');
  const load = root.querySelector('button[id$="t-load"]'), help = root.querySelector('button[id$="t-help"]');
  if (![csrf, token, credit, answer, frame, fg, bg, slider, sliderLabel, msg, load, help].every(Boolean)) return null;
  let storage;
  try { storage = win.localStorage; } catch { /* In-memory ticket reuse still works. */ }
  const ticketKey = `26chan:captcha-ticket:${board}`;
  let ticket = null;
  try { const saved = storage?.getItem(ticketKey); if (TICKET.test(saved)) ticket = saved; } catch { /* Storage is optional. */ }
  let generation = 0, request, timeout, expiry, timer, deadline = 0, offset = 0, state = 'empty', busy = false, disposed = false, submitting = false;
  const now = () => win.Date.now();
  let previousProof = '';
  const update = () => {
    load.disabled = busy || submitting || now() < deadline;
    const proof = `${state}|${csrf.value}|${token.value}|${credit.value}`;
    const proofChanged = proof !== previousProof; previousProof = proof;
    changed(proofChanged);
  };
  function clearProof() {
    win.clearTimeout(expiry); expiry = null;
    token.value = ''; credit.value = ''; answer.value = ''; answer.required = false; frame.hidden = true;
    fg.removeAttribute('src'); bg.removeAttribute('src'); bg.hidden = true; bg.style.transform = ''; slider.value = '0'; slider.hidden = true; sliderLabel.hidden = true;
  }
  function stop() { generation++; request?.abort(); request = null; win.clearTimeout(timeout); win.clearTimeout(expiry); win.clearInterval(timer); busy = false; deadline = 0; }
  function invalidate(message = '') {
    if (disposed) return;
    stop(); clearProof(); state = 'empty'; root.hidden = false; load.textContent = 'Get Captcha'; msg.textContent = message; update();
  }
  function wait(seconds, visible, resume = false) {
    win.clearInterval(timer); deadline = now() + seconds * 1000;
    const tick = () => {
      const remaining = Math.max(0, Math.ceil((deadline - now()) / 1000));
      load.textContent = visible && remaining ? `Wait ${remaining}s` : state === 'ticket' ? 'Continue' : 'Get Captcha';
      update();
      if (!remaining) { win.clearInterval(timer); timer = null; if (resume && !disposed && !submitting) void fetchChallenge(); }
    };
    tick(); if (deadline > now()) timer = win.setInterval(tick, 250);
  }
  function expire(seconds) {
    expiry = win.setTimeout(() => { clearProof(); state = 'empty'; root.hidden = false; msg.textContent = 'CAPTCHA expired. Click Get Captcha to try again.'; update(); }, Math.max(0, seconds * 1000 - 3000));
  }
  function apply(reply, focus = true) {
    clearProof(); csrf.value = reply.csrf; root.hidden = false;
    const c = reply.challenge;
    if (c.state === 'disabled') { state = reply.report_exempt ? 'exempt' : 'disabled'; root.hidden = true; msg.textContent = ''; update(); return; }
    if (c.state === 'credits') {
      state = 'credits'; credit.value = '1'; msg.textContent = 'Verification not required.'; load.textContent = 'Get Captcha'; expire(c.ttl_seconds); update(); return;
    }
    if (c.state === 'wait') {
      state = 'wait';
      if (c.ticket) { ticket = c.ticket; try { storage?.setItem(ticketKey, ticket); } catch { /* Keep the memory copy. */ } }
      msg.textContent = 'Please wait a while.'; wait(c.seconds, true, true); return;
    }
    state = reply.ticket_challenge ? 'ticket' : 'ready'; token.value = c.token; fg.src = c.image; bg.hidden = !c.slider;
    if (c.slider) bg.src = c.background;
    offset = c.offset_max; slider.hidden = !c.slider; sliderLabel.hidden = !c.slider; frame.hidden = false;
    answer.required = !reply.ticket_challenge; msg.textContent = reply.ticket_challenge ? 'Complete this CAPTCHA to start the waiting period.' : '';
    load.textContent = reply.ticket_challenge ? 'Continue' : 'Get Captcha'; wait(c.cooldown_seconds, false); expire(c.ttl_seconds); if (focus) answer.focus(); update();
  }
  async function fetchChallenge() {
    if (disposed || busy || submitting || now() < deadline) return;
    const ticketAnswer = state === 'ticket' ? { challenge: token.value, answer: answer.value.trim() } : null;
    if (ticketAnswer && !ticketAnswer.answer) { msg.textContent = 'Type the CAPTCHA text, then click Continue.'; answer.focus(); return; }
    const current = ++generation; request?.abort(); request = new AbortController(); const controller = request;
    win.clearTimeout(expiry); clearProof(); busy = true; msg.textContent = 'Loading CAPTCHA…'; update();
    const body = new URLSearchParams({ action: NONCE.test(csrf.value) ? 'challenge' : 'init', board, target, purpose });
    if (body.get('action') === 'challenge') body.set('csrf', csrf.value);
    if (ticket) body.set('ticket', ticket);
    if (ticketAnswer) { body.set('ticket_challenge', ticketAnswer.challenge); body.set('ticket_answer', ticketAnswer.answer); }
    let rejectAbort;
    const aborted = new Promise((_, reject) => { rejectAbort = reject; });
    const abort = () => rejectAbort(new Error('CAPTCHA request stopped.'));
    controller.signal.addEventListener('abort', abort, { once: true });
    timeout = win.setTimeout(() => controller.abort(), 12000);
    let releaseInitial;
    try {
      if (body.get('action') === 'init') {
        // The first response establishes the shared anonymous/form cookies.
        // Keep simultaneous main/QR initialization from replacing each other.
        const previous = initialRequests.get(win) ?? Promise.resolve();
        const finished = new Promise(resolve => { releaseInitial = resolve; });
        const tail = previous.then(() => finished); initialRequests.set(win, tail);
        void tail.then(() => { if (initialRequests.get(win) === tail) initialRequests.delete(win); });
        await Promise.race([previous, aborted]);
        if (disposed || current !== generation || controller.signal.aborted) return;
      }
      const exchange = async () => {
      const response = await win.fetch('/captcha', { method: 'POST', credentials: 'same-origin', mode: 'same-origin', redirect: 'error', cache: 'no-store', body, signal: controller.signal });
      if (!response.ok || !response.headers.get('content-type')?.startsWith('application/json')) throw new Error();
      if (controller.signal.aborted) { await response.body?.cancel(); throw new Error(); }
      const text = await response.text(); if (text.length > 262144) throw new Error();
      const reply = JSON.parse(text); if (!validReply(reply) || (reply.report_exempt && purpose !== 'report')) throw new Error();
      return reply;
      };
      const reply = await Promise.race([exchange(), aborted]);
      if (disposed || current !== generation) return;
      busy = false; request = null; apply(reply);
    } catch {
      if (disposed || current !== generation) return;
      busy = false; clearProof(); state = 'empty'; csrf.value = ''; root.hidden = false;
      msg.textContent = 'Could not load the CAPTCHA. Please try again.'; wait(60, true);
    } finally { releaseInitial?.(); controller.signal.removeEventListener('abort', abort); if (current === generation) { win.clearTimeout(timeout); timeout = null; } }
  }
  function canSubmit() {
    if (disposed || busy || submitting) return false;
    if (['disabled', 'exempt', 'credits'].includes(state)) return true;
    if (state === 'ready' && TOKEN.test(token.value) && answer.value.trim()) return true;
    msg.textContent = state === 'ticket' ? 'Complete this verification before posting.' : 'Complete the CAPTCHA before submitting.';
    if (state === 'ticket' && answer.value.trim()) void fetchChallenge();
    return false;
  }
  const submittingForm = event => { if (!canSubmit()) { event.preventDefault(); event.stopImmediatePropagation(); } };
  const sliding = () => { const value = Number(slider.value); if (Number.isFinite(value)) bg.style.transform = `translateX(${-Math.max(0, Math.min(100, value)) * offset / 100}px)`; };
  const showingHelp = () => win.alert(root.dataset.classic === 'true' ? 'Type the text in the image. Load another CAPTCHA if it is unreadable.' : 'Move the slider to align the image, then type the text you see. Load another CAPTCHA if it is unreadable.');
  const hiding = () => { invalidate(); csrf.value = ''; };
  const showing = event => { if (event.persisted) hiding(); };
  function memberAdvice() {
    // The public hint only requests advice. The issuer decides whether the
    // HttpOnly session qualifies; this cookie never grants a bypass itself.
    try { if (purpose !== 'report' && /(?:^|;\s*)pass_enabled=1(?:;|$)/.test(win.document?.cookie ?? '')) void fetchChallenge(); }
    catch { /* Cookie access may be unavailable; manual loading still works. */ }
  }
  const consumed = () => { invalidate(); memberAdvice(); };
  load.addEventListener('click', fetchChallenge); help.addEventListener('click', showingHelp); slider.addEventListener('input', sliding);
  if (interceptSubmit) form.addEventListener('submit', submittingForm, true); form.addEventListener('captcha:submitted', consumed); win.addEventListener('pagehide', hiding); win.addEventListener('pageshow', showing);
  const api = {
    canSubmit, invalidate, afterSubmit: consumed, load: fetchChallenge,
    cooldownBypass: () => !disposed && state === 'disabled',
    setBusy(value) { submitting = value === true; update(); },
    setTarget(value) { value = String(value); if (!validScope(board, value, purpose)) return false; if (value !== target) { target = value; root.dataset.target = value; invalidate(); } return true; },
    dispose() {
      if (disposed) return; stop(); clearProof(); csrf.value = ''; disposed = true;
      load.removeEventListener('click', fetchChallenge); help.removeEventListener('click', showingHelp); slider.removeEventListener('input', sliding);
      if (interceptSubmit) form.removeEventListener('submit', submittingForm, true); form.removeEventListener('captcha:submitted', consumed); win.removeEventListener('pagehide', hiding); win.removeEventListener('pageshow', showing); mounted.delete(root);
    },
  };
  mounted.set(root, api);
  const initialToken = token.value, initialImage = fg.getAttribute('src'), initialNonce = csrf.value, initialAnswer = answer.value, ttl = Number(root.dataset.ttl);
  if (root.dataset.exempt === 'true' && NONCE.test(initialNonce)) { state = 'exempt'; root.hidden = true; }
  else if (root.dataset.classic === 'true' && validReply({ csrf: initialNonce, report_exempt: false, ticket_challenge: false,
    challenge: { state: 'ready', token: initialToken, image: initialImage, background: null, slider: false, offset_max: 0, ttl_seconds: ttl, cooldown_seconds: 0 } })) {
    apply({ csrf: initialNonce, report_exempt: false, ticket_challenge: false,
      challenge: { state: 'ready', token: initialToken, image: initialImage, background: null, slider: false, offset_max: 0, ttl_seconds: ttl, cooldown_seconds: 0 } }, false);
    answer.value = initialAnswer;
  } else { clearProof(); }
  update(); memberAdvice(); return api;
}
