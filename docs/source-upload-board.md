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
publication or make executable content safe for the board origin. Publication
requires an immutable input receipt, the current processing lease and an
approved SWF manifest. Posting consumes that capability once, on an upload-board
opening post. Replies cannot carry attachments. Filename claims are serialized
by a private, case-sensitive database constraint; deletion and thread retirement
release the claim. Public file-only deletion retains the source refusal.

## Reference checks

`fixtures/swf-header-reference.json` contains twelve owned movies and their
PHP 8.3 `getimagesize` results. This identifies the qualification runtime; the
historical production PHP version is unknown. Cases that expose malformed or
unsigned-wrap geometry are recorded separately from matching valid inputs.

`fixtures/upload-board-reference.json` captures the original category table and
admission statements, including numeric coercion. Its 36 cases cover ordinary
identifiers, decimal/exponent forms, whitespace and rejected values. Form order
is 0, 6, 1, 2, 3, 5, 4. Missing subject prefixes select Other for HTML; unknown
numeric prefixes are stripped but do not acquire a known category.

Local guest and transport checks are separate from the real VM qualification.
The latter must run on the owned CI Linux host before this feature can be
considered qualified. These primitives do not yet establish complete `/f/`
listing, posting, download or playback parity.

## Player runtime preparation

`scripts/media/prepare-swf-player.py` prepares the official Ruffle 0.7.1
self-hosted release in a new directory. It verifies the release archive against
the pinned SHA-256 before extracting a fixed asset list. Both WebAssembly
modules receive a maximum of 4,096 memory pages (256 MiB); their reviewed
73-page minimum is unchanged. The output manifest records each resulting hash.
The HTTP-side asset loader verifies against the manifest compiled into the
application, not a mutable manifest beside the files.

This cap applies only to WebAssembly linear memory. It is not a limit on total
browser, JavaScript or GPU memory, nor a guarantee that every movie plays.
Player loading and interaction still need final-head browser qualification.

Ruffle documents `allowNetworking: "none"` as unimplemented. That option must
not be treated as a network boundary. The player integration needs a restrictive
browser Content Security Policy and sandbox independently of player options.
The player document uses an opaque-origin sandbox with scripts enabled but no
same-origin, form, popup or top-navigation permissions. Its connection policy
allows only the selected movie and pinned WebAssembly files. The raw download
endpoint remains attachment-only; the separate player-data endpoint accepts
anonymous cross-origin reads of already-public, approved bytes.
See the [Ruffle networking API documentation](https://ruffle.rs/js-docs/master/enums/Config.NetworkingAccessMode.html).

## Listing and thread controls

The upload listing uses the source column order, category labels, filename and
subject truncation, 30-row window and oldest-row highlighting. The 254 owned
row cases include escaped text boundaries and the source's distinct listing
and thread size arithmetic. Thread pages show file metadata without an image
thumbnail. JSON retains SWF geometry and zero thumbnail dimensions. The initial
PHP-shaped page and client-rendered quote/update fragments stay distinct: the
client uses its regular size arithmetic, full filename and JSON category value.
A preview receives only an inert download link, never a player iframe. Missing
and unknown historical JSON tags retain the source client's `undefined` and
`null` labels. New posts still require one of the seven admitted categories.

Embed creates the player only after a click. Desktop listing playback uses a
closeable overlay; thread playback uses Embed/Remove at the file information.
Mobile listing pages omit the extra Embed column. Source sizing arithmetic is
covered by 30 captured cases. The browser suite compares table fonts, colors,
padding, borders, dimensions and old-row highlights against pinned source CSS
for all six themes. It saves owned component screenshots for review; this is
not a comparison of the surrounding full-page layout. Display frames reject
dimensions above 8,192 pixels or an area above 16 megapixels; this is a browser resource
limit, not a change to stored movie metadata.

## Qualification and remaining work

The upgrade exercise covers both SWF migrations, rejection of existing schema
or grant drift, preservation of historical content, private filename authority
and an administrator dump/restore. Database tests exercise capability
consumption, filename reuse after retirement and the public file-only refusal.
The no-script HTTP test covers real intake, approval, posting, original-byte
downloads and capability revocation after a malformed trailing field. A real
browser version runs with page scripts disabled; hosted qualification remains
required. Other legacy forms retain their 256 KiB request-body limit.

The real-browser player test uses owned FWS/CWS movies and checks visible
playback, opaque-origin storage denial, network blocking and original-byte
downloads. The test must pass in CI; local unit checks do not establish playback.

The JavaScript-enabled `/f/` form submits the selected file and category through
the isolated upload service, waits for approval, then posts once. An expired
CAPTCHA keeps the approved file while requesting fresh proof. A lost posting
response blocks a duplicate attempt in that page session. Without JavaScript,
the same form streams its file to intake within the posting request and waits
for approval before invoking ordinary posting admission. It does not decode
the movie, retain the draft in a database or queue a background post. The
existing handler deadline still applies; processing that does not finish in
time returns a retryable error and revokes the unused capability. Quick Reply
replies remain attachment-free. All-theme visual comparison and final-head
browser qualification remain open. Ruffle is a replacement runtime, not evidence
that every historical Flash movie behaves identically. External networking and
persistent Flash storage are intentionally unavailable.

## Enable the local player

Prepare the pinned runtime once, then give the separate media reader its
absolute directory:

```bash
python3 scripts/media/prepare-swf-player.py .local/swf-player
export MEDIA_PLAYER_DIR="$PWD/.local/swf-player"
```

Set this variable in the reader's environment, alongside `MEDIA_APPROVED_DIR`,
`MEDIA_ORIGIN`, `MEDIA_BIND_ADDR` and its restricted reader database URL.
`PUBLIC_ORIGIN` must match the public site's origin; it controls which page may
frame the player. Follow [the reader setup](media-http.md) for separate roles
and storage. Do not pass writer or migration credentials to the reader.

An omitted player directory disables player routes without disabling original
SWF downloads. A relative directory or a runtime file that fails the compiled
hash manifest makes startup fail. The browser UI only receives the media
origin, never the directory, database credentials or upload capabilities from
other posts.
