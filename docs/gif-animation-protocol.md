# GIF animation protocol

The `gif-v3` transport carries a GIF upload to the disposable guest and returns
uncompressed `IBGIF001` frame data. It does not replace the `image-v1` or
`paired-v2` contracts. The public upload coordinator has not been switched to
this transport yet; durable GIF approval and serving remain to be integrated.

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
These storage primitives still require durable approval from their caller.

These output bytes do not establish the source's post-cleanup checksum. The
supplied PHP conditionally invokes an external `gifsicle` binary whose version
and implementation are absent from the supplied tree. Source cleanup/checksum
qualification, board admission, approved `.gif` routes, deletion, browser playback
and JavaScript-disabled presentation remain required for issue #203.

References: [GIF89a specification](https://www.w3.org/Graphics/GIF/spec-gif89a.txt)
and the pinned [weezl buffer API](https://docs.rs/weezl/0.1.12/weezl/decode/struct.Decoder.html).
