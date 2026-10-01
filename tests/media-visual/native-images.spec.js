import { watcherSettingsOpener } from '../browser/helpers/watcher-settings.js';
import { test, expect } from '../helpers/visual-diagnostics.js';

test.use({ javaScriptEnabled: true });

const origin = 'http://127.0.0.1:3000';
const mediaOrigin = 'http://localhost:3004';
const threadPath = '/img/thread/1000201';

async function openThread(page, settings) {
  if (settings !== undefined) {
    await page.addInitScript(({ origin, settings }) => {
      if (location.origin === origin) localStorage.setItem('4chan-settings', JSON.stringify(settings));
    }, { origin, settings });
  }
  await page.goto(threadPath);
  await expect(page.getByRole('button', { name: 'Post menu for post 1000201', exact: true })).toBeVisible();
}

async function waitForImage(image) {
  await expect.poll(() => image.evaluate(node => node.complete && node.naturalWidth > 0)).toBe(true);
}

async function expand(page, id) {
  const anchor = page.locator(`#f${id} > a.fileThumb`);
  await anchor.click();
  const image = anchor.locator('.expanded-thumb');
  await expect(image).toBeVisible();
  await waitForImage(image);
  return { anchor, image };
}

async function updaterSnapshot(browser) {
  const context = await browser.newContext({ javaScriptEnabled: false });
  try {
    const source = await context.newPage();
    await source.goto(`${origin}${threadPath}`);
    const posts = await source.locator('.postContainer').evaluateAll(nodes => nodes.map(node => ({
      no: node.id.slice(2), file_deleted: !!node.querySelector('.fileDeletedRes'), html: node.outerHTML,
    })));
    return { version: 2, tail_size: 0, tail_id: null, board: 'img', thread: '1000201',
      closed: false, archived: false, sticky: false, replies: posts.length - 1, images: 4, posts };
  } finally { await context.close(); }
}

test('image settings keep reference defaults while normal and legacy files expand, collapse and quote safely', async ({ page }, info) => {
  await openThread(page);

  await watcherSettingsOpener(page).click();
  await expect(page.locator('#setting-imageExpansion')).toBeChecked();
  for (const key of ['fitToScreenExpansion', 'imageHover', 'imageHoverBg', 'revealSpoilers', 'noPictures']) {
    await expect(page.locator(`#setting-${key}`)).not.toBeChecked();
  }
  await page.locator('#settings-close').click();

  const normal = await expand(page, 1000201);
  await expect(normal.image).toHaveAttribute('src', `${mediaOrigin}/img/1000201.png`);
  expect(await normal.image.evaluate(node => [node.naturalWidth, node.naturalHeight])).toEqual([600, 360]);
  const desktopCapture = info.outputPath('native-image-expanded-desktop.png');
  await page.locator('#f1000201').screenshot({ path: desktopCapture, animations: 'disabled' });
  await info.attach('native image expanded desktop', { path: desktopCapture, contentType: 'image/png' });
  await normal.image.click();
  await expect(normal.anchor.locator('.expanded-thumb')).toHaveCount(0);
  await expect(normal.anchor.locator('img')).toHaveAttribute('src', `${mediaOrigin}/img/1000201s.jpg`);

  const legacy = page.locator('#f1000204 > a.fileThumb');
  await expect(legacy.locator('img')).toHaveAttribute('src', `${mediaOrigin}/img/1000204.png`);
  const legacyExpanded = await expand(page, 1000204);
  await expect(legacyExpanded.image).toHaveAttribute('src', `${mediaOrigin}/img/1000204.png`);
  await legacyExpanded.anchor.click();
  await expect(legacy.locator('.expanded-thumb')).toHaveCount(0);

  const quoted = await expand(page, 1000202);
  await page.locator('#pi1000202 > .postNum > a[title="Reply to this post"]').click();
  await expect(page.locator('#qrCom')).toHaveValue('>>1000202\n');
  await expect(page.locator('#quickReply .expanded-thumb, #quickReply img')).toHaveCount(0);
  await expect(quoted.image).toBeVisible();
});

