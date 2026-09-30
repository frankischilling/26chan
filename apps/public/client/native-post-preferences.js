const encoder = new TextEncoder();

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
