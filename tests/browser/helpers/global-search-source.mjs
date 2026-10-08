import { readFileSync } from 'node:fs';
import vm from 'node:vm';

export const source = JSON.parse(readFileSync(new URL('../../fixtures/global-search-source.json', import.meta.url), 'utf8'));

// Execute the unchanged pinned methods. Only their DOM/history/exec boundary is stubbed.
export function sourceSearch(hash, boards = ['a', 'g'], text = source.text) {
  const field = { value: '' };
  const selector = { selectedIndex: 0, options: ['', ...boards].map(value => ({ value })) };
  const location = { hash, href: `https://search.example/globalsearch.php${hash}` };
  const context = vm.createContext({ window: { location, history: { replaceState() {} } },
    $: { id: id => id === 'js-sf-qf' ? field : selector } });
  vm.runInContext(`var Search = { pageSize: 10, maxPages: 10, ${text} exec: function() {} };`, context);
  try { context.Search.initFromURL(false); }
  catch (error) { if (error.name === 'URIError') return null; throw error; }
  return { query: context.Search.query, board: context.Search.board, offset: context.Search.offset };
}

export const hashCases = [
  '', '#', '#owned', '#owned/g/2', '#/owned', '#/owned/g/10', '#/owned/all/2',
  '#/owned/missing/3', '#//g/2', '#/owned/g/2/ignored',
  '#/two%20words/g/2', '#/a%2Fb%252Fc+plus/g/2', '#/%F0%9F%98%80/g/2',
  '#/%', '#/%E0%A4%A', '#/%ED%A0%80', '#/%FF',
  ...[509, 510, 511, 512, 513].map(n => `#/${'x'.repeat(n)}`),
  `#/${'😀'.repeat(255)}`, `#/${'😀'.repeat(255)}x`,
  `#/${'%41'.repeat(170)}`, `#/${'%41'.repeat(171)}`,
  `#/${'x'.repeat(510)}%`,
  ...['', '0', '1', '2', '10', '11', '-1', '-0', '2.9', '10.9', '1e1', '2e0',
    '0x2', '0b10', '0o2', '+2', ' 2 ', '2junk', '2%20', 'Infinity', '-Infinity',
    'NaN', '2147483648', '4294967296', '4294967298', '-4294967294', '9007199254740991']
    .map(page => `#/owned/g/${page}`),
];
