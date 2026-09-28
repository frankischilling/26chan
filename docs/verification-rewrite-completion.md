# Consolidated rewrite verification

This batch builds on `70022ed58b4b19d1599409a9f22bdda714d99d4b` and combines
[legacy deletion/report routes](legacy-actions.md),
[local dates and custom navigation](native-display.md), and
[single-frame GIF input](gif-media.md) in one integration branch.

## Local results

The initial four legacy-form parser tests passed on Windows, and the first
persisted deletion test passed against the disposable PostgreSQL database.
Later report and attachment cases extend those checks and require the complete
current-source run below.

Seven GIF decoder tests passed on Windows, including byte-for-byte fixture
reproduction and the bounded mutation test. All eight native-display core/DOM
cases passed after rebuilding the pinned filter, quote-feature and image bundles.

An earlier Linux workspace run stopped at the media-reader browser test because
no Linux Node executable was installed. Its error was OS permission denied while
spawning Node. The test was retained. Node 24.14.0 was then installed from the
official archive after verifying its published SHA-256, and the pinned Linux
Playwright browser was installed for a complete rerun.

The final combined Rust/database, browser, visual and native guest results are
pending. No earlier snapshot or merged PR's checks certify this branch.

## Remaining acceptance work

The catalog and server-link review retained #82 and #165 where public evidence
does not establish the missing server rules. The historical Windows failures in
#139 and #142 have not been assigned an invented root cause or fixed by weakening
their assertions. Their diagnostic coverage remains active.

Full reference parity remains tracked in #6. Production processing, deployed
identities and network/storage rules, hardware-authenticator and operator
qualification, recovery evidence and independent review remain launch
requirements in #5 and [readiness](readiness.md). This batch does not enable
production uploads or publish a production release.
