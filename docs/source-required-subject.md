# Required subjects

Issue: https://github.com/frankischilling/26chan/issues/140

`imgboard.php:5400-5412` checks `REQUIRE_SUBJECT` for new threads after subject
cleanup. The active source default is false (`config/global_config.ini:284`);
`config/boards/qst.config.ini:32` and `vg.config.ini:21` enable it. Migration
0030 adds operator-owned `content.boards.require_subject` and sets those two
existing boards to true. Other and future boards default to false. Operators
creating a source-configured board after migration must set its overrides
explicitly; the synthetic `/qst/` demo fixture does so.

The handler checks early. Both normal and approved-attachment store paths check
again using committed board policy under the board row lock, before allocating
a post number, making room for a thread or consuming an attachment receipt.
Replies remain exempt. Raw field/character/storage bounds and control rejection
precede subject cleanup. A cleaned, zero-byte OP subject then returns the exact
`S_NOSUB`: `Error: New threads require a subject.` This precedes repeated-line,
line-count and empty-comment admission. The same input remains subject to those
comment rules once it has a valid subject.

The check is byte emptiness, not a second Unicode trim. A private codepoint's
removal can leave a stored space that satisfies the source check. The existing
100-byte raw limit cannot be bypassed by submitting characters cleanup removes.
Subjects remain escaped text. The source form only adds HTML `required` for
`TEXT_ONLY`, not `REQUIRE_SUBJECT` (`views/imgboard.php:91-104`); this slice does
not add client-side required validation or invent a JSON board capability flag.

HTML denial is 422; JSON posting retains the implemented HTTP 200 error object.
Image-required, upload-board, text-only, forced-anonymous, ordinary OP
subject-or-comment admission and other posting errors remain separate unfinished
policies. In particular, media-disabled development still permits text-only
threads. This change does not establish complete source error ordering or
posting parity.

## Deployment and authority

Apply migrations through 0030 before deploying the binary. No historical board
fields, subjects, posts or thread clocks are rewritten. Keep the additive column
during a binary rollback; older binaries will not enforce this posting rule.
No new runtime grants, media decoding, HTML trust or staff authority are added.
Public database credentials can read but cannot edit this operator setting or
alter its schema. As with other application admission rules, a compromised
public runtime with existing direct insert privileges can bypass the rule;
this is not a new database-level content invariant or containment guarantee.

## Verification

Domain tests cover all spacing modes, cleaned-empty and surviving-space subjects,
replies/optional policy, raw bounds, controls and error precedence. The database
test exercises both aliases, both form encodings and both response modes,
successful OPs/replies, unchanged rejected post counts/sequence/thread clocks,
two observed row-lock waits, public policy-write denial and historical subjects.
The approved-media matrix rejects a cleaned-empty subject before accepting the
same receipt with a valid subject, then retains the one-use check. The actual
no-JavaScript browser form covers denial, escaped OP success and subjectless reply.

`sudo bash scripts/test-required-subject-migration.sh` upgrades an owned database
through 0029 then 0030, checks all 82 source board defaults, unchanged historical
rows/clocks, operator updates and denied public policy/schema writes, and removes
that database. CI runs it against its verified disposable PostgreSQL cluster.
Local Rust compilation is not database, upgrade or posting-browser execution;
those results require current-head hosted qualification before merge. Existing
production containment, restore and independent-review prerequisites remain.
