import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';

const source = await readFile(new URL('../../apps/public/client/native-quick-reply.js', import.meta.url), 'utf8');
const clear = /  function clearAttachment\(\) \{\n[^]*?\n  \}/.exec(source)?.[0];
const cancelListener = source.split('\n').find(line => line.includes("uploadCancel.addEventListener('click'"));
const shiftListener = source.split('\n').find(line => line.includes("uploadInput.addEventListener('click'"));

// Execute the actual registered event handlers, not a parallel implementation.
// The drawing controller owns metadata clearing and retained-canvas semantics;
// cancelInlineUpload alone only retires the capability.
for (const route of ['button', 'shift-click']) test(`QR ${route} clears drawing metadata through the drawing Clear contract`, async () => {
  assert.ok(clear && cancelListener && shiftListener, 'actual QR attachment handler wiring exists');
  const calls = [], handlers = {};
  const drawing = { hasData: true, fileHidden: true, blocked: true, canvasRetained: true,
    pending() { return this.hasData; },
    async clear() {
      calls.push('drawing-clear'); this.hasData = false; this.fileHidden = false; this.blocked = false; return true;
    },
  };
  const context = { qrDrawing: drawing, cancelInlineUpload: async () => { calls.push('file-cancel'); return true; },
    uploadCancel: { addEventListener: (_, handler) => { handlers.button = handler; } },
    uploadInput: { addEventListener: (_, handler) => { handlers['shift-click'] = handler; } },
  };
  vm.runInNewContext(`${clear}\n${cancelListener}\n${shiftListener}`, context, { timeout: 100 });
  let prevented = false;
  handlers[route]({ shiftKey: true, preventDefault: () => { prevented = true; } });
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(calls, ['drawing-clear']); assert.equal(drawing.fileHidden, false); assert.equal(drawing.blocked, false);
  assert.equal(drawing.canvasRetained, true); if (route === 'shift-click') assert.equal(prevented, true);
  handlers[route]({ shiftKey: true, preventDefault() {} });
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(calls, ['drawing-clear', 'file-cancel'], 'ordinary files retain the existing cancellation flow');
  context.qrDrawing = null; handlers[route]({ shiftKey: true, preventDefault() {} });
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(calls, ['drawing-clear', 'file-cancel', 'file-cancel'], 'boards without drawing still cancel files');
});

test('ordinary file-picker clicks do not cancel an attachment', () => {
  let handler; const calls = [];
  vm.runInNewContext(`${clear}\n${shiftListener}`, { qrDrawing: { pending: () => true, clear: () => calls.push('drawing') },
    cancelInlineUpload: () => calls.push('file'), uploadInput: { addEventListener: (_, value) => { handler = value; } },
  }, { timeout: 100 });
  handler({ shiftKey: false, preventDefault: () => calls.push('prevented') }); assert.deepEqual(calls, []);
});
