import { FILTER_LIMITS } from './native-filter-limits.js';

export function readFilterRules(raw, { migrateLegacy = false } = {}) {
  if (raw === null) return { status: 'ok', rules: [] };
  if (typeof raw !== 'string' || raw.length > FILTER_LIMITS.settings) return { status: 'invalid-settings' };
  try {
    const value = JSON.parse(raw);
    if (!Array.isArray(value) || value.length > FILTER_LIMITS.filters) throw new Error('filter-limit');
    const rules = value.map(row => {
      if (!row || typeof row !== 'object' || Array.isArray(row)) throw new Error('invalid-filter');
      const type = migrateLegacy && row.type === 3 ? 4 : row.type;
      const boards = row.boards === undefined || row.boards === null || row.boards === false ? '' : row.boards;
      if (![0, 1, 2, 4, 5, 6].includes(type) || typeof row.pattern !== 'string'
        || row.pattern.length > FILTER_LIMITS.patterns || typeof boards !== 'string' || boards.length > FILTER_LIMITS.boardText
        || ['active', 'auto', 'hide'].some(key => row[key] !== undefined && typeof row[key] !== 'boolean')
        || (row.color !== undefined && (typeof row.color !== 'string' || row.color.length > 128))) throw new Error('invalid-filter');
      return { type, pattern: row.pattern, boards, active: row.active === true, auto: row.auto === true,
        hide: row.hide === true, ...(row.color ? { color: row.color } : {}) };
    });
    return { status: 'ok', rules };
  } catch { return { status: 'invalid-settings' }; }
}

// Parse only a color property on a detached element. Never accept declarations,
// URLs, custom-property substitutions or inherited values as a shadow fragment.
export function filterColor(raw) {
  if (!raw) return '';
  if (typeof raw !== 'string' || raw.length > 128 || /[;{}]|\b(?:var|env|attr)\s*\(/i.test(raw)) return null;
  const probe = document.createElement('span');
  probe.style.color = raw;
  const color = probe.style.color;
  return color && !/^(?:inherit|initial|unset|revert|revert-layer|currentcolor)$/i.test(color) ? color : null;
}
