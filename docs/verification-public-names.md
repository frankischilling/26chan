# Ordinary public names and trip encoding

Ordinary public posting follows the supplied name-preparation block rather
than hashing UTF-8 password bytes directly. `#password` saves an empty display
name with `!ozOtJW9BFA`; JSON omits its `name` field. `Name#かみ` uses CP932 bytes
and produces `!v/ClhaTjaY`. Existing saved names and trip strings are retained.
This is part of issue #208; staff preparation and privileged exceptions remain
unfinished.

## Source and independent fixtures

The supplied checkout revision is
`545b7812d1849f7958d914950c91fdbbe38f6b22`. Both extractors reject changed
source files before evaluating selected code.

| File | SHA-256 |
| --- | --- |
| `imgboard.php` | `caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445` |
| `lib/postfilter.php` | `d0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207` |

Extraction uses PHP 8.3.6, mbstring with substitution character 63, PCRE 10.42
and ICU 74.2. The runtime rewrite needs none of these PHP or codec libraries.

`extract-trip-encoding-reference.php` scans all 1,112,064 valid Unicode
scalars. ASCII converts unchanged; 9,278 non-ASCII scalars have CP932 mappings;
every other scalar becomes `?`. None of the converted scalar bytes are rejected
by Shift_JIS `htmlspecialchars`. Forty-eight multi-scalar cases independently
record conversion and escaping, with 47 PHP DES results and the empty input
that skips hashing in the actual name caller. The fixture and generated Rust
table must match extraction byte for byte. Rust tests compare every scalar and
the multi-scalar byte sequences with that fixture.

`extract-public-name-reference.php` evaluates selected pure helpers, four
audited preprocessing statements and the bounded name block ending before
cookies. This checkpoint qualified five synthetic policies covering ordinary
`/g/`, `/a/`, `/b/`, code spacing and `/jp/` SJIS spacing. Its 335 cases record display text, escaped
text, delimiter decisions, normal trips, secure-trip selection and length
rejection. Constants, errors, magic quotes and the source secure salt have
fixed synthetic substitutes. No application bootstrap, private configuration,
real salt, cookies, network access or database handler is executed.

Workers have fixed memory, time and output bounds. PHP 8 reports `Undefined
array key 2` for the original two-part list assignment. Extraction records
only that exact audited warning from the selected block; every other warning
fails. This warning does not invent a third password field.

## Runtime and database behavior

The source cleans the whole name field before CP932 conversion, including
private trip input. Display preparation then normalizes reserved symbols and
spacing before stripping exclamation marks. CP932 conversion, trailing-hash
removal and ENT_COMPAT escaping precede the three-part split. Normal trips
hash the resulting bytes. The ordinary 100-byte raw-input limit and the later
255-byte escaped-display/generated-wrapper limit are separate checks.

Secure trips retain deployment-owned HMAC-SHA256. The selected secret uses
cleaned UTF-8 bytes and ENT_COMPAT escaping so distinct CP932-unrepresentable
characters cannot become identical modern secrets. Independent PHP HMAC
vectors use a synthetic key, and a regression requires `##é` and `##€` to
remain distinct. This security replacement does not reproduce the original
server's secure identities. Corrected preparation may change future trips
even without key rotation; historical trips remain unchanged.

This checkpoint fixed `STRIP_TRIPCODE` to zero. The later
[trip suppression change](verification-trip-suppression.md) extends the current
fixture to 469 cases across seven policies and imports the supplied `/b/` and
`/s4s/` overrides. The five original profiles alone do not qualify suppression.

Migration 0067 changes only `posts_name_check`: stored names may be empty and
may contain up to 255 bytes after source spacing. The populated upgrade
exercise applies migrations through 0066, witnesses rejection by the previous
constraint, then checks empty trip-only and expanded spacing names after
0067. Historical content, clocks and role ACLs remain identical. Names above
255 bytes, saved name/trip changes and runtime policy grants remain denied.
Apply the migration before the new binary and retain it during binary rollback.

JSON names use the source's HTML entities; trip-only JSON omits `name`.
Templates display escaped text. The template engine uses decimal references
for five characters where the source uses named references or a padded
apostrophe. HTTP checks canonicalize only those five aliases and still require
the exact name span and absence of an injected element. Browser checks require
the actual decoded name, saved trip and escaped JSON. Catalog name filtering
uses the source-encoded name, including an empty trip-only field.

## Executed checks and retained failures

