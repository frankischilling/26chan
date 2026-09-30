# Posting identities

Enter `User#password` in Name to post as `User !ozOtJW9BFA`. Leave the display
name before `#` empty to use `Anonymous` with the same tripcode. A name without
`#` has no tripcode. Normal tripcodes are public pseudonyms with a weak legacy
hash. They never authorize deletion, moderation or access to a staff account.

## Reference and scope

The [official FAQ](https://4chan.org/faq#trip) documents name/password separation,
the normal example above and name-independent hashing. Its
[secure-trip section](https://4chan.org/faq#sectrip) documents `##` and a server
secret. The September 29 response matches the SHA-256 already recorded for the
September FAQ in [the manifest](reference-manifest.json).
The pinned [thread API](https://github.com/4chan/4chan-API/blob/2bd670d507ba2daa37a3961a661e088cf6f89d57/pages/Threads.md)
specifies optional `trip` strings prefixed by `!` or `!!`.

Normal trips use a bounded, safe Rust implementation of Unix DES crypt, with
the legacy salt mapping and escaping. Twenty-seven independent libxcrypt
vectors cover the FAQ example, short passwords, salt punctuation, escaping,
eight-byte truncation and this deployment's UTF-8 input policy. The FAQ does
not establish original Unicode encoding or every parser edge case. Those
cases are local policy, not a claim of complete original-server parity.

Secure trips use HMAC-SHA256 with a private 32-byte deployment key. The display
is `!!` followed by eleven standard base64 characters. This is a security
replacement for the original secret-dependent hash. Original site's secret
and resulting secure identities cannot be imported or reproduced. Rotating
the local key changes future secure trips; saved posts keep their existing
public trip strings.

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
suffix is never stored in content, a cookie or a public response. Input retains
the existing 100-byte name limit and rejects control characters. A
forced-anonymous board discards both identity parts before admission.

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
intended as tripcodes.

Board/thread HTML, catalog hover details, staff previews and native post
projections escape identities. Tripcodes render in a `postertrip` span using
the theme's name color and ordinary font weight. Thread, page and catalog JSON
include `trip` only when present. Remote previews and the updater admit that
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
records executed commands and current CI results.
