# Anonymous posting sessions

Normal posting forms and Quick Reply use automatic anonymous ownership. The
source's `pwd` field remains hidden and empty. A successful post or report
returns an opaque cookie; later posts in the same browser use that identity,
and deletion checks its private post membership. No account or typed deletion
password is required.

Clearing or rejecting the cookie loses that browser's deletion authority.
Posting still works. Name/options cookies and own-post display receipts grant
no authority over another browser's posts. The earlier rewrite's explicit
password API remains available for historical recovery; a nonempty password
on a newly minted session retains that explicit credential. Ordinary source
forms use the automatic capability.

## Reference

The reference is the operator-supplied `4chan-old` checkout at revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`.

| File | SHA-256 | Relevant behavior |
| --- | --- | --- |
| `lib/userpwd.php` | `0a753a44e091aeb23b0a99fa09be6c1d513c8163f85989c8d4dedf7949e1bc26` | Activity ages, counters, known-user decisions, idle reset and cookie lifetime |
| `imgboard.php` | `caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445` | Posting and deletion callers |
| `views/imgboard.php` | `6c2771682a20672f4f8395e78a2cb9fa80a9ed55933b77a0fcc54cc9f23e17eb` | Empty hidden `postPassword`/`pwd` input |
| `modes/report.php` | `e52c0042295c942c12bfbfa404ca02b14932309fecd8aa9c2b3cc298aefa91ae` | Successful reports update activity and refresh the cookie |

The options writer at 5634 and name writer at 6795 both use seven days.
The anonymous capability's cookie uses one year. These lifetimes serve
different purposes.

The source retains its decoded password before its seven-day idle branch
resets activity. The rewrite retains the capability and memberships while
resetting ages, counters, pending activity, verification and change score.
Network changes reset network/address ages; an address change within the same
network resets only the address age. A country change resets the environment
age. Verification survives these changes until an idle reset.

The domain preserves the source's decision order, including its recent network
change rejection, requested age/since thresholds, pending count bits and
verified-level distinction. Successful posts contribute post/image/thread bits
derived from their actual rows; reports contribute the report bit. Each kind
is counted at most once per four-hour buffer interval, with stored counters
capped at 255 and change score capped at 32. The source's absolute creation
timestamp fallback when no activity timestamp exists is retained.

`scripts/extract-anonymous-reference.php` extracts 23 named pure methods and
allowlisted numeric constants from the hash-pinned file. It executes those
methods on synthetic state in a separate class. It does not execute the
original constructor, cookie codec, cryptography or application. The fixture
records 1,370 known-user cases, 966 activity cases and 12 idle-reset cases with
PHP 8.3.6. Database comparisons exercise all 486 non-dummy activity vectors;
dummy activity remains a domain operation for the separate source workflows.

## Security replacement

The original encrypted/signed cookie is replaced by a versioned capability
containing 32 bytes from the operating system random generator. The cookie
uses `__Host-board-anon` in production, with `Secure`, `HttpOnly`, `Path=/`
and `SameSite=Strict`. Loopback development uses `board-anon` without `Secure`.
Unknown, malformed and obsolete values receive fresh server randomness on a
successful action; client-chosen nonces are not adopted. Duplicate matching
cookies and cookie headers over 8,192 bytes fail before persistence.

The private store keeps a domain-separated capability hash and session-specific
HMAC fingerprints of address, network and country. It stores no raw address in
these new tables. IPv4 uses the source's first two octets for network scope;
IPv6 uses a /64 replacement, and mapped IPv4 addresses are canonicalized.
Country comes from the configured trusted database and actual transport peer.
Headers, public names, tripcodes and poster IDs cannot supply this authority.

Cacheable board pages and JSON never mint cookies or include private activity.
Successful actions that refresh the cookie use `no-store`. JavaScript can read
the bounded display preferences and consume its existing display receipts;
it cannot read the anonymous capability.

Migration 0065 introduces `board_anonymous_owner`, a NOLOGIN function owner
with no staff, deployment or processing authority. The public login can call
fixed snapshot, registration and ownership-proof functions. It cannot read or
write the private tables, call the unrestricted activity helper, set
verification or assume the owner role. The public runtime retains its existing
public write authority; this is not a containment boundary for a compromised
public application.

Registration shares the content/report transaction. Failed validation,
revocation, capacity exhaustion and unavailable policy leave no partial post,
report, activity or ownership. Robot9000's existing rejected-post state commit
does not register an anonymous session or send its cookie. Deletion rechecks
the stored hash fingerprint and locks the session and membership after
acquiring the board mutation lock. Session revocation, membership removal,
password rotation and expiry during that wait deny the deletion.
Anonymous OP formatting on replies checks that same locked membership inside
the posting transaction. The source's independent address match and explicit
legacy password proof remain separate formatting eligibility paths. Session
locks use their final update strength before posting changes activity, so
same-session replies on different boards cannot deadlock on a shared-lock
upgrade.

The private capacity policy defaults to 100,000 sessions and allows at most
1,000,000. New identities serialize on its policy row; existing identities lock
their own row. A mint collects at most 64 expired sessions while skipping
locked ones. A missing policy denies new registration. Backend session expiry
also enforces the cookie's one-year lifetime. This bounded server state replaces
the original client-carried state and is a security exception.

## Upgrade and verification

On an existing installation, an administrator must create the NOLOGIN owner
and grant the migration login SET-only membership before migration 0065:

```sql
CREATE ROLE board_anonymous_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT board_anonymous_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
```

Fresh bootstrap already includes these statements in `deploy/roles.sql`.
The migration preserves historical posts and password hashes and invents no
session membership for them. Existing explicit-password recovery remains
available. Old encrypted source cookies are not imported.

`scripts/test-anonymous-session-migration.sh` uses a private generated cluster
with no TCP listener. It verifies historical preservation, actual restricted
login denials, owner grants, atomic capacity/policy failures, the 64-row cleanup
bound, and populated dump/restore of all four private tables. The restored
public function must return the same usable deletion proof. Cleanup checks
the actual cluster directory before removing it.

The source-state, database, HTTP and browser results are recorded in
[verification-anonymous-sessions.md](verification-anonymous-sessions.md).
Configurable admission hooks (#212), CAPTCHA credits/challenges (#199), Pass
(#201), complete deletion rules (#214) and anonymous staff Robot9000 bypass
remain separate implementation work. This slice provides their private
activity distinctions; it does not claim those features are complete.
