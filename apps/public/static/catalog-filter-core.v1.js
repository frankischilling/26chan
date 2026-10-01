export const CATALOG_FILTER_LIMITS = Object.freeze({
  rules: 64, pattern: 1024, boards: 1024, storage: 131072,
  cards: 512, field: 65536, request: 2097152, response: 65536, deadline: 1000,
});

const record = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const flag = value => value === true || value === 1;
const validFlag = value => value === undefined || [true, false, 0, 1].includes(value);
const id = value => typeof value === 'string' && /^[1-9][0-9]{0,18}$/.test(value)
  && (value.length < 19 || value <= '9223372036854775807');

export function readCatalogFilters(raw) {
  if (raw === null) return { status: 'ok', rules: [] };
  if (typeof raw !== 'string' || raw.length > CATALOG_FILTER_LIMITS.storage) return { status: 'invalid-settings' };
  try {
    const value = JSON.parse(raw);
    if (!record(value) || Object.keys(value).length > CATALOG_FILTER_LIMITS.rules) throw new Error('invalid-settings');
    const rules = Object.entries(value).map(([key, row]) => {
      if (!/^(?:0|[1-9][0-9]{0,4})$/.test(key) || !record(row)
        || typeof row.pattern !== 'string' || row.pattern.length > CATALOG_FILTER_LIMITS.pattern
        || typeof row.boards !== 'string' || row.boards.length > CATALOG_FILTER_LIMITS.boards
        || ['active', 'hidden', 'top'].some(name => !validFlag(row[name]))
        || row.color !== undefined && (typeof row.color !== 'string' || row.color.length > 128)) throw new Error('invalid-settings');
      return { active: Number(flag(row.active)), pattern: row.pattern, boards: row.boards,
        hidden: Number(flag(row.hidden)), top: Number(flag(row.top)), ...(row.color ? { color: row.color } : {}) };
    });
    return { status: 'ok', rules };
  } catch { return { status: 'invalid-settings' }; }
}

export function writeCatalogFilters(rules) {
  if (!Array.isArray(rules) || rules.length > CATALOG_FILTER_LIMITS.rules) return { status: 'invalid-settings' };
  const raw = rules.length ? JSON.stringify(Object.fromEntries(rules.map((rule, index) => [index, rule]))) : null;
  const checked = readCatalogFilters(raw);
  return checked.status === 'ok' ? { ...checked, raw } : checked;
}

function checkedJob(value) {
  if (!record(value) || value.version !== 1 || typeof value.board !== 'string' || !/^[a-z0-9]{1,32}$/.test(value.board)
    || !Array.isArray(value.cards) || value.cards.length > CATALOG_FILTER_LIMITS.cards) throw new Error('invalid-request');
  const parsed = writeCatalogFilters(value.rules);
  if (parsed.status !== 'ok') throw new Error('invalid-request');
  const seen = new Set();
  const cards = value.cards.map(card => {
    if (!record(card) || !id(card.id) || seen.has(card.id) || typeof card.text !== 'string') throw new Error('invalid-card');
    seen.add(card.id);
    const output = { id: card.id };
    for (const name of ['text', 'file', 'author', 'trip', 'capcode']) {
      if (card[name] === undefined) continue;
      if (typeof card[name] !== 'string' || card[name].length > CATALOG_FILTER_LIMITS.field) throw new Error('invalid-card');
      output[name] = card[name];
    }
    return output;
  });
  return { version: 1, board: value.board, rules: parsed.rules, cards };
}

// This escaping list and matching grammar follow public catalog v1025. In
// particular, its literal escaping leaves ^, $ and | untouched.
const escaped = new Set('/.*+?()[]{}\\');
function escape(text) { return [...text].map(char => (escaped.has(char) ? '\\' : '') + char).join(''); }
function compile(raw) {
  if (raw.startsWith('#')) {
    const type = raw.startsWith('##') ? 2 : 1;
    return { type, pattern: new RegExp(escape(raw.slice(type))) };
  }
  const regular = /^\/(.*)\/(i?)$/.exec(raw);
  if (regular) return { type: 0, pattern: new RegExp(regular[1], regular[2]) };
  if (raw.startsWith('"') && raw.endsWith('"')) return { type: 0, pattern: new RegExp(escape(raw.slice(1, -1))) };
  const terms = raw.replace(/\s*\|+\s*/g, '|').split(' ').map(term => {
    const parts = term.includes('|') ? '(' + term.split('|').filter(Boolean).reverse().map(escape).join('|') + ')' : escape(term);
    return '(?=.*\\b' + parts.replace(/\\\*/g, '[^\\s]*') + '\\b)';
  });
  return { type: 0, pattern: new RegExp('^' + terms.join(''), 'i') };
}

