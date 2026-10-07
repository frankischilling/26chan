// Independent worker-result boundary. No browser or MathJax dependencies.
export const GEOMETRY_LIMITS = Object.freeze({
  nodes: 4096, bytes: 1048576, depth: 64, coordinate: 100000, width: 256, height: 128,
});
const NUMBER = '[+-]?(?:\\d+(?:\\.\\d*)?|\\.\\d+)(?:[eE][+-]?\\d+)?';
const scalar = new RegExp(`^${NUMBER}$`);
const token = new RegExp(NUMBER, 'g');
const boxSyntax = new RegExp(`^\\s*${NUMBER}(?:(?:\\s*,\\s*|\\s+)${NUMBER}){3}\\s*$`);
const paint = ['transform', 'fill', 'stroke', 'stroke-width'];
const allowed = {
  svg: new Set(['x', 'y', 'width', 'height', 'viewBox', 'preserveAspectRatio', 'overflow']),
  g: new Set(paint),
  path: new Set(['d', ...paint]),
  rect: new Set(['x', 'y', 'width', 'height', ...paint]),
  line: new Set(['x1', 'y1', 'x2', 'y2', ...paint]),
};
const plain = value => value !== null && typeof value === 'object' && !Array.isArray(value)
  && (Object.getPrototypeOf(value) === Object.prototype || Object.getPrototypeOf(value) === null);
const keys = (value, names) => Object.keys(value).every(key => names.includes(key));
const finite = (value, bound = GEOMETRY_LIMITS.coordinate) =>
  typeof value === 'number' && Number.isFinite(value) && Math.abs(value) <= bound;

export function numericList(value) {
  if (typeof value !== 'string' || value.length > 65536
      || value.replace(token, '').replace(/[\s,]/g, '')) return null;
  const values = (value.match(token) || []).map(Number);
  return values.every(number => finite(number)) ? values : null;
}

export function viewBoxNumbers(value) {
  if (typeof value !== 'string' || value.length > 1024 || !boxSyntax.test(value)) return null;
  const box = numericList(value);
  return box && box[2] >= 1 && box[3] >= 1 ? box : null;
}

const identity = [1, 0, 0, 1, 0, 0];
function multiply(a, b) {
  const result = [
    a[0] * b[0] + a[2] * b[1], a[1] * b[0] + a[3] * b[1],
    a[0] * b[2] + a[2] * b[3], a[1] * b[2] + a[3] * b[3],
    a[0] * b[4] + a[2] * b[5] + a[4], a[1] * b[4] + a[3] * b[5] + a[5],
  ];
  if (!result.every(value => finite(value, 1000000))) throw Error('Transform budget');
  return result;
}

function transform(value) {
  if (typeof value !== 'string' || value.length > 1024) throw Error('Invalid transform');
  const matches = [...value.matchAll(/(translate|scale|matrix)\(([^()]*)\)/g)];
  if (!matches.length || value.replace(/(translate|scale|matrix)\([^()]*\)/g, '').trim()) {
    throw Error('Invalid transform');
  }
  let result = identity;
  for (const match of matches) {
    const numbers = numericList(match[2]);
    if (!numbers || !(match[1] === 'matrix'
      ? numbers.length === 6 : numbers.length === 1 || numbers.length === 2)) {
      throw Error('Invalid transform arguments');
    }
    let next = numbers;
    if (match[1] === 'translate') next = [1, 0, 0, 1, numbers[0], numbers[1] ?? 0];
    if (match[1] === 'scale') next = [numbers[0], 0, 0, numbers[1] ?? numbers[0], 0, 0];
    result = multiply(result, next);
  }
  return result;
}

