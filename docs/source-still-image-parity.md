# PNG and JPEG parity

Issue [#202](https://github.com/frankischilling/26chan/issues/202) remains open. The image-presentation fixture and geometry helpers cover source sizing decisions. A separate still-image worker now produces retained PNG/JPEG candidates and JPEG derivatives. Its tests and larger resource profile are awaiting hosted qualification. Authenticated posting-policy binding, host validation and publication, mobile-file lifecycle and end-to-end parity remain unfinished.

## Source policy

The pinned `imgboard.php` has SHA-256 `caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445`. Its `make_thumb` function uses the board's `MAX_W`/`MAX_H` for opening posts and `MAXR_W`/`MAXR_H` for replies. All imported boards use 250 by 250 and 125 by 125, except `/vg/`, whose opening-post bound is 200 by 200. Smaller images keep their dimensions. Larger images use the smaller floating-point scale and floor each output dimension. Extremely narrow inputs can produce a zero dimension; the helper rejects that result.

Opening-post thumbnails use JPEG quality 50; replies use quality 40. Transparent images are resampled over the board's default background: `#ffffee` for opening posts and `#f0e0d6` for replies, or `#eef2ff` and `#d6daf0` when `DEFAULT_BURICHAN` is enabled. The source then invokes `jpegoptim --strip-all`.

Mobile resizing is enabled on `/aco/`, `/e/`, `/h/`, `/k/`, `/m/`, `/v/`, `/co/`, `/fa/`, `/tg/`, `/tv/`, `/b/`, `/pol/`, `/hc/`, `/vg/` and `/vt/`. `resize_mobile_image` accepts `.jpg` and `.png`. PNGs at or below 524,288 processed bytes have no mobile derivative. Otherwise, a derivative requires both a side greater than 1,024 and an area greater than 524,288 pixels. The longer output side is 1,024; the other is rounded to the nearest integer, with positive halves rounded upward. It writes a quality-80 JPEG named `<tim>m.jpg`. Unlike thumbnails, this function does not explicitly fill a theme-colored background.

Every imported board strips JPEG EXIF except `/p/`. The capcode-specific strip switch is enabled on all 82 boards, including `/p/`. The source invokes `jpegtran -copy none`, and recomputes the public MD5 after cleanup. The pre-cleanup MD5 has a separate blacklist purpose. Public hashes must describe the bytes actually published.

PNG cleanup retains the original framing and bytes of allowed chunks, discards other chunks and stops at the first IEND. Its allowed types are IHDR, PLTE, IDAT, IEND, tRNS, gAMA, sBIT, pHYs, sRGB, bKGD, tIME, cHRM and iCCP, compared without case sensitivity. Decoder validation remains necessary before publication. The existing retained-byte digest scanner does not establish image validity or permission to publish a file.

## Qualification boundaries

The supplied application source does not pin the deployed GD, jpegtran or jpegoptim versions. Geometry and configured cleanup decisions can be compared directly with captured source arithmetic. Exact historical JPEG encoder bytes require additional version evidence; a quality setting alone does not establish byte identity. Tests must distinguish pixel behavior, retained metadata, encoded-file checksums and the input checksum.

The current worker's address-space limit is 96 MiB and its default VM memory is 128 MiB. Supporting 10,000 by 10,000 images with full-frame decoders requires a separate resource review. On October 14, 2026, the owner approved a separate still-image profile capped at 2 GiB of VM memory and 2.5 GiB of host memory, one job at a time, with hard time limits. Implementation and runtime qualification are still in progress; this approval is not evidence that the profile has passed VM tests. This profile must not change the limits for existing image, replay, GIF, PDF or video jobs. The runner must retain single-job exclusion, process-group termination, fixed output sizes, no network interface, unprivileged decoding and post-termination output validation.

Run the source checks with:

```sh
python3 scripts/extract-image-presentation-reference.py /path/to/4chan-old --check --php /path/to/php
python3 scripts/test-image-presentation-reference.py
python3 scripts/test-image-board-policy-reference.py
cargo test -p board-media-guest --test image_geometry --locked
```

The PHP check executes only the captured arithmetic and format/size conditions. Its 21 expected cases are literal values independent of the Rust implementation. The board extractor also preserves the applied 0045, 0064 and 0132 migration output.
