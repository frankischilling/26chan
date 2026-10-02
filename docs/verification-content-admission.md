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
| Windows domain suite | 131 passed, including native work guards and Unicode properties |
| Linux workspace with all features | 670 passed, zero failed or ignored |
| Workspace Clippy and formatting | Passed with warnings denied |
| Focused database and HTTP rules | Eight database cases and one real-router case passed |
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
generation denial. The filename case authenticates an intake capability,
requires approval, rejects unready metadata and preserves the non-UTF source
regexp's `phpé` byte count. Its queue exercise uses synthetic metadata, so it
does not establish decoder, file publication or VM containment qualification.

The HTTP case submits real form bodies through the public router and checks
HTML/JSON error behavior, quiet-success IDs, preference cookies without trip
secrets, actual socket identity despite spoofed forwarding headers and
unavailable-policy responses. No configured operator patterns, logs, addresses
or action metadata are exposed through public HTTP reads.

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

Complete caller ordering, the staff `/test/` exception, remaining name and
randomizer scope, unsupported regexp modifiers, IP/ASN rules, file blacklists
and optional classifiers remain open. The supplied source lacks private
operator lists and trained data; none are invented as production policy or
passing parity evidence.
