# Source-processed media digests

`board_media::source_digest` computes the PNG-only MD5 of the bytes retained by the supplied
`4chan-old/lib/postfilter.php:306-439` transformation. The original posting
path hashes the processed file in `imgboard.php:5259`.

The async coordinator records private source provenance from the immutable
input snapshot it dispatches. The trusted coordinator independently scans that
same snapshot; guest replies and client fields cannot supply provenance.
This binds the receipt to dispatched input bytes. It is not cryptographic
attestation that the guest's output pixels came from those bytes. Isolated
decoding, output validation and approval remain separate requirements.

Existing `EncodedOutput` MD5 and SHA-256 values still describe newly encoded
output. Public, staff and media-reader projections keep those output meanings.
Source provenance adds no duplicate-image rejection or original-file download,
and does not establish full media or duplicate-admission parity.

## PNG v1 contract

The scanner hashes the eight-byte PNG signature, followed by complete retained
chunks in their original order. It compares chunk types case-insensitively
against `IHDR`, `PLTE`, `IDAT`, `IEND`, `tRNS`, `gAMA`, `sBIT`, `pHYs`, `sRGB`,
`bKGD`, `tIME`, `cHRM` and `iCCP`. Each retained chunk includes its original
length, type spelling, payload and CRC bytes. It does not decompress or
re-encode pixels. Changing IDAT segmentation can therefore change the digest
even when compressed payload and pixels are unchanged.

Processing ends at the first complete IEND chunk. Bytes after it are excluded
from the digest, but count toward the source-byte limit. Before IEND, `acTL`
is rejected case-insensitively. The PHP source's `$chink_type` typo leaves
`fcTL` and `fdAT` outside that rejection; the scanner drops them like other
unlisted chunks. Reproducing that behavior does not admit APNG input.

The trusted caller supplies source, retained-output and chunk-payload byte
limits, each no larger than the current 8 MiB intake ceiling. Every chunk,
including a dropped chunk, must fit its payload limit and have complete data
and CRC framing. The scanner rejects missing IEND and truncated dropped
chunks, where the PHP EOF/seek paths can succeed. These are deliberate
fail-closed differences. The compatibility contract applies to valid admitted
PNGs within the configured limits, not arbitrary malformed inputs.

This is a bounded framing scanner, not an image validator. It does not check
CRCs, chunk semantics, required chunk order or decoded pixels. A successful
scan cannot authorize publication. Integration uses immutable intake bytes
held by the trusted caller and preserves the isolated decoder and publication
boundary. It never accepts a guest- or client-asserted digest.

The result has private fields and exposes a 16-byte binary MD5, retained byte
count and `PngV1` profile. MD5 is legacy comparison data, not an integrity or
authorization check. The scanner returns no source bytes.

## JPEG and GIF remain unavailable

JPEG and GIF signatures produce explicit unavailable errors. There is no
fallback to a raw-upload hash or encoder-output hash. The supplied source
uses `jpegtran -copy none` for configured JPEG stripping and conditional
`gifsicle` inspection/rewriting for GIFs. Exact tool versions, builds and
source-processed sample outputs are still missing. Until that provenance and
byte-level fixtures are available, neither transformation has a compatible
digest implementation. Existing JPEG and single-frame GIF decoder support
does not supply this evidence.

## Storage and recovery

Migration 0112 adds an optional, complete five-field tuple: input SHA-256,
input length, source profile, retained length and source MD5. Partial tuples
are invalid. A new tuple requires a live processing job with the same lease
and input length. Existing receipts remain NULL, as do manual publication and
unsupported JPEG/GIF provenance. No digest is inferred from encoded output.
Tuple values cannot be changed or backfilled, including by the migration
identity through ordinary DML. The migration adds no grants or read fields.

Receipt reuse requires equality of the entire tuple, including NULL versus
present provenance. The CLI has no same-lease resume command. After an
uncertain result, inspect durable approval and job state first. If approval
committed, preserve that receipt. Otherwise let the lease expire, reconcile
and requeue under a fresh lease. Do not weaken tuple equality to force a retry.

## Test scope

The module's ten unit tests cover independent golden digests, retained chunk
order and bytes, dropped chunks, case matching, the APNG typo, IDAT splitting,
trailing data, truncation, limits and explicit unavailable formats. Several
fixtures intentionally test framing without claiming decoder admission.
The migration qualification script uses a private disposable PostgreSQL 16
cluster to check populated 0111 upgrades, transactional rollback, retained
rows, output checksums, ACLs and read projections. It also checks tuple shape,
bounds, lease binding, immutability and denied runtime access.

Local qualification on October 7, 2026 passed 16 media library checks,
11 storage checks, the restricted-role provenance case and all nine media-admin
checks. The full store run passed 236 checks plus 24 repeated concurrent
admission checks. Formatting, strict all-target/all-feature workspace Clippy
and the parser dependency guard passed.

Populated 0111-to-0112 upgrade, rollback and fresh role bootstrap checks passed
on PostgreSQL 16.15 using an owned loopback TCP cluster. Hosted CI runs the
privileged Unix-socket scripts. Exact-head hosted checks remain pending; these
local results do not establish source-wide media parity.

The async tests cover short/excess reads, pathname replacement, mutation after
snapshotting, cancellation, malformed framing, invalid guest output, expired
or replaced leases, and unsupported-format NULL provenance. A TLS fixture
compares the exact dispatched PNG bytes with an independent persisted golden
tuple. The controlled fixture does not qualify an external decoder.
