# File-blocking parity

File blocking is unfinished. Trusted publication records the original upload's
MD5 in a private receipt. Posting rules do not yet enforce those hashes.
Existing SHA-256 integrity checks remain unchanged. MD5 is only a compatibility key for
matching operator-owned rules; it is not an approval or integrity proof.

## Original and processed bytes

`InputSnapshot::raw_md5()` hashes the same immutable input bytes as its SHA-256.
`PairedInputSnapshot::raw_image_md5()` hashes only the uploaded image component,
after validating the frame and component descriptors. It excludes the replay
and framing. Neither accessor reopens a path or hashes worker output.

The existing `SourceProvenance.md5` describes processed bytes according to its
source profile. It cannot be repurposed as the untouched input digest. Durable
raw-input metadata has its own private, lease-bound receipt and exact retry
checks. Old uploads remain unknown rather than acquiring an inferred digest.
Approved media views do not expose the private metadata.

## Durable receipt

`media.input_receipts` belongs to the reserved asset, not its temporary queue job.
It stores the original SHA-256, byte length, MD5, and component kind. Queue cleanup
does not remove it; deleting the asset cascades to the receipt.

The trusted host records it under the publication filesystem lock after reserving
the asset and before installing files. The writer locks the job before the asset
and requires the current processing lease, matching byte length, and a pending
asset. Drawing receipts additionally match the stored image-component SHA-256.
Existing processed-source provenance, when present, must agree on input identity.

The production approval paths require an existing exact receipt within their
approval transaction. They cannot fill a missing receipt. A failed or uncertain
write leaves the reservation recoverable and prevents file installation; an
uncertain approval preserves files for reconciliation. Exact approved-receipt
verification is allowed after queue cleanup, but historical approvals cannot
acquire a new receipt. Manual publication APIs without an original snapshot
continue to leave original identity unknown.

Both private functions have fixed search paths and the migration owner. Runtime
publication receives only function execution, with no direct receipt-table grant.
Public posting, staff, intake, and media-reader roles cannot read the table.
Posting roles have a separate capability-scoped metadata function; it is not an
HTTP endpoint and does not add fields to public media views.

## Posting preflight

Before content-rule evaluation, every attachment passes a read-only preflight under
the board, reply-thread, and media-job locks. It checks the live upload capability,
approval, board media limits, reply image quota, PDF/video consistency and the
stored still-image context. Staff identity comes from validated posting authority,
not upload flags. The final proof handoff and one-use attachment consume remain
in place.

A file that cannot attach must not create a filename ban or other committed
content-filter effect. Rejection and quiet-success paths therefore revalidate the
metadata and capability expiry immediately before committing their effects.
The separate automatic-range hit also revalidates at the refusal decision,
before rolling back unrelated posting effects. Preflight itself does not attach
the file or consume its capability.

The scoped result includes normalized MD5, nullable original MD5, and nullable
original-digest eligibility. `/p/` JPEG and PDF are ineligible. The capcode-only
JPEG strip on `/p/` changes normalized bytes but does not enable original fallback.
A generic PNG output can have originated from a different format; without a
retained-format manifest or the PNG source profile, eligibility stays unknown.
Later blacklist decisions must distinguish unknown eligibility from false.

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
tests qualify snapshot capture. Database tests separately exercise receipt
privacy, lease expiry, exact retries, approval refusal without a receipt,
historical backfill refusal, paired-component checks, and cleanup. Controlled
TLS dispatch tests compare frozen raw-input digests independently of the
processed-source digest. They do not qualify blacklist enforcement.
