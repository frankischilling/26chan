# Final opening-comment rules

The original `imgboard.php` checks `COM_REGEX` after comment formatting and before
its duplicate-comment and posting cooldown checks. Replies bypass it. The rule
has no staff, VIP or Pass exemption. Configured quiet word filters exit earlier
inside `postfilter.php`, before this rule; they are distinct from the later
`$silent_reject` flood flag.

The supplied configuration enables `@https?://@` only on `/news/`. Its message is
`Your post must contain at least one valid URL`. This is a case-sensitive substring
match, not URL validation: `http://` alone matches, while `HTTP://` does not.
Escaping, word filters, wrapping and generated source markup precede the test.
Checking the raw form input or the rewrite's rendered HTML would change it.

Migration 0245 imports the effective pattern and message into
`content.boards.opening_comment_pattern` and `opening_comment_refusal`. Empty and
exactly `0` disable the pattern, matching PHP truthiness. The posting transaction
holds the board lock throughout evaluation; ordinary runtime roles cannot edit
either field. Operators can change these fields with migration authority.

The writer uses its existing typed source comment projection. It does not rerun
random word-filter choices. Failed checks roll back provisional effects and do
not consume the upload or record a post. Quiet word-filter rejection still exits before this rule. Staff refusals revalidate the session, CSRF, recent authentication
and board access before returning the configured message.

## Limits and deployment

Patterns are bounded to 2,048 bytes and refusal text to 1,024 bytes. Matching uses
the existing bounded PCRE2 implementation and shared two-worker admission pool.
It permits one match call, with 50,000 match operations, depth 64 and 1,024 KiB
native heap. The evaluated comment is limited to 2 MiB. Cancellation retains the
worker permit until its native work finishes.

Malformed patterns, unavailable source projections and exhausted work limits
refuse posting. This is a security difference from the source's `=== 0` check,
which can let a PCRE error pass. Unsupported EXIF provenance on `/p/` and `/test/`
is still unknown; enabling a nonempty OP rule there does not invent a projection.
Neither board enables the rule in the supplied configuration.

Apply migration 0245 before starting the updated applications. Readiness requires
both fields. The migration verifies existing authority seals before modifying
board structure and reseals the existing catalog boundaries afterward. Historical
posts and duplicate-comparison history are unchanged. Rolling back the binary
leaves the columns intact but stops enforcing this rule, so suspend posting if
the rule is required during rollback.

## Checks

`fixtures/opening-comment-source.json` records the extracted source branch,
configuration file hashes and all 82 effective board policies. The PHP harness
executes the unchanged branch in separate constant profiles, using 2,624 synthetic
OP/reply examples. Rust compares its decisions with those results.

The database test checks every imported policy, actual posting, escaped source
text, cooldown precedence, malformed policy, runtime write denial and quiet-filter
precedence. The HTTP test covers legacy JSON, script-free HTML, source error text,
reply bypass and unavailable policy. The populated history upgrade script includes
0245 drift rejection, rollback, preserved history and dump/restore.

These tests cover the final OP rule, not complete posting or rewrite parity.
Issues #192, #212 and #213 retain their other requirements.

```sh
python3 scripts/extract-opening-comment-reference.py /path/to/4chan-old --check
php -n scripts/test-opening-comment-source.php
cargo test --locked -p board-domain opening_comment_rule
cargo test --locked -p board-store --features database-tests --test opening_comment_rules
cargo test --locked -p board-public --features database-tests --test content_admission
bash scripts/test-comment-history.sh
```

Local qualification passed 2,624 PHP cases, 305 domain library tests, the real
writer/lock-wait case, both public content-admission HTTP tests, and the staff
role/badge HTTP case. The populated upgrade script passed drift rejection,
rollback, retained-history comparison and restore. Strict Clippy passed for the
domain, store, public and staff packages with all targets and features. The full
store regression sweep and exact-head hosted CI remain pending at this checkpoint.