test('Images & Media settings save through the UI and restore behavior after navigation', async ({ page }) => {
  await openThread(page);
  await watcherSettingsOpener(page).click();
  const category = page.locator('#settings-images');
  if (await category.isHidden()) await page.getByRole('button', { name: 'Images & Media', exact: true }).click();
  await page.locator('#setting-imageExpansion').uncheck();
  await page.locator('#setting-revealSpoilers').check();
  await page.locator('#setting-noPictures').check();

  const navigation = page.waitForNavigation();
  await page.locator('#settings-save').click();
  await navigation;
  await expect(page.locator('.board')).toHaveClass(/\bnoPictures\b/);
  await expect(page.locator('#f1000205')).toHaveClass(/\bnativeSpoilerRevealed\b/);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')))).toMatchObject({
    imageExpansion: false, revealSpoilers: true, noPictures: true,
  });

  const anchor = page.locator('#f1000201 > a.fileThumb');
  const controllerPrevented = await anchor.evaluate(node => {
    let prevented;
    document.addEventListener('click', event => {
      prevented = event.defaultPrevented;
      event.preventDefault();
    }, { once: true });
    node.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, button: 0 }));
    return prevented;
  });
  expect(controllerPrevented).toBe(false);
  await expect(anchor.locator('.expanded-thumb')).toHaveCount(0);

  await watcherSettingsOpener(page).click();
  if (await category.isHidden()) await page.getByRole('button', { name: 'Images & Media', exact: true }).click();
  await expect(page.locator('#setting-imageExpansion')).not.toBeChecked();
  await expect(page.locator('#setting-revealSpoilers')).toBeChecked();
  await expect(page.locator('#setting-noPictures')).toBeChecked();
});

test('noPictures preserves thumbnail geometry and fitted expansions remain visible with their aspect ratio', async ({ page }, info) => {
  await page.setViewportSize({ width: 390, height: 220 });
  await openThread(page, { noPictures: true, fitToScreenExpansion: true });
  const board = page.locator('.board');
  await expect(board).toHaveClass(/\bnoPictures\b/);
  await expect(page.locator('#f1000205 .imgspoiler > img')).toHaveCSS('opacity', '0');
  await expect(page.locator('#f1000205 .imgspoiler > .mFileInfo')).toBeHidden();

  const thumbnail = page.locator('#f1000201 > a.fileThumb > img');
  await thumbnail.scrollIntoViewIfNeeded();
  await waitForImage(thumbnail);
  const before = await thumbnail.boundingBox();
  expect([before.width, before.height]).toEqual([125, 125]);
  await expect(thumbnail).toHaveCSS('opacity', '0');

  const { image } = await expand(page, 1000201);
  await expect(image).toHaveCSS('opacity', '1');
  const box = await image.boundingBox();
  expect(box.width).toBeLessThanOrEqual(390);
  expect(box.height).toBeLessThanOrEqual(220);
  expect(Math.abs((box.width / box.height) - (600 / 360))).toBeLessThan(0.01);
  expect(await image.evaluate(node => [node.style.maxWidth, node.style.maxHeight])).toEqual([
    expect.stringMatching(/px$/), expect.stringMatching(/px$/),
  ]);
  const mobileCapture = info.outputPath('native-image-expanded-mobile.png');
  await page.locator('#f1000201').screenshot({ path: mobileCapture, animations: 'disabled' });
  await info.attach('native image expanded mobile', { path: mobileCapture, contentType: 'image/png' });
});

test('hover follows viewport geometry and its background setting uses the visible page color', async ({ page }) => {
  await page.setViewportSize({ width: 640, height: 480 });
  await openThread(page, { imageHover: true, imageHoverBg: true });
  const thumbnail = page.locator('#f1000201 > a.fileThumb > img');
  await thumbnail.scrollIntoViewIfNeeded();
  await waitForImage(thumbnail);
  const expected = await thumbnail.evaluate(node => {
    const ratio = Math.min(1, (innerWidth - node.getBoundingClientRect().right - 20) / 600,
      document.documentElement.clientHeight / 360);
    return { width: 600 * ratio, height: 360 * ratio,
      background: getComputedStyle(document.body).backgroundColor };
  });
  await thumbnail.hover();
  const preview = page.locator('#image-hover');
  await expect(preview).toBeVisible();
  await waitForImage(preview);
  const style = await preview.evaluate(node => ({ width: parseFloat(node.style.maxWidth), height: parseFloat(node.style.maxHeight) }));
  expect(style.width).toBeCloseTo(expected.width, 4);
  expect(style.height).toBeCloseTo(expected.height, 4);
  expect(expected.background).not.toBe('rgba(0, 0, 0, 0)');
  await expect(preview).toHaveCSS('background-color', expected.background);
});

