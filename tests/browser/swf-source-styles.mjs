// Compare rendered upload-table properties with the pinned source CSS in the
// same browser. This covers the component, not the surrounding page layout.
import assert from 'node:assert/strict';
import {readFile,mkdir,writeFile} from 'node:fs/promises';
const source=JSON.parse(await readFile(new URL('../../fixtures/swf-style-reference.json',import.meta.url),'utf8'));
function properties(table) {
  const nodes=[table,...table.querySelectorAll('tr,td,.name,.subject,a')];
  return nodes.map(node=>{
    const css=getComputedStyle(node),rect=node.getBoundingClientRect();
    const value={tag:node.tagName,text:node.textContent,rect:[rect.width,rect.height].map(value=>Math.round(value*100)/100)};
    for(const key of ['color','backgroundColor','fontSize','fontWeight','fontStyle','lineHeight','paddingTop','paddingRight','paddingBottom','paddingLeft','textAlign','whiteSpace','textOverflow','maxWidth','overflowX','overflowY','overflowWrap','textDecorationLine'])value[key]=css[key];
    value.fontFamily=css.fontFamily.toLowerCase().replaceAll('"',"'").replace(/\s+/g,' ');
    for(const side of ['Top','Right','Bottom','Left']) {
      const width=css[`border${side}Width`];value[`border${side}`]=width==='0px'?[width]:[width,css[`border${side}Style`],css[`border${side}Color`]];
    }
    return value;
  });
}
export async function assertUploadTableStyles(page,context,theme,label='desktop') {
  assert.ok(['desktop','mobile'].includes(label));
  const css=source.themes[theme]?.css;assert.equal(typeof css,'string');assert.ok(!css.includes('</')&&!css.includes('url('));
  const table=page.locator('table.flashListing');
  const html=await table.evaluate(table=>table.outerHTML);
  const reference=await context.newPage();
  await reference.route('**/*',route=>route.abort());
  try {
    await reference.setContent(`<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Owned source table styles</title><style>html{-webkit-text-size-adjust:100%}${css}</style>${html}`);
    const expected=reference.locator('table.flashListing');
    const actual=await table.evaluate(properties),referenceValues=await expected.evaluate(properties);
    await mkdir('test-results/swf-style',{recursive:true});
    await writeFile(`test-results/swf-style/${theme}-${label}.json`,JSON.stringify({actual,source:referenceValues},null,2));
    assert.deepEqual(actual,referenceValues,`${theme}/${label}: upload-table source CSS`);
    await table.locator('tr').nth(1).evaluate(row=>row.classList.add('highlightPost'));
    await expected.locator('tr').nth(1).evaluate(row=>row.classList.add('highlightPost'));
    assert.deepEqual(await table.evaluate(properties),await expected.evaluate(properties),`${theme}/${label}: source oldest-row highlight`);
    await table.locator('tr').nth(1).evaluate(row=>row.classList.remove('highlightPost'));
    await expected.locator('tr').nth(1).evaluate(row=>row.classList.remove('highlightPost')); 
  } finally {
    await mkdir('test-results/swf-style',{recursive:true});
    await table.screenshot({path:`test-results/swf-style/${theme}-${label}.png`}).catch(()=>{});
    await reference.locator('table.flashListing').screenshot({path:`test-results/swf-style/${theme}-${label}-source-css.png`}).catch(()=>{});
    await reference.close();await page.bringToFront();
  }
}
