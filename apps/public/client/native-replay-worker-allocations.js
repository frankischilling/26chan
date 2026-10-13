// A conservative cumulative allocation charge, not a browser RSS measurement.
// Install only in a fresh disposable worker before importing the pinned source.
// Parent termination deadlines are still required for native or JS computation.
export function installReplayAllocationGuard(scope = globalThis) {
  const maximum = 128 * 1024 * 1024;
  let charged = 0;
  function charge(bytes) {
    if (!Number.isSafeInteger(bytes) || bytes < 0 || charged + bytes > maximum) {
      throw new Error('Replay allocation budget exceeded');
    }
    charged += bytes;
  }
  function pixels(width, height) {
    if (!Number.isInteger(width) || !Number.isInteger(height) || width < 1 || height < 1
      || width > 1024 || height > 1024) throw new Error('Replay pixel allocation bounds exceeded');
    charge(width * height * 4);
  }
  const NativeBuffer = scope.ArrayBuffer;
  const bufferLength = Object.getOwnPropertyDescriptor(NativeBuffer.prototype, 'byteLength').get;
  const isBuffer = value => { try { bufferLength.call(value); return true; } catch { return false; } };
  for (const name of ['Uint8Array', 'Uint8ClampedArray', 'Uint16Array', 'Uint32Array']) {
    const Native = scope[name];
    if (typeof Native !== 'function') throw new Error(`Missing native ${name}`);
    const guarded = new Proxy(Native, {
      construct(target, args) {
        const value = args[0];
        if (typeof value === 'number') charge(value * target.BYTES_PER_ELEMENT);
        else if (value === undefined) charge(0);
        else if (!isBuffer(value)) {
          // Pinned code only copies typed arrays, never an arbitrary iterable.
          if (!NativeBuffer.isView(value) || !Number.isSafeInteger(value.length)) throw new Error('Unexpected replay allocation input');
          charge(value.length * target.BYTES_PER_ELEMENT);
        }
        return Reflect.construct(target, args);
      },
    });
    Object.defineProperty(scope, name, { value: guarded, writable: false, configurable: false });
  }
  const NativeImage = scope.ImageData;
  if (typeof NativeImage !== 'function') throw new Error('Missing native ImageData');
  Object.defineProperty(scope, 'ImageData', { value: new Proxy(NativeImage, {
    construct(target, args) {
      if (typeof args[0] === 'number') pixels(args[0], args[1]);
      else if (!NativeBuffer.isView(args[0])) throw new Error('Unexpected replay image input');
      // Typed data was already charged at its allocation; ImageData retains it.
      return Reflect.construct(target, args);
    },
  }), writable: false, configurable: false });
  const context = scope.OffscreenCanvasRenderingContext2D?.prototype;
  if (!context) throw new Error('Missing native worker context');
  for (const name of ['createImageData', 'getImageData']) {
    const original = context[name];
    if (typeof original !== 'function') throw new Error(`Missing native ${name}`);
    Object.defineProperty(context, name, { value: function(...args) {
      if (name === 'getImageData') pixels(args[2], args[3]);
      else if (typeof args[0] === 'number') pixels(args[0], args[1]);
      else pixels(args[0]?.width, args[0]?.height);
      return Reflect.apply(original, this, args);
    }, writable: false, configurable: false });
  }
  return Object.freeze({ charge, pixels, audit: () => ({ chargedBytes: charged, maximumBytes: maximum }) });
}