test('hover retains the base page background when the optional theme stylesheet fails', async ({ page, visualDiagnostics }) => {
  let themeFailures = 0;
  await page.route(url => url.origin === origin && url.pathname === '/static/theme.css', route => {
    themeFailures++;
    return route.abort('failed');
  });
  await page.setViewportSize({ width: 640, height: 480 });
  await openThread(page, { imageHover: true, imageHoverBg: true });
  expect(themeFailures).toBe(1);
  expect(visualDiagnostics.failedStylesheets).toEqual([{ path: '/static/theme.css', error: 'net::ERR_FAILED' }]);
  expect(await page.locator('html').evaluate(node => getComputedStyle(node).getPropertyValue('--paper').trim())).toBe('');
  await expect(page.locator('html')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(page.locator('body')).toHaveCSS('background-color', 'rgb(255, 255, 238)');
  const thumbnail = page.locator('#f1000201 > a.fileThumb > img');
  await thumbnail.scrollIntoViewIfNeeded();
  await waitForImage(thumbnail);
  await thumbnail.hover();
  const preview = page.locator('#image-hover');
  await expect(preview).toBeVisible();
  await waitForImage(preview);
  await expect(preview).toHaveClass('nativeImageBackground');
  await expect(preview).toHaveCSS('background-color', 'rgb(255, 255, 238)');
});

for (const [theme, background] of Object.entries({
  yotsuba: 'rgb(255, 255, 238)', futaba: 'rgb(255, 255, 238)',
  'yotsuba-b': 'rgb(238, 242, 255)', burichan: 'rgb(238, 242, 255)',
  photon: 'rgb(238, 238, 238)', tomorrow: 'rgb(29, 31, 33)',
})) {
  test(`${theme} hover uses the selected page background`, async ({ page, context }) => {
    await context.addCookies(['board-theme', 'board-theme-ws'].map(name => ({
      name, value: theme, url: origin, httpOnly: true, sameSite: 'Lax',
    })));
    await page.setViewportSize({ width: 640, height: 480 });
    await openThread(page, { imageHover: true, imageHoverBg: true });
    await expect(page.locator('html')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
    await expect(page.locator('body')).toHaveCSS('background-color', background);
    const thumbnail = page.locator('#f1000201 > a.fileThumb > img');
    await thumbnail.scrollIntoViewIfNeeded();
    await waitForImage(thumbnail);
    await thumbnail.hover();
    const preview = page.locator('#image-hover');
    await expect(preview).toBeVisible();
    await waitForImage(preview);
    await expect(preview).toHaveCSS('background-color', background);
  });
}

test('hover cancellation, load errors and rejected media links cannot leave preview DOM or start foreign fetches', async ({ page }) => {
  await openThread(page, { imageHover: true, imageHoverBg: true });

  let release;
  let reached;
  let finished;
  const intercepted = new Promise(resolve => { reached = resolve; });
  const gate = new Promise(resolve => { release = resolve; });
  const done = new Promise(resolve => { finished = resolve; });
  await page.route(`${mediaOrigin}/img/1000201.png`, async route => {
    try {
      const response = await route.fetch();
      reached();
      await gate;
      await route.fulfill({ response }).catch(() => {});
    } finally { finished(); }
  });
  await page.locator('#f1000201 > a.fileThumb').hover();
  await intercepted;
  await expect(page.locator('#image-hover')).toHaveClass(/\bnativeImageBackground\b/);
  await page.mouse.move(0, 0);
  await expect(page.locator('#image-hover')).toHaveCount(0);
  release();
  await done;
  await page.unroute(`${mediaOrigin}/img/1000201.png`);

  await page.route(`${mediaOrigin}/img/1000202.png`, route => route.abort('connectionfailed'));
  await page.locator('#f1000202 > a.fileThumb').hover();
  await expect(page.locator('.nativeImageFeedback')).toHaveText('Image preview could not be loaded.');
  await expect(page.locator('#image-hover')).toHaveCount(0);
  await page.unroute(`${mediaOrigin}/img/1000202.png`);

  let foreignRequests = 0;
  await page.route('https://attacker.invalid/**', route => { foreignRequests++; return route.abort(); });
  const anchor = page.locator('#f1000203 > a.fileThumb');
  await anchor.evaluate(node => node.setAttribute('href', 'https://attacker.invalid/img/1000203.png'));
  const controllerPrevented = await anchor.evaluate(node => {
    let prevented;
    document.addEventListener('click', event => {
      prevented = event.defaultPrevented;
      event.preventDefault();
    }, { once: true });
    node.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, button: 0 }));
    return prevented;
  });
  expect(controllerPrevented).toBe(false);
  await anchor.dispatchEvent('mouseover');
  await expect(page.locator('#image-hover, #f1000203 .expanded-thumb')).toHaveCount(0);
  expect(foreignRequests).toBe(0);
});

