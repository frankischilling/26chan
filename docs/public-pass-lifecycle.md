# Public Pass lifecycle

Issue #201 covers public membership authentication and its posting benefits.
This work begins with the source state model. The policy module alone grants no
identity, session or posting authority. Storage, HTTP routes and posting
integration are still pending.

## Source decisions

`auth.php` applies a 15-minute login hold when the previous nonzero IPv4 /16
network differs. It checks this hold before the account status. Status 0
continues, 6 activates a new Pass, and 7 activates a delayed Pass with one year
starting at activation. Statuses 1 through 5 distinguish expired, refunded,
disputed, spam-revoked and illegal-content-revoked accounts.

`lib/auth.php:valid_captcha_bypass` uses different rules for posting. It first
rejects expiry at or before the request timestamp, then rejects any status
other than 0. Its network hold is 30 minutes with an IPv4 /24 comparison.
Same-scope use and an unset previous network bypass those holds. Future
last-used timestamps remain inside the hold rather than wrapping the clock.

Ordinary login cookies last 86,400 seconds. Remembered sessions use 31,556,900
seconds in the supplied login code. A trusted bypass halves reply and
duplicate-image cooldowns, rounding upward. Thread cooldowns remain unchanged.
The `since4pass` option exposes the original purchase year. Pass-only posting
uses the trusted bypass outcome, including the source's separately authenticated
staff path; it does not grant staff roles to a member.

The login source uses /16 and 900 seconds while posting uses /24 and 1,800
seconds. These are distinct contracts. The native implementation must not merge
them into a single timeout. Unknown stored statuses will fail closed rather than
fall through the PHP switch.

## Reference qualification

`fixtures/pass-policy-source.json` pins the two source files and contains only
the network/status decision fragments. Its PHP harness replaces cookie writes,
HTTP termination and activation SQL with recorded outcomes. It uses owned
scalar timestamps and documentation addresses, without loading the source
application, database adapters, keys or credentials.

The 1,120 cases cover all eight statuses, the different subnet boundaries,
unset prior networks, future timestamps, exact hold boundaries and posting
expiry. Rust compares each outcome with the executed source fragment. The
harness does not qualify credential verification, persistent updates, cookies,
recovery, payment callbacks or posting integration.

```sh
python3 scripts/extract-pass-policy-reference.py /path/to/4chan-old --check
php -n scripts/test-pass-policy-source.php
cargo test -p board-domain pass_policy --locked
```

## Integration boundaries

The public listener must not read membership credential hashes or update
entitlements directly. Authentication belongs to the isolated verifier, with
bounded password hashing and database-controlled login attempts. Browser
cookies carry opaque session material; network comparisons use trusted peer
information. Neither a presentation cookie nor a browser-supplied membership
flag can authorize a bypass.

Every protected write must recheck the current entitlement and session inside
the posting transaction. Issuing an advisory receipt must not let a revoked,
expired or signed-out session post later. The same write captures the private
membership identity for restrictions and staff history, while public output
receives only the explicitly requested visible membership fields.

Local provisioning must use deployment-owned test entitlements. Source tokens,
PINs, production salts and payment accounts are never imported. Recovery and
checkout dependencies remain part of the parent issue until they have working,
qualified boundaries.
