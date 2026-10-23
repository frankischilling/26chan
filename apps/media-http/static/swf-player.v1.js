'use strict';
// Ruffle's networking option is not an enforcement boundary. The document's
// HTTP CSP and opaque-origin sandbox independently restrict this frame.
const assetOrigin = new URL(document.currentScript.src).origin;
const policy = Object.freeze({
  polyfills: false, allowScriptAccess: false, allowNetworking: 'none',
  openUrlMode: 'deny', credentialAllowList: [], socketProxy: [], fontSources: [],
  autoplay: 'on', unmuteOverlay: 'visible', maxExecutionDuration: 5,
  showSwfDownload: false, contextMenu: false, splashScreen: false,
  warnOnUnsupportedContent: true,
});
window.RufflePlayer = { config: policy };
window.addEventListener('DOMContentLoaded', async () => {
  const container = document.getElementById('player');
  const status = document.getElementById('status');
  const controller = new AbortController();
  const deadline = setTimeout(() => controller.abort(), 15000);
  try {
    const url = new URL(container.dataset.movie);
    if (url.origin !== assetOrigin || !/^\/player-data\/[a-z0-9]{1,16}\/[1-9][0-9]*$/.test(url.pathname) || url.search || url.hash || url.username || url.password) throw new Error('Invalid movie URL');
    const response = await fetch(url, { credentials: 'omit', redirect: 'error', cache: 'no-store', signal: controller.signal });
    if (!response.ok) throw new Error('Movie unavailable');
    const length = Number(response.headers.get('content-length'));
    if (!Number.isSafeInteger(length) || length < 8 || length > 10 * 1024 * 1024) throw new Error('Invalid movie length');
    const reader = response.body.getReader();
    const data = new Uint8Array(length);
    let offset = 0;
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        if (offset + value.byteLength > length) throw new Error('Oversized movie');
        data.set(value, offset);
        offset += value.byteLength;
      }
      if (offset !== length) throw new Error('Incomplete movie');
    } finally {
      await reader.cancel().catch(() => {});
    }
    clearTimeout(deadline);
    const player = window.RufflePlayer.newest().createPlayer();
    container.append(player);
    await player.ruffle().load({ ...policy, data, swfFileName: container.dataset.filename });
    status.hidden = true;
  } catch {
    container.replaceChildren();
    status.textContent = 'This Flash file could not be played. You can still download the original file.';
  } finally {
    clearTimeout(deadline);
  }
}, { once: true });
