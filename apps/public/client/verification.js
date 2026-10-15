const MAX_IMAGE = 65536;
export function validChallenge(value) {
  const png = (image) => typeof image === 'string' && image.length <= MAX_IMAGE && /^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/.test(image);
  return value && value.state === 'ready' && /^c1_[a-f0-9]{64}$/.test(value.token)
    && Number.isInteger(value.ttl_seconds) && value.ttl_seconds >= 4 && value.ttl_seconds <= 120
    && Number.isInteger(value.cooldown_seconds) && value.cooldown_seconds >= 0 && value.cooldown_seconds <= 300
    && typeof value.slider === 'boolean' && Number.isInteger(value.offset_max)
    && value.offset_max >= 0 && value.offset_max <= 80 && png(value.image)
    && (value.slider ? png(value.background) : value.background === null);
}
export function mountVerification(doc = document, win = window) {
  const form = doc.getElementById('auth-form');
  if (!form) return;
  const find = (id) => doc.getElementById(id);
  const frame = find('t-frame'), fg = find('t-fg'), bg = find('t-bg'), slider = find('t-slider');
  const load = find('t-load'), msg = find('t-msg'), answer = find('t-resp'), token = find('t-challenge'), submit = find('verification-submit');
  let epoch = 0, abort, expiry, cooldown, deadline = 0, offset = 0, busy = false;
  const clearProof = () => {
    win.clearTimeout(expiry); token.value = ''; answer.value = ''; frame.hidden = true;
    fg.removeAttribute('src'); bg.removeAttribute('src'); slider.value = '0'; bg.style.transform = ''; submit.disabled = true;
  };
  const reset = () => {
    epoch++; abort?.abort(); abort = undefined; win.clearInterval(cooldown); deadline = 0; busy = false;
    clearProof(); load.disabled = false; load.textContent = 'Get Captcha'; msg.textContent = '';
  };
  const wait = (seconds, visible) => {
    win.clearInterval(cooldown); deadline = Date.now() + seconds * 1000;
    const tick = () => {
      const left = Math.max(0, Math.ceil((deadline - Date.now()) / 1000));
      load.disabled = left > 0 || busy;
      load.textContent = visible && left ? `Wait ${left}s` : 'Get Captcha';
      if (!left) win.clearInterval(cooldown);
    };
    tick(); cooldown = win.setInterval(tick, 250);
  };
  const fetchChallenge = async () => {
    if (busy || Date.now() < deadline) return;
    clearProof(); busy = true; load.disabled = true; msg.textContent = 'Loading CAPTCHA…';
    const current = ++epoch; abort?.abort(); abort = new AbortController();
    const timer = win.setTimeout(() => abort?.abort(), 12000);
    try {
      const body = new URLSearchParams({ csrf: form.elements.namedItem('csrf').value });
      const emailToken = form.elements.namedItem('tkn');
      if (emailToken) body.set('tkn', emailToken.value);
      const response = await win.fetch('/signin/challenge', { method: 'POST', credentials: 'same-origin', mode: 'same-origin', redirect: 'error', cache: 'no-store', body, signal: abort.signal });
      if (!response.ok || !response.headers.get('content-type')?.startsWith('application/json')) throw new Error();
      const text = await response.text();
      if (text.length > 262144) throw new Error();
      const value = JSON.parse(text);
      if (current !== epoch) return;
      busy = false;
      if (value.state === 'wait' && Number.isInteger(value.seconds) && value.seconds >= 1 && value.seconds <= 300) {
        msg.textContent = 'Please wait before loading another CAPTCHA.'; wait(value.seconds, true); return;
      }
      if (!validChallenge(value)) throw new Error();
      fg.src = value.image; bg.hidden = !value.slider;
      if (value.slider) bg.src = value.background;
      slider.hidden = !value.slider; slider.previousElementSibling.hidden = !value.slider;
      offset = value.offset_max; frame.hidden = false; token.value = value.token; msg.textContent = '';
      submit.disabled = false; answer.focus(); wait(value.cooldown_seconds, false);
      expiry = win.setTimeout(() => { clearProof(); msg.textContent = 'CAPTCHA expired. Click Get Captcha to try again.'; }, value.ttl_seconds * 1000 - 3000);
    } catch {
      if (current !== epoch) return;
      busy = false; clearProof(); load.disabled = false; msg.textContent = 'Could not load the CAPTCHA. Click Get Captcha to try again.';
    } finally { win.clearTimeout(timer); }
  };
  slider.addEventListener('input', () => { bg.style.transform = `translateX(${-Number(slider.value) * offset / 100}px)`; });
  load.addEventListener('click', fetchChallenge);
  find('t-help').addEventListener('click', () => win.alert('Move the slider to align the image, then type the text you see. If it is unreadable, load another CAPTCHA.'));
  form.addEventListener('submit', (event) => {
    if (!token.value || !answer.value.trim()) { event.preventDefault(); msg.textContent = 'Complete the CAPTCHA first.'; return; }
    // Leave the successful controls enabled so native form submission includes
    // the proof. Never retry an email request or verification automatically.
    submit.disabled = true; load.disabled = true;
  });
  win.addEventListener('pagehide', reset);
  win.addEventListener('pageshow', (event) => { if (event.persisted) reset(); });
  reset();
  if (form.dataset.mode === 'verify') void fetchChallenge();
  return { reset };
}
if (typeof document !== 'undefined') mountVerification();
