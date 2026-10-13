import { test, expect } from '@playwright/test';
test('release player works under fixed CSP and owns pause, rewind, speed, gapless and close',async({page})=>{
  const failures=[];page.on('pageerror',error=>failures.push(error.message));
  await page.goto('/player.html');await page.getByRole('link',{name:'View',exact:true}).click();
  const dialog=page.getByRole('dialog',{name:'Oekaki replay'});
  await expect(dialog).toBeVisible();await expect(dialog.getByRole('status')).toHaveText('Replay finished.');
  await expect(dialog.locator('#tegaki-canvas')).toHaveAttribute('width','24');
  await dialog.getByRole('button',{name:'Rewind',exact:true}).click();
  await expect(dialog.getByRole('button',{name:'Play',exact:true})).toBeEnabled();
  await dialog.getByRole('checkbox', {name:'Gapless'}).uncheck();
  await dialog.getByRole('button',{name:'Faster',exact:true}).click();await expect(dialog.locator('#tegaki-replay-speed-lbl')).toHaveText('2.0');
  await dialog.getByRole('button',{name:'Play',exact:true}).click();
  await dialog.getByRole('button',{name:'Pause',exact:true}).click();
  await expect(dialog.getByRole('button',{name:'Play',exact:true})).toBeVisible();
  await page.keyboard.press('Escape');await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('link',{name:'View',exact:true})).toBeFocused();
  await page.getByRole('link',{name:'View',exact:true}).click();await expect(dialog.getByRole('status')).toHaveText('Replay finished.');
  await dialog.getByRole('button',{name:'Close',exact:true}).click();await expect(dialog).toHaveCount(0);
  expect(failures).toEqual([]);
});
test('close during acquisition prevents late playback and a missing recording shows a bounded failure',async({page})=>{
  let resolve;const held=new Promise(done=>{resolve=done;});
  await page.route('**/replay/i/123.ibr',async route=>{await held;try{await route.continue();}catch{}});
  await page.goto('/player.html');await page.getByRole('link',{name:'View',exact:true}).click();
  const dialog=page.getByRole('dialog',{name:'Oekaki replay'});
  await expect(dialog.getByRole('status')).toHaveText('Loading replay…');
  await dialog.getByRole('button',{name:'Close',exact:true}).click();resolve();await page.unroute('**/replay/i/123.ibr');
  await expect(dialog).toHaveCount(0);
  await page.getByRole('link',{name:'Missing',exact:true}).click();
  await expect(dialog.getByRole('status')).toContainText('Could not load');
  await expect(dialog.getByRole('button',{name:'Play',exact:true})).toBeDisabled();
  await dialog.getByRole('button',{name:'Close',exact:true}).click();await expect(dialog).toHaveCount(0);
});

test('replay zoom uses source bounds without resizing the native bitmap and resets on reopen', async ({page}) => {
  await page.goto('/player.html');
  const open = page.getByRole('link', {name: 'View', exact: true});
  await open.click();
  const dialog = page.getByRole('dialog', {name: 'Oekaki replay'});
  await expect(dialog.getByRole('status')).toHaveText('Replay finished.');
  const canvas = dialog.locator('#tegaki-canvas'), label = dialog.locator('#tegaki-zoom-lbl');
  await expect(label).toHaveText('100%');
  await dialog.getByRole('button', {name: 'Zoom out', exact: true}).click();
  await expect(label).toHaveText('50%');
  await expect(dialog.getByRole('button', {name: 'Zoom out', exact: true})).toBeDisabled();
  await expect(canvas).toHaveCSS('width', '12px');
  for (let i = 0; i < 5; i++) await dialog.getByRole('button', {name: 'Zoom in', exact: true}).click();
  await expect(label).toHaveText('1600%');
  await expect(dialog.getByRole('button', {name: 'Zoom in', exact: true})).toBeDisabled();
  await expect(canvas).toHaveCSS('width', '384px');
  await expect(canvas).toHaveAttribute('width', '24');
  await dialog.getByRole('button', {name: 'Close', exact: true}).click();
  await open.click();
  await expect(dialog.getByRole('status')).toHaveText('Replay finished.');
  await expect(label).toHaveText('100%');
  await expect(canvas).toHaveCSS('width', '24px');
});

test('source replay chrome shows elapsed time, tools and layers without touching a retained painter', async ({page}) => {
  await page.goto('/player.html');
  await page.evaluate(() => {
    const retained = document.createElement('div'); retained.id = 'tegaki';
    retained.textContent = 'retained unsaved painter'; document.body.append(retained);
  });
  await page.getByRole('link', {name:'View', exact:true}).click();
  const dialog = page.getByRole('dialog', {name:'Oekaki replay'});
  await expect(dialog.getByRole('status')).toHaveText('Replay finished.');
  await expect(dialog.locator('#tegaki-tools-grid .tegaki-tool-btn')).toHaveCount(8);
  await expect(dialog.locator('#tegaki-layers-grid .tegaki-layers-cell')).toHaveCount(1);
  await expect(dialog.locator('#tegaki-replay-now-lbl')).toHaveText('01:00');
  await expect(dialog.locator('#tegaki-replay-end-lbl')).toHaveText('01:00');
  await expect(dialog.locator('#tegaki-size')).toBeDisabled();
  await expect(dialog.locator('#tegaki-colorpicker')).toBeDisabled();
  await dialog.getByRole('button', {name:'Rewind', exact:true}).click();
  await expect(dialog.locator('#tegaki-replay-now-lbl')).toHaveText('00:00');
  await dialog.getByRole('button', {name:'Close', exact:true}).click();
  expect(await page.evaluate(() => document.getElementById('tegaki').textContent)).toBe('retained unsaved painter');
});

