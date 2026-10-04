const encoder = new TextEncoder();
const flagPreferences = new WeakMap();

export function mountBoardFlagPreference(field, board) {
  const window = field?.ownerDocument?.defaultView;
  if (!window || !(field instanceof window.HTMLSelectElement) || field.name !== 'flag'
    || typeof board !== 'string' || !/^[a-z0-9]{1,10}$/.test(board)
    || field.options.length < 1 || field.options.length > 84) return;
  const values = [...field.options].map(option => option.value);
  const allowed = new Set(values);
  if (!allowed.has('0') || allowed.size !== values.length
    || values.some(value => value !== '0' && !/^[A-Z0-9]{2,3}$/.test(value))) return;
  flagPreferences.get(field)?.();
  const key = `4chan_flag_${board}`;
  try {
    const value = window.localStorage.getItem(key);
    if (typeof value === 'string' && value.length <= 3 && allowed.has(value)) field.value = value;
  } catch { /* A denied preference store leaves the current form usable. */ }
  const change = () => {
    if (!field.isConnected || field.options.length > 84 || !allowed.has(field.value)
      || ![...field.options].some(option => option.value === field.value)) return;
    try {
      if (field.value === '0') window.localStorage.removeItem(key);
      else window.localStorage.setItem(key, field.value);
    } catch { /* Keep the choice in this form when storage is unavailable. */ }
  };
  const cleanup = () => {
    field.removeEventListener('change', change);
    if (flagPreferences.get(field) === cleanup) flagPreferences.delete(field);
  };
  flagPreferences.set(field, cleanup);
  field.addEventListener('change', change);
  return cleanup;
}

export function readPostPreferences(raw) {
  const result = { name: '', options: '' };
  if (typeof raw !== 'string' || raw.length > 4096) return result;
  for (const [key, field] of [['4chan_name', 'name'], ['options', 'options']]) {
    const values = [];
    for (const pair of raw.split(';')) {
      const index = pair.indexOf('=');
      if (index >= 0 && pair.slice(0, index).trim() === key) values.push(pair.slice(index + 1));
    }
    if (values.length !== 1 || values[0].length > 300) continue;
    try {
      const value = decodeURIComponent(values[0]);
      if (encoder.encode(value).length > 100 || /\p{Cc}/u.test(value)) continue;
      // Old or manually supplied cookies must not restore a private trip suffix.
      result[field] = field === 'name' ? value.split('#', 1)[0].trim() : value;
    } catch { /* Malformed preferences leave the ordinary empty default. */ }
  }
  return result;
}

export function restorePostPreferences(source) {
  if (!source) return;
  let preferences;
  try { preferences = readPostPreferences(document.cookie); }
  catch { return; }
  for (const [key, field] of [['name', 'name'], ['email', 'options']]) {
    const input = source.elements.namedItem(key);
    if (input?.type === 'text' && !input.value) input.value = preferences[field];
  }
}
