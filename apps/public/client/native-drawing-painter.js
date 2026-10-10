import { drawingDimensions, drawingPngFile } from './native-drawing-core.js';
import { postId } from '../static/thread-watcher-core.v1.js';

// Own browser download URLs without exposing them to upload/posting state.
export function createDrawingDownload({ exportFile = drawingPngFile,
  createURL = blob => URL.createObjectURL(blob), revokeURL = value => URL.revokeObjectURL(value),
  schedule = setTimeout, unschedule = clearTimeout,
  download = (url, name) => {
    const link = document.createElement('a'); link.hidden = true; link.href = url; link.download = name;
    document.body.append(link); try { link.click(); } finally { link.remove(); }
  } } = {}) {
  let url = null, timer = null, generation = 0, busy = false, disposed = false;
  function release() { unschedule(timer); timer = null; if (url) { revokeURL(url); url = null; } }
  function invalidate() { generation++; release(); }
  async function save(canvas, current = () => true) {
    if (disposed || busy) return false;
    busy = true; const epoch = ++generation;
    try {
      const file = await exportFile(canvas);
      if (disposed || epoch !== generation || !current()) return false;
      release(); url = createURL(file);
      try { download(url, file.name); }
      catch (error) { release(); throw error; }
      // Allow the browser to acquire the download, with a bounded lifetime even
      // if the editor remains open. Dismissal revokes it sooner.
      const owned = url;
      if (owned) timer = schedule(() => { if (url === owned) release(); }, 30_000);
      return true;
    } finally { busy = false; }
  }
  return { save, invalidate, busy: () => busy, dispose() { disposed = true; invalidate(); } };
}

