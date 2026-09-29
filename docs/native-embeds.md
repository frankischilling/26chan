# Native YouTube and SoundCloud embeds

Board and thread pages add a small control after eligible YouTube and SoundCloud
links. The original post link is left in place. On desktop, YouTube shows
**Embed** by default. On the mobile layout it shows **Open** and does not create a
player. SoundCloud shows **Embed** only when `embedSoundCloud` is enabled.
`disableAll` removes every control and active player.

## Permitted behavior reference

The compatibility reference is the released public extension v1191,
`https://s.4cdn.org/js/extension.min.1191.js`, retained locally with SHA-256
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
Its `Media` helpers and `Parser.parsePost` establish the visible `Embed`,
`Remove`, and mobile `Open` controls, along with the desktop YouTube and
SoundCloud settings. The public defaults are `embedYouTube: true` on desktop,
an explicit `false` mobile override, and `embedSoundCloud: false`. The public
parser still adds the mobile YouTube `Open` control independently of that
mobile default.

No private or leaked source is used for this module. The public asset is read as
text; it is not executed in the application or test fixtures.

Current YouTube embed requirements were also checked against Google for
Developers on September 28, 2026:

- `https://developers.google.com/youtube/terms/required-minimum-functionality`
  requires an HTTP Referer or equivalent client identity for embedded players
  and recommends `strict-origin-when-cross-origin`.
- `https://developers.google.com/youtube/iframe_api_reference` documents error
  153 when the player request lacks that identity.

SoundCloud's current oEmbed and Widget API documentation was checked at
`https://developers.soundcloud.com/docs/oembed` and
`https://developers.soundcloud.com/docs/api/html5-widget`. It documents the
`w.soundcloud.com/player/` iframe and its `url` and `auto_play` parameters.

## Provider URL rules

Provider detection reads existing post anchors. It never turns arbitrary HTML
into provider markup. Every candidate is bounded to 2,048 characters, must use
HTTPS, and must not contain credentials, a non-default port, ASCII controls,
whitespace, or a backslash.

YouTube accepts these source forms:

- `https://www.youtube.com/watch?v=VIDEO_ID` and the equivalent bare
  `youtube.com` host;
- `https://youtu.be/VIDEO_ID`;
- one optional `t` parameter in decimal seconds or the public client's
  minute/second form, including values such as `90s` and `90m`, capped at
  seven days. The single `t` may be in the query or query-shaped fragment.

The video ID is exactly 11 URL-safe identifier characters. A watch URL must have
exactly one valid `v`, and a source may have at most one `t`; duplicate `v` or
`t` values, including a query plus fragment `t`, are rejected as ambiguous.
Other source parameters are retained on the provider/Open URL and ignored for
the player. This includes common `youtu.be` `si` parameters and watch `list`,
`index`, and sharing parameters. Arbitrary ignored values are safe to accept
because none is copied into the iframe URL. Embed URLs, alternate subdomains,
HTTP, credentials, and non-default ports are rejected. Timing preservation is a
deliberate subset of v1191's broad `[ms0-9]+` token: decimal seconds and ordered
minute/second forms are converted to integer seconds up to seven days. An
unsupported single `t` value does not suppress the video control; its timing is
dropped so no unvalidated player parameter is forwarded. The generated player
URL is always under `https://www.youtube-nocookie.com/embed/` and contains only
the validated video ID plus an optional integer `start` value.

SoundCloud accepts the exact `soundcloud.com` host with a canonical lower-case
profile, track, or `/artist/sets/name` path. Query strings, fragments, shortened
hosts such as `snd.sc`, alternate hosts, and additional path forms are rejected.
Resolving those variants would require redirects or provider metadata before a
user asks to load a player. The generated frame is always under
`https://w.soundcloud.com/player/` and sets `auto_play=false`.

Native linkification uses a same-origin `/derefer?url=...` destination. Such an
anchor is eligible only when it has the native-linkified marker, contains one
`url` parameter, and the decoded parameter exactly matches its visible URL. The
provider parser then applies the same rules above. This keeps a linkified post
eligible without trusting a class or data attribute by itself.

## Network and frame boundary

Scanning, displaying an affordance, and hovering a link create no iframe and
perform no provider request. There is no YouTube thumbnail request and no
SoundCloud oEmbed lookup. A normal left click on **Embed** is the operation that
creates the iframe; its `src` is assigned only after all attributes and the
owned container are in place.

YouTube frames use `referrerPolicy="strict-origin-when-cross-origin"`. The site
otherwise uses a same-origin referrer policy, which would suppress the client
identity YouTube now requires. Cross-origin player requests therefore receive
the board site's origin after the click, without its path or query. SoundCloud
frames use `no-referrer`. Both frames are sandboxed and have a fixed permission
set. YouTube uses the privacy-enhanced host and permits fullscreen; SoundCloud
permits audio playback while its URL keeps autoplay disabled. No provider
script is loaded into the parent document.

The CSP integration permits only the two player path prefixes above in
`frame-src`. The module has no configurable frame origin and never accepts a
provider-supplied iframe fragment. This differs from v1191, which fetched
SoundCloud oEmbed HTML and inserted it, and from its YouTube hover preview,
which fetched a remote thumbnail before an embed click.

## DOM ownership and lifecycle

The board scan visits at most 32,768 elements and 4,096 anchors, with a maximum
source-to-board depth of 32. At most eight provider frames may be open. Tests can
lower those ceilings but cannot raise them. Crossing a scan ceiling fails closed
and removes active controls and frames.

Generated controls and player containers are claimed by the shared comment
projection. Quote serialization and synthetic copies therefore retain the
original provider link and omit the enhancement UI. The projection intentionally
keeps weak ownership after node removal so queued mutation records can still be
identified; it exposes no release operation. The embed controller drops its
strong references when entries close or the mount is destroyed.

A source hidden by the post/thread hiding classes or `hidden`, removed from the
board, or disabled through settings loses its control and player. Player `src`
is removed before the frame is detached. A persisted `pagehide` suspends the
mount and clears all provider frames; `pageshow` restores only fresh controls.
Players do not resume automatically. A terminal page exit destroys the mount.
Mounting again on the same board destroys the previous owner, and an old retained
toggle cannot create a frame after suspension or destruction.

The fixed CSS hooks are `nativeEmbedControls`, `nativeEmbedToggle`,
`nativeMediaEmbed`, `nativeMediaEmbedYouTube`, `nativeMediaEmbedSoundCloud`, and
`nativeEmbedFrame`. Shared board CSS owns responsive sizing. YouTube keeps a
16:9 area with a minimum player height of 200 pixels and a maximum container
width of 640 pixels; SoundCloud uses its fixed responsive player height. The
JavaScript does not construct style values from post content.

## Verification

`tests/browser/native-embeds.test.mjs` covers canonical URL parsing, the
click-only network boundary, YouTube's origin-only Referer, projection output,
native-linkified anchors, hidden and detached posts, global disabling, scan and
frame ceilings, duplicate mounts, stale controls, and BFCache restoration.

`tests/browser/native-embeds.spec.js` exercises the same contract against an
owned real thread: persisted provider links remain unchanged, hover performs no
external request, the real controls create only the two CSP-approved frames,
`disableAll` removes them, and the mobile layout exposes YouTube as **Open**
without preloading a player.
