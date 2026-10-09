export const BLOTTER_STORAGE_KEY = '4chan-blotter';
// Decimal seconds only. Bound both parsing work and numeric precision.
export function blotterTimestamp(value) {
  if (typeof value !== 'string' || !/^(0|[1-9][0-9]{0,12})$/.test(value)) return null;
  const timestamp = Number(value);
  return timestamp <= 8_640_000_000_000 ? timestamp : null;
}

const mounted = new WeakMap();
export function mountNativeBlotter(document) {
  const button = document?.getElementById('toggleBlotter');
  const messages = document?.getElementById('blotter-msgs');
  const all = document?.getElementById('blotter-all');
  if (!button || !messages || !all) return null;
  if (mounted.has(button)) return mounted.get(button);
  const timestamp = blotterTimestamp(button.getAttribute('data-utc'));
  if (timestamp === null) return null;
  const storage = action => {
    try { return action(document.defaultView.localStorage); } catch { return null; }
  };
  const render = hidden => {
    messages.hidden = hidden;
    all.hidden = hidden;
    button.textContent = hidden ? 'Show Blotter' : 'Hide';
    button.setAttribute('aria-expanded', String(!hidden));
  };
  const seen = blotterTimestamp(storage(store => store.getItem(BLOTTER_STORAGE_KEY)));
  const initiallyHidden = seen !== null && timestamp <= seen;
  render(initiallyHidden);
  if (initiallyHidden) storage(store => store.setItem(BLOTTER_STORAGE_KEY, String(timestamp)));
  const toggle = event => {
    event.preventDefault();
    const hidden = !messages.hidden;
    render(hidden);
    storage(store => hidden
      ? store.setItem(BLOTTER_STORAGE_KEY, String(timestamp))
      : store.removeItem(BLOTTER_STORAGE_KEY));
  };
  button.addEventListener('click', toggle);
  const controller = { destroy() {
    button.removeEventListener('click', toggle);
    mounted.delete(button);
    render(false);
  } };
  mounted.set(button, controller);
  return controller;
}
