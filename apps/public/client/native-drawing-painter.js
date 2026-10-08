import { drawingDimensions, drawingPngFile } from './native-drawing-core.js';

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
  exportFile = drawingPngFile, downloads = createDrawingDownload({ exportFile }) } = {}) {
  let engine, loading, owner = null, active = false, generation = 0, disposed = false;
  const setActive = value => { active = value; if (!value) downloads.invalidate(); activeChanged(value); };
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
      // The source image importer decodes without bounds and never revokes its
      // object URL. This initial drawing slice explicitly excludes image import.
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
  async function open(client, width, height) {
    if (disposed || active) return false;
    let dimensions;
    try { dimensions = drawingDimensions(width, height); } catch (error) { client.error(error.message); return false; }
    const epoch = ++generation;
    client.loading(true);
    try {
      const painter = await ready();
      if (disposed || epoch !== generation || client.disposed()) return false;
      if (owner && owner !== client && painter.bg && (owner.key !== client.key || owner.target !== client.target)) {
        if (owner.pending() && !confirmReplacement()) return false;
        if (!await (owner.clearForReplacement?.() ?? owner.clear())) return false;
        if (disposed || epoch !== generation || client.disposed()) return false;
        owner.replaced(); downloads.invalidate(); painter.destroy();
      }
      if (!await client.prepare()) return false;
      if (disposed || epoch !== generation || client.disposed()) return false;
      owner = client;
      const onDone = async () => {
        if (disposed || owner !== client || epoch !== generation || client.disposed()) return;
        setActive(false); client.exporting();
        try {
          // Check before flatten allocates another canvas, including canvas sizes
          // changed inside Tegaki's own New/Open menu.
          drawingDimensions(painter.baseWidth, painter.baseHeight);
          const file = await exportFile(painter.flatten());
          if (disposed || owner !== client || epoch !== generation || client.disposed()) return;
          await client.finished(file);
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
      const menu = painter.bg?.querySelector?.('#tegaki-menu-bar');
      const openButton = menu?.querySelectorAll?.('.tegaki-mb-btn')?.[1];
      if (openButton) {
        openButton.textContent = 'Open (unavailable)'; openButton.title = 'Importing an existing image is not supported in this drawing workflow.';
        openButton.setAttribute('aria-disabled', 'true'); openButton.classList.add('tegaki-disabled');
        openButton.dataset.drawingImportUnavailable = '';
      }
      const picker = painter.bg?.querySelector?.('#tegaki-filepicker'); if (picker) picker.disabled = true;
      return true;
    } catch {
      if (epoch === generation && !disposed && !client.disposed()) {
        setActive(false); client.error('The drawing editor could not be loaded. Try Draw again.');
      }
      return false;
    } finally { if (!client.disposed()) client.loading(false); }
  }
  function invalidate(client, { destroy = false } = {}) {
    if (owner !== client) return;
    generation++;
    if (active && engine?.bg) engine.hide();
    setActive(false);
    if (destroy && engine?.bg) engine.destroy();
    // Keep this owner after Clear/QR dismissal: resuming the same form preserves
    // layers. A different form deliberately replaces the retained canvas.
  }
  function dispose() {
    disposed = true; generation++; downloads.dispose();
    if (engine?.bg) engine.destroy(); owner = null; setActive(false);
  }
  return { open, invalidate, dispose, active: () => active };
}
