# GIF animation protocol

The `gif-v3` transport carries a GIF upload to the disposable guest and returns
uncompressed `IBGIF001` frame data. It does not replace the `image-v1` or
`paired-v2` contracts. The coordinator selects it from the sealed upload's fixed
GIF87a/GIF89a signature, then independently validates its stopped output. Runtime
media admission remains subject to the existing development and deployment
guards. The new mode passes disposable native dispatch qualification; production
activation still requires the existing operator qualification.

The guest accepts GIF87a/GIF89a, at most 8 MiB of input, a canvas up to 1024 by
1024, at most 512 frames and at most 16 MiB of decoded indexed pixels across all
frames. Each frame fits inside the canvas. It checks complete input framing,
LZW end codes, exact decoded pixel counts and palette indices. Plain-text drawing
extensions, user-input frames, duplicate loop extensions and reserved control
values fail closed. Comments and unrelated application metadata are discarded.
Recognized NETSCAPE2.0/ANIMEXTS1.0 loop counts, including an extension after a
frame, become a single numeric field.

The guest uses the pinned `gif` parser and `weezl` decoder. LZW decompression uses
a fixed output buffer with one extra byte to detect excess pixels. Complete
stream decoding avoids a reader error at a valid sub-block boundary while still
requiring an actual end code. The existing single-frame decoder remains separate.
Credentialed runtimes do not link either parser.

All neutral multibyte values are big-endian. The 32-byte header contains:

| Offset | Bytes | Value |
| --- | --- | --- |
| 0 | 8 | `IBGIF001` |
| 8 | 2 | Canvas width |
| 10 | 2 | Canvas height |
| 12 | 2 | Frame count, 1 through 512 |
| 14 | 2 | Global palette entries: 0 or a power of two from 2 through 256 |
| 16 | 2 | Global background index, or 256 for absent |
| 18 | 4 | Loop count: 0 for infinite, 1 through 65535 for finite, 65536 for absent |
| 22 | 10 | Zero reserved bytes |

The global RGB palette follows. Each frame has a 20-byte header, its active RGB
palette and exactly width times height indexed pixels:

| Offset | Bytes | Value |
| --- | --- | --- |
| 0 | 2 | Left offset |
| 2 | 2 | Top offset |
| 4 | 2 | Frame width |
| 6 | 2 | Frame height |
| 8 | 2 | Delay in hundredths of a second |
| 10 | 2 | Palette entries, a power of two from 2 through 256 |
| 12 | 2 | Transparent index, or 256 for absent |
| 14 | 1 | Disposal method, 0 through 3 |
| 15 | 1 | Zero reserved byte |
| 16 | 4 | Exact indexed pixel count |

Interlaced input is emitted in display order. The host independently checks every
field, index, rectangle, count, aggregate limit and stream completion before
constructing a validated animation. A stopped output disk is exactly 17,825,792
bytes with all-zero padding and EOF. Callers must impose an absolute deadline.

The host writes GIF89a from validated values only. It retains rectangles, local
colors, transparency, delays, disposal and loop count. It pads local palettes to
256 colors and writes 9-bit literal LZW codes, clearing the dictionary every 254
pixels. This avoids a host decoder, compression dictionary or code-width growth.
The resulting file is bounded by 20 MiB. Encoding is deterministic; SHA-256 and
the interoperability MD5 describe the actual encoded bytes. MD5 is never used
for authorization. The first-frame PNG thumbnail uses a transparent canvas and
the validated first rectangle; source thumbnail comparison is still required.

Publication accepts only the host encoder's `EncodedGif` type, installs a fixed
`{id}.gif` name under the existing publication lock and checks hash, size and
contents on replay. A reservation cannot switch between PNG and GIF while its
file exists. GIF readers check the same fixed name and a 20 MiB limit; thumbnails
retain their PNG format and limit. Cleanup removes only the known output names.
Migration 0123 records the output format in the immutable reservation and
restricted approval views. PNG retains its 5 MiB limit; GIF has a 20 MiB limit
because bounded literal encoding can exceed the input file's size. Thumbnails
remain PNG. Approval, lease completion and cleanup use the existing transaction
and publication lock. Source GIF cleanup provenance remains unknown.

Both opaque and board/timestamp routes require the requested suffix to match
the approved format. They check actual file size and SHA-256 before a conditional
response. GIF responses use `image/gif`; PNG thumbnails use `image/png`.
Pages, JSON, RSS and staff links take their format from approved metadata rather
than the submitted filename. Image controls and the updater accept only the fixed
PNG and GIF paths on the configured media origin. Spoiler links remain usable
without JavaScript, and GIF URL retirement closes active image copies.

These output bytes do not establish the source's post-cleanup checksum. The
supplied PHP conditionally invokes an external `gifsicle` binary whose version
and implementation are absent from the supplied tree. Source cleanup/checksum
qualification, board admission and thumbnail comparison remain required for
issue #203. The controlled browser tests verify visible animation and retirement;
source timing/disposal/loop qualification remains incomplete. The native dispatch
check uses actual Firecracker output
for static, transparent, interlaced, animated and partial-canvas GIFs through the
nonroot gateway and coordinator, root broker, fenced approval and checked reader.
It also retains revoked-certificate, invalid-output, expired/replaced-lease and
live-VM cancellation checks. The native HTTP qualification sends an independent
red/blue animation through public HTML upload, authenticated intake, the actual
guest, durable attachment and separate reader. A browser with scripts disabled
checks GIF and thumbnail responses, JSON, visible frame changes, deletion and
removal of both files while retaining the attachment tombstone.

References: [GIF89a specification](https://www.w3.org/Graphics/GIF/spec-gif89a.txt)
and the pinned [weezl buffer API](https://docs.rs/weezl/0.1.12/weezl/decode/struct.Decoder.html).
