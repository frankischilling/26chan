// Fixed source names; a missing recorded image still uses its ordinary 404 path.
const counts = Object.freeze({"a":1,"m":4,"v":1,"co":5,"jp":1,"mlp":1,"tg":2,"tv":5,"lit":1,"vp":1,"vg":1,"vr":2,"s4s":6,"news":1,"vrpg":3,"vmg":3,"vst":1,"vt":3,"vm":1});
export function isSpoilerAssetPath(value) {
  if (value === '/static/catalog/spoiler.png' || value === '/static/catalog/spoiler-vst.png') return true;
  if (typeof value !== 'string') return false;
  const match = /^\/static\/catalog\/spoiler-([a-z0-9]{1,10}?)([1-9][0-9]?)\.png$/.exec(value);
  return !!match && Object.hasOwn(counts, match[1]) && Number(match[2]) <= counts[match[1]];
}
