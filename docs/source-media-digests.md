# Source-processed media digests

`board_media::source_digest` is an unused PNG-only prerequisite for source
compatibility. It computes the MD5 of the bytes retained by the supplied
`4chan-old/lib/postfilter.php:306-439` transformation. The original posting
path hashes the processed file in `imgboard.php:5259`.

No runtime calls this scanner. Intake, guest decoding, dispatch, publication,
database records, attachment admission and public responses do not use its
result. It adds no duplicate-image rejection, migration or original-file
download. Existing `EncodedOutput` MD5 and SHA-256 values still describe the
newly encoded PNG output. This prerequisite does not establish full media or
duplicate-admission parity.

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
scan cannot authorize publication. A future integration must use immutable
intake bytes held by the trusted caller, preserve the existing isolated
decoder and publication boundary, and never accept a guest- or client-asserted
digest.

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

## Test scope

The module's ten unit tests cover independent golden digests, retained chunk
order and bytes, dropped chunks, case matching, the APNG typo, IDAT splitting,
trailing data, truncation, limits and explicit unavailable formats. Several
fixtures intentionally test framing without claiming decoder admission.
These tests do not exercise a runtime integration or establish source-wide
media parity.
