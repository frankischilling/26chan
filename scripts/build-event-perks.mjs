import fs from 'node:fs';
const fixture = JSON.parse(fs.readFileSync('fixtures/event-perk-reference.json', 'utf8'));
const glyphs = [...new Set(fixture.cases.map(row => row.glyph))];
if (fixture.cases.length !== 231 || glyphs.some(value => typeof value !== 'string' || value.includes(' '))) throw new Error('Invalid pinned perk corpus');
const plain = glyphs.filter(value => Array.from(value).length === 1).map(value => value.codePointAt(0)).sort((a, b) => a - b);
const ranges = [];
for (const code of plain) {
  const last = ranges.at(-1);
  if (last && last[1] + 1 === code) last[1] = code;
  else ranges.push([code, code]);
}
const compound = glyphs.filter(value => Array.from(value).length !== 1);
const source = `// Generated from the pinned source perk corpus; ranges contain no extra glyphs.
const glyphs = new Set(${JSON.stringify(compound)});
for (const [start, count] of ${JSON.stringify(ranges.map(([start, end]) => [start - 0x1f000, end - start]))}) {
  for (let offset = 0; offset <= count; ++offset) glyphs.add(String.fromCodePoint(0x1f000 + start + offset));
}
export function eventPerkRecipe(tag, attrs, children) {
  return tag === 'div' && attrs && Object.keys(attrs).length === 1 && attrs.class === 'like-perk-cnt'
    && Array.isArray(children) && children.length === 1 && glyphs.has(children[0]);
}
`;
const path = 'apps/public/client/native-event-perks.js';
if (process.argv.includes('--check')) {
  if (fs.readFileSync(path, 'utf8') !== source) throw new Error('Event perk recipes are stale');
} else fs.writeFileSync(path, source);
console.log(`Event perk recipes: ${glyphs.length} distinct source glyphs.`);
