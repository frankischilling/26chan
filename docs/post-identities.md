# Posting identities

Enter `User#password` in Name to post as `User !ozOtJW9BFA`. Leave the display
name before `#` empty to display only the tripcode in thread headers. A name without a password
has no tripcode; an empty ordinary name becomes `Anonymous`.
Normal tripcodes are public pseudonyms with a weak legacy
hash. They never authorize deletion, moderation or access to a staff account.

## Reference and scope

The [official FAQ](https://4chan.org/faq#trip) documents name/password separation,
the normal example above and name-independent hashing. Its
[secure-trip section](https://4chan.org/faq#sectrip) documents `##` and a server
secret. The September 29 response matches the SHA-256 already recorded for the
September FAQ in [the manifest](reference-manifest.json).
The pinned [thread API](https://github.com/4chan/4chan-API/blob/2bd670d507ba2daa37a3961a661e088cf6f89d57/pages/Threads.md)
specifies optional `trip` strings prefixed by `!` or `!!`.

The supplied `imgboard.php` converts the cleaned whole field from UTF-8 to
CP932, removes trailing hashes, escapes it as Shift_JIS, then splits at the
first two hashes. A nonempty third part selects the secure trip; otherwise a
nonempty second part selects the normal trip. `Name#`, `Name##` and `###`
therefore have no trip. Normal trips use the escaped CP932 bytes, the legacy
salt mapping, and a bounded safe Rust implementation of Unix DES crypt.

The [public-name verification record](verification-public-names.md) pins the
source and conversion environment. Its independent fixture covers every
Unicode scalar, 9,278 non-ASCII CP932 mappings and 48 conversion cases, including
47 DES results and the empty field that skips hashing. The complete ordinary
name block has 335 cases across five board/spacing policies. ASCII libxcrypt
vectors remain an additional check on the DES implementation.

Secure trips use HMAC-SHA256 with a private 32-byte deployment key. The display
is `!!` followed by eleven standard base64 characters. This is a security
replacement for the original secret-dependent hash. Original site's secret
and resulting secure identities cannot be imported or reproduced. Rotating
the local key changes future secure trips; saved posts keep their existing
public trip strings. The secure preimage uses the source's whole-field cleanup
and delimiter rules, then escapes the selected secret in UTF-8. Keeping UTF-8
prevents distinct characters unsupported by CP932 from collapsing to the same
modern credential. This parser correction can change future trips even with
an unchanged deployment key; saved trips are never recomputed.

## Configuration

Set `TRIPCODE_KEY` only in the public runtime's private environment file. Use
64 hexadecimal digits generated from 32 random bytes. For example, create an
ignored local file without printing the key:

```sh
umask 077
printf 'TRIPCODE_KEY=%s\n' "$(openssl rand -hex 32)" > .local/public-identities.env
```

Load that file alongside the public service's configuration. Keep the key out
of source control, worker environments, logs, screenshots and staff enrollment
configuration. Back it up privately if secure identities must survive a
restore. An unset key disables secure trips with a fixed error. An empty,
malformed or all-zero key fails startup before listeners or database access.
The application does not generate a different key on each restart.

## Storage and rendering

New posts save only the display name and public trip. The posting password
suffix is never stored in content, a cookie or a public response. Ordinary
input has a 100-byte UTF-8 limit. Source cleanup removes zero-width and
disallowed emoticon characters from the whole field, removes CR/LF, then
normalizes and sanitizes the display prefix with the board's spacing policy.
It removes reserved name symbols and exclamation marks. The final escaped
display text and generated trip wrapper have a separate 255-byte limit.
Tabs follow source spacing rules; other control characters remain rejected
as a backend validation rule. A forced-anonymous board discards both identity
parts before admission.

Migration 0038 adds a nullable, format-constrained `trip` column and an insert
trigger. A transaction-local value carries the pseudonym through ordinary
inserts and the existing attachment function. The trigger checks forced
anonymity under the board lock. Reusing a pooled connection cannot carry a trip
to a later plain-name post. Public SQL cannot insert or update the trip column
directly or execute the trigger function. A compromised public writer can
already publish content; a trip string grants no additional authority.

Existing names and timestamps remain unchanged, with no inferred tripcode.
Historical rows that already contain password-like text need an operator's
content review; the migration does not guess which historical names were
intended as tripcodes. Migration 0067 admits empty display names and expands
the stored name bound to 255 bytes for source spacing results. It changes only
that constraint. Apply it before this binary; keep it during binary rollback.
It does not rewrite rows, trip strings, timestamps or role grants.

Board/thread HTML, catalog hover details, staff previews and native post
projections escape identities. Tripcodes render in a `postertrip` span using
the theme's name color and ordinary font weight. Thread, page and catalog JSON
include `trip` only when present and omit `name` for an empty trip-only display
name. Nonempty JSON names use the source's HTML entities, while rendered HTML
uses the template engine's equivalent escaped text. Catalog name filters use
the source-encoded field, including an empty field for a trip-only author.
The catalog OP preview retains the source's separate anonymous fallback;
the last-reply preview preserves a trip-only author's empty name.
Remote previews and the updater admit that
specific class through their existing inert recipe validation. Native trip
filters consume the persisted public pseudonym.

## Verification

The domain suite checks independent DES and HMAC vectors, bounds, malformed
keys, missing-key errors and 96 bounded Unicode property cases. Store tests
check real connection reuse and rollback; public handler tests exercise both
posting routes, both form encodings, JSON, catalog output and escaped previews.
Attachment tests check named and forced-anonymous identities through actual
one-use approval. The migration exercise checks retained history, constraints,
transaction isolation and denied runtime mutations in a disposable database.

Browser coverage uses persisted posts for remote previews, ordinary rendering
and exact trip filters. The parser regression also rejects event handlers and
inline style on a trip span. [The combined verification record](verification-rewrite-completion.md)
records earlier commands and CI results. The [public-name record](verification-public-names.md)
records this source preparation and encoding checkpoint. Staff-specific name
preparation, its 255-byte raw-input limit and privileged posting exceptions
remain unfinished; this scope covers ordinary public posting.
