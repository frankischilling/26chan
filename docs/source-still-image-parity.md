# PNG and JPEG parity

The PNG/JPEG implementation for [issue #202](https://github.com/frankischilling/26chan/issues/202) retains cleaned files in their accepted format and produces JPEG thumbnails and optional mobile images. Immutable board/thread/capcode context follows an upload through isolated processing, publication and posting. Public API fields, download routes and native presentation describe the approved retained bytes.

The sections below distinguish captured source decisions, executed checks and compatibility limits. Deployment qualification remains separate from the local behavior and frontend contract.

## Source policy

The pinned `imgboard.php` has SHA-256 `caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445`. Its `make_thumb` function uses the board's `MAX_W`/`MAX_H` for opening posts and `MAXR_W`/`MAXR_H` for replies. All imported boards use 250 by 250 and 125 by 125, except `/vg/`, whose opening-post bound is 200 by 200. Smaller images keep their dimensions. Larger images use the smaller floating-point scale and floor each output dimension. Extremely narrow inputs can produce a zero dimension; the helper rejects that result.

Opening-post thumbnails use JPEG quality 50; replies use quality 40. Transparent images are resampled over the board's default background: `#ffffee` for opening posts and `#f0e0d6` for replies, or `#eef2ff` and `#d6daf0` when `DEFAULT_BURICHAN` is enabled. The source then invokes `jpegoptim --strip-all`.

Mobile resizing is enabled on `/aco/`, `/e/`, `/h/`, `/k/`, `/m/`, `/v/`, `/co/`, `/fa/`, `/tg/`, `/tv/`, `/b/`, `/pol/`, `/hc/`, `/vg/` and `/vt/`. `resize_mobile_image` accepts `.jpg` and `.png`. PNGs at or below 524,288 processed bytes have no mobile derivative. Otherwise, a derivative requires both a side greater than 1,024 and an area greater than 524,288 pixels. The longer output side is 1,024; the other is rounded to the nearest integer, with positive halves rounded upward. It writes a quality-80 JPEG named `<tim>m.jpg`. Unlike thumbnails, this function does not explicitly fill a theme-colored background.

The [EXIF-comment path](exif-comments.md) records extraction before capcode-only cleanup and has its own qualification status.

Every imported board strips JPEG EXIF except `/p/`. The capcode-specific strip switch is enabled on all 82 boards, including `/p/`. The source invokes `jpegtran -copy none`, and recomputes the public MD5 after cleanup. The pre-cleanup MD5 has a separate blacklist purpose. Public hashes must describe the bytes actually published.

PNG cleanup retains the original framing and bytes of allowed chunks, discards other chunks and stops at the first IEND. Its allowed types are IHDR, PLTE, IDAT, IEND, tRNS, gAMA, sBIT, pHYs, sRGB, bKGD, tIME, cHRM and iCCP, compared without case sensitivity. Decoder validation remains necessary before publication. The existing retained-byte digest scanner does not establish image validity or permission to publish a file.

## Upload names

Source-board intake removes the four directional controls stripped by `imgboard.php`, applies its board-specific whitespace rules and balances retained right-to-left embeddings. Stored metadata stays plain text and HTML rendering escapes it once. The final ASCII-alphanumeric suffix is removed when deriving the source filename; the displayed extension comes from the approved output format. A 255-byte admitted name can therefore produce a 259-byte display name when a canonical `.png` or `.jpg` suffix is appended. The shared presentation validator permits up to 260 bytes to also accommodate `.webm`.

Control characters and over-limit names remain rejected at intake. This bounds metadata independently of decoding and prevents control characters from entering headers or rendered attributes. Filenames never select storage paths or served MIME types.

## Qualification boundaries

The supplied application source does not pin the deployed GD, jpegtran or jpegoptim versions. Geometry and configured cleanup decisions can be compared directly with captured source arithmetic. Exact historical JPEG encoder bytes require additional version evidence; a quality setting alone does not establish byte identity. Tests must distinguish pixel behavior, retained metadata, encoded-file checksums and the input checksum.

On October 14, 2026, the owner approved a separate still-image profile with 2 GiB of VM memory and a 2.5 GiB host memory cap. It runs one job at a time with no network interface, unprivileged decoding, fixed output sizes and post-termination output validation. The VM has a 45-second lifetime; the guest has a 20-second CPU budget and 1.5 GiB address-space limit. Existing image, replay, GIF, PDF and video profiles retain their own limits.

[CI run 38636503018](https://github.com/frankischilling/26chan/actions/runs/38636503018), at worker checkpoint `9d17da82`, passed every job. Its real-VM tests cover JPEG cleanup and deliberate metadata retention, a 10,000-by-10,000 PNG, geometry, cancellation, deadline enforcement and isolation. The large-image fixture recorded a host cgroup memory peak of 395,907,072 bytes, about 378 MiB. That observation applies to the tested fixture, not every accepted image. Later database, posting and frontend integration is outside that checkpoint.

The media-operation and browser lanes in [run 38646924577](https://github.com/frankischilling/26chan/actions/runs/38646924577), at `d3f0e883`, also passed. Native intake cases cover contextual PNG/JPEG input through the real worker, retained-byte checksums, JPEG thumbnail reads and `/p/` metadata retention. Browser cases cover mobile expansion, spoiler previews, original-URL projection, deletion retirement and canonical filename bounds. That run's Rust lane stopped at the JavaScript release-size guard; the image controller was subsequently reduced to fit the unchanged 16 KiB bound. Final merge eligibility depends on the complete checks attached to the PR's current head.

The resampler has a separate 65-case pixel reference captured with libgd 2.3.3. It covers seven-bit inverse alpha, opaque-background blending and fractional resizing. The Rust worker matches those captured RGB values before JPEG encoding. This records the tested GD baseline; the original deployment's version remains unknown. Regenerate or check it with `scripts/extract-gd-still-resampling-reference.py` on a host with libgd 2.3.3.

Run the source checks with:

```sh
python3 scripts/extract-image-presentation-reference.py /path/to/4chan-old --check --php /path/to/php
python3 scripts/test-image-presentation-reference.py
python3 scripts/test-upload-filename-reference.py --php /path/to/php
python3 scripts/test-image-board-policy-reference.py
cargo test -p board-media-guest --test image_geometry --locked
```

The geometry PHP check executes only the captured arithmetic and format/size conditions. A separate 17-case check executes the pinned upload-name sanitizer without the application bootstrap. Its 21 expected cases are literal values independent of the Rust implementation. The board extractor also preserves the applied 0045, 0064 and 0132 migration output.

## Runtime build and selection

The still initramfs requires the pinned libjpeg-turbo 3.2.0 cleaner. On the owned Linux worker host, download its official source archive, then run `scripts/media/build-still-runtime.sh ARCHIVE NEW_OUTPUT_DIRECTORY`. The builder verifies the fixed source SHA-256 before compiling. Install the resulting `jpegtran` as root-owned `/usr/lib/26chan-still/jpegtran`, mode 0755, with trusted root-owned parents. Keep the accompanying license with the runtime distribution.

Rebuild the guest and worker binaries and use `scripts/media/build-initramfs.py` with `--still-runtime` to create the separate image. The collector verifies the cleaner version, root ownership, bounded ELF sizes and its limited C runtime dependency set. It records file hashes in `still-runtime.json` inside the initramfs. The disposable `scripts/media/provision-test.py` procedure creates and hashes this image alongside the other profiles and records `still_initramfs` in the private runner configuration. An older configuration without that image cannot process a still job; there is no fallback to in-process decoding.

Apply migrations through 0153 before starting the corresponding services. The original development profile and authorization requirements still apply. This work does not authorize enabling uploads on an unqualified production host.

## Deliberate input restrictions

PNG animation is rejected rather than silently publishing only part of an animated container. JPEG decoding retains the existing 64-scan bound and requires an end marker; this marker check alone is not an exhaustive container validator. Both decoders run under the separate VM, CPU, allocation and deadline limits, and only a complete validated output manifest can be published. Oversized, truncated or malformed inputs do not gain a legacy fallback. Tests keep healthy-image controls after rejected inputs.

The source does not apply EXIF orientation when creating thumbnails. The still worker likewise uses stored pixel dimensions and does not rotate those pixels. Retained `/p/` JPEGs can still carry orientation metadata that a browser applies to the full image. ICC metadata is retained or stripped according to the source cleanup path; retaining a profile is not a claim that the thumbnail decoder performs color management. The captured GD pixel checks and explicit orientation tests describe the verified behavior.
