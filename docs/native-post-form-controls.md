# Ordinary posting-form controls

With JavaScript enabled, the desktop form starts collapsed. Its bracketed
Start a New Thread or Post a Reply link reveals the table and disappears.
An initial `#reply` fragment performs the same reveal. Empty-catalog
new-thread links use that fragment in both server HTML and live search.

At widths up to 480 pixels, the top and bottom controls toggle the ordinary
form. They retain every field and change the top label to Close Post Form,
then Start New Thread or Post Reply when closed. The bottom control also
scrolls the top control into view. On a thread page with Quick Reply enabled,
either mobile control opens that editor instead, without quoting selected
page text. Disabling Quick Reply or the extension restores the ordinary
mobile toggle. Core form controls do not depend on optional keybindings.

Desktop and mobile expansion states stay separate across viewport changes,
as in the source's display and hideMobile rules. In particular, `#reply`
expands the desktop state but does not remove hideMobile. Posting authority
still comes from the server; revealing a form does not bypass a closed thread
or any posting limit.

Source evidence, inspected as text:

- `views/imgboard.php:23-30,63-65,561-565`: top/bottom mobile controls and
  the desktop link/table.
- `js/core.js:1406-1422,1523-1544,2042-2053,2431-2438,2484-2488`:
  Quick Reply dispatch, draft-preserving toggles, bottom scrolling, desktop
  reveal and initial fragment handling.
- `css/yotsubanew.css:1495-1504` and `css/yotsubamobile.css:144-146,953-958`:
  collapsed table, 22px bold centered desktop link and mobile override.
- `imgboard.php:3719`: no-JavaScript table fallback.

The rewrite keeps its ordinary server form visible until handlers are mounted.
It therefore needs no inline script or inline noscript stylesheet, and the
form remains usable if scripts are disabled or fail to load. It retains the
existing visible deletion-password security replacement. The separate
isolated-upload entry remains available; an approved upload-result form starts
expanded because it is already completing that security-specific workflow.
No raw-file parser or extra posting authority is added to the browser.

The fixture suite checks desktop/mobile transitions, draft retention, bottom
scrolling, Quick Reply selection behavior, disabled-extension operation and
absence on closed pages. Six-theme captures cover collapsed/expanded states.
The real browser suite submits ordinary desktop and mobile posts after using
the controls, with persistence, tracking, updater and isolated-upload coverage.
No-JavaScript posting tests continue to use the visible fallback directly.

Identity cookies, Pass/captcha, drawing, inline file selection and complete
source-page rendering remain unfinished. These controls do not establish
whole-page pixel parity or production deployment readiness.
