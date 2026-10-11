# Source bans and warnings

Issue [#220](https://github.com/frankischilling/26chan/issues/220) covers the
missing restriction system. This branch starts its source comparisons; it
does not yet enforce bans, issue warnings or add a public status page.

## Posting decision

The supplied `imgboard.php:8554` checks active matches for the deletion
credential, Pass and transport address. Credential/Pass fields retain the
caller's order; the address field is appended unless a present Pass and the
qualified CAPTCHA bypass both authorize skipping IP bans. A supplied browser
checkbox or unauthenticated account string establishes neither fact.

Warnings have a nonzero end timestamp at or before their start. They block
before the expiry, board/global scope and entitlement exceptions are checked.
Consequently a warning from another board still requires viewing the warning.
Other finite restrictions expire at `end <= request time`; a zero end denotes
an indefinite restriction. The source retires only expired rows visited before
the first warning or applicable ban. Its result order is not specified by SQL.

Pass matches skip template 124 after warning and expiry handling. A verified
user skips an address match issued by exactly `Auto-ban` when the complete
persisted reason contains case-sensitive `Proxy`, including the private part.
Other address matches and deletion-credential bans remain applicable. The
rewrite must derive these facts from server authority, retain private reasons
behind staff access and define a stable database order explicitly.

`admin.php:353` calls the batch operation `cpban`. It bans the OP globally,
bans each distinct reply address except the OP address globally with a
separate duration/reason, then deletes the thread. The supplied posting
`check_for_ban` receives a thread ID but never uses it. A restriction confined
to that thread would replace the actual source behavior.

## Remaining implementation

Staff issuance requires board/global authority, current session and board
checks, private target resolution, bounded reasons, durable audit, expiry and
unban. Warning viewing needs an authenticated acknowledgement boundary that
cannot clear another identity's warning. Public/staff posting and report
admission must enforce restrictions in the transaction that writes activity,
including races against new bans and expiry. Private identity material must
not become readable or writable by application roles.

The source form also supports templates, warning/finite/indefinite duration,
configured public notices, deletion, file deletion, all-poster deletion,
reply-only deletion, thread movement/archive/close/permasage and manager-only
special restrictions. The public recent-bans task filters active public
templates and omits `/test/` and `/j/`. These flows need implementation and
independent source comparisons, not inferred behavior from the posting helper.

The supplied snapshot has no public `/banned` controller, warning acknowledgement
or appeal template, authoritative ban-template rows or Pass provider. Their
contracts must be identified separately from executed source behavior.
Original media preservation and unavailable transfer modes retain their
existing issue boundaries. No unsafe original-file publication is introduced.

## Comparison boundary

`scripts/extract-ban-enforcement-reference.py` executes the unchanged pinned
source function through PHP. Only exact active synthetic database matches,
request time, salt and recorded expiry writes are stubbed. A fresh owned empty
directory confines thumbnail cleanup. The fixture includes field and row ordering, warning
precedence, expiry boundaries, scopes and the two qualified exceptions.
`crates/domain/tests/ban_source.rs` compares the pure Rust policy with every
executed result. Neither test proves real identity binding, transactions,
staff permissions, warning acknowledgement or page fidelity. Runtime and
hosted qualification remain required for the eventual implementation.
