import { readWatcherPosition } from '../static/watcher-position.v1.js';

export function quickReplyPosition(value, { width, height, panelWidth, panelHeight }) {
  if (![width, height, panelWidth, panelHeight].every(number => Number.isFinite(number) && number >= 0)) return null;
  if (typeof value === 'string') {
    const coordinates = readWatcherPosition(value);
    if (!coordinates) return null;
    const pixels = (coordinate, span) => Number.parseFloat(coordinate) * (coordinate.endsWith('%') ? span / 100 : 1);
    value = {
      left: coordinates.left !== undefined ? pixels(coordinates.left, width)
        : width - panelWidth - pixels(coordinates.right, width),
      top: coordinates.top !== undefined ? pixels(coordinates.top, height)
        : height - panelHeight - pixels(coordinates.bottom, height),
    };
  }
  if (!value || !Number.isFinite(value.left) || !Number.isFinite(value.top)) return null;
  return {
    left: Math.max(0, Math.min(width - panelWidth, value.left)),
    top: Math.max(0, Math.min(height - panelHeight, value.top)),
  };
}
