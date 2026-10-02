# Admission normalization

Issue [#212](https://github.com/frankischilling/26chan/issues/212) remains open.
The domain module supplies the three normalization projections used by the
configured content rules. It is not yet connected to persisted rules, posting,
hit records or ban actions. No private operator rules are imported or invented.

The reference is the supplied revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`, specifically the hash-pinned
`normalize_ascii`, `strip_zerowidth` and `normalize_text` functions in
`lib/postfilter.php`. The extractor evaluates only those three pure bodies.
It does not load the application, includes, database, session class or RPC code.
It requires PHP's ICU and UTF-8 extensions and records PHP 8.3.6, PCRE2 10.42
and ICU/data 74.2. The independent JSON contains 1,849 synthetic inputs and
four outputs per input: ASCII lowercase, case-preserving transliteration,
normalized matching text and zero-width removal.

The Rust wrapper uses the same fixed ICU transform ID and preserves its order:
Any-Latin, NFD, nonspacing-mark removal, NFKC, then Latin-ASCII. Bracketed
`dot` replacement precedes that transform. Lowercasing changes only ASCII
bytes, as PHP `strtolower` does. Matching text keeps only the source's ASCII
whitelist. Zero-width removal reuses the already qualified fixed source
predicate. The commented-out short-URL callback is not active.

The native handle is private and exclusively owned. The wrapper exposes no
cloning, registration, custom transformation, native-pointer or filter API.
Construct, use and drop it inside one synchronous operation before awaiting.
The reviewed call surface is the fixed factory, string transformation and
destructor; this is not a claim that the entire binding is sound. Native ICU
text processing belongs to the application's trust base and never decodes media.

## Security differences and bounds

Raw input is capped at 131,072 bytes and returned output at 524,288 bytes.
Non-ASCII processing additionally permits at most 18,000 input scalars and
65,536 compatibility-normalized scalars. A streaming ICU4X NFKC projection
estimates that work before native processing. Its output is never used for
matching. The complete original ICU 74 transform still produces the result.
ASCII uses its identical bounded projection directly.

These work caps are a security exception for expensive native expansion.
An initial test that transformed an oversized repeated compatibility ligature
before checking output took 35.15 seconds on Linux and 50.30 seconds on
Windows. The guarded checks reject that workload before ICU. Tests separately
exercise compatibility expansion, the native scalar ceiling and an accepted
18,000-scalar accented input. No wall-clock timing assertion or retry is added.
These counters do not establish a native allocation sandbox or a universal
execution deadline. Integration still needs bounded scheduling and failure
behavior before the module can serve configured posting rules.

Construction, input, work and output failures return explicit errors. The
source's fallback when ICU construction fails is not used; an unavailable
normalizer must not turn configured admission into an allow decision.

## Build and qualification

The crate pins rust_icu 5.8.0 with its supplied ICU 74 declarations, disables
bindgen/default features, and requires the ICU 74 ABI through Cargo's environment
configuration. Ubuntu 24.04 development and deployment use OS-maintained ICU
packages; `libicu-dev` supplies linker files. Apply OS security updates and
requalify source projections when the native dependency or data changes.

For Windows development/CI, `scripts/install-windows-icu.ps1` downloads the
official ICU 74.2 MSVC archive into the checkout's ignored `.local` directory,
checks its pinned SHA-512 and the three runtime DLL SHA-256 digests, and sets
the process linker/runtime paths. The archive digest was recorded from the
official HTTPS release asset. The release's published checksum manifests omit
that Windows archive; no upstream checksum match is claimed. The frozen
Windows test distribution is not a maintained production deployment profile.

On October 2, 2026, all four projections match every extracted case on Linux
and Windows with Rust 1.94.0. The final three normalization tests pass, including
128 bounded Unicode property cases. Linux's complete domain suite passes all
125 tests without failures or ignored tests. The original transform-chain
prototype had 174 corpus mismatches and was kept outside the application.
Windows CI now runs the normalization checks explicitly; that changed workflow
has not yet run. Full workspace and integrated admission qualification remain
required for this slice.

```text
php scripts/extract-admission-normalization-reference.php SOURCE crates/domain/tests/fixtures/admission-normalization.json --check
cargo test -p board-domain --locked
cargo clippy -p board-domain --all-targets --all-features --locked -- -D warnings
```
