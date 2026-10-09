# Always use HTTPS

The supplied `js/extension.js` at `545b7812d1849f7958d914950c91fdbbe38f6b22`
defines this control for desktop and mobile. `Config.load` (8850-8867) uses the
`https` host cookie after reading a nonempty settings record. The stored
`forceHTTPS` boolean is overridden by whether that cookie equals `1`.
`Main.init` (9342-9347) navigates before importing settings or starting native
features, even when Disable All is enabled. `Config.save(old)` (8934-8948)
updates the cookie on an explicit Settings save. First-open initialization and
configuration import call `Config.save()` without `old` and leave it alone.

The rewrite exposes Always use HTTPS in Miscellaneous when `PUBLIC_ORIGIN` is
HTTPS on the document's current hostname. Board, thread, catalog and approved
upload pages receive that fixed configuration from the server. Request Host
headers, submitted preferences and transfer URLs cannot choose another host.
HTTP-only development configurations omit the control; they do not assume a
TLS listener exists. A configured HTTPS port may differ from the HTTP port.

An enabled startup preference navigates to the configured HTTPS origin while
preserving the path, query and fragment. Saving Settings clears the fragment
as the source does. HTTPS documents stay on HTTPS when the preference is
disabled. The cookie is host-only, lasts one year and uses `Path=/` and
`SameSite=Lax`. It is readable on HTTP so the startup check can run there. It
has no authentication or server-policy authority.

Closing or cancelling Settings does not change the cookie. A successful
persistent save sets or removes it. A denied cookie write leaves the dialog
open and reports that the other settings were saved; it does not navigate or
claim the HTTPS choice succeeded. Unavailable settings storage cannot enable
startup navigation. Import/export preserves the boolean, explains that the
cookie is not transferred, and requires the receiving browser to choose the
control in Settings to enable it.

Server HTTPS enforcement and verified proxy identity are independent of this
optional client preference. Disabling it cannot permit insecure production
requests. Malformed settings and oversized cookie headers are rejected by
finite client bounds. Endpoint validation rejects credentials, non-root
paths, queries, fragments and a different hostname.

`tests/fixtures/native-https-preference-source.json` contains bounded source
snippets tied to the original file hash. Run
`node scripts/record-native-https-preference.mjs --check --source 4chan-old/js/extension.js`
to compare them with the supplied source. `native-https-preference.test.mjs`
executes those callbacks with synthetic storage and cookie stubs, then runs
the actual native modules in Chromium. Its intercepted HTTPS destination
proves the navigation request and URL, not the availability of a deployment's
TLS listener. Production proxy/TLS qualification remains a separate check.
