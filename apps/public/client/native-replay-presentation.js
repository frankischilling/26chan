// The worker exposes a small display receipt, never source events, URLs or HTML.
const integer = (n, min, max) => Number.isInteger(n) && n >= min && n <= max;
const unit = n => Number.isFinite(n) && n >= 0 && n <= 1;
const keys = (value, expected) => value && typeof value === 'object' && !Array.isArray(value)
  && Object.keys(value).sort().join(',') === expected;
export function validReplayPresentation(value) {
  if (!keys(value, 'color,layers,tool') || (value.color !== null && (typeof value.color !== 'string' || !/^#[0-9a-f]{6}$/i.test(value.color)))
    || !keys(value.tool, 'alpha,alphaDynamicsEnabled,flow,flowDynamicsEnabled,id,preserveAlphaEnabled,size,sizeDynamicsEnabled,tipId')
    || !integer(value.tool.id, 1, 8) || !integer(value.tool.size, 1, 64)
    || !unit(value.tool.alpha) || !unit(value.tool.flow) || !integer(value.tool.tipId, 0, 2)
    || !['sizeDynamicsEnabled','alphaDynamicsEnabled','flowDynamicsEnabled','preserveAlphaEnabled'].every(k => typeof value.tool[k] === 'boolean')
    || !Array.isArray(value.layers) || value.layers.length < 1 || value.layers.length > 25) return false;
  const ids = new Set(); let active = 0;
  for (const layer of value.layers) {
    if (!keys(layer, 'active,alpha,height,id,pixels,selected,visible,width')
      || !integer(layer.id, 1, 16385) || ids.has(layer.id) || !unit(layer.alpha)
      || !['active','selected','visible'].every(k => typeof layer[k] === 'boolean')
      || !integer(layer.width, 1, 24) || !integer(layer.height, 1, 24)
      || !(layer.pixels instanceof Uint8ClampedArray) || layer.pixels.length !== layer.width * layer.height * 4) return false;
    ids.add(layer.id); active += Number(layer.active);
  }
  return active === 1;
}
