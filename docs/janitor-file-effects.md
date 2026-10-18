# Janitor request file effects

The source file-policy model is checked against 1,728 executed PHP cases from
`admin.php:adminBanReq` and `reports/ReportQueue.php`. The extractor verifies both
complete source-file hashes before retaining the bounded branches. The oracle
intercepts every file and SQL call and uses synthetic posts, a fixed synthetic
salt and owned paths. It never reads source keys or copies real uploads.

## Source behavior

Request creation attempts both original and thumbnail copies only for an
`everything` template without an illegal-category report. The source ignores
copy errors and attempts these paths even when the post has no extension. In
the other branch it puts the raw digest into captured post JSON. A failed copy
does not switch branches or expose that digest.

Amending a request with an extension to a `json_only` template removes both
captured files and restores the raw digest from the serialized post. The source
checks an undefined `has_approved` variable on this path; its false value causes
the thumbnail removal. The oracle records that warning instead of inventing a
successful approval state.

A non-warning review inserts an image rule only when the captured raw digest is
nonempty and the selected policy is `image` or `rejectimage`. `image` requests an
indefinite automatic ban; `rejectimage` rejects the upload. The caller's warning
branch controls this decision, separately from the resulting stored duration.

Clearing a request always deletes its original file. Denial also deletes its
thumbnail; approved cleanup leaves the thumbnail. The JSON-only amendment can
have removed that thumbnail before approved cleanup runs.

## Native storage requirements

These are effect decisions, not file custody or authorization. Native storage
must require approved media, bind illegal-report checks to the actual locked
request target, keep copied material private, and commit deletion/capture/review
without partial effects. Browser fields cannot supply a digest or retained path.
Unknown retained digest evidence must remain distinct from known absence.

The original creation query uses the submitted post number for its illegal-report
check. Native authorization must use the captured target instead. A source
negative control removes that exclusion and confirms that the isolated fixture
observes both copy calls; the unchanged branch performs neither.

## Transactional image-rule writer

Migration 0211 admits `image` and `rejectimage` review policies for existing
`none`/`json_only` captures. It uses the request's private normalized digest,
records immutable reviewer/requester attribution, and inserts a global rule
through the existing admission table. Warnings and known absence create no
rule. Unknown evidence refuses the review instead of pretending the file is
absent. The ban, rule, counted outcome and pending-request removal commit
or roll back together.

Only the non-login restriction owner gains the required writer permissions.
Staff, public and authentication roles cannot write the rule table, advance its
sequence or call the internal writer. Startup checks require that separation.
The catalog includes the rule sequence's grants as well as table/function
permissions. Rule inventory is bounded to 128 rows; reasons and descriptions
must fit the existing 4,096-byte fields.

Policy-lock contention refuses immediately while the review holds its private
locks. Local checks prove no partial ban, count, capacity or policy-generation
change during that refusal, followed by a successful retry on a separate staff
connection. Populated restore preserves the issued rule, attribution and count.
Ten owned database cases cover both rule modes, warnings, absence, unknown
metadata, quota exhaustion, oversized reasons, repeated approval and a template
changed to zero days after request creation. The real public admission reader
observes the issued global rule.

The model, all domain tests, strict domain Clippy and source oracle pass locally.
The SQL decision matches all 1,728 source cases. The restored-evidence fixtures
supply synthetic private digests; they do not claim to test public upload
processing. The real HTTP integration passes creation, image-rule approval,
repeated-review refusal, denial, statistics and origin checks. All 59 staff unit
tests pass. Fresh hosted qualification remains pending.
Private custody and authenticated retained-media reads remain unfinished.
Issue #223 stays open.

## Reproduce the checks

The PHP oracle is a test dependency, not an application runtime. PostgreSQL 16
is required for the owned database exercise; `PG_BIN` can select its bin directory.
The database script creates and removes its own temporary cluster.

```sh
python3 scripts/extract-janitor-file-effects.py /path/to/4chan-old --check
php -n scripts/test-janitor-file-effects.php
cargo test -p board-domain --locked
REQUEST_FILE_EFFECTS=1 bash scripts/test-janitor-request-review.sh
```

The source-case harness passes raw-digest presence between the extracted
branches. It does not execute the complete JSON renderer or copy filesystem
bytes. Media custody and end-to-end upload processing need their own checks.
