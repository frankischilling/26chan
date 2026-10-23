// Source embed geometry. Movie dimensions are metadata, not viewport authority.
function valid(width, height, viewportWidth, viewportHeight) {
  return [width, height, viewportWidth, viewportHeight].every(value => Number.isSafeInteger(value) && value > 0);
}

export function threadSize(width, height, viewportWidth, mobile) {
  if (!valid(width, height, viewportWidth, 1) || typeof mobile !== 'boolean') return null;
  const maximum = viewportWidth - (mobile ? 30 : 100);
  if (maximum < 1) return null;
  if (width > maximum) {
    const ratio = width / height;
    width = maximum;
    height = Math.round(maximum / ratio);
  }
  return width > 0 && height > 0 ? { width, height } : null;
}

export function indexSize(width, height, viewportWidth, viewportHeight) {
  if (!valid(width, height, viewportWidth, viewportHeight)) return null;
  const maxWidth = viewportWidth - 10;
  const maxHeight = viewportHeight - 30;
  if (maxWidth < 1 || maxHeight < 1) return null;
  const ratio = width / height;
  if (width > maxWidth) {
    width = maxWidth;
    height = Math.round(maxWidth / ratio);
  }
  if (height > maxHeight) {
    height = maxHeight;
    width = Math.round(maxHeight * ratio);
  }
  return width > 0 && height > 0 ? { width, height } : null;
}