test('spoiler media stays unfetched by default and revealSpoilers alone creates only the approved thumbnail', async ({ page }) => {
  const requests = [];
  page.on('request', request => {
    const url = new URL(request.url());
    if (url.origin === mediaOrigin && url.pathname.includes('1000205')) requests.push(url.pathname);
  });
  await openThread(page);
  await expect(page.locator('#f1000205 > .fileText > a')).toHaveText('Spoiler Image');
  await expect(page.locator('#f1000205 > a.fileThumb.imgspoiler > img')).toHaveAttribute('src', '/static/catalog/spoiler.png');
  await page.locator('#f1000205 .imgspoiler').scrollIntoViewIfNeeded();
  await waitForImage(page.locator('#f1000204 .fileThumb img'));
  expect(requests).toEqual([]);

  await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ imageExpansion: false })));
  await page.reload();
  await expect(page.locator('#f1000205 > a.fileThumb.imgspoiler')).toBeVisible();

  requests.length = 0;
  await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ imageExpansion: false, revealSpoilers: true })));
  await page.reload();
  const file = page.locator('#f1000205');
  await expect(file).toHaveClass(/\bnativeSpoilerRevealed\b/);
  await expect(file.locator('.imgspoiler')).toBeHidden();
  await expect(file.locator(':scope > .fileText > a:visible')).toHaveText(await file.getAttribute('data-image-filename'));
  const thumbnail = file.locator(':scope > a.fileThumb:not(.imgspoiler) > img');
  await expect(thumbnail).toHaveAttribute('src', `${mediaOrigin}/img/1000205s.jpg`);
  await thumbnail.scrollIntoViewIfNeeded();
  await waitForImage(thumbnail);
  expect(requests).toContain('/img/1000205s.jpg');
  expect(requests).not.toContain('/img/1000205.png');
  await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ revealSpoilers: false })));
  await page.reload();
  await expect(page.locator('#f1000205 > .fileText > a')).toHaveText('Spoiler Image');
  await expect(page.locator('#f1000205 > a.fileThumb.imgspoiler')).toBeVisible();
  await expect(page.locator('#f1000205 > a.fileThumb:not(.imgspoiler)')).toHaveCount(0);
});

