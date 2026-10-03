# Poster IDs

Ordinary posts on boards with `user_ids` enabled show an eight-character ID beside their
name. The same verified network address keeps the same label within one thread.
Other threads, boards and deployment keys produce different labels. Addresses
shared through a NAT or proxy can therefore share an ID. A label identifies a
network posting context, not a person, account or authenticated staff member.

Badged staff posts use the [source static label](source-staff-poster-ids.md)
when IDs are enabled. That saved label appears in JSON; HTML keeps the badge
without a network ID span. Archived JSON omits both kinds of saved ID.

## Reference and hashing policy

The pinned [thread API](https://github.com/4chan/4chan-API/blob/2bd670d507ba2daa37a3961a661e088cf6f89d57/pages/Threads.md)
defines the optional eight-character `id` string. The
[boards API](https://github.com/4chan/4chan-API/blob/2bd670d507ba2daa37a3961a661e088cf6f89d57/pages/Boards.md)
defines the optional integer `user_ids` switch. Public extension v1191 renders
`posteruid` and `hand` spans and enables `IDColor` by default. Its `IDColor` and
string-hash functions establish the color calculation and presentation.
The release is pinned in [the public asset manifest](public-watcher-assets.json).

Public references do not specify the original server's hashing algorithm.
This deployment uses HMAC-SHA256 over a fixed version prefix, board, thread and
canonical IP address, then encodes six digest bytes as eight base64 characters.
The domain test includes an independently calculated Python HMAC vector.
This keyed replacement limits address guessing from public labels and avoids
publicly reusable labels across threads. It does not reproduce original ID
values. The choice is the E-014 security exception, with its scope stated here.

## Configuration and persistence

Set a private `POSTER_ID_KEY` of 64 hexadecimal digits in the public service's
environment. Generate it from 32 random bytes, independently of `TRIPCODE_KEY`.
Keep it out of source control, logs, media workers and staff authentication
configuration. Empty, malformed and all-zero keys fail startup. No key is
generated during a restart. Back up the key privately when continuity matters;
rotation changes IDs on subsequent posts, including replies to existing threads.
Saved posts retain their original labels.

After migration 0039, an operator may enable `content.boards.user_ids`. New
boards default to disabled. An enabled board rejects posting if the key or
verified connection identity is unavailable. Ordinary HTTP headers and form
fields cannot supply identity. IPv4-mapped IPv6 addresses use their canonical
IPv4 form. The existing authenticated Unix-proxy transport remains the only
way to substitute a client address for the connection peer.

The posting transaction creates the label after deciding the thread ID under
the board mutation lock. A transaction-local value feeds a constrained insert
trigger for ordinary posts and the existing scoped attachment operation.
Public SQL cannot insert or update the saved ID column, administer the board
switch or call the trigger. An enabled board's trigger rejects a missing
label. A disabled board discards the transaction value. Historical content and
timestamps remain unchanged. The additive migration can remain during rollback;
older binaries cannot post to an enabled ID board.

HTML, public JSON and bounded updater/preview projections expose only the saved
label. The JSON `id` field is absent on unlabeled posts; `user_ids` is absent
when disabled. ID filters use the public label. No new raw-address storage,
public address endpoint or staff-authentication authority is added.

## Browser behavior

Color user IDs is enabled by default and can be changed in Settings or a
reviewed settings restore. The client uses the released 31-based string hash,
its upper three bytes as RGB and the same luminance threshold for text color.
It assigns only fixed CSS properties to validated labels. Untrusted text never
becomes markup, a selector, a URL or arbitrary CSS.

The controller handles newly inserted replies and preview copies, changed
labels, settings changes and back-forward cache restoration. It bounds each
scan to 40,000 elements and 10,000 labels. Disabling the feature or removing its
root restores only the styles it still owns, preserving later changes by other
features. No extra HTTP request is needed to color a label.

[Private poster counts](private-poster-counts.md) use full, separately scoped
fingerprints rather than truncated labels or rendered posts. Privileged
capcodes remain unfinished. [Country and board flags](post-flags.md)
now have their own saved metadata, policy and qualification.

[Native ID controls](native-poster-id-controls.md) add click and keyboard
highlighting plus delayed loaded-post counts, with explicit ownership and
projection rules. These counts do not replace `unique_ips`.

## Verification

`cargo test -p board-domain --lib poster_id --locked` checks key validation,
the independent HMAC vector, address canonicalization and scope changes.
`cargo test -p board-public --test poster_ids --all-features --locked` checks
persisted posts, both JSON listeners, HTML and remote projections, missing keys
and peers, header spoofing, mutation denials, cache changes and disabled boards.
The browser color tests retain independent hash vectors and exercise the real
Chromium style policy, dynamic labels, preferences, BFCache and cleanup.
Executed results are recorded in [the batch verification](verification-rewrite-completion.md).
