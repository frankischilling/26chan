// Decode only screenshots made by the pinned test browser, never uploaded media.
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';

const require = createRequire(import.meta.url);
const { PNG } = require(join(dirname(require.resolve('playwright-core/package.json')), 'lib/utilsBundle.js'));

export function screenshotPixel(bytes, point, scale) {
  const png = PNG.sync.read(bytes);
  const x = Math.floor((point.x + 0.5) * scale);
  const y = Math.floor((point.y + 0.5) * scale);
  if (x < 0 || y < 0 || x >= png.width || y >= png.height) throw new Error('Screenshot sample is outside its bounds.');
  const offset = (y * png.width + x) * 4;
  return [...png.data.subarray(offset, offset + 4)];
}
