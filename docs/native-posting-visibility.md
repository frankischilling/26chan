# Posting forms on closed threads

The supplied `views/imgboard.php:16-17,42` omits the posting form when the
current thread is closed or archived. The rewrite's server-rendered thread
page applies that rule to both the text editor and the isolated-upload entry
form. A board index still offers a new-thread form even if a listed thread is
closed. Catalogs remain form-free, and an open thread retains both supported
posting workflows.

Quick Reply retains its closed-thread quote alert even when the page has no
ordinary editor. Q and post-number clicks cannot create a replacement editor
or navigate while that alert applies. Global/Quick Reply disabling still
controls those handlers. The controller cannot submit without a rendered
posting form.

This rendering rule does not grant or revoke server authority. Both public
posting aliases independently reject stale replies to closed threads. An
already-open page keeps its draft disabled across updater close/archive
transitions and restores it on reopening. A page initially loaded without a
form needs a reload after reopening to obtain the ordinary form.

`posting_options.rs` exercises actual persisted open, closed and reopened
thread HTML, the healthy board-index form and rejection through both posting
aliases. `closed-posting.spec.js` checks production templates at desktop and
mobile sizes with JavaScript enabled and disabled, healthy rendered media,
retained reporting/deletion controls and closed Quick Reply alerts. These
tests do not establish full original-page visual parity.

The source has a `/qa/` capcode-specific exception. The rewrite does not yet
implement that board's capcode policy, so that exception remains unsupported;
closed threads stay read-only. Desktop/mobile form toggles, identity cookies,
Pass/captcha and complete posting-form source parity are separate work.
