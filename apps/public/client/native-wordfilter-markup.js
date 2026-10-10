// Test's two saved choices alter only these generated markup literals. Keep
// their finite inert recipes separate from the surrounding post grammar.
const recipes = new Map();
const tags = new Set(['span', 'sp4n', 'pre', 'pr3', 's']);
const ordinaryTags = new Set(['SPAN', 'B', 'S', 'SMALL', 'PRE', 'BR', 'WBR', 'A']);

function transform(literal, first, second) {
  for (const choice of new Set([first, second])) {
    if (choice === 5) literal = literal.replace(/([^gl])[tT]/g, (_, before) => `${before}7`);
    else literal = literal.replace([/a/gi, /e/gi, /i/gi, /o/gi, /s/gi][choice], ['4', '3', '1', '0', '5'][choice]);
  }
  return literal;
}
for (let first = 0; first < 6; first++) {
  for (let second = 0; second < 6; second++) {
    for (const [tag, class_] of [['s', null], ['pre', 'prettyprint'], ['span', 'sjis'],
      ['span', 'mu-s'], ['span', 'mu-i'], ['span', 'mu-r'], ['span', 'mu-g'], ['span', 'mu-b']]) {
      const name = transform(tag, first, second);
      if (name.startsWith('5')) continue; // Invalid HTML openings are escaped text.
      const attrs = class_ === null ? {} : { [transform('class', first, second)]: transform(class_, first, second) };
      const keys = Object.keys(attrs);
      const key = JSON.stringify([name, keys[0] ?? '', keys.length ? attrs[keys[0]] : '']);
      recipes.set(key, true);
    }
  }
}

export function isWordfilterMarkupTag(tag) { return tags.has(tag); }
export function isWordfilterMarkup(tag, attrs) {
  if (!tags.has(tag) || !attrs || typeof attrs !== 'object' || Array.isArray(attrs)) return false;
  const keys = Object.keys(attrs);
  if (keys.length > 1 || (keys.length === 1 && typeof attrs[keys[0]] !== 'string')) return false;
  return recipes.has(JSON.stringify([tag, keys[0] ?? '', keys.length ? attrs[keys[0]] : '']));
}
export function isCommentElement(node, attributes = node?.attributes) {
  if (ordinaryTags.has(node?.tagName)) return true;
  if (!['PR3', 'SP4N'].includes(node?.tagName)) return false;
  return isWordfilterMarkup(node.localName, Object.fromEntries(Array.from(attributes, ({ name, value }) => [name, value])));
}
