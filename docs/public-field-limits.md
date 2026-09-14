# Public name and subject limits

New public names and subjects each permit 100 UTF-8 input bytes. The supplied
`imgboard.php:5299-5306` checks unauthenticated fields using `strlen` before
later cleanup. This rule is separate from comment scalar counting. A name of
25 four-byte emoji fits; adding one ASCII character fails. Whitespace counts
before name trimming. Empty names still use the board's current anonymous
presentation, and unsupported control characters remain rejected.

The shared normal/approved-image form has no `maxlength` attribute on these
fields, matching `views/imgboard.php:78,101`. Browser string-length limits do
not measure UTF-8 bytes. The server validates both posting endpoints and the
store validates again under the board lock, including attachment writes.

Run `cargo run -p board-store --bin board-migrate --locked` using the migration
login before deploying this version. Migration 0021 expands the name storage
ceiling from 80 to 100 bytes. It preserves all existing rows and grants.
Subject storage retains its 120-byte ceiling for older data; the application
enforces 100 bytes on new input. This avoids breaking deletion or moderation
of older subjects. A compromised public database login retains its existing
ability to insert subjects up to that storage ceiling; this change adds no
such authority. Keep the wider name constraint on rollback while any names
over 80 bytes exist. Earlier binaries can still read them but enforce their
earlier input policy.

[Issue #94](https://github.com/frankischilling/26chan/issues/94) tracks this
change. Domain and HTTP tests cover ASCII/multibyte boundaries, both posting
routes and unchanged post counts after rejection. The approved attachment
workflow persists a 100-byte name. A JavaScript-disabled browser test checks
native submission, exact visible fields, escaping and over-limit errors.
`scripts/test-public-field-migration.sh` upgrades a disposable old schema,
checks retained text and deletion, accepts a new 100-byte name with the public
login, and rejects a 101-byte name and runtime schema mutation. CI runs that
exercise; local syntax or unit checks alone do not qualify database behavior.

This addresses the field-length portion of B-003 and V-004. Tripcodes, forced
anonymous policy, authenticated posting, later board-specific cleanup and the
remaining form/response contract are separate compatibility work. It does not
qualify production media, deployment or staff authentication boundaries.