function viewportMatrix(attrs, parent) {
  const box = viewBoxNumbers(attrs.viewBox);
  const width = Number(attrs.width), height = Number(attrs.height);
  const x = Number(attrs.x ?? 0), y = Number(attrs.y ?? 0);
  if (!box || box.length !== 4 || box[2] < 1 || box[3] < 1
      || attrs.width === undefined || attrs.height === undefined
      || width < 0.01 || height < 0.01) throw Error('Invalid viewport');
  const scale = Math.min(width / box[2], height / box[3]);
  if (!finite(scale, 1000) || scale < 0.000001) throw Error('Viewport scale budget');
  // Bound the clip rectangle in outer SVG coordinates as well as the effective
  // content matrix. Small viewBoxes must not hide enormous ancestor scales.
  for (const px of [x, x + width]) for (const py of [y, y + height]) {
    if (!finite(parent[0] * px + parent[2] * py + parent[4], 1000000)
        || !finite(parent[1] * px + parent[3] * py + parent[5], 1000000)) {
      throw Error('Viewport bounds budget');
    }
  }
  return multiply(parent, [scale, 0, 0, scale,
    x + (width - box[2] * scale) / 2 - box[0] * scale,
    y + (height - box[3] * scale) / 2 - box[1] * scale]);
}

export function geometrySize(geometry) {
  try {
    if (!plain(geometry) || !keys(geometry, ['viewBox', 'width', 'height', 'children'])) return null;
    const {viewBox, width, height, children} = geometry;
    if (!Array.isArray(viewBox) || viewBox.length !== 4 || !viewBox.every(value => finite(value))
        || viewBox[2] < 1 || viewBox[3] < 1 || !finite(width, 256) || !finite(height, 128)
        || width < 0.001 || height < 0.001 || !Array.isArray(children)) return null;
    let nodes = 1;
    let bytes = 128;
    const seen = new Set();
    function visit(node, depth, parent = identity) {
      if (++nodes > GEOMETRY_LIMITS.nodes || depth > GEOMETRY_LIMITS.depth
          || !plain(node) || seen.has(node) || !keys(node, ['tag', 'attrs', 'children'])
          || !Object.hasOwn(allowed, node.tag) || !plain(node.attrs)) throw Error('Invalid node');
      seen.add(node);
      bytes += 32;
      let matrix = multiply(parent,
        node.attrs.transform === undefined ? identity : transform(node.attrs.transform));
      for (const [key, value] of Object.entries(node.attrs)) {
        if (!allowed[node.tag].has(key) || typeof value !== 'string' || value.length > 65536) {
          throw Error('Invalid attribute');
        }
        bytes += 2 * (key.length + value.length);
        if (bytes > GEOMETRY_LIMITS.bytes) throw Error('Geometry byte budget');
        if (key === 'd') {
          if (!value || /[^MmLlHhVvCcSsQqTtAaZz\d\s.,eE+\-]/.test(value)
              || !numericList(value.replace(/[MmLlHhVvCcSsQqTtAaZz]/g, ' '))) {
            throw Error('Invalid path');
          }
        } else if (key === 'viewBox') {
          const box = viewBoxNumbers(value);
          if (!box || box.length !== 4 || box[2] < 1 || box[3] < 1) throw Error('Invalid viewBox');
        } else if (key === 'preserveAspectRatio') {
          if (value !== 'xMidYMid meet') throw Error('Invalid viewport alignment');
        } else if (key === 'overflow') {
          if (!['hidden', 'visible'].includes(value)) throw Error('Invalid viewport overflow');
        } else if (key === 'transform') {
          transform(value);
        } else if (key === 'fill' || key === 'stroke') {
          if (!['none', 'currentColor'].includes(value)) throw Error('Invalid paint');
        } else if (!scalar.test(value) || !finite(Number(value))
            || (['width', 'height', 'stroke-width'].includes(key) && Number(value) < 0)) {
          throw Error('Invalid scalar');
        }
      }
      if (node.tag === 'svg') matrix = viewportMatrix(node.attrs, matrix);
      if (node.tag === 'path' && !node.attrs.d) throw Error('Missing path');
      if (node.children !== undefined) {
        if (!['g', 'svg'].includes(node.tag) || !Array.isArray(node.children)) throw Error('Invalid children');
        for (const child of node.children) visit(child, depth + 1, matrix);
      }
    }
    for (const child of children) visit(child, 1);
    return {nodes, bytes};
  } catch {
    return null;
  }
}
export const validateGeometry = geometry => geometrySize(geometry) !== null;
