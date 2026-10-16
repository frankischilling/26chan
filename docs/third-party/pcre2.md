# PCRE2 Rust wrapper

`vendor/pcre2` contains the high-level Rust wrapper from the published
[pcre2 0.2.11 package](https://crates.io/crates/pcre2/0.2.11). The registry checksum
and original source hashes are recorded in [the source record](pcre2-source.json).
The native `pcre2-sys` dependency and its version are unchanged.

The local patch exposes `RegexBuilder::dollar_endonly`. It sets the existing
`PCRE2_DOLLAR_ENDONLY` compile flag through the wrapper's safe builder. This
supports PHP's `D` modifier without rewriting dollar anchors or changing inline
multiline scopes. The option defaults to false. No project-owned unsafe block,
JIT behavior, match budget, depth ceiling or heap ceiling is added or changed.

The vendored package is excluded from workspace membership and marked private.
Its upstream rustfmt configuration is retained so formatting preserves its style.

Upstream offers the [MIT license](pcre2/LICENSE-MIT) or
[Unlicense](pcre2/UNLICENSE); its [COPYING notice](pcre2/COPYING) is preserved.

The source archive SHA-256 matches the original Cargo.lock checksum. The source
record hashes every copied upstream Rust file and the original manifest and
license notices. Only `src/bytes.rs` and the private package manifest differ.

PHP 8.3.6 maps `D` to this compile option and ignores `X` in its
[PCRE extension](https://github.com/php/php-src/blob/php-8.3.6/ext/pcre/php_pcre.c).
The [PHP 8 migration notes](https://www.php.net/manual/en/migration80.incompatible.php)
explain that invalid escapes fail without `X`. PCRE2 documents the dollar anchor,
multiline scopes and `\Z` in its
[pattern reference](https://www.pcre.org/current/doc/html/pcre2pattern.html).

The 21 synthetic cases in `crates/domain/tests/fixtures/regex-modifiers.json`
record PHP 8.4.26 with PCRE2 10.46. The domain tests compare match counts and
compile failures. CI reruns the PHP oracle and compares decisions, allowing a
different runtime version only when every result agrees:

```sh
php -n scripts/extract-regex-modifier-reference.php \
  crates/domain/tests/fixtures/regex-modifiers.json --check
```
