# Checkout source and native contract

Issue #226 covers checkout, donations and advertisements. Public Pass fulfillment
also belongs to #201.

## What the archive contains

The eight provider/controller entries under `www.4chan/payments/` are short text
files containing absolute paths into `/www/global/yotsuba/payments/`. They include
`index.php`, its test entry, the Stripe request entries, `paypal.php`,
`coinbase.php` and both one-board-ad entries. The ZIP contains no corresponding
controller implementations. Their names establish intended integrations, but not
prices, callback formats, transaction semantics or entitlement updates.

The approved/declined templates are present. Donation and self-ad templates are
also present. Donations use USD, confirmed email, optional public name/message,
and source wording that gifts are non-refundable. The self-ad form includes
board selection, daily frequency, start/end dates, a USD amount, destination URL
and an image upload. Constants defining its minimum price and upload limits are
outside these templates. Both forms reference hosted Stripe scripts; the native
implementation must keep payment secrets and card handling off the board origin.

The Pass mail task establishes a twelve-month renewal: an active Pass extends
its current expiry, while an expired Pass starts from payment. The public Pass
sales/reset controllers, price and external stylesheets are absent. No legacy
price or provider callback can be inferred from a filename.

## Initial native boundaries

Checkout prices and enabled products will be operator policy, with no inferred
legacy price default. Sandbox purchases must be visibly identified and remain
separate from live provider credentials. The public application must never accept
card details or grant a Pass from a browser return URL.

The initial domain checks bind an order to its product, amount, currency, provider
object and capture window. Duplicate success is idempotent. Refund/dispute states
cannot be reactivated by a late success; failed or cancelled orders require a new
order before another attempt. These are conservative native rules, not recovered
legacy-controller behavior.

Callback verification operates on the raw body before parsing. Sandbox signatures
have their own domain prefix. The Stripe verifier follows the official
[stripe-node v1 framing](https://github.com/stripe/stripe-node/blob/master/src/Webhooks.ts):
timestamp, a period and the unchanged body, authenticated with HMAC-SHA256.
Headers and bodies are bounded, signature rotation is limited to four v1 values,
and ambiguous timestamps are rejected. The native freshness window permits up to
five minutes of delivery age and thirty seconds of forward clock skew.

A valid signature alone does not establish payment. An adapter must reconcile
provider state and bind it to the stored order before granting an entitlement.
Stripe's current [event guidance](https://docs.stripe.com/events/set-up-events)
requires verifying events and consulting current provider state for critical
snapshot decisions. Callback replay, product/amount mismatches and concurrent
fulfillment need transaction-level tests before these helpers can grant anything.

The private sandbox adapter and renewal ledger are now connected. Migration 0234
keeps prices disabled until an operator enables a quote. Orders bind their price,
product, provider object and browser-view capability. The verifier can create
orders; a separate `board_checkout` runtime can acknowledge signed events. Neither
runtime has direct access to contact or credential tables.

Paid renewal extends an active expiry by twelve calendar months, or starts an
expired Pass from capture time. Duplicate/concurrent notifications extend it only
once. A newly restricted account is held for review instead of reactivated. Native
full-refund/dispute handling places an eligible associated Pass on a payment hold;
existing spam/content restrictions remain intact. These hold rules are conservative
native policy because the original payment controller is missing.

The sandbox adapter accepts only an explicit development configuration and a
loopback listener. It does not collect card data or contact Stripe, PayPal or
Coinbase. A valid Stripe signature helper does not make a live provider adapter;
provider reconciliation and live checkout remain unfinished. Public checkout,
new-Pass purchases, donations and ad publication are also still open.

Local checks pass raw-body authentication, stale/wrong-provider signatures,
product/amount refusal, concurrent callback replay, exact renewal, refund holds,
late-success refusal, private-role checks and migration rollback. The ordinary
Pass lifecycle and populated restore suite also passes after the content-schema
permission change is recorded in its classifier/report catalogs. Hosted checks
remain pending. No payment account, real order or external charge was created.

## Local adapter setup

Bootstrap `board_checkout` before migration 0234 on an existing installation.
The role starts with NOLOGIN and has no role memberships. Configure its login
outside application runtimes. `deploy/roles.sql` lists its timeouts and permissions.
For disposable development clusters, `scripts/dev-checkout-db.sh` writes separate
adapter and test credential files without replacing existing files.

The adapter requires `APP_ENV=development`, `CHECKOUT_PROVIDER=sandbox`,
`CHECKOUT_BIND` on loopback, `CHECKOUT_DATABASE_URL` for the restricted checkout
role and a separate `CHECKOUT_CALLBACK_KEY`. Run the `board-checkout` binary and
POST signed JSON to `/events/sandbox`. The signature header is
`x-checkout-signature: t=<unix-seconds>,v1=<lowercase-hmac-sha256>`; its authenticated
input is the sandbox domain prefix, timestamp, period and exact body. Use only
owned fixtures. The callback integration test supplies the complete schema and
checks the signed callback boundary and its transaction effects.

The order inventory has a fixed operator cap and fails closed when full. Automatic
financial-history retention/export is not implemented yet; do not treat this
checkpoint as a production checkout release.
