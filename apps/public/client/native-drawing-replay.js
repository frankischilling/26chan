// A client-side upload hint only. The isolated guest and independent host must
// validate the complete replay before any storage or playback approval.
export const DRAWING_REPLAY_LIMITS = Object.freeze({
  bytes: 8 * 1024 * 1024,
  inflatedBytes: 18 * 1024 * 1024,
});

export async function drawingReplayFile(blob) {
  if (!(blob instanceof Blob) || blob.size <= 12 || blob.size > DRAWING_REPLAY_LIMITS.bytes) {
    throw new Error('The drawing replay exceeds the supported upload bounds.');
  }
  // Read only the fixed header here. Never inflate untrusted replay bytes in
  // the board page, and never infer validation from a filename or MIME type.
  const bytes = new Uint8Array(await blob.slice(0, 12).arrayBuffer());
  if (bytes.length !== 12 || bytes[0] !== 0x54 || bytes[1] !== 0x47
    || bytes[2] !== 0x4b || bytes[3] !== 1) {
    throw new Error('The drawing recorder did not produce a supported replay.');
  }
  const inflated = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(4);
  if (inflated === 0 || inflated > DRAWING_REPLAY_LIMITS.inflatedBytes) {
    throw new Error('The drawing replay exceeds the supported decoded bounds.');
  }
  return new File([blob], 'tegaki.tgkr', { type: 'application/octet-stream' });
}
