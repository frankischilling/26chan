# Native layout and dark-theme options

The pinned public v1191 client exposes `compactThreads`, `centeredThreads` and
`darkTheme` in its native settings. These preferences remain browser-local and
have no content, posting or authentication authority.

## Observed behavior

On desktop, `compactThreads` takes precedence when both layout options are
enabled. Compact mode limits each thread to 75% of the browser width. When
compact mode is not active, centered mode makes post containers 75% wide,
centers them, hides reply side arrows and offsets the thread summary by 12.5%.
The local board DOM names that summary row `.omitted`.

The public mobile layout is active when `(max-width: 480px)` matches and
`4chan_never_show_mobile` is not the exact string `true`. Mobile policy forces
compact mode off. A stored centered preference can therefore become effective
on mobile even when both preferences are stored as true. Crossing the viewport
boundary or changing the mobile opt-out recomputes the effective layout.

On desktop, dark mode selects Tomorrow through a finite stylesheet override.
Mobile dark mode keeps the selected desktop stylesheet and applies the released
`m-dark` class to the body. The ordinary server theme preference stays in its
existing HttpOnly `board-theme[-ws]` cookie. Crossing the mobile boundary restores
the ordinary stylesheet before applying mobile dark colors.

`disableAll` suppresses all three options. With JavaScript disabled, none of
the browser-local layout state is applied and the server-selected stylesheet is
left unchanged.

## Runtime ownership

`native-layout.v1.js` derives state from the settings reader and the existing
mobile-layout inputs. It does not infer preference state from body classes.
The module owns the layout attribute:

```text
data-native-thread-layout="compact"
data-native-thread-layout="centered"
```

The shared board stylesheet maps those finite values to the observed public
rules. The runtime never accepts CSS text or selectors from storage.

It also sets `data-native-never-mobile` to the effective opt-out and owns any
`m-dark` class it adds. The stylesheet uses that opt-out to show the desktop
header at narrow widths. An existing dark class is preserved, and teardown
restores the prior opt-out only while the controller still owns its value.

Desktop dark mode operates only on the marked server theme stylesheet. The ordinary
stylesheet must be a same-origin `/static/theme.css` URL with no query other
than an optional single `worksafe=true|false` value. The runtime derives a
Tomorrow request by adding `theme=tomorrow`. Foreign origins, credentials,
fragments, duplicate parameters and unknown parameters are rejected.

The exact ordinary `href` is remembered when the override begins. Turning
dark mode off, enabling `disableAll`, a stylesheet load error, teardown, or a
non-BFCache page exit restores that `href` only while the runtime still owns
the override. If the ordinary theme request changes while dark mode is active,
the next refresh treats the new valid request as the one to restore. This lets
manual theme selection remain authoritative without changing the HttpOnly
theme cookie behind the user's back.

The Style link clears the stored `darkTheme` request through the normal locked
settings writer before navigating to the server theme chooser. A manual theme
choice therefore cannot remain visually masked by a stale dark-mode request.

The controller announces `boardThemeChanged` after the requested stylesheet has
loaded and its computed icon family matches the expected theme. Existing watcher
and navigation icons, post-menu controls, hiding controls, backlinks and image
presentation then refresh from the loaded family. An expanded image keeps its
existing element. A display-density change separately refreshes navigation icons
so moving a page between displays updates the `@2x` asset choice.

## Lifecycle and synchronization

The controller listens for the normal `4chanSettingsSaved` event, cross-tab
storage changes for `4chan-settings`, the mobile opt-out key and storage clear,
plus changes from the shared 480px media query. These inputs are recomputed
instead of trusting DOM state.

A persisted `pagehide` suspends refresh work without removing the current
presentation. The matching persisted `pageshow` recomputes settings, viewport
policy and theme state. A normal page exit destroys the controller. Teardown
restores the attributes, dark class and stylesheet `href` that the controller
still owns, so another owner that replaced an attribute or URL is left intact.

## Shared integration contract

The shared settings UI exposes the three public labels as independent
checkboxes. Storing both layout preferences is intentional because their
effective precedence changes on mobile. The watcher bootstrap mounts
`mountNativeLayout` with the existing settings reader, the shared
`(max-width: 480px)` media query, the existing exact mobile opt-out reader and
the marked theme stylesheet.

The shared stylesheet supplies these finite rules for the module-owned body
attribute:

```css
body[data-native-thread-layout="compact"] .thread { max-width: 75%; }
body[data-native-thread-layout="centered"] .opContainer { display: block; }
body[data-native-thread-layout="centered"] .postContainer { margin: auto; width: 75%; }
body[data-native-thread-layout="centered"] .sideArrows { display: none; }
body[data-native-thread-layout="centered"] .omitted { margin-left: 12.5%; display: block; }
```

The theme endpoint accepts only the finite transient Tomorrow override needed
by this controller. The normal `/settings/theme` preference flow and its cookie
remain unchanged.

## Verification

The self-contained Node and Chromium tests cover precedence, exact mobile
opt-out semantics, same-tab and real cross-tab changes, `disableAll`, strict
theme URL derivation, stylesheet failure recovery, requested-theme restoration,
BFCache suspension/restoration and teardown ownership:

```text
node --test tests/browser/native-layout.test.mjs tests/browser/native-layout-dom.test.mjs
```

`tests/browser/native-layout.spec.js` exercises the integrated application. It
saves all three options through the real Settings dialog, checks layout CSS
across desktop/mobile policy changes, verifies cross-tab disabling, proves that
Tomorrow does not replace the HttpOnly Photon preference, checks manual Style
navigation clears the dark request, and verifies the no-JavaScript baseline.

The media fixture uses the same server templates, assets and normal bootstrap.
Its image-control test holds the Tomorrow stylesheet response while the page
still uses Photon, then verifies the loaded transition and cross-tab restoration.
It checks the existing image controller and expanded image directly.
