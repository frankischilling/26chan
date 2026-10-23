// The browser orchestrates existing capability-scoped endpoints. No decoder,
// persisted credential or automatic retry of a possibly committed post lives here.
export function createUploadPostSession({ upload, check, cancel, send, wait, notify = () => {} }) {
  let file = null, receipt = null, uncertain = false, busy = false;
  function stopped(signal) { if (signal?.aborted) throw new Error('Upload stopped.'); }
  function available() {
    if (busy) throw new Error('A request is already in progress.');
    if (uncertain) throw new Error('Check the board before posting again. The previous post may have succeeded.');
  }
  async function submit(selected, fields, ready, signal) {
    available();
    if (!selected) throw new Error('Choose one SWF file.');
    if (file && file !== selected) throw new Error('Cancel the previous file before choosing another.');
    busy = true;
    try {
      stopped(signal);
      if (!receipt) {
        file = selected; notify('Uploading file…');
        receipt = await upload(file, signal);
      }
      stopped(signal);
      for (let attempt = 0; receipt.state !== 'approved' && attempt < 60; attempt++) {
        if (!['queued', 'processing'].includes(receipt.state)) throw new Error('File processing failed. Cancel this file before trying another.');
        notify('Processing file…');
        await wait(signal); stopped(signal);
        receipt = await check(receipt, signal); stopped(signal);
      }
      if (receipt.state !== 'approved') throw new Error('The file is still processing. Submit again to check its status.');
      if (!ready()) { notify('File ready. Complete the CAPTCHA, then submit.'); return null; }
      stopped(signal);
      notify('Posting…');
      let result;
      try {
        result = await send({ ...fields(), upload_id: receipt.upload_id, upload_capability: receipt.upload_capability }, signal);
      } catch (error) { uncertain = true; throw error; }
      if (!result.error) { receipt = null; file = null; }
      return result;
    } finally { busy = false; }
  }
  async function discard(signal) {
    available(); busy = true;
    try {
      if (receipt) await cancel(receipt, signal);
      receipt = null; file = null; notify('');
    } finally { busy = false; }
  }
  return { submit, discard, state: () => ({ busy, uncertain, hasFile: file !== null, approved: receipt?.state === 'approved' }) };
}
