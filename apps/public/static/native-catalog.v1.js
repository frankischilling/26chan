import { FILTER_LIMITS } from './native-filter.v1.js';

// String limits count UTF-16 code units. Transport byte limits are separate.
export const CATALOG_LIMITS = Object.freeze({
  source: 4194304, depth: 24, values: 131072, array: 4096,
  keys: 128, key: 128, pages: 128, comment: FILTER_LIMITS.html,
  posts: FILTER_LIMITS.posts, field: FILTER_LIMITS.field,
});

const maximumId = '9223372036854775807';
const textFields = ['trip', 'name', 'id', 'sub', 'filename', 'com'];
const record = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const canonicalId = value => typeof value === 'string' && /^[1-9][0-9]*$/.test(value)
  && value.length <= 19 && (value.length < 19 || value <= maximumId);

// Numeric `no` members keep their source lexeme instead of passing through Number.
// The parser also rejects duplicate keys and excessive nesting before projection.
function catalogJson(raw) {
  let offset = 0, values = 0;
  const number = /-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/y;
  const fail = () => { throw new Error('invalid-catalog'); };
  const whitespace = () => {
    while (raw[offset] === ' ' || raw[offset] === '\t' || raw[offset] === '\r' || raw[offset] === '\n') offset++;
  };
  const string = () => {
    if (raw[offset] !== '"') fail();
    const start = offset++;
    while (offset < raw.length) {
      const character = raw[offset++];
      if (character === '\\') { offset++; continue; }
      if (character === '"') {
        const result = JSON.parse(raw.slice(start, offset));
        if (result.length > CATALOG_LIMITS.comment) fail();
        return result;
      }
    }
    return fail();
  };
  const value = (depth, key = '') => {
    if (depth > CATALOG_LIMITS.depth || ++values > CATALOG_LIMITS.values) fail();
    whitespace();
    const character = raw[offset];
    if (character === '"') return string();
    if (character === '{') {
      offset++;
      whitespace();
      const result = Object.create(null);
      let keys = 0;
      if (raw[offset] === '}') { offset++; return result; }
      while (true) {
        whitespace();
        const member = string();
        if (member.length > CATALOG_LIMITS.key || ++keys > CATALOG_LIMITS.keys || Object.hasOwn(result, member)) fail();
        whitespace();
        if (raw[offset++] !== ':') fail();
        result[member] = value(depth + 1, member);
        whitespace();
        const separator = raw[offset++];
        if (separator === '}') return result;
        if (separator !== ',') fail();
      }
    }
    if (character === '[') {
      offset++;
      whitespace();
      const result = [];
      if (raw[offset] === ']') { offset++; return result; }
      while (true) {
        if (result.length >= CATALOG_LIMITS.array) fail();
        result.push(value(depth + 1));
        whitespace();
        const separator = raw[offset++];
        if (separator === ']') return result;
        if (separator !== ',') fail();
      }
    }
    for (const [literal, result] of [['true', true], ['false', false], ['null', null]]) {
      if (raw.startsWith(literal, offset)) { offset += literal.length; return result; }
    }
    number.lastIndex = offset;
    const found = number.exec(raw);
    if (!found) return fail();
    offset = number.lastIndex;
    if (key === 'no') return found[0];
    const result = Number(found[0]);
    if (!Number.isFinite(result)) fail();
    return result;
  };
  const result = value(0);
  whitespace();
  if (offset !== raw.length) fail();
  return result;
}

// This returns raw filter fields, not worker-ready comments. No HTML is parsed here.
export function parseNativeCatalog(raw) {
  if (typeof raw !== 'string' || raw.length > CATALOG_LIMITS.source) return { status: 'invalid-catalog' };
  try {
    const pages = catalogJson(raw);
    if (!Array.isArray(pages) || pages.length > CATALOG_LIMITS.pages) throw new Error('invalid-pages');
    const seenPages = new Set(), seenPosts = new Set(), posts = [];
    for (const page of pages) {
      if (!record(page) || !Number.isInteger(page.page) || page.page < 0 || page.page > 2147483647
        || seenPages.has(page.page) || !Array.isArray(page.threads)) throw new Error('invalid-page');
      seenPages.add(page.page);
      for (const thread of page.threads) {
        if (posts.length >= CATALOG_LIMITS.posts || !record(thread) || !canonicalId(thread.no)
          || seenPosts.has(thread.no)) throw new Error('invalid-thread');
        seenPosts.add(thread.no);
        const post = { no: thread.no };
        for (const field of textFields) {
          if (!Object.hasOwn(thread, field)) continue;
          const limit = field === 'com' ? CATALOG_LIMITS.comment : CATALOG_LIMITS.field;
          if (typeof thread[field] !== 'string' || thread[field].length > limit) throw new Error('invalid-field');
          post[field] = thread[field];
        }
        posts.push(post);
      }
    }
    return { status: 'ok', posts };
  } catch { return { status: 'invalid-catalog' }; }
}