test('cross-tab disable and pagehide release expanded images while a persisted pageshow restores normal admission', async ({ context, page }) => {
  await openThread(page);
  await expand(page, 1000201);

  const peer = await context.newPage();
  await peer.goto('/readyz');
  await peer.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
  await expect(page.locator('.expanded-thumb')).toHaveCount(0);

  await peer.evaluate(() => localStorage.setItem('4chan-settings', '{}'));
  await expect.poll(() => page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe('{}');
  await expand(page, 1000201);
  await page.evaluate(() => dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
  await expect(page.locator('.expanded-thumb, #image-hover')).toHaveCount(0);
  await page.evaluate(() => dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
  await expand(page, 1000201);
});

test('loaded theme changes refresh existing media and navigation without replacing an expanded image', async ({ context, page }) => {
  await context.addCookies([
    { name: 'board-theme', value: 'photon', url: origin, httpOnly: true, sameSite: 'Lax' },
    { name: 'board-theme-ws', value: 'photon', url: origin, httpOnly: true, sameSite: 'Lax' },
  ]);
  await openThread(page, { darkTheme: false, threadWatcher: true, stickyNav: true, noPictures: true, threadStats: false });
  const board = page.locator('.board');
  const themeLink = page.locator('link[data-native-theme-stylesheet]');
  const ordinaryHref = await themeLink.getAttribute('href');
  const ordinaryURL = new URL(ordinaryHref, origin).href;
  const family = () => page.evaluate(() => getComputedStyle(document.documentElement)
    .getPropertyValue('--watcher-icon-family').trim());
  const expectFamily = async value => {
    await expect.poll(family).toBe(value);
    await expect(board).toHaveAttribute('data-image-family', value);
    await expect(board).toHaveClass(/\bnoPictures\b/);
    await expect(page.locator('#twPrune img')).toHaveAttribute('src', `/static/watcher/${value}/refresh.png`);
    await expect(page.locator('#stickyNav button').first().locator('img')).toHaveAttribute('src', `/static/navigation/${value}/arrow_up.png`);
    await expect(page.locator('#p1000201 [data-post-menu]')).toHaveAttribute('data-family', value);
  };
  await expectFamily('photon');
  const { image } = await expand(page, 1000201);
  await image.evaluate(element => { window.ownedExpandedImage = element; });

  let release, reached, finished, releaseRestore = () => {};
  const pending = new Promise(resolve => { reached = resolve; });
  const gate = new Promise(resolve => { release = resolve; });
  const done = new Promise(resolve => { finished = resolve; });
  const stylesheet = '**/static/theme.css?*theme=tomorrow';
  await page.route(stylesheet, async route => {
    try {
      const response = await route.fetch();
      reached();
      await gate;
      await route.fulfill({ response }).catch(() => {});
    } finally { finished(); }
  });
  const other = await context.newPage();
  try {
    await other.goto('/readyz');
    await other.evaluate(() => {
      const settings = JSON.parse(localStorage.getItem('4chan-settings') || '{}');
      localStorage.setItem('4chan-settings', JSON.stringify({ ...settings, darkTheme: true }));
    });
    await pending;
    await expect(page.locator('link[data-native-theme-stylesheet]')).toHaveAttribute('href', /(?:\?|&)theme=tomorrow$/);
    await expectFamily('photon');
    release();
    await done;
    await expectFamily('tomorrow');
    await expect(image).toBeVisible();
    expect(await image.evaluate(element => element === window.ownedExpandedImage)).toBe(true);

    let restoreReached, restoreFinished;
    const restorePending = new Promise(resolve => { restoreReached = resolve; });
    const restoreGate = new Promise(resolve => { releaseRestore = resolve; });
    const restoreDone = new Promise(resolve => { restoreFinished = resolve; });
    await page.route(ordinaryURL, async route => {
      try {
        const response = await route.fetch();
        restoreReached();
        await restoreGate;
        await route.fulfill({ response }).catch(() => {});
      } finally { restoreFinished(); }
    });
    await other.evaluate(() => {
      const settings = JSON.parse(localStorage.getItem('4chan-settings') || '{}');
      localStorage.setItem('4chan-settings', JSON.stringify({ ...settings, darkTheme: false }));
    });
    await restorePending;
    await expect(themeLink).toHaveAttribute('href', ordinaryHref);
    await expectFamily('tomorrow');

    const reenabled = page.waitForResponse(response => response.url().includes('/static/theme.css?')
      && response.url().includes('theme=tomorrow'));
    await other.evaluate(() => {
      const settings = JSON.parse(localStorage.getItem('4chan-settings') || '{}');
      localStorage.setItem('4chan-settings', JSON.stringify({ ...settings, darkTheme: true }));
    });
    await reenabled;
    releaseRestore();
    await restoreDone;
    await page.unroute(ordinaryURL);
    await expect(themeLink).toHaveAttribute('href', /(?:\?|&)theme=tomorrow$/);
    await expectFamily('tomorrow');
    expect(await image.evaluate(element => element === window.ownedExpandedImage)).toBe(true);

    await other.evaluate(() => {
      const settings = JSON.parse(localStorage.getItem('4chan-settings') || '{}');
      localStorage.setItem('4chan-settings', JSON.stringify({ ...settings, darkTheme: false }));
    });
    await expectFamily('photon');
    await expect(image).toBeVisible();
    expect(await image.evaluate(element => element === window.ownedExpandedImage)).toBe(true);
  } finally {
    release();
    releaseRestore();
    await page.unroute(stylesheet);
    await page.unroute(ordinaryURL);
    await other.close();
  }
});

test('updater-added media expands and hidden or deleted live posts invalidate owned full-image DOM', async ({ browser, page }) => {
  const snapshot = await updaterSnapshot(browser);
  await openThread(page);
  await page.locator('.replyContainer').evaluateAll(nodes => nodes.forEach(node => node.remove()));
  await page.route('**/_watch/img/thread/1000201/posts', route => route.fulfill({
    contentType: 'application/json', body: JSON.stringify(snapshot),
  }));
  await page.locator('.threadNav.desktop a[data-cmd="update"]').first().click();
  await expect(page.locator('.threadNav.desktop .nativeUpdaterStatus').first()).toHaveText('5 new posts');

  await expand(page, 1000202);
  await page.getByRole('button', { name: 'Post menu for post 1000202', exact: true }).click();
  await page.getByRole('menuitem', { name: 'Hide post', exact: true }).click();
  await expect(page.locator('#pc1000202')).toHaveClass(/\bpost-hidden\b/);
  await expect(page.locator('#p1000202 .expanded-thumb')).toHaveCount(0);

  await expand(page, 1000203);
  await page.locator('#pc1000203').evaluate(node => node.classList.add('deleted'));
  await expect(page.locator('#p1000203 .expanded-thumb')).toHaveCount(0);
});
