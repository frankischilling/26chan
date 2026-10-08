// Regenerate the standard-API multipart fixture for Rust transport tests.
// This uses Node's FormData/Blob/Request implementation; it is not browser QA.
import { writeFileSync } from 'node:fs';
const form = new FormData();
for (const [key, value] of [['resto', '19'], ['png_bytes', '3'], ['replay_bytes', '6']]) {
  form.append(key, value);
}
form.append('upfile', new Blob(['png'], { type: 'image/png' }), 'tegaki.png');
form.append('replay', new Blob(['replay'], { type: 'application/octet-stream' }), 'tegaki.tgkr');
const request = new Request('http://localhost/unused', { method: 'POST', body: form });
const root = new URL('../../apps/public/tests/fixtures/paired-intake/', import.meta.url);
writeFileSync(new URL('formdata.bin', root), new Uint8Array(await request.arrayBuffer()));
writeFileSync(new URL('content-type.txt', root), request.headers.get('content-type'));
