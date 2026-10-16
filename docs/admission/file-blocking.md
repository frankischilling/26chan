# File-blocking parity

File blocking is unfinished. The snapshot layer records the original upload's
MD5, but the publication database and posting rules do not yet use it. Existing
SHA-256 integrity checks remain unchanged. MD5 is only a compatibility key for
matching operator-owned rules; it is not an approval or integrity proof.

## Original and processed bytes

`InputSnapshot::raw_md5()` hashes the same immutable input bytes as its SHA-256.
`PairedInputSnapshot::raw_image_md5()` hashes only the uploaded image component,
after validating the frame and component descriptors. It excludes the replay
and framing. Neither accessor reopens a path or hashes worker output.

The existing `SourceProvenance.md5` describes processed bytes according to its
source profile. It cannot be repurposed as the untouched input digest. Durable
raw-input metadata needs its own private, lease-bound fields and exact retry
checks. Old uploads must remain unknown rather than acquire an inferred digest.
Approved media views must not expose the private metadata.

## Required posting behavior

The original posting path has two distinct checks:

- Before content filters, the prior-ban upload check uses the normalized file
  digest. It skips `/b/`, blocks after three matching qualifying bans, and includes
  the work-safe-only template when appropriate. It has no capcode exemption.
- After content and network filters, the exact file blacklist checks the
  normalized digest first. Only a miss can fall back to a different original
  digest. A trusted active capcode bypasses this second check.

Original-digest fallback is format-dependent. The original implementation records
it before WebM/MP4 validation and PNG/GIF stripping; JPEG records it only when
EXIF stripping is enabled. PDF does not supply an original digest. Capturing a
raw digest for provenance does not authorize using it as fallback for every
format.

Exact matches can reject, issue an automatic ban, or enter the DMCA warning and
repeat-attempt path. The DMCA path precedes quiet fake-success handling. Those
side effects, trusted ban identity, attachment locking, and rule selection still
need implementation and transactional tests. The original query has no ordering
for overlapping matches, so a new scope priority must not be presented as an
established source behavior.

## Snapshot qualification

The media suite checks a fixed `abc` MD5 vector, unchanged snapshots after a path
or inode is overwritten, and identical image digests across absent or changed
replays. It also checks that a frame digest differs from the image digest. These
tests qualify capture only, not publication persistence or blacklist enforcement.
