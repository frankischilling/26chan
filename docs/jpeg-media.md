# JPEG input in isolated media jobs

[Issue #50](https://github.com/frankischilling/26chan/issues/50) adds JPEG input to
the development attachment workflow. The guest selects PNG or JPEG from bounded
input bytes. Public intake, staff, dispatch, promotion and media serving do not
link the JPEG decoder. Browser `accept` values are usability hints, not security
checks. Unsupported input can reach quarantine but cannot gain approval without
a valid bounded pixel response.

The JPEG decoder accepts baseline and progressive RGB/grayscale images and
converts CMYK/YCCK to RGB. Input is at most 8 MiB, dimensions are 1 through 1024
on each axis, progressive decoding permits at most 64 scans, and output is the
existing 16-byte `IBRGBA01` header followed by at most 4 MiB of RGBA pixels.
JPEG alpha is always 255. The pinned decoder uses strict mode and scalar safe
paths. Input must end with an EOI marker; this marker check is not an exhaustive
container validator. Decoder-ignored bytes never enter the output protocol.

The decoder still runs as guest UID/GID 1000 with the existing external CPU,
memory, process, file-size and wall-clock ceilings. The VM receives only its job
input and bounded output disks, without networking, application credentials,
host shares or management interfaces. A decoder failure cannot fall back to
publishing the upload. Native tests must establish these properties for the
rebuilt guest; local library tests alone do not.

## Download and compatibility policy

All published full images and thumbnails remain newly encoded PNGs. Public
metadata, MD5 and byte counts describe that normalized output. No original-file
download route exists. Source EXIF, ICC and other metadata are not published;
EXIF orientation is not applied and ICC profiles are not used for color
management. Some camera images can therefore appear rotated or have different
colors. JPEG quality loss already present in the source is not recoverable.

These are explicit normalization/containment exceptions under E-001 and E-008,
not proof of original-site image processing parity. GIF, WebM, PDF and SWF input
remain unsupported. The legacy-looking thumbnail URL still serves truthful
`image/png`, not JPEG bytes. The strict size/dimension/scan limits can reject
otherwise viewable files. Production uploads remain disabled under issue #5;
the pinned public-reference and exact visual gaps remain under issue #6.

## Tests and current evidence

Synthetic JPEG fixtures contain constant test pixels, not photographs or user
metadata. Their [provenance and hashes](../tests/media/fixtures/jpeg/README.md)
identify the pinned generator. Local Windows checks passed:

```text
cargo test -p board-media-guest --locked --jobs 1
cargo test -p board-public --all-features --locked --jobs 1
cargo clippy -p board-media-guest -p board-public --all-targets --all-features --locked --jobs 1 -- -D warnings
python scripts/check-media-parser-dependencies.py
cargo audit
python -m py_compile tests/media/test_vm.py tests/media/test_intake_service.py tests/media/public_upload_fixture.py
node --check tests/browser/public-upload.mjs
npm run test:media-visual
```

The guest suite passed 3 PNG and 8 JPEG tests: baseline/progressive RGB and gray,
CMYK/YCCK conversion, discarded synthetic APP1 metadata, excess dimensions,
truncation, unsupported formats, maximum dimensions, excess progressive scans
with a healthy control, exact fixture regeneration, and 64 bounded mutation
cases. All 42 public tests passed, including the real no-JavaScript browser
flow with trusted synthetic worker output. Clippy and the dependency graph
controls passed; the advisory check found no known issues. These results do not
constitute a complete transitive security review.

Only the four board/thread desktop/mobile baselines affected by the PNG/JPEG
upload label were regenerated and inspected. Forms remain readable, filenames
wrap, image proportions are unchanged, and there is no horizontal overflow.
All eight media comparisons then passed without updates. With
`VISUAL_FIXTURE_SERVER=1`, `npm run test:visual` and
`npm run test:archive-visual` passed all nine unchanged comparisons. Browser,
font and platform pins are recorded in [visual verification](verification-media-visuals.md).

The owned Linux CI profile rebuilds the guest and runs four actual VM cases
(baseline, progressive, grayscale and CMYK). HTTP intake qualification adds
complete PNG, baseline-JPEG and progressive-JPEG browser uploads through real
authenticated dispatch, posting, separate-origin reads, deletion and physical
cleanup. Excess-dimension and truncated JPEG cases must fail with no approval,
no output identifier and no surviving VM. These new native cases await CI;
local WSL was unavailable. Existing containment tests and their healthy
connectivity controls remain required. No production guest rollout is included.

This slice adds no migration or runtime credential. Rebuild both guest binaries
and the initramfs using [the isolated execution procedure](firecracker.md), then
repeat native qualification before selecting the guest in a development
dispatcher. An older guest will reject JPEG input; there is no host-side fallback.

The Linux-musl cross-build also passed locally with
`CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld` and
`cargo build -p board-media-guest --bins --example containment-probe --target x86_64-unknown-linux-musl --locked --jobs 1`.
This verifies compilation, not execution of the new guest.

PR run [34736728273](https://github.com/frankischilling/26chan/actions/runs/34736728273)
on `e0335f9` passed Linux guest qualification, including the four JPEG variants,
and native dispatch, but failed during the second public browser upload. A local
two-upload regression reproduced the exact failure: after deletion redirects
to the board, two legitimate posts contain "File deleted", making the browser's
unscoped locator ambiguous. The browser now verifies the marker inside the exact
post identified by the API. The new two-upload test failed before that change
and passed afterward on owned Windows PostgreSQL 16.15. Focused all-feature
browser-test clippy passed with warnings denied. Native multi-format intake,
negative JPEG cases and complete corrected-head CI still require a fresh run;
no application behavior or assertion was disabled.
