import test from 'node:test';
import assert from 'node:assert/strict';
import {
  darkThemeStylesheetHref,
  nativeDarkTheme,
  nativeThreadLayout,
  sourceMobileLayout,
} from '../../apps/public/static/native-layout.v1.js';

test('source layout precedence keeps compact desktop-only and lets centered win on mobile', () => {
  assert.equal(nativeThreadLayout({}, false), null);
  assert.equal(nativeThreadLayout({ compactThreads: true }, false), 'compact');
  assert.equal(nativeThreadLayout({ centeredThreads: true }, false), 'centered');
  assert.equal(nativeThreadLayout({ compactThreads: true, centeredThreads: true }, false), 'compact');
  assert.equal(nativeThreadLayout({ compactThreads: true, centeredThreads: true }, true), 'centered');
  assert.equal(nativeThreadLayout({ compactThreads: true }, true), null);
  assert.equal(nativeThreadLayout({ centeredThreads: true }, true), 'centered');
  assert.equal(nativeThreadLayout({ compactThreads: true, centeredThreads: true, disableAll: true }, false), null);
});

test('mobile policy uses the exact public 480px opt-out string semantics', () => {
  assert.equal(sourceMobileLayout(false, null), false);
  assert.equal(sourceMobileLayout(true, null), true);
  assert.equal(sourceMobileLayout(true, 'false'), true);
  assert.equal(sourceMobileLayout(true, 'TRUE'), true);
  assert.equal(sourceMobileLayout(true, 'true'), false);
});

test('dark theme follows requested preference only while the extension is enabled', () => {
  assert.equal(nativeDarkTheme({}), false);
  assert.equal(nativeDarkTheme({ darkTheme: true }), true);
  assert.equal(nativeDarkTheme({ darkTheme: true, disableAll: true }), false);
  assert.equal(nativeDarkTheme(null), false);
});

test('Tomorrow override is finite, same-origin and preserves only the work-safe selector', () => {
  const page = 'https://boards.test/demo/thread/123';
  assert.equal(darkThemeStylesheetHref('/static/theme.css', page), '/static/theme.css?theme=tomorrow');
  assert.equal(
    darkThemeStylesheetHref('/static/theme.css?worksafe=true', page),
    '/static/theme.css?worksafe=true&theme=tomorrow',
  );
  assert.equal(
    darkThemeStylesheetHref('https://boards.test/static/theme.css?worksafe=false', page),
    '/static/theme.css?worksafe=false&theme=tomorrow',
  );

  for (const href of [
    'https://evil.test/static/theme.css?worksafe=true',
    '//evil.test/static/theme.css',
    '/static/theme.css?worksafe=yes',
    '/static/theme.css?worksafe=true&worksafe=false',
    '/static/theme.css?worksafe=true&other=x',
    '/static/theme.css?theme=photon',
    '/static/other.css?worksafe=true',
    '/static/theme.css?worksafe=true#fragment',
    'https://user:pass@boards.test/static/theme.css',
  ]) assert.equal(darkThemeStylesheetHref(href, page), null, href);
});