// Only a fresh, externally timed worker calls this evaluator in the browser.
export function runCatalogFilterJob(raw) {
  if (typeof raw !== 'string' || raw.length > CATALOG_FILTER_LIMITS.request) return JSON.stringify({ status: 'invalid-request' });
  let job;
  try { job = checkedJob(JSON.parse(raw)); }
  catch { return JSON.stringify({ status: 'invalid-request' }); }
  const compiled = [];
  for (let index = 0; index < job.rules.length; index++) {
    const rule = job.rules[index];
    if (!rule.active || rule.pattern === '' || rule.boards && !rule.boards.split(' ').includes(job.board)) continue;
    try { compiled.push({ index, ...compile(rule.pattern) }); }
    catch { return JSON.stringify({ status: 'invalid-filter', index }); }
  }
  const matches = [];
  for (const card of job.cards) {
    for (const { index, type, pattern } of compiled) {
      const trip = card.capcode ? (card.trip ?? '') + '!#' + card.capcode : card.trip;
      if (type === 0 ? pattern.test(card.text) || pattern.test(card.file)
        : pattern.test(type === 1 ? trip : card.author)) {
        matches.push({ id: card.id, filter: index });
        break;
      }
    }
  }
  return JSON.stringify({ status: 'ok', matches });
}

function checkedResult(raw, job) {
  if (typeof raw !== 'string' || raw.length > CATALOG_FILTER_LIMITS.response) return { status: 'invalid-result' };
  try {
    const value = JSON.parse(raw);
    if (!record(value)) throw new Error('invalid-result');
    if (value.status === 'invalid-request') return { status: 'invalid-request' };
    if (value.status === 'invalid-filter' && Number.isInteger(value.index) && value.index >= 0 && value.index < job.rules.length) return value;
    if (value.status !== 'ok' || !Array.isArray(value.matches) || value.matches.length > job.cards.length) throw new Error('invalid-result');
    const positions = new Map(job.cards.map((card, index) => [card.id, index]));
    let previous = -1;
    const matches = value.matches.map(match => {
      const position = positions.get(match?.id), rule = job.rules[match?.filter];
      if (!record(match) || position === undefined || position <= previous || !Number.isInteger(match.filter)
        || !rule?.active || !rule.pattern || rule.boards && !rule.boards.split(' ').includes(job.board)) throw new Error('invalid-result');
      previous = position;
      return { id: match.id, filter: match.filter };
    });
    return { status: 'ok', matches };
  } catch { return { status: 'invalid-result' }; }
}

export class CatalogFilterMatcher {
  constructor({ createWorker = () => new Worker(new URL(import.meta.url), { type: 'module' }), deadline = CATALOG_FILTER_LIMITS.deadline } = {}) {
    if (!Number.isInteger(deadline) || deadline < 25 || deadline > 2000) throw new RangeError('Invalid filter deadline');
    this.createWorker = createWorker;
    this.deadline = deadline;
  }
  match(rules, board, cards, { signal } = {}) {
    if (signal?.aborted) return Promise.resolve({ status: 'cancelled' });
    let job, raw;
    try { job = checkedJob({ version: 1, rules, board, cards }); raw = JSON.stringify(job); }
    catch { return Promise.resolve({ status: 'invalid-request' }); }
    if (raw.length > CATALOG_FILTER_LIMITS.request) return Promise.resolve({ status: 'invalid-request' });
    return new Promise(resolve => {
      let worker, timer, settled = false;
      const finish = result => {
        if (settled) return;
        settled = true; clearTimeout(timer); signal?.removeEventListener('abort', cancel);
        if (worker) { worker.onmessage = worker.onerror = null; try { worker.terminate(); } catch { /* No caller-thread fallback. */ } }
        resolve(result);
      };
      const cancel = () => finish({ status: 'cancelled' });
      try {
        worker = this.createWorker();
        worker.onmessage = event => finish(checkedResult(event.data, job));
        worker.onerror = event => { event.preventDefault?.(); finish({ status: 'worker-error' }); };
        timer = setTimeout(() => finish({ status: 'timeout' }), this.deadline);
        signal?.addEventListener('abort', cancel, { once: true });
        if (signal?.aborted) { cancel(); return; }
        worker.postMessage(raw);
      } catch { finish({ status: 'unavailable' }); }
    });
  }
}

if (typeof WorkerGlobalScope !== 'undefined' && globalThis instanceof WorkerGlobalScope) {
  globalThis.addEventListener('message', event => globalThis.postMessage(runCatalogFilterJob(event.data)));
}
