# Comment character limits

The pinned API documents `max_comment_chars` as a character count, but the current board setting and post validation count UTF-8 bytes. A board advertising 4,000 characters therefore rejects 4,000 accented letters or emoji. Correct this mismatch across validation, persistence, public JSON and rendered public/staff comments. This extends I-001 and B-003; it does not establish undocumented original Unicode or posting behavior.

## Global constraints

- Count Unicode scalar values using Rust `chars()` and PostgreSQL `char_length` in a UTF-8 database. Combining marks count individually. Preserve existing text, numeric board settings, whitespace, normalization and newline behavior; CR and LF count individually when both are received. The original service's code-point/grapheme/UTF-16 distinction remains unknown.
- Keep board limits between 1 and 16,000 characters. Add shared `MAX_COMMENT_CHARS = 16_000` and `MAX_COMMENT_BYTES = 64_000` constants in the domain crate. Validate the independent byte ceiling before counting; reject blank comments and existing unsupported controls as before.
- The shared formatting parser must preserve all accepted comments, including 16,000 four-byte characters, and independently stop at 16,000 scalar values on oversized input. Keep typed tokens, nonrecursive formatting, safe URL policy and escaped Askama rendering.
- Rename the persisted and Rust board field to `max_comment_chars` with migration 0007; do not edit old migrations. Preserve numeric settings and existing post text. Replace the global post byte constraint with explicit 1..16,000 characters and at most 64,000 bytes; require UTF-8 database encoding. Retain all database grants.
- Allow up to 256 KiB per public form request, enough for a 192,000-byte percent-encoded maximum comment plus bounded form fields. Continue enforcing the streaming bound without relying on Content-Length. Staff's 32 KiB auth/form bound, concurrency, rate limits, timeouts and media enablement remain unchanged.
- Show the board comment limit as characters. Do not add a browser maxlength measured in UTF-16 or a client-only authority check. Both posting routes, replies, HTML/JSON/staff previews and cache invalidation must use the same persisted content.
- No dependency changes, schema-owner runtime privileges, media worker, production deployment or 1:1 parity claim. Larger comments increase possible response memory; production load and response-budget qualification remain open.

## Work and verification

1. Write and run failing domain/parser boundary tests; implement character validation and independent bounds. Cover ASCII, accented/BMP text, astral characters, combining marks, exact/over limits and parser preservation. Add migration and store tests with actual runtime credentials, board limits and direct database constraint denials.
2. Update the public app, synthetic fixtures and templates for the renamed field and encoded-request ceiling. Add real HTTP tests for both posting routes, maximum Unicode payloads, persisted full HTML/JSON, one-character overflow without mutation, and streaming overflow. Add browser posting at the advertised Unicode limit with JavaScript disabled, with an over-limit denial and unchanged persisted reply count.
3. Exercise migration 0006 to 0007 in a new disposable database, preserving real historical text/settings. Run workspace fmt/clippy/locked build/tests, public/staff browser suites, audit, restore and actionlint. Inspect the expected help-text screenshot differences and update only the two affected board baselines; retain the catalog baseline.
4. Review the complete branch, fix material findings, update compatibility/operations/evidence docs, commit through the human-identity helper and publish a draft PR. Record hosted Linux and Windows results before the final checkpoint report.

Sources: pinned [Boards.md](https://github.com/4chan/4chan-API/blob/2bd670d507ba2daa37a3961a661e088cf6f89d57/pages/Boards.md) as recorded in the reference manifest and local frozen copy; [PostgreSQL 16 string functions](https://www.postgresql.org/docs/16/functions-string.html). The frozen document defines characters but not their Unicode counting unit. Schema/application consistency is tested directly.
