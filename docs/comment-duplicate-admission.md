# Duplicate-comment admission

The source checks equal comments from the same host on the same board before its
ordinary posting timers. The candidate timestamp must be strictly newer than
request time minus 300 seconds. It uses the greatest timestamp, including future
timestamps; the exact five-minute boundary is allowed. PHP treats a zero maximum
as false. Quiet rejection and staff exemptions precede this check. VIP and Pass
membership alone do not exempt a poster.

The extracted branch and `sec2hms` formatter are pinned in
`fixtures/comment-duplicate-source.json`. Run:

```
python3 scripts/extract-comment-duplicate-reference.py ../26chan-reference --check
php -n scripts/test-comment-duplicate-source.php
cargo test --locked -p board-domain comment_duplicates
```

The PHP oracle covers 1,260 synthetic cases for role, quiet rejection, meta-board
settings, name/comment truthiness and temporal boundaries. Four Rust tests pass,
including comparison against every oracle result. The oracle supplies scoped SQL
candidates; it does not establish the original deployed database collation.

This branch currently contains the domain model and source fixtures. Protected
history storage, transactional writer integration, Oekaki/EXIF comparison paths,
and browser qualification remain unfinished. Issue #213 stays open. Existing
rendered HTML must not be substituted for source comparison data.
