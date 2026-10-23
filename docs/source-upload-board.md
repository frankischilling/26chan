# Upload-board compatibility

Issue #206 covers `/f/`. The supplied `views/upboard.php`, `config/boards/f.config.ini`
and `imgboard.php` define its listing, category admission and upload behavior.
The original player document is external to that source archive. Its embed
controls are present in `js/extension.js`; the external player's implementation
has not been established from the archive.

## Media boundary

The `swf-v8` transport carries at most 10 MiB to the disposable guest. It has a
separate request/response version and an 11 MiB stopped output disk. The guest
accepts uncompressed FWS and zlib-compressed CWS, inspects signed RECT geometry
and checks tag framing through the final End tag. It never runs ActionScript.
The output preserves the submitted bytes. Host validation reads only the fixed
envelope, format signature and hashes, not compressed movie structures.

The guest rejects truncated streams, inconsistent declared lengths, trailing
bytes, zero or reversed geometry and expansion beyond 32 MiB. These are
intentional security differences from PHP's header-only admission. Valid
positive geometry retains integer twip-to-pixel truncation. Signed negative
origins use the actual canvas size instead of reproducing PHP's unsigned wrap.
Metadata inspection does not allocate a pixel canvas or impose the ordinary
image dimension ceiling.

The profile uses the existing small-guest external resource limits, including
its memory, CPU, wall-clock and process limits. SWF validation does not authorize
publication or make executable content safe for the board origin. Integration
with posting authority and a separately isolated player is still required.

## Reference checks

`fixtures/swf-header-reference.json` contains twelve owned movies and their
PHP 8.3 `getimagesize` results. This identifies the qualification runtime; the
historical production PHP version is unknown. Cases that expose malformed or
unsigned-wrap geometry are recorded separately from matching valid inputs.

`fixtures/upload-board-reference.json` captures the original category table and
admission statements, including numeric coercion. Its 35 cases cover ordinary
identifiers, decimal/exponent forms, whitespace and rejected values. Form order
is 0, 6, 1, 2, 3, 5, 4. Missing subject prefixes select Other for HTML; unknown
numeric prefixes are stripped but do not acquire a known category.

Local guest and transport checks are separate from the real VM qualification.
The latter must run on the owned CI Linux host before this feature can be
considered qualified. These primitives do not yet establish complete `/f/`
listing, posting, download or playback parity.
