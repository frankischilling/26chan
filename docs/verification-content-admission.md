# Content admission verification

This checkpoint adds configured public content rules under
[#212](https://github.com/frankischilling/26chan/issues/212), with private
transactional effects and the source's distinct literal, regexp and OP autosage
projections. [The implementation record](content-admission.md) describes its
limits, permissions, deployment steps and remaining source work. The wider
rewrite remains in draft PR #227.

On October 2, 2026, local qualification passes:

| Check | Result |
| --- | --- |
| Original normalization extractor `--check` | 1,849 inputs, four projections each, ICU 74.2 |
| Original content extractor `--check` | 434 cases; 414 decision/effect comparisons and 20 recorded unavailable-query cases |
| Original fixed name/trip extractor `--check` | 352 cases, including ASCII/Unicode display text, exact trips, literal pipe classes and moderator decisions |
| Windows domain suite | 133 passed, including native work guards, fixed name/trip checks and Unicode properties |
| Linux workspace with all features | 674 passed, zero failed or ignored |
| Workspace Clippy and formatting | Passed with warnings denied |
| Focused database and HTTP rules | Ten database cases and one real-router case passed |
| Public startup and paired listeners | Ten startup and two paired-listener cases passed on Linux and Windows |
| Restricted-role bootstrap | All 66 migrations, historical preservation and owner/runtime grants passed |
| Client tests and generated bundles | 481 passed; all generated bundles match their pinned sources |
| Actual posting browser regressions | 19 passed: anonymous ownership, script-free forms, trips, wordfilters and randomizers |
| BFCache layout DOM checks | Seven passed on Linux and Windows |
| Redacted browser-response reader/supervisor | Four Node and eight Python tests passed |
| Parser dependency separation | Credentialed applications exclude guest JPEG/GIF parsers; injected-edge control passed |
| Dependency advisories | 361 locked entries, 1,280 fetched advisories; no vulnerabilities, one unsuppressed `paste` maintenance warning |

The actual PostgreSQL cases cover ordered actions, log/ban precedence, fake
success with no post or proof consumption, hit deduplication, expiry and global
ban scope. They also cover trip-secret exclusion, bounded capacity rollback,
NULL-argument rejection, actual runtime permission denials, known-user
thresholds, cross-board activity locking, policy-update waiting and obsolete
generation denial. Rule rejection and quiet success now precede final blank
and text-only subject checks. Raw limits, repeated-line spam, required subjects
and line counts stay earlier. Positive logging/autosage controls establish that
the same matching rules work, while a later blank-content failure leaves no hit,
log or new thread. That rollback is a deliberate security difference from the
source's separate writes. The filename case authenticates an intake capability,
requires approval, rejects unready metadata and preserves the non-UTF source
regexp's `phpé` byte count. Its queue exercise uses synthetic metadata, so it
does not establish decoder, file publication or VM containment qualification.

The HTTP case submits real form bodies through the public router and checks
HTML/JSON error behavior, quiet-success IDs, preference cookies without trip
secrets, actual socket identity despite spoofed forwarding headers and
unavailable-policy responses. No configured operator patterns, logs, addresses
or action metadata are exposed through public HTTP reads.

The fixed public name/trip follow-up passes two focused domain cases on Linux
and Windows, all ten database admission cases and the expanded real HTTP case.
It checks Unicode display names, configured rejection/quiet precedence, later
log/autosage rollback, the earlier fixed-subject decision and raw-password
exclusion. This qualifies the public caller; the source moderator decision is
supplied only by the independent pure fixture until staff exceptions are wired.
The first standalone ASCII projection omitted source dot substitutions and
failed normalization fixture input 3. Reusing the established dot scanner
restores all 1,849 projections; the failed attempt is retained locally.

The broader follow-up run failed the media lease/approval concurrency assertion.
A focused diagnostic passed that assertion, so the original cause remains
unconfirmed. The test now witnesses an actual job-row lock wait before shortening
the lease, then requires expiry on PostgreSQL's wall clock before releasing the
lock. Both reservation and approval must still reject with `Conflict` and leave
the output unpublished. Changing either final lease predicate to use a
transaction timestamp makes its case fail for accepting the expired lease.
Those temporary mutations were reverted, and all four media approval tests pass.
No runtime media code, role timeout or approval assertion was weakened.

Earlier attempts remain part of the record. An integrated build caught
a shadowed quiet-post ID before tests ran. The initial database fixture cleanup
omitted dependent posts; its action assertions completed, then cleanup failed.
The initial HTTP fixture expected the legacy cookie name rather than the
already established `board-anon` capability cookie. A client attempt lacked
the installed browser-cache path; the next exposed the BFCache scheduling
assumption described in [CI fixture verification](verification-ci-board-fixtures.md).
The workspace attempt then lacked the restored Node path, and a later attempt
reached the stale seven-function attachment-owner assertion. It now requires
the eighth authenticated filename function and retains every definer,
search-path and PUBLIC-ACL check. The complete final repeat passes.

Review also found NULL generation/action parameters that bypassed SQL's
three-valued predicates, and a Unicode-vs-byte mismatch in the filename effect
check. Those predicates are explicit now. Unpublished migration development
was qualified in fresh owned databases; earlier fixture databases and their
credentials were retained. No applied checksum was changed to force a pass.

Hosted [run 37001161463](https://github.com/frankischilling/26chan/actions/runs/37001161463)
on the preceding head passes Linux and Windows but fails the inline Quick Reply
receipt read in media/operations. The diagnostic now covers receipt and posting
stages with fixed status/type/stage/failure classifications and no raw body,
URL, header or capability. A streamed 100-attempt transport probe did not
reproduce the failure. Its cause remains unresolved; the next head requires
the real isolated upload and complete CI checks. Existing assertions,
deadlines, retries and screenshot expectations are retained.

The first call-order HTTP attempt expected status 422 for ordinary JSON
validation errors. The established source response is status 200 with an
`error` body; the corrected case requires that status and exact message,
alongside the HTML 422 response. The failed attempt is retained locally.
On `8195d65`,
[run 37022763521](https://github.com/frankischilling/26chan/actions/runs/37022763521)
passes Linux Rust/browser checks, media/operations and the aggregate gate.
Advisory and monitoring runs also pass. Windows startup fails before the visual
groups, so the complete run fails. The Windows failure and its loader-path correction are recorded in
[CI fixture verification](verification-ci-board-fixtures.md).

The corrected checkpoint `a6fe0f32eb2a71945bed72d68f57a0725b5fa3ca` passes
[build/test run 37028174415](https://github.com/frankischilling/26chan/actions/runs/37028174415)
in full: Linux Rust/browser checks, Windows visual checks, media/operations and
the required aggregate gate. Its
[dependency advisory run](https://github.com/frankischilling/26chan/actions/runs/37028174884)
and [monitoring run](https://github.com/frankischilling/26chan/actions/runs/37028174423)
also pass. This qualifies that checkpoint. The fixed public name/trip follow-up,
`d44ea1f`, passes hosted Rust, media/operations, all Windows visual groups and
dependency advisories, but fails its Linux Robot9000 browser case and independent
monitoring workflow. The mobile initialization race and duplicate-port fixture
failure are recorded in [CI fixture verification](verification-ci-board-fixtures.md).
The corrected `b4f0ef6` checkpoint passes all build/test, monitoring and advisory
jobs, including hosted resource and maintenance checks. The linked CI record
contains the exact runs. Later changes require their own complete qualification.

[Public-name verification](verification-public-names.md) separately records
ordinary name preparation, CP932 normal-trip bytes, delimiter selection and
saved/API display fields. Staff-specific preparation and exceptions remain open.

Complete caller ordering, the staff `/test/` exception, remaining name and
randomizer scope, unsupported regexp modifiers, IP/ASN rules, file blacklists
and optional classifiers remain open. The supplied source lacks private
operator lists and trained data; none are invented as production policy or
passing parity evidence.
