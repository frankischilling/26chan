# Animated GIF source comparison

Issue [#203](https://github.com/frankischilling/26chan/issues/203) remains open. The existing `gif-v3` pipeline preserves animation through a bounded neutral protocol, but its normalized GIF bytes and PNG thumbnails do not yet match the source cleanup and presentation contract.

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

The existing animation resource profile remains unchanged during this reference work. The PNG/JPEG worker's larger profile does not implicitly authorize larger GIF jobs.
