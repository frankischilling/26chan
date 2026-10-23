// Generated from the pinned source perk corpus; ranges contain no extra glyphs.
const glyphs = new Set(["✔️","❤️","♨️"]);
for (const [start, count] of [[-117227,0],[-117160,0],[-117154,0],[-116916,0],[776,0],[813,2],[822,0],[836,59],[898,0],[928,0],[967,0],[1024,26],[1055,32],[1100,0],[1123,0],[1172,0],[1177,3],[1186,0],[1199,0],[1354,0],[1444,0],[1538,1],[1592,9],[1608,2],[2370,2],[2384,27],[2432,1],[2435,7],[2444,10],[2496,0]]) {
  for (let offset = 0; offset <= count; ++offset) glyphs.add(String.fromCodePoint(0x1f000 + start + offset));
}
export function eventPerkRecipe(tag, attrs, children) {
  return tag === 'div' && attrs && Object.keys(attrs).length === 1 && attrs.class === 'like-perk-cnt'
    && Array.isArray(children) && children.length === 1 && glyphs.has(children[0]);
}
