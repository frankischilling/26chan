# Dice rolls and fortunes

Issue #210 implements the posting randomizers from the pinned source revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`.

`imgboard.php:5548-5560` handles fortunes. The active list has 13 entries and
the feature runs when the Options field is exactly `fortune` after removing
all case-insensitive `sage` occurrences. Spaces are retained. The
Christmas 2021 list in the same branch is commented out, so it is recorded as a
seasonal variant rather than active behavior. The selected fortune and its
derived color are generated once during posting and stored with the post.

`imgboard.php:5562-5595` handles dice. Its pattern is
`dice[ +](\d+)[ d+](\d+)(([ +-]+?)(-?\d+))?`, searched inside the Options
field and matched case-sensitively. Roll count is capped at 25. The source's
non-greedy sign capture also means `dice+2d6-3` and `dice+2d6 -3` do not
produce the same modifier; the rewrite preserves that behavior. The displayed
prefix is `Rolled`, from `config/global_strings.ini`.

The pinned board configuration enables dice on `/b/`, `/mlp/`, `/qst/`
and `/tg/`. It enables fortunes on `/b/` and `/s4s/`. Migration 0055
stores those switches on the board row. The posting transaction locks that row
before parsing the feature, so changing the policy while a post is waiting
cannot produce a result under stale settings. Other boards leave dice-looking
or fortune-looking Options text alone.

## Numeric and rendering bounds

The source limits roll count to 25 but otherwise relies on native PHP integer and
`mt_rand` behavior. The rewrite rejects zero rolls and zero-sided dice, caps
sides at 2,147,483,647, and accepts modifiers only within signed 64-bit range.
These are deliberate resource and panic-safety bounds. Totals use a wider
integer internally, so a valid maximum modifier cannot overflow while adding
rolls. A malformed programmatic `DiceRequest` is validated again by the
generator before randomness is requested.

Randomness comes from the operating system through `ring::rand::SystemRandom`.
Failure aborts the post before thread or post mutation. Stored dice text,
fortune text and the six-digit fortune color are constrained by the database.
The public JSON projection retains the source-style inline color representation.
Live pages map the 13 source-derived colors to fixed external CSS classes so the
fortune remains colored under the site's `style-src 'self'` policy. Templates
provide the fixed `<b>`, `<br>` and fortune wrapper elements while escaping
the stored text. Clients never supply trusted HTML or generated metadata.

## Operations and rollback

Apply migration 0055 before deploying a binary that reads the two new board
flags or three post metadata columns. Existing boards are updated to the pinned
source settings above; synthetic boards default to both features disabled. Binary
rollback should keep the additive columns in place because older binaries ignore
them.

Posting, the retained randomizer result and attachment consumption share one
transaction. A rejected parent or later insertion error rolls the generated
metadata back with the post. Page refreshes, thread JSON, board JSON, catalogs
and updater, search and RSS projections read the stored result instead of
rolling again. Catalogs apply the source tag stripping and `/b/` truncation
after adding the generated text, so a dice prefix uses part of that teaser's
300-character allowance.

The focused domain tests cover grammar quirks, maximum modifiers, invalid
constructed requests and randomness failure. The database test covers board
guards, stable reloads and rollback; its exercise runs in a spawned task so
fixture cleanup still executes after an assertion panic.

The [verification record](verification-posting-randomizers.md) lists the source
fixture checks, persisted tests, browser scenarios and current CI limits.
