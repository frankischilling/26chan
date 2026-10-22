import { createHash } from 'node:crypto';
import path from 'node:path';

// Independent upload modes can share a source file, board and socket peer.
// Their comments must differ without clearing history or bypassing admission.
export function uploadFixtureTag(source, flags) {
  return createHash('sha256')
    .update(JSON.stringify([path.basename(source), [...flags].sort()]))
    .digest('hex').slice(0, 12);
}
