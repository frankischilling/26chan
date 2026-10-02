# Remembered posting preferences

Successful ordinary and Quick Reply posts remember the display name and Options
field in the public host's `4chan_name` and `options` cookies. The next ordinary
form restores those values; Quick Reply copies that form's current fields.
Restoration leaves an existing draft intact and ignores a hidden Name field on
a forced-anonymous board. Deletion passwords remain empty after navigation.

## Reference

The pinned [public extension v1191](https://s.4cdn.org/js/extension.min.1191.js)
uses those two cookie names in `QR.show`, with percent decoding through
`Main.getCookie`. Its generic `Main.setCookie` helper uses a one-year lifetime.
[The reference manifest](public-watcher-navigation-reference.json) records the
release's SHA-256. Only released client text was used for this addition.

The supplied `4chan-old/imgboard.php` establishes the server writers: options
at 5634 and name at 6795 both use seven days. The local writer now uses that
seven-day maximum age and updates preferences only after
a committed post. Failed requests do not change them. A blank value clears an
existing preference. Forced-anonymous posts leave the remembered name from
other boards alone. The [anonymous session](anonymous-sessions.md) uses a
separate one-year HttpOnly cookie. These checks do not establish full original
posting parity.

## Security restrictions

Only the display portion before `#` is remembered. A tripcode password must be
entered again after navigation. Restoring an old or manually supplied name
cookie also removes its suffix. The local writer does not retain capcode
authentication attempts in Options. No deletion password, secure-trip key,
upload capability or staff credential is stored in these preferences.

Cookies are host-only, use `Path=/` and `SameSite=Strict`, and require `Secure`
in production. They are readable by the public page because the page restores
the fields; they grant no posting, deletion or staff authority. The parent-domain
cookie scope and reusable private name suffix are deliberately excluded.
These restrictions are the E-013 security exception.

Values are percent-encoded for cookie syntax. Restoration accepts only the two
fixed names, at most 4,096 characters of cookie text, at most 300 encoded
characters per value and 100 decoded UTF-8 bytes. Duplicate keys, malformed
encoding and control characters use an empty default. Form values are assigned
through the input's `value` property, never interpreted as markup.

An unavailable cookie reader leaves the form usable. Browser rejection of a
cookie does not turn a committed post into a failed post or defer restoration
until cookies are enabled. These values are separate from the short-lived
posting receipts and the settings export format.

## Verification

Three Rust unit tests cover encoding, secret removal, expiry, scope and invalid
values. Seven JavaScript cases cover decoding, duplicate keys, UTF-8 limits,
legacy suffixes, drafts, hidden fields and denied storage. The persisted handler
test exercises both posting routes and both form encodings, secure-cookie
attributes, failed secure posting, capcode attempts and forced anonymity.
The earlier JSON posting tests retain their exact receipt scopes and now assert
the two new preference cookies separately.

Browser cases cover both editors, failed posts, hostile cookie text and an
unavailable reader. The actual full-Chromium cookie-policy test adds these
preferences to its healthy and blocked controls, including a restart after
rejection. Executed results and current publication limits are recorded in
[the combined verification record](verification-rewrite-completion.md).