// Tegaki is a single retained editor. Finish and form Clear do not destroy its
// layers. Cancel is confirmed and destroyed by Tegaki itself, before onCancel.
export function createDrawingPainter({ load = () => import('/static/tegaki/tegaki-0.9.4.v1.js').then(module => module.Tegaki),
  activeChanged = () => {}, confirmReplacement = () => confirm('Replace the drawing in the other form?'),
  confirmImportReplacement = () => confirm('Replace the current drawing with this post image?'),
  exportFile = drawingPngFile, downloads = createDrawingDownload({ exportFile }),
  createImage = () => new Image(), schedule = setTimeout, unschedule = clearTimeout, now = Date.now } = {}) {
  let engine, loading, owner = null, active = false, generation = 0, disposed = false, pendingSource = null, pendingOpen = null;
  const setActive = value => { active = value; if (!value) downloads.invalidate(); activeChanged(value); };
  // Never let a cross-origin Image enter Tegaki until the browser has completed
  // its anonymous CORS load and its decoded dimensions satisfy the same budget
  // as the disposable media decoder. Cancelling drops both callbacks and src.
  function sourceImage(url) {
    const image = createImage();
    let done = false, resolve, timer;
    const promise = new Promise(complete => { resolve = complete; });
    function finish(value) {
      if (done) return;
      done = true; unschedule(timer); image.onload = image.onerror = null;
      if (!value) { try { image.src = ''; } catch { /* Already detached. */ } }
      resolve(value);
    }
    image.crossOrigin = 'anonymous';
    image.referrerPolicy = 'no-referrer';
    image.onload = () => {
      try { drawingDimensions(image.naturalWidth, image.naturalHeight); finish(image); }
      catch { finish(null); }
    };
    image.onerror = () => finish(null);
    timer = schedule(() => finish(null), 10000);
    try { image.src = url; } catch { finish(null); }
    return { promise, cancel() { finish(null); } };
  }
  function cancelPending() {
    const pending = pendingOpen;
    pendingOpen = null;
    const image = pendingSource;
    pendingSource = null;
    image?.cancel();
    if (pending && !pending.client.disposed()) pending.client.loading(false);
  }
  const ready = async () => {
    if (engine) return engine;
    const painter = await (loading ??= Promise.resolve().then(load).catch(error => { loading = null; throw error; }));
    if (!engine && typeof painter.resizeCanvas === 'function') {
      const resize = painter.resizeCanvas;
      painter.resizeCanvas = function(width, height) {
        try { drawingDimensions(width, height); }
        catch (error) { error.drawingBounds = true; throw error; }
        return resize.call(this, width, height);
      };
      // Bound New before it allocates; local image import is disabled below.
      for (const name of ['onNewClick']) {
        const original = painter[name];
        if (typeof original !== 'function') continue;
        painter[name] = function(...args) {
          try { return original.apply(this, args); }
          catch (error) {
            if (!error?.drawingBounds) throw error;
            owner?.error(error.message);
            if (typeof alert === 'function') alert(error.message);
          }
        };
      }
    }
    if (!engine) {
      // The vendor's local-file importer decodes without dimensions checks and
      // retains object URLs. Only a validated remote post image may be imported
      // through the separate anonymous CORS loader below.
      painter.onOpenClick = painter.onOpenFileSelected = () => {
        owner?.error('Opening an image in the drawing editor is unavailable.'); return false;
      };
      painter.onExportClick = async () => {
        const client = owner, epoch = generation;
        if (!active || disposed || !client || client.disposed() || downloads.busy()) return;
        try {
          drawingDimensions(painter.baseWidth, painter.baseHeight);
          await downloads.save(painter.flatten(), () => active && !disposed && epoch === generation && owner === client && !client.disposed());
        } catch {
          if (!disposed && epoch === generation && owner === client && !client.disposed()) {
            client.error('The PNG could not be downloaded. Try Export again.');
          }
        }
      };
    }
    engine = painter; return engine;
  };
  async function open(client, width, height, sourceUrl = null, sourceId = null) {
    if (disposed || active || client.disposed() || (client.allowed && !client.allowed())) return false;
    if (pendingOpen?.client === client) return false;
    if (pendingOpen || pendingSource) { generation++; cancelPending(); }
    if (disposed || client.disposed() || (client.allowed && !client.allowed())) return false;
    let dimensions;
    if (!sourceUrl) {
      try { dimensions = drawingDimensions(width, height); } catch (error) { client.error(error.message); return false; }
    }
    const epoch = ++generation;
    pendingOpen = { client, epoch };
    client.loading(true);
    try {
      let image = null;
      if (sourceUrl) {
        const pending = sourceImage(sourceUrl);
        pendingSource = { client, ...pending };
        image = await pending.promise;
        if (pendingSource?.promise === pending.promise) pendingSource = null;
        if (disposed || epoch !== generation || client.disposed() || (client.allowed && !client.allowed())) return false;
        if (!image) { client.error('The source image could not be loaded (maximum 1024 × 1024).'); return false; }
        dimensions = drawingDimensions(image.naturalWidth, image.naturalHeight);
      }
      const painter = await ready();
      if (disposed || epoch !== generation || client.disposed() || (client.allowed && !client.allowed())) return false;
      const replacing = !!painter.bg && (sourceUrl || (owner && owner !== client && (owner.key !== client.key || owner.target !== client.target)));
      if (replacing && owner) {
        // A canvas can contain unsent strokes after Finish, Clear, or QR
        // dismissal even when the old controller reports no pending upload.
        // The retained layers themselves require consent before replacement.
        if (!(sourceUrl ? confirmImportReplacement() : confirmReplacement())) return false;
        if (!await (owner.clearForReplacement?.() ?? owner.clear())) return false;
        if (disposed || epoch !== generation || client.disposed() || (client.allowed && !client.allowed())) return false;
      }
      if (!await client.prepare()) return false;
      if (disposed || epoch !== generation || client.disposed() || (client.allowed && !client.allowed())) return false;
      if (replacing) {
        owner?.replaced(); downloads.invalidate(); painter.destroy();
      }
      owner = client;
      const onDone = async () => {
        if (disposed || owner !== client || epoch !== generation || client.disposed()) return;
        setActive(false); client.exporting();
        try {
          // Check before flatten allocates another canvas, including canvas sizes
          // changed inside Tegaki's own New/Open menu.
          drawingDimensions(painter.baseWidth, painter.baseHeight);
          const tracked = painter.startTimeStamp && (!painter.hasCustomCanvas || client.sourcePost?.());
          const seconds = tracked ? Math.max(0, Math.round((now() - painter.startTimeStamp) / 1000)) : 0;
          const file = await exportFile(painter.flatten());
          if (disposed || owner !== client || epoch !== generation || client.disposed()) return;
          await client.finished(file, seconds);
        } catch {
          if (!disposed && owner === client && epoch === generation && !client.disposed()) {
            client.error('The drawing could not be exported. Use Edit to check the canvas (maximum 1024 × 1024), then Finish again.');
          }
        }
      };
      const onCancel = () => {
        if (disposed || owner !== client || epoch !== generation || client.disposed()) return;
        generation++; setActive(false); void client.cancelled();
      };
      // open() resumes retained canvases before reading its options, so callbacks
      // must be rebound explicitly when a form is reopened.
      painter.onDoneCb = onDone; painter.onCancelCb = onCancel;
      setActive(true);
      painter.open({ ...dimensions, onDone, onCancel, saveReplay: false, replayMode: false });
      if (image) {
        // Tegaki's own image importer creates one layer and resets history.
        // Never expose its unbounded file picker or arbitrary-URL loading path.
        painter.onOpenImageLoaded.call(image);
        client.imported?.(sourceId);
      }
      const menu = painter.bg?.querySelector?.('#tegaki-menu-bar');
      const openButton = menu?.querySelectorAll?.('.tegaki-mb-btn')?.[1];
      if (openButton) {
        openButton.textContent = 'Open (unavailable)'; openButton.title = 'Opening a local file is unavailable. Use Edit on an approved post PNG.';
        openButton.setAttribute('aria-disabled', 'true'); openButton.classList.add('tegaki-disabled');
        openButton.dataset.drawingImportUnavailable = '';
      }
      const picker = painter.bg?.querySelector?.('#tegaki-filepicker'); if (picker) picker.disabled = true;
      return true;
    } catch {
      if (epoch === generation && !disposed && !client.disposed()) {
        if (sourceUrl && engine?.bg) engine.destroy();
        setActive(false); client.error(sourceUrl
          ? 'The source image could not be opened in Tegaki.'
          : 'The drawing editor could not be loaded. Try Draw again.');
      }
      return false;
    } finally {
      // A canceled operation must not clear the loading state of a newer
      // import from the same form.
      if (pendingOpen?.epoch === epoch) {
        pendingOpen = null;
        if (!client.disposed()) client.loading(false);
      }
    }
  }
  async function importFromPost(client, source) {
    if (typeof source === 'string' && source) return open(client, null, null, source);
    const id = postId(source?.id);
    if (!id || typeof source.url !== 'string' || !source.url) return false;
    return open(client, null, null, source.url, id);
  }
  function invalidate(client, { destroy = false } = {}) {
    const retained = destroy && owner?.key === client.key && owner?.target === client.target;
    if (owner !== client && !retained && pendingOpen?.client !== client && pendingSource?.client !== client) return;
    generation++;
    cancelPending();
    if (owner !== client && !retained) return;
    if (active && engine?.bg) engine.hide();
    setActive(false);
    if (destroy && engine?.bg) engine.destroy();
    if (destroy) owner = null;
    // Keep this owner after Clear/QR dismissal: resuming the same form preserves
    // layers. A different form deliberately replaces the retained canvas.
  }
  function suspend() {
    generation++;
    cancelPending();
    if (active && engine?.bg) engine.hide();
    setActive(false);
    if (owner && !owner.disposed()) owner.suspended?.();
  }
  const retained = (key, target) => !!engine?.bg && owner?.key === key && owner.target === target;
  function dispose() {
    disposed = true; generation++;
    cancelPending();
    downloads.dispose();
    if (engine?.bg) engine.destroy(); owner = null; setActive(false);
  }
  return { open, importFromPost, invalidate, suspend, retained, dispose, active: () => active };
}
