# Source forced-anonymous policy

The supplied `imgboard.php:5390-5393` replaces an ordinary public poster's name
with `S_ANONAME` and clears the subject when `FORCED_ANON` is enabled. This runs
before required-subject admission, comment formatting and final OP-content
admission. The global configuration defaults off at line 446; the only active
board overrides, `/b/` and `/s4s/`, also explicitly disable it. Migration 0035
therefore defaults every existing and future board to false.

For public posting, an enabled board saves `Anonymous` and an empty subject.
The store reads the setting under its existing board lock and clears those
fields before admission. A subject-only OP becomes empty and is rejected; an
enabled `REQUIRE_SUBJECT` or `TEXT_ONLY` setting still rejects an OP whose
subject was cleared. Replies remain subject-exempt. Raw size and control
bounds still apply to discarded input. Public capcode-looking text supplies
no administrative exception. The source's privileged posting exception is
outside the current public handler's authority.

An invoker insertion trigger also clears these fields for direct public SQL
and the scoped attachment inserter. It locks the operator's setting through
the write. The attachment owner gains only a read of the non-private boolean.
No new function can be used to change board policy, authentication, deletion
authorization or media approval. The trigger runs on insertion, so toggling
the setting never rewrites historical names or subjects.

The source native form (`views/imgboard.php:57-114`) provides a hidden name,
omits the visible name and subject rows, and places the submit button beside
Options. Both native forms follow that arrangement. Quick Reply reads the
source form's hidden name and supplies its own empty hidden field, so it
cannot offer an identity that the board would discard. `/boards.json` on both
listeners emits integer `forced_anon: 1` only when enabled, matching
`imgboard.php:7920-7921`; policy changes invalidate the representation's ETag.

Apply migration 0035 before the binary. Keep its additive column and trigger
during binary rollback. The previous binary may show name and subject fields
and perform admission before the trigger clears them; the database continues
to discard those identities on enabled boards. Rollback does not restore
discarded input or change existing posts. Production rollout remains separate.

## Verification

The HTTP/database matrix covers off/on/off policy, both posting aliases,
both encodings and both response modes, OPs and replies, actual stored values,
both JSON listeners and their ETags, native form fields, source admission
ordering, an observed board-lock wait, raw bounds and public policy-write
denial. Historical rows retain their original names and subjects. This matrix
uses an explicit higher request limit because it exercises many writes through
one router; the production default and dedicated limiter checks are unchanged.

The attachment suite covers the ordinary store path and direct scoped SQL
insertion using approved capabilities. The populated upgrade harness checks
all 82 board names, future defaults, retained historical rows and clocks,
runtime reads, denied policy/schema writes, direct inserts and later toggles.
Its four unchanged SQL blocks passed on a separately created native PostgreSQL
database, which was removed afterward. The HTTP matrix also passed locally.

Passing browser cases exercise native posting without JavaScript and live Quick Reply
with JavaScript, reload persistence and JSON identity fields. Separate desktop
and mobile fixtures passed and their screenshots were inspected. All 46 public
library tests, six Quick Reply Node tests, the attachment authorization suite,
and strict public/store Clippy for all targets/features passed. Formatting,
actionlint, shell/JavaScript syntax, bundle generation and diff checks passed.
Complete current-head CI qualification remains required before merge. This change
does not implement full name/trip sanitation or privileged posting.
