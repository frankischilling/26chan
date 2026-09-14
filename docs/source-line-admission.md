# Source comment line admission

Issue: https://github.com/frankischilling/26chan/issues/135

New public posts apply three source rules in their original order. Historical
comments are neither reprocessed nor rewritten during migration or reads.

`imgboard.php:5348-5352` removes intra-word `[spoiler]` markers only when the board
enables `SPOILERS`. Its case-sensitive regex requires a non-whitespace byte on
both sides and does not cross LF. Global matches consume both boundary bytes;
an adjacent marker cannot reuse the same byte. Multibyte boundaries can therefore
behave differently from one-byte boundaries. Lazy capture can extend to a later
closing marker when an earlier closing marker is followed by whitespace. This
stage follows early Unicode cleanup and precedes same-board quote rewriting.
It does not enable or disable the separate markup renderer.

`imgboard.php:5430-5438` checks repetition when the cleaned comment has more than
six LF separators. The pattern `([^\n]+\n+)\1{5,}` requires six identical
text/newline groups. It can begin at a suffix of the first text run; the final
newline run can extend beyond the matched group. Detection uses the source's
`htmlspecialchars(..., ENT_QUOTES)` representation, including entity suffixes,
because `sanitize_text` at lines 7289-7320 precedes this check. This bounded
projection is used only for admission; stored comments remain text and templates
remain escaped. The source automatic-ban call is commented out. A match returns
`Error: Our system thinks your post is spam.` and creates no ban.

Repetition runs before ordinary four-or-more blank-line collapse. Code/SJIS
spacing skips that collapse, but does not bypass repetition or line limits.
`imgboard.php:5447` then rejects an LF count strictly greater than `MAX_LINES`,
with `Error: Too many lines.` These are separator counts, not wrapped display
lines. The strings come from `config/global_strings.ini:198,268`. Public HTML
rule failures retain status 422; the existing source JSON envelope returns 200
with only `error`. No public form field grants the source staff exemption.

## Configuration and deployment

Apply migration 0028 before deploying the binary. Its operator-only board fields
are `comment_max_lines` (0 through 16000) and `comment_spoiler_cleanup` (boolean).
Zero permits no LF separators; it does not disable validation. Global defaults
are 70 and false. Existing source boards receive active CATEGORY/board overrides:
56 boards use 100, `/b/` and `/bant/` use 50, and 24 enable spoiler cleanup. The
migration and upgrade fixture enumerate the complete sets. The commented `/s4s/`
line limit is inactive. Source category inheritance is not inferred from the
rewrite's `worksafe` display flag. Future boards need explicit operator overrides;
the synthetic `/test/` seed selects its source code-spacing and line settings.

Handlers prevalidate before expensive work. Posting rechecks the current board
policy under its existing transaction lock before allocating a post number or
consuming an approved attachment capability. Public database credentials can
read these fields but cannot update them. Raw character limits and independent
storage ceilings still apply, including when cleanup would shorten a comment.
Control-character rejection, attachment approval and safe rendering remain
security boundaries. No new dependency or runtime grant is required.

Keep the additive columns during binary rollback. Older binaries do not apply
these admission rules to future posts. Rollback cannot reconstruct removed
spoiler markers; backups preserve stored text without rerunning cleanup.

## Verification and limits

Domain tests compare all 131071 binary x/LF strings through length 16 against an
independent byte-capture detector. Two 128-case properties compare spoiler and
repetition scans with independent bounded enumerators. Focused tests cover byte
boundaries, entity suffixes, stage order, strict thresholds, all spacing modes,
raw limits, and limits 0, 3, 50, 70 and 100. Full cleanup is not idempotent:
blank collapse can expose repetition only on a later pass, which the source does
not perform. The spacing property checks admitted content and exact line errors.

Database/HTTP tests cover both posting aliases and encodings, HTML/JSON errors,
stored cleanup, escaped rendering, historical retention, denied policy changes,
unchanged rejection clocks/counters, and observed board-lock waits for changing
limits and spoiler policy. Approved attachments remain usable after line/spam
rejection and unusable after successful consumption. A no-JavaScript browser case
checks spoiler cleanup, 100/101 LF boundaries, spam errors and unchanged JSON/ETags.
`sudo bash scripts/test-comment-line-migration.sh` upgrades a separate owned
database through migrations 1-27 then 28, checks all 82 source defaults and actual
public privileges, and removes that database afterward.

Local unit/compile checks do not establish database, upgrade or posting-browser
results. Complete current-head hosted qualification is required before merge.
Source markup, name/subject sanitation and word filters remain unfinished.
Production containment, recovery and independent review remain launch prerequisites.