for (const id of ['pencil-pressure','brush-dynamics','layer-history-order']) {
  test(`source replay display state matches the original UI: ${id}`, async ({page}) => {
    await page.goto('/');
    const result = await page.evaluate(async id => {
      const {FULL_CASES} = await import('/full-cases.mjs');
      const {encodeOwnedCase} = await import('/cases.mjs');
      const bytes = encodeOwnedCase(FULL_CASES.find(row => row.id === id));
      const {renderReference} = await import('/full-generated/reference.js');
      renderReference(bytes);
      function read(root) {
        const find = id => root.querySelector(`#${id}`);
        return {
          tool: root.querySelector('.tegaki-tool-active').id,
          size: find('tegaki-size').value, alpha: find('tegaki-alpha').value, flow: find('tegaki-flow').value,
          color: find('tegaki-colorpicker').value,
          modes: [...find('tegaki-toolmode-bar').querySelectorAll('.tegaki-sw-btn-a')].map(node=>node.id),
          layers: [...find('tegaki-layers-grid').children].map(row => {
            const canvas = row.querySelector('canvas');
            return {id:row.id,active:row.classList.contains('tegaki-layers-cell-a'),selected:row.classList.contains('tegaki-layers-cell-s'),
              visible:!!row.querySelector('.tegaki-ui-cb-a'), pixels:[...canvas.getContext('2d').getImageData(0,0,canvas.width,canvas.height).data]};
          }),
        };
      }
      const expected = read(document);
      const {mountReplayPlayer} = await import('/static/native-replay/player.v1.js');
      const host = document.createElement('article'); host.className='postContainer';
      const message = document.createElement('blockquote'); message.className='postMessage';
      const link = document.createElement('a'); link.href='#replay-123'; link.dataset.drawingReplay='123';
      message.append(link);host.append(message);document.body.append(host);
      const player = mountReplayPlayer({board:'i',mediaOrigin:location.origin,acquire:async()=>bytes.buffer});
      link.click();
      try {
        const shadow = document.getElementById('native-replay-player').shadowRoot;
        await new Promise((resolve,reject) => {
          const deadline = performance.now()+5000;
          function check() {
            const text=shadow.querySelector('[role=status]').textContent;
            if(text==='Replay finished.') return resolve();
            if(text.startsWith('Could not') || performance.now()>deadline) return reject(new Error(text || 'Playback deadline'));
            setTimeout(check,10);
          }
          check();
        });
        return {expected,actual:read(shadow)};
      } finally { player.dispose(); }
    }, id);
    expect(result.actual).toEqual(result.expected);
  });
}

test('visibility auto-resume respects manual pause, rewind and close', async ({page}) => {
  await page.goto('/player.html'); await page.getByRole('link',{name:'View',exact:true}).click();
  const dialog=page.getByRole('dialog',{name:'Oekaki replay'});
  await expect(dialog.getByRole('status')).toHaveText('Replay finished.');
  await dialog.getByRole('button',{name:'Rewind',exact:true}).click();
  await expect(dialog.getByRole('button',{name:'Play',exact:true})).toBeEnabled();
  await dialog.getByRole('checkbox',{name:'Gapless'}).uncheck();
  const visibility = state => page.evaluate(state => {
    Object.defineProperty(document,'visibilityState',{configurable:true,value:state});
    document.dispatchEvent(new Event('visibilitychange'));
  },state);
  await dialog.getByRole('button',{name:'Play',exact:true}).click(); await visibility('hidden');
  await expect(dialog.getByRole('button',{name:'Play',exact:true})).toBeVisible();
  await visibility('visible'); await expect(dialog.getByRole('button',{name:'Pause',exact:true})).toBeVisible();
  await dialog.getByRole('button',{name:'Pause',exact:true}).click();
  await visibility('hidden'); await visibility('visible');
  await expect(dialog.getByRole('button',{name:'Play',exact:true})).toBeVisible();
  await dialog.getByRole('button',{name:'Play',exact:true}).click(); await visibility('hidden');
  await dialog.getByRole('button',{name:'Rewind',exact:true}).click(); await visibility('visible');
  await expect(dialog.getByRole('button',{name:'Play',exact:true})).toBeEnabled();
  await dialog.getByRole('button',{name:'Close',exact:true}).click(); await visibility('visible');
  await expect(dialog).toHaveCount(0);
});
