# Single-frame GIF input

The disposable guest accepts GIF87a and GIF89a with one full-canvas frame.
Decoded pixels pass through the existing stopped-disk validator and PNG
normalizer. The public form accepts a GIF filename, and the published full image
and thumbnail retain the existing normalized-media contract.

## Decoder boundary

`gif` 0.14.2 and its locked LZW dependency `weezl` 0.1.12 are confined to
`board-media-guest`. The direct dependency disables default features and enables
`std`. No codec is added to the public, staff, dispatcher, publisher or reader
runtime. `scripts/check-media-parser-dependencies.py` checks normal/build
dependency reachability from credential-bearing applications, confirms the guest
contains its decoders, and proves an injected forbidden edge is rejected.

The guest classifies bounded input by its signature. It accepts at most 8 MiB
and a logical screen from 1 through 1,024 pixels per dimension. The decoder
requests RGBA output, checks frame consistency and LZW termination, rejects
unknown blocks, and bounds each decoded frame at 4 MiB. This decoder limit is
not a total heap limit; the [guest address-space and VM limits](firecracker.md)
remain necessary.

The first frame must cover the complete logical screen with zero offsets and an
exactly sized RGBA buffer. A second frame causes rejection before any pixels are
returned. Interlaced rows and transparent palette entries are decoded. Comments
and application metadata do not enter the output protocol.

Animation and partial-frame composition are unsupported. A future compositor
needs explicit limits for frames, aggregate decoded pixels, disposal state and
processing time, plus native guest and publication tests. Larger images, other
formats, original downloads and full original-site media behavior remain in #6.

The host receives the existing `IBRGBA01` header and pixels. It revalidates the
entire fixed output disk, including dimensions, padding and EOF, before issuing
approval. No input path, filename or success claim authorizes publication.
Production uploads remain disabled under the launch requirements in #5.

## Owned fixtures and checks

The [six GIF fixtures](../tests/media/fixtures/gif/README.md) contain constant
indexed pixels and reproduce byte-for-byte with the pinned Rust encoder.
The guest tests cover exact opaque output, transparency, metadata removal,
interlacing, both signatures, the largest accepted canvas, every truncation of a
small fixture, malformed input, animation, partial frames and bounded mutations.
A healthy decode follows rejection cases.

Native VM tests exercise static, transparent and interlaced input through the
real guest, require complete output and zero padding, and check orphan cleanup.
The intake service test verifies that excessive, animated, partial and truncated
GIFs create no approval or published file, then publishes a healthy GIF through
the same dispatcher. The real browser attachment workflow includes a GIF upload,
processing status, receipt consumption, normalized display and owned cleanup.

```text
cargo test -p board-media-guest --locked
python3 scripts/check-media-parser-dependencies.py
```

Rebuild the guest initramfs before running the [native harness](firecracker.md).
The consolidated result is recorded in
[rewrite completion verification](verification-rewrite-completion.md). Local
decoder tests cannot qualify a deployed processing tier or its operational
identities, storage and network policy.
