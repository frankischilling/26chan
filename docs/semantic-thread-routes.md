# Inbound semantic thread URLs

The public HTML listener accepts `/{board}/thread/{id}/{context}` for ordinary
source-generated word slugs. It reuses the canonical thread handler, storage
snapshot, visibility rules and response headers. The context is cosmetic and
cannot select a different post or board. Outbound URL generation is unchanged.

The supplied renderer emits these links in `imgboard.php:3616–3622,9317–9329`.
Its context generation and cleanup are at lines 7938–7978 and 8084–8110. The
extension identifies board, page and thread ID without using the context segment
at `extension.js:9369–9372`.

Accepted contexts contain lowercase ASCII letters or digits, joined by single
hyphens, with a maximum of 49 bytes. IDs retain exact positive signed-64-bit
precision. The handler and CSP classifier share raw-path validation, including
the board segment. Encoded syntax, extra/trailing segments, malformed IDs and
JSON keys are rejected. Contexts named `archive` or `catalog` still receive the
thread page's policy.

The database test compares complete headers and bodies with canonical responses,
including query behavior and IDs above JavaScript's safe-integer range. It also
checks missing/private targets and the rejected path forms.

The supplied files contain no original webserver rewrite configuration. Arbitrary
suffix acceptance and unusual control-whitespace generator output remain
unverified. This bounded inbound alias does not complete issue #216 or establish
full original-page appearance and client behavior.
