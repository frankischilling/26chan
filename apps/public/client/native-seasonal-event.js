// Source seasonal preferences are separate from ordinary Tomorrow selection.
export const SEASONAL_STOP_KEY = '4chan_stop_css_event';
export function seasonalStyle(event, version, stopped, preferred) {
  if (event && stopped !== `${event}-${version}`) return '_special';
  return ({ 'Yotsuba B': 'Yotsuba B New', Yotsuba: 'Yotsuba New',
    Burichan: 'Burichan New', Futaba: 'Futaba New' })[preferred] || preferred;
}
export function seasonalChange(event, version, selected) {
  if (!event) return { stopped: null, action: null };
  return selected === '_special'
    ? { stopped: null, action: 'init' }
    : { stopped: `${event}-${version}`, action: 'cleanup' };
}
export function snowCount(random) { return Math.floor(random() * 50) + 50; }

// Keep the source draw order, including the first draw on initial placement.
// Only this fixed local asset family is selectable; callers cannot supply URLs.
export function rotateSkeleton(document, random, event) {
  const threshold = event?.detail?.count ? 0.33 : 0;
  if (random() < threshold) return false;
  const previous = document.getElementById('skellington');
  if (previous) previous.parentNode.removeChild(previous);
  const index = 1 + Math.floor(random() * 23);
  const image = document.createElement('img');
  image.id = 'skellington';
  image.className = 'desktop' + (random() < 0.25 ? ' topskel' : '');
  image.alt = '';
  image.src = random() < 0.01 ? '/static/events/temp/dinosaur.gif' : `/static/events/skeletons/${index}.gif`;
  document.body.insertBefore(image, document.body.firstElementChild);
  return true;
}

// A controller owns one listener and decoration. Asset validation and explicit
// operator activation belong to its caller; importing this module does nothing.
export function spookyController(window, random = Math.random) {
  const rotate = event => rotateSkeleton(window.document, random, event);
  return {
    start() {
      if (!window.matchMedia?.('(min-width: 481px)').matches) return false;
      window.document.addEventListener('4chanThreadUpdated', rotate, false);
      window.dark_captcha = true;
      rotate();
      return true;
    },
    stop() {
      window.dark_captcha = false;
      window.document.removeEventListener('4chanThreadUpdated', rotate, false);
      const image = window.document.getElementById('skellington');
      if (image) image.parentNode.removeChild(image);
    },
  };
}
