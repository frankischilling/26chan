import assert from 'node:assert/strict';
import test from 'node:test';
import { notificationKind, notificationIcon } from '../../apps/public/client/native-tracked-quotes.js';

test('reply markers outrank highlights, highlights outrank new posts, and ordinary replies retain unread priority', () => {
  for (const current of [null, 'new', 'hl', 'rep']) {
    assert.equal(notificationKind(current, { you: true, highlighted: true, unread: 4 }), 'rep');
    assert.equal(notificationKind(current, { you: false, highlighted: true, unread: 4 }), current === 'rep' ? 'rep' : 'hl');
    assert.equal(notificationKind(current, { you: false, highlighted: false, unread: 4 }), current);
  }
  assert.equal(notificationKind(null, { you: false, highlighted: false, unread: 0 }), 'new');
});

test('notification states select only fixed local worksafe and non-worksafe resources', () => {
  for (const worksafe of [true, false]) {
    for (const [kind, suffix] of [['new', 'newposts'], ['rep', 'newreplies'], ['hl', 'newfilters'], ['dead', 'deadthread']]) {
      assert.equal(notificationIcon(worksafe, kind), `/static/notifications/favicon-${worksafe ? 'ws' : 'nws'}-${suffix}.ico`);
    }
    for (const kind of ['../../secret', 'https://foreign.example/a.ico', 'constructor', '__proto__', '', undefined]) {
      assert.equal(notificationIcon(worksafe, kind), null);
    }
  }
  assert.equal(notificationIcon(true, null), '/static/notifications/favicon-ws.ico');
  assert.equal(notificationIcon(false, null), '/static/notifications/favicon.ico');
});
