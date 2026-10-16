# Automatic-ban exemptions

An operator-owned IPv4 range can suppress automatic ban insertion without
allowing the rejected post. Content-rule and filename-proxy bans use this check.
A matching post still receives its rejection or quiet-success response, and
ordinary filter hits remain. Adding an exemption does not remove an existing
ban or bypass its earlier posting check.

`admission.automatic_ban_exemptions` starts empty. Runtime roles cannot read or
edit it. The private checker runs inside the content-effect recorder, which
already holds the shared policy generation and exact-peer lock. Exemption edits
use the same generation trigger as text, network and file rules. The checker
never reads forwarding headers or accepts an exemption flag from a request.

## Bounds and source differences

Entries are canonical IPv4 CIDRs with prefixes from 1 through 32 and a nonzero
network base. The database rejects host bits, IPv6 ranges, zero-base networks
and invalid prefixes. At most 128 active entries can participate in a decision;
a 129th row makes a needed IPv4 check unavailable. Inactive entries do not match.
Native IPv6 peers are not exempt. The existing posting path canonicalizes
IPv4-mapped peers before recording effects.

The original helper also rejects `/0` and zero networks and requires an aligned
base, but malformed lines usually become silent misses. It treats missing or
unreadable files as an empty list. The replacement validates configuration and
propagates query failures, so an unavailable policy cannot silently issue a ban.
The transaction then rolls back earlier filter effects too. A successfully read
empty table means there are no exemptions.

## Qualification and remaining scope

The independent fixture executes only the pinned CIDR helpers and whitelist
wrapper against synthetic lists. Its 924 cases cover prefix boundaries, matching
endpoints, adjacent addresses and an empty list. Database tests also check
invalid configuration, private-role boundaries, inactive entries, IPv6, overflow
and generation locking. Posting tests check ordinary rejection, quiet success,
retained filter hits, existing-ban precedence and rollback on overflow. The
filename branch is checked at the shared effect recorder; its existing upload
preflight tests remain separate.

No private ranges are imported. Future file-blacklist and DMCA-repeat ban
issuance must use this same private checker. Those issuance paths remain
unfinished; this change does not add file-policy enforcement.
