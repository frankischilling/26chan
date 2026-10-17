# Request classifiers

Classifier coverage is incomplete. The request-header credit predicate is
connected to public posting through migration 0193. The edge automation
predicate has a qualified domain model but no trusted runtime score provider.

## Credit eligibility

The source's `spam_filter_is_bad_actor` uses header presence, case-sensitive
user-agent markers and two referer substrings. Empty header values count as
present. The native predicate accepts qualified byte values or an unavailable
provider and limits inspected values to 4,096 bytes. A definite independent
match can still withhold credits when another provider is unavailable. A
missing required fact cannot qualify the request as eligible.

The active caller at `imgboard.php:4959–4961` uses this predicate only after
successful CAPTCHA validation, alongside its unsolved-count and operator-policy
checks. It does not independently reject posts or turn a failed CAPTCHA into a
success. The similarly named CAPTCHA difficulty checks have a separate caller
and belong to the CAPTCHA workflow.

The extractor runs the pinned pure function on PHP 8.4.24 over 450 synthetic
requests, with a
fresh static cache for each case. It never loads the application, database or
RPC files. Cases distinguish absent and empty headers, marker case, embedded
NUL bytes and non-ASCII user agents. The public caller derives facts from the posting request. Accept and language
use presence alone; repeated values still prove presence. Repeated user-agent
or referer values are unavailable when their contents matter. Required unknowns
withhold courtesy credits without rejecting a valid solved-CAPTCHA post.

The proof carrier defaults to withholding credits. Its request eligibility is
installed in a separate transaction-local setting and cleared by the existing
private receipt consumer. The private issuer's permission, unsolved count,
challenge validation and registered-activity requirements still apply. The
request predicate cannot authorize a challenge or replenish credits spent by
a credit-backed receipt. Report proof serialization stays unchanged.

The database cases cover eligible, withheld and unavailable request facts,
accepted posts without credits, one-use receipts, late attachment rollback and
credit-spending rollback. Migration qualification retains pending challenges,
receipts, existing balances and anonymous sessions through rollback and upgrade;
the migration pins the predecessor owner, path, grants and body. The upgrade
exercise checks path, grant and body changes.

## Optional automation evidence

The domain automation predicate matches 429 source score/cache/user-agent cases.
It preserves the source's default threshold, WebView exclusions and early return
when the edge score is absent, even if a cache marker exists. A definite low
score does not need a cache lookup, while missing required evidence remains
unavailable. No client-supplied header has been wired as a trusted score.

## Remaining source behavior

The source threat score is a deterministic heuristic. Its function contains
header, cookie, timing and country-language rules; it does not call a trained
model. Posting uses it in several late refusal branches. Reporting has its own
weighting thresholds. The existing report-threshold witness proves only selected
positive cases and is not a complete scorer or a wired production provider.

The separate edge bot-score path requires a trusted ingress score and private
same-session event history. A public request header cannot supply that authority.
The source's country-change path also records private session events; current
anonymous-session fingerprints alone do not represent that event history.

The flood helper's posting call is commented out and the caller assigns zero.
Activating its dormant rejection branch would change the supplied behavior.
Disabled sampling and logging branches likewise must not become active filters.

Source evidence is in `lib/postfilter.php:1639–1682,1684–2316,2397–2545` and
`imgboard.php:4937–4969,6258–6449`. Optional external classifier data, native
history producers and complete late caller ordering remain open under #212 and
the related CAPTCHA, session and restriction work.
