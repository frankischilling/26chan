import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
const root = new URL('../../', import.meta.url);
const reference = JSON.parse(await readFile(new URL('apps/public/tests/fixtures/board-flags.json', root)));
const assets = JSON.parse(await readFile(new URL('docs/source-board-flag-assets.json', root)));
const staffLimits = await readFile(new URL('apps/staff/static/post-limits.js', root), 'utf8');

test('staff board changes retain source menu order and clear unavailable choices', async ({ page }) => {
  await page.route('**/__test/staff-post-limits.js', route => route.fulfill({
    status: 200, contentType: 'text/javascript', body: staffLimits,
  }));
  await page.goto('/flags/pol');
  await page.evaluate(reference => {
    const board = document.createElement('select'); board.id = 'staff-board';
    const choices = document.createElement('template'); choices.id = 'staff-flag-choices';
    for (const [kind, table] of Object.entries(reference.tables)) {
      const policy = document.createElement('option'); policy.value = kind;
      policy.dataset.flagType = kind;
      policy.dataset.flags = table.selector_order.join(' ');
      policy.dataset.commentLimit = kind === 'pol' ? '1500' : '16000';
      board.append(policy);
      for (const code of table.selector_order) {
        const option = document.createElement('option');
        option.value = code; option.textContent = table.selector[code]; option.dataset.flagType = kind;
        choices.content.append(option);
      }
    }
    const flag = document.createElement('select'); flag.id = 'staff-flag';
    const prior = document.createElement('option'); prior.value = 'AN'; flag.append(prior);
    const comment = document.createElement('textarea'); comment.id = 'staff-comment';
    document.body.append(board, flag, choices, comment);
  }, reference);
  await page.addScriptTag({ url: '/__test/staff-post-limits.js' });
  await expect(page.locator('#staff-flag')).toHaveValue('AN');
  await expect(page.locator('#staff-comment')).toHaveAttribute('maxlength', '3000');
  await page.selectOption('#staff-flag', 'CM');
  for (const kind of ['mlp', 'lgbt', 'test', 'pol']) {
    await page.selectOption('#staff-board', kind);
    const menu = await page.locator('#staff-flag option').evaluateAll(nodes => nodes.map(node => [node.value, node.textContent]));
    expect(menu).toEqual([['', 'None'], ...reference.tables[kind].selector_order.map(code => [code, reference.tables[kind].selector[code]])]);
    await expect(page.locator('#staff-flag')).toHaveValue('');
    await expect(page.locator('#staff-comment')).toHaveAttribute('maxlength', kind === 'pol' ? '3000' : '32000');
  }
  await page.selectOption('#staff-flag', 'AN');
  await page.selectOption('#staff-board', 'mlp');
  await expect(page.locator('#staff-flag')).toHaveValue('AN');
  await expect(page.locator('#staff-flag option:checked')).toHaveText('Anon');
});

for (const device of [
  { name: 'desktop', viewport: { width: 1280, height: 900 }, deviceScaleFactor: 1 },
  { name: 'mobile density 2', viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
]) {
  test.describe(device.name, () => {
    test.use(device);
    for (const kind of ['pol', 'mlp', 'lgbt']) {
      test(`${kind} preserves every source label, sprite coordinate and menu position`, async ({ page }) => {
        const errors = []; page.on('pageerror', error => errors.push(error.message));
        await page.goto(`/flags/${kind}`);
        const table = reference.tables[kind];
        const header = device.isMobile ? '.postInfoM' : '.postInfo';
        const sprites = await page.locator(`${header} .bfl`).evaluateAll(nodes => nodes.map(node => {
          const style = getComputedStyle(node);
          return { title: node.title, className: node.className, width: style.width, height: style.height,
            position: style.backgroundPosition, image: style.backgroundImage, visible: node.getBoundingClientRect().width > 0 };
        }));
        expect(sprites).toHaveLength(table.selector_order.length);
        for (let index=0; index<sprites.length; index++) {
          const code = table.selector_order[index], sprite = sprites[index];
          expect(sprite.title).toBe(table.display[code]);
          expect(sprite.className).toBe(`bfl bfl-${code.toLowerCase()}${kind === 'pol' ? '' : ` bfl-type-${kind}`}`);
          const geometry = assets[kind][0].geometry[code.toLowerCase()];
          expect([sprite.width, sprite.height, sprite.position]).toEqual([geometry.width, geometry.height, geometry.position]);
          expect(sprite.visible).toBe(true);
          expect(sprite.image).toContain(`/static/flags/${kind === 'pol' ? 'board-flags.2.png' : `${kind}-flags.${kind === 'mlp' ? 3 : 1}.png`}`);
        }
        const choices = await page.locator('#flag option').evaluateAll(nodes => nodes.map(node => [node.value, node.textContent]));
        expect(choices).toEqual([['0','None'], ...table.selector_order.map(code => [code, table.selector[code]])]);
        // Feed the actual production post HTML through the released worker.
        const parsed = await page.evaluate(async kind => {
          const html = await (await fetch(`/flags/${kind}`)).text();
          const document = new DOMParser().parseFromString(html,'text/html');
          const posts = [...document.querySelectorAll('article.postContainer')].map(node => ({
            no: node.id.slice(2), file_deleted: false, html: node.outerHTML,
          }));
          const raw = { version: 2, tail_size: 0, tail_id: null, board: `flag${kind}`, thread: '1002000', closed: false,
            archived: false, sticky: false, replies: posts.length-1, images: 0, posts };
          const worker = new Worker('/static/native-filter.v1.js',{type:'module'});
          try {
            return await new Promise((resolve,reject) => {
              const timer=setTimeout(()=>reject(new Error('flag worker timeout')),5000);
              worker.onmessage=event=>{clearTimeout(timer);resolve({status:event.data.status, count:event.data.snapshot?.posts.length});};
              worker.onerror=event=>{clearTimeout(timer);reject(new Error(event.message));};
              worker.postMessage({kind:'updater-snapshot',raw:JSON.stringify(raw),context:{origin:location.origin,board:`flag${kind}`,thread:'1002000',mediaOrigin:'http://localhost:3004'}});
            });
          } finally { worker.terminate(); }
        },kind);
        expect(parsed).toEqual({status:'ok',count:sprites.length});
        expect(errors).toEqual([]);
      });
    }
    test('unavailable test artwork has no political sprite fallback', async ({ page }) => {
      await page.goto('/flags/test');
      expect(reference.boards.find(row=>row.board==='test').enabled).toBe(false);
      await expect(page.locator('.postInfo .bfl').first()).toHaveCSS('background-image','none');
      expect(assets.test[0].status).toBe(404);
      expect((await page.request.get('/static/flags/test-flags.1.png')).status()).toBe(404);
    });
  });
}
