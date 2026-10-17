# Request classifiers

Classifier coverage is incomplete. The request-header credit predicate is
connected to public posting through migration 0193. The edge automation
predicate and numeric request scorer have qualified domain models but no
production score providers or enforcement callers.

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
requests, with a fresh static cache for each case. It never loads the
application, database or RPC files. Cases distinguish absent and empty headers, marker case, embedded
NUL bytes and non-ASCII user agents.

The public caller derives facts from the posting request. Accept and language
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

## Numeric request score

`content_admission::threat` evaluates the source's deterministic numeric rules
against a qualified request snapshot. It takes the complete ordered server
entries, parsed cookies, challenge/response values, request time, country and
posting/reporting mode. A normal unordered HTTP header map cannot establish
this snapshot. The caller must qualify transport order and trusted country/time
facts before supplying it; client headers cannot assert that provenance.

The domain model preserves byte matching, empty-versus-absent fields, PHP's
falsey `"0"`, browser version branches, cookie timing, all 224 country-language
mappings and ordered multiplication. The Linux platform branch retains the
source's regex-error comparison; an ordinary non-match adds no penalty. Server
entry order includes non-HTTP entries for the separated-header and model-distance
checks. Parsed cookies and the raw Cookie header remain independent facts.

Snapshots are bounded to 256 unique entries per map, 128-byte ASCII keys,
4,096-byte values and 65,536 total key/value bytes per map. Duplicate keys,
oversized inputs and unavailable provenance produce no score. Time subtraction
accepts finite decimal/exponent strings with surrounding ASCII whitespace;
unqualified or unsupported numeric inputs remain unavailable. Request time is
bounded to the exact integer range of a double. No score means unknown, not zero.

The isolated extractor executes only the two hash-pinned pure functions on
64-bit PHP 8.4.24 with PCRE 10.46. It rejects other engine versions. Its 4,517
cases cover browser/version branches, request mode, header presence and order,
cookie timing, language boundaries, every country-language
alternative and combined terms. Arbitrary-byte tests also check finite results
and bounds. This is domain qualification; posting/reporting integration and
complete trusted request capture remain open. The older report-threshold model
still provides only its selected positive witnesses.

Regenerate or verify the corpus without loading application code:

```text
php -n scripts/extract-threat-score-reference.php SOURCE crates/domain/tests/fixtures/threat-score.json --check
```

## Remaining source behavior

Posting uses the numeric score in several late refusal branches. Reporting has
its own weighting thresholds. Neither caller is wired to this numeric model.

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