Local Linux domain tests and all 138 native Windows domain tests pass,
including the exhaustive conversion fixture. Both extractors' `--check`
commands pass. A fresh owned PostgreSQL database applies all 58 migrations through 0067
and seeds successfully; the populated display-name upgrade exercise passes.
The complete Linux workspace run passes all 680 tests with no failures or
ignored tests. Strict workspace Clippy for all targets/features and formatting
pass. All 34 affected browser cases pass, including real desktop/mobile
CP932 trip-only authors, cleaned Quick Reply names, catalog filters, remote
previews, preferences, anonymous ownership, wordfilters and randomizers.
PHP, shell and JavaScript syntax checks pass. Hosted checks at this checkpoint's
head are recorded below; its full build run fails at a stale browser JSON
expectation.

The attachment exercise passes with both comment-plus-file and file-only
posts. It saves a CP932 trip-only author and a source-expanded display name
through the actual scoped writer. A rejected escaped name leaves the post
count unchanged and the approved capability available for a valid post;
another use after successful posting must fail. Its role/function denials,
approval, image-slot races, deletion and retained visibility checks remain.

Reproduction commands, using the owned database roles and native ICU setup:

```sh
php scripts/extract-public-name-reference.php 4chan-old crates/domain/tests/fixtures/public-name.json --check
php scripts/extract-trip-encoding-reference.php 4chan-old crates/domain/tests/fixtures/trip-encoding.json crates/domain/src/trip_cp932_data.rs --check
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test -p board-domain --locked
sudo bash scripts/test-public-display-name-migration.sh
node node_modules/@playwright/test/cli.js test tests/browser/post-identities.spec.js tests/browser/post-preferences.spec.js tests/browser/quick-reply.spec.js tests/browser/wordfilters.spec.js tests/browser/posting-randomizers.spec.js tests/browser/anonymous-session.spec.js
```

The first encoding attempt treated an empty password as a DES caller, although
the source skips hashing it. Its fixture now records that actual no-trip
decision; nonempty DES expectations remain independently generated. Initial
name extraction exceeded its stderr bound on the source's PHP 8 list warning.
The exact warning is now counted in each case; unrelated warnings still fail.
The original worker output is retained locally.

The first HTTP attempt compared source and template HTML entity spellings
directly. A diagnostic established the five equivalent references described
above. The next attempt expected all 14 posts in page JSON; the established
page contract emits the OP and last five replies. It now checks those six
exact IDs and identity fields against the complete thread snapshot and
requires the exact omitted count. Neither the API contract nor its excerpt
size changed.

The initial browser attempt exposed a real catalog mismatch: a trip-only
author's filter field contained `Anonymous`. The corrected template retains
the empty encoded field. The regression also requires an `Anonymous` filter
not to hide that trip-only thread. Its failing trace and assertion are retained.

A full-workspace attempt found older tests expecting raw JSON names and
accepting 100 bytes whose escaped name exceeds 255 bytes. They now check the
source-encoded JSON and both independent bounds. Accepted 100-byte ASCII,
accented, Kanji and emoticon inputs still exercise both posting routes; each
101-byte name and subject must fail without saving a post. Emoticon cleanup
also remains visible in the saved name and subject assertions.

A later workspace attempt found the same stale emoticon expectation in the
attachment suite. Its 100-byte emoji input remains unchanged; the saved-name
assertion now requires the source's `Anonymous` result. The expanded real
attachment cases described above pass without changing runtime media code,
role grants or the capability's single-use behavior.

The next workspace attempt reached the name corpus but stopped at the
production default of 30 writes per minute. The 42 source-name cases, catalog
last-reply checks and rollback controls use an explicit higher budget on their
synthetic router,
as other large HTTP matrices already do. The production default and
`http_limits.rs` rejection assertions remain unchanged.

The preceding `b4f0ef6` checkpoint passes all hosted build/test, monitoring and
advisory jobs, recorded in [CI fixture verification](verification-ci-board-fixtures.md).
That result does not qualify these later name changes. The `f2d1b62` name
checkpoint's [build run 37054194081](https://github.com/frankischilling/26chan/actions/runs/37054194081)
passes media/operations and every Windows visual group. Linux Rust checks
pass, but the mobile-header browser test expects ten raw `<` characters in
JSON instead of the source's ten `&lt;` entities; the aggregate gate fails.
The rendered-name assertions pass. The later suppression change corrects this
expectation and still requires a complete hosted run at its own head.

Staff-specific raw limits/preparation, privileged exceptions and
generated-randomizer filter scope remain open. Source suppression is covered
by its separate verification record.
