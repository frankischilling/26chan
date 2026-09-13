import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';

const contract = JSON.parse(await readFile(new URL('../fixtures/catalog-search-cases.json', import.meta.url), 'utf8'));
test('pinned browser search operators and case mapping match the shared Rust cases', async ({ page }) => {
  const actual = await page.evaluate(contract => {
    const escape = new RegExp('(' + contract.escape_characters.map(character => '\\' + character).join('|') + ')', 'g');
    return contract.cases.map(entry => new RegExp(entry.query.replace(escape, '\\$1'), contract.flags).test(entry.text));
  }, contract);
  expect(actual).toEqual(contract.cases.map(entry => entry.matches));
});
