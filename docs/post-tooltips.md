# Post tooltips

Mobile shortened names show their full text after a 300 ms hover. Hovering the
mobile date or either number link shows the relative posting date after 500 ms.
Thread statistics show their existing reply, image, poster and page labels.
The original post links keep their navigation and Quick Reply behavior.

[Released helper vectors](public-post-tooltip-reference.json) pin core v1128
and extension v1191. They cover the date formatter's unit boundaries, timer
cancellation and the full-label callback's handling of synthetic names and
filenames. The direct subject shape produced by the released formatter returns
no full-subject callback value; this implementation retains its escaped native
`title` instead of inventing a callback result. The filename vectors describe
the reference only; no custom filename tooltip is implemented here.

[Style facts](public-tooltip-style-reference.json) record 18 synthetic cases
across six pinned stylesheets, with left, center and right placement. They
capture tooltip and arrow properties in Chromium 151.0.7922.34 at 1280 by 900
pixels and density 1. Tests compare the production page controller and styles
with those facts. These comparisons cover the tooltip, not the original full
page or original-server name serialization.

## Content and lifecycle

Names and dates must belong to a bounded, validated original post recipe.
Mobile names must match the full desktop name. Malformed timestamps, mixed
headers, detached posts and copied quote presentations cannot become tooltip
sources. Statistics accept only their fixed label vocabulary. No stored
callback name is invoked, and all text enters the tooltip through `textContent`.
This replaces the released callback's HTML insertion, including its treatment
of filename titles, with inert text and finite admission.

The full-name `title` remains available without scripting. A relative-date
tooltip temporarily suppresses only the local-date controller's own timezone
title and restores it when dismissed. A later title change or retired date
controller cannot be overwritten by that restoration.

Mouse exit, resize, scroll, theme changes, extension disablement and removal of
the original post dismiss the tooltip and pending timer. BFCache suspension
clears transient state; restoration rearms the controller. Teardown removes
owned nodes and descriptions. There is one tooltip and no additional network,
worker, media, posting or storage authority.

```sh
node scripts/verify-public-post-tooltip-reference.mjs /path/to/extension.1191.js /path/to/core.1128.js
node scripts/verify-public-tooltip-style-reference.mjs /path/to/core.1128.js /path/to/desktop-css
npm run test:post-tooltips
npx playwright test --config playwright.themes.config.js tests/themes/post-tooltips.spec.js
```
