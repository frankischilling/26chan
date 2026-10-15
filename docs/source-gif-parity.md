# Animated GIF source comparison

Issue [#203](https://github.com/frankischilling/26chan/issues/203) remains open. The legacy `gif-v3` pipeline preserves animation through a bounded neutral protocol. The new `gif-v7` integration retains validated source GIF bytes, applies conditional metadata cleanup and returns a JPEG thumbnail. End-to-end qualification is still pending.

## Cleanup decisions

The active upload path calls `strip_gif_extra_data` from `lib/postfilter.php`, then reads the resulting size and calculates the public MD5. The older `cleanup_uploaded_file` helper has different thresholds and must not replace this call path.

The active helper reads `gifsicle --sinfo`. It requests `--no-comments --no-extensions` when the report contains a frame comment or an extension count, or when reported input bytes minus the sum of compressed frame sizes is at least 51,200. Otherwise, the original file is retained. The loop-count report alone does not request cleanup. An end-comment report also does not match the frame-comment expression, though its bytes can still reach the overhead threshold.

`fixtures/source-gif-cleanup.json` captures the exact function and 14 literal decision cases. `scripts/test-gif-cleanup-reference.py` executes that function in an isolated PHP namespace with command doubles. It verifies the requested commands and return decisions; it does not run gifsicle or establish encoded output equivalence. The source file SHA-256 is `d0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207`.

Gifsicle's reported compressed frame size includes the minimum-code-size byte, data sub-block length bytes, compressed data and the terminating zero block. This was checked in the [upstream 1.96 source](https://github.com/kohler/gifsicle/tree/v1.96), in `src/gifread.c` and `src/support.c`. The original application does not identify its deployed gifsicle version. A pinned replacement can establish reproducible current output, but cannot establish historical encoded-byte identity by itself.

## First-frame thumbnails

The source uses `ImageCreateFromGIF`, then the same OP/reply JPEG quality, board geometry and theme background choices as PNG images. It passes the logical-screen dimensions returned by `GetImageSize` to the resampler.

The captured libgd 2.3.3 baseline has an important partial-frame behavior: the decoded image has the first image descriptor's dimensions, and its left/top offset does not position that image on a full logical-screen canvas. The subsequent resample still receives the logical-screen dimensions. Compositing the first rectangle onto a transparent logical screen would therefore produce different pixels for these cases.

`fixtures/gd-gif-thumbnails.json` records 12 independently decoded synthetic GIF cases covering full and partial first frames, transparency and downsampling. Run `scripts/extract-gd-gif-thumbnail-reference.py --check` with libgd 2.3.3 to reproduce them. These are pre-JPEG pixels from a named baseline, not a claim about the unknown original deployment's GD version.

## Remaining work

- Preserve validated input bytes when cleanup is unnecessary, and qualify the configured cleanup path when it is required.
- Produce source-compatible JPEG thumbnails and carry their format through approval, serving, API and native presentation.
- Bind board/thread context and verify final admission, including GIF-only boards and reply limits.
- Compare timing, transparency, disposal and loop semantics using independent frame observations, including malformed and resource-limit controls.
- Qualify the complete isolated intake-to-post path, deletion and JavaScript-disabled playback before closing the issue.

On October 15, 2026, the owner approved a separate GIF profile with a 2 GiB VM and 2.5 GiB host memory cap. It keeps one active job, no network interface, a 45-second VM lifetime, a 20-second decoder CPU budget and a 1.5 GiB address-space limit. GIF-v3 and the other existing profiles keep their own limits.

The new candidate validator admits canvases up to 10,000 by 10,000, at most 512 frames and at most 256 MiB of decoded indexed pixels across all frames. Logical-canvas pixels multiplied by frame count must also stay within 256 MiB; tiny partial frames cannot conceal an unbounded full-canvas rendering workload. It decompresses every frame, retaining only the first frame's thumbnail raster. The upload and retained GIF each have an 8 MiB limit; JPEG thumbnails have a 1 MiB limit. Plain-text rendering, user-input frames, ambiguous loop extensions, reserved control values, trailing bytes and malformed LZW data remain rejected. These bounded restrictions can reject unusual source-accepted files and must be covered by rejection tests with healthy controls.

The fixed cleanup executable is built from official gifsicle 1.96 source, SHA-256 `fd23d279681a6dfe3c15264e33f344045b3ba473da4d19f49e67a50994b077fb`. Run `scripts/media/build-gif-runtime.sh ARCHIVE NEW_OUTPUT_DIRECTORY`; install its `gifsicle` and `COPYING` under root-owned `/usr/lib/26chan-gif/`. The collector verifies version, ownership, ELF size and the limited C runtime dependency set. `build-initramfs.py --gif-runtime` creates a separate image, selected by the explicit `gif_initramfs` configuration entry. Missing artifacts fail closed.

Cleanup runs with fixed arguments, cleared environment, private filenames and a 15-second deadline. A nonzero exit is rejected, unlike the original PHP helper's weaker return-value test. The resulting GIF is fully validated again before release. A complete manifest binds retained bytes and their MD5/SHA-256 to the JPEG thumbnail and immutable board/thread context. No GIF mobile derivative is created.

Migration 0154 admits only complete `gif-v7` manifests with JPEG thumbnails. Historical GIF-v3 rows retain their old constraints and PNG thumbnails. GIF attachments are rejected on the source SWF upload board; implementing that board's SWF feature remains tracked separately.
