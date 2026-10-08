// No raster emulation. Canvas/context/ImageData operations are always native.
// Sinks are closed to the exact DOM vocabulary reached by the pinned core.
const fail = name => { throw new Error(`Unreviewed worker boundary: ${String(name)}`); };
const allowedClasses = new Set(['tegaki-layer', 'tegaki-hidden', 'tegaki-tool-active']);
export function installWorkerBoundary(scope = globalThis) {
  if (scope.document !== undefined) throw new Error('worker document boundary already exists');
  if (typeof scope.OffscreenCanvas !== 'function' || typeof scope.ImageData !== 'function') {
    throw new Error('Native OffscreenCanvas and ImageData are required inside the worker');
  }
  const counts = Object.create(null), metadata = new WeakMap();
  const record = name => { counts[name] = (counts[name] || 0) + 1; };
  const checked = (name, values) => values.includes(name) || fail(name);
  const closed = (object, label) => new Proxy(Object.seal(object), {
    get(target, key, receiver) {
      if (!Object.hasOwn(target, key)) fail(`${label}.${String(key)}`);
      record(`${label}:get:${String(key)}`);
      return Reflect.get(target, key, receiver);
    },
    set(target, key, value, receiver) {
      if (!Object.hasOwn(target, key)) fail(`${label}.${String(key)}`);
      record(`${label}:set:${String(key)}`);
      return Reflect.set(target, key, value, receiver);
    },
  });
  const style = (keys, label) => closed(Object.fromEntries(keys.map(key => [key, ''])), label);
  function decorate(target, label, ids, styleKeys, classes = []) {
    const state = { id: '', className: classes.join(' '), parent: null, children: [], attributes: {} };
    const descriptors = {
      id: { get: () => state.id, set(value) { checked(value, ids); state.id = value; record('id'); } },
      className: { get: () => state.className, set(value) {
        const tokens = String(value).split(/\s+/).filter(Boolean);
        for (const token of tokens) if (!allowedClasses.has(token)) fail(`class ${token}`);
        state.className = tokens.join(' '); record('className');
      } },
      style: { value: style(styleKeys, `${label}.style`) },
      parentNode: { get: () => state.parent },
      children: { get: () => state.children.slice() },
      firstElementChild: { get: () => state.children[0] || null },
      nextElementSibling: { get: () => {
        if (!state.parent) return null;
        const siblings = metadata.get(state.parent).children;
        return siblings[siblings.indexOf(target) + 1] || null;
      } },
      setAttribute: { value(name, value) {
        if (name !== 'data-id' || !/^[1-8]$/.test(String(value))) fail(`attribute ${name}`);
        state.attributes[name] = String(value); record('setAttribute:data-id');
      } },
      getAttribute: { value(name) {
        if (name !== 'data-id') fail(`attribute ${name}`);
        return state.attributes[name] ?? null;
      } },
      getElementsByClassName: { value(name) {
        if (!allowedClasses.has(name)) fail(`class query ${name}`);
        record(`classQuery:${name}`);
        return state.children.flatMap(node => [
          ...(node.classList.contains(name) ? [node] : []), ...node.getElementsByClassName(name),
        ]);
      } },
      insertBefore: { value(node, reference) {
        const child = metadata.get(node);
        if (!child || (reference !== null && !state.children.includes(reference))) fail('insertBefore');
        if (node === reference) return node;
        if (child.parent) child.parent.removeChild(node);
        const index = reference === null ? state.children.length : state.children.indexOf(reference);
        state.children.splice(index, 0, node); child.parent = target; record('insertBefore'); return node;
      } },
      appendChild: { value(node) { return target.insertBefore(node, null); } },
      removeChild: { value(node) {
        const index = state.children.indexOf(node);
        if (index < 0) fail('removeChild');
        state.children.splice(index, 1); metadata.get(node).parent = null; record('removeChild'); return node;
      } },
      classList: { value: closed({
        contains(name) { if (!allowedClasses.has(name)) fail(`class ${name}`); return state.className.split(' ').includes(name); },
        add(name) {
          if (!allowedClasses.has(name)) fail(`class ${name}`);
          if (!target.classList.contains(name)) state.className = `${state.className} ${name}`.trim();
          record(`classAdd:${name}`);
        },
        remove(name) {
          if (!allowedClasses.has(name)) fail(`class ${name}`);
          state.className = state.className.split(' ').filter(token => token !== name).join(' ');
          record(`classRemove:${name}`);
        },
      }, `${label}.classList`) },
    };
    for (const [key, descriptor] of Object.entries(descriptors)) {
      if (descriptor.get) {
        const get = descriptor.get;
        descriptor.get = function () { record(`${label}:get:${key}`); return get.call(this); };
      }
      if (descriptor.set) {
        const set = descriptor.set;
        descriptor.set = function (value) { record(`${label}:set:${key}`); return set.call(this, value); };
      }
      if (typeof descriptor.value === 'function') {
        const call = descriptor.value;
        descriptor.value = function (...args) { record(`${label}:call:${key}`); return call.apply(this, args); };
      }
    }
    Object.defineProperties(target, descriptors); metadata.set(target, state); return target;
  }
  // Native branded objects remain the actual drawImage inputs and ctx.canvas.
  // A guarded intermediate prototype intercepts unknown property reads while
  // retaining OffscreenCanvas.prototype in the chain and the native receiver.
  const nativePrototype = scope.OffscreenCanvas.prototype;
  const guardedPrototype = new Proxy(Object.create(nativePrototype), {
    get(target, key, receiver) {
      if (!['width', 'height', 'getContext'].includes(key) && key !== Symbol.toStringTag) fail(`canvas.${String(key)}`);
      record(`canvas:${String(key)}`); return Reflect.get(target, key, receiver);
    },
    set(target, key, value, receiver) {
      if (key !== 'width' && key !== 'height') fail(`canvas.${String(key)}`);
      if (!Number.isInteger(value) || value < 1 || value > 24) fail(`canvas geometry ${value}`);
      record(`canvas:${key}=`); return Reflect.set(target, key, value, receiver);
    },
  });
  let surfaces = 0;
  function canvas() {
    if (++surfaces > 9) fail('canvas count');
    const result = new scope.OffscreenCanvas(1, 1);
    if (!Object.isExtensible(result)) throw new Error('Native OffscreenCanvas metadata attachment unsupported');
    decorate(result, 'canvas', ['tegaki-canvas', ...Array.from({ length: 8 }, (_, i) => `tegaki-canvas-${i + 1}`)], ['opacity']);
    Object.setPrototypeOf(result, guardedPrototype); Object.preventExtensions(result); return result;
  }
  const inertPrototype = new Proxy(Object.create(null), {
    get(_target, key) { return fail(`element.${String(key)}`); },
    set(_target, key) { return fail(`element.${String(key)}`); },
  });
  const nodes = new Map();
  function element(id, styles = []) {
    const result = decorate({}, id, [id], styles);
    result.id = id; Object.setPrototypeOf(result, inertPrototype); Object.preventExtensions(result); nodes.set(id, result); return result;
  }
  const layers = element('probe-layers', ['width', 'height']);
  const bg = element('probe-bg'), canvasContainer = element('probe-canvas-container');
  bg.appendChild(canvasContainer); canvasContainer.appendChild(layers);
  element('tegaki-color', ['backgroundColor']);
  const colorPicker = decorate({}, 'tegaki-colorpicker', ['tegaki-colorpicker'], []);
  Object.defineProperty(colorPicker, 'value', { writable: true, value: '' });
  colorPicker.id = 'tegaki-colorpicker'; Object.setPrototypeOf(colorPicker, inertPrototype); Object.preventExtensions(colorPicker); nodes.set(colorPicker.id, colorPicker);
  for (const name of ['pencil', 'pen', 'airbrush', 'bucket', 'tone', 'pipette', 'blur', 'eraser']) element(`tegaki-tool-btn-${name}`);
  const document = closed({
    documentElement: closed({}, 'documentElement'),
    createElement(tag) { if (tag !== 'canvas') fail(`createElement ${tag}`); record('createElement:canvas'); return canvas(); },
    getElementById(id) { if (!nodes.has(id)) fail(`id ${id}`); record(`getId:${id}`); return nodes.get(id); },
    getElementsByClassName(name) {
      if (name !== 'tegaki-tool-active') fail(`document class query ${name}`);
      record(`documentClassQuery:${name}`);
      return [...nodes.values()].filter(node => node.classList.contains(name));
    },
  }, 'document');
  Object.defineProperty(scope, 'document', { value: document, configurable: false, writable: false });
  return Object.freeze({
    containers: Object.freeze({ bg, canvasContainer, layers }),
    record,
    audit: () => ({ counts: { ...counts }, sourceCanvasSurfaces: surfaces }),
  });
}
