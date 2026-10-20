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
provider reconciliation and live checkout remain unfinished. Native sandbox purchases
are under qualification below; donations and ad publication are still open.

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


## Public renewal forms

The native `/pass?renew=...` form now displays the operator quote and an explicit
sandbox warning. A separate checkout CSRF cookie binds submission. The private
issuer derives an idempotent order capability from the form nonce and renewal
reference; concurrent retries return the same order. A changed amount or currency
cannot override the stored quote. Result pages use a private view capability and
an origin-only referrer policy. They read the ledger; browser query parameters
cannot assert a successful payment.

The owned public/private HTTP test passes quote rendering, wrong-origin and CSRF
refusal, price mismatch, concurrent order retries, pending status, a signed sandbox
callback and the final renewed state. Rendering-only browser tests cover native
submission and all result states on desktop/mobile with scripts enabled and
disabled. Those hosted browser checks are still pending.


Contacts created before migration 0234 need a renewal-reference index before old
email links can create an order. Run `board-pass-bind-renewals` as the migration
operator with the existing `VERIFICATION_KEY` and `VERIFICATION_MAIL_KEY`, then
repeat until it reports zero remaining bindings. Each transaction handles at most
100 contacts. It decrypts and verifies the stored contact before deriving the
private index; it sends no mail and changes neither the PIN nor account status.
This covers expired and delayed Passes that the reminder producer does not select.
A wrong key or invalid contact aborts the batch. Run this before enabling checkout
on an upgraded installation.

## Native purchase flow under qualification

Migration 0235 adds a separately disabled purchase quote. An operator must set
`purchase_amount_minor` and `purchase_enabled`; the renewal switch does not enable
sales. `/pass` collects a confirmed owner email, an optional confirmed gift email,
and a delayed-activation choice. Gift mail goes to the gift recipient. The native
form and initial PIN setup are not verified reproductions of the missing sales
controller.

The issuer reserves a bounded order before hashing a random initial secret. A
30-second preparation lease permits at most five attempts on the same order.
Prepared contact and setup-mail envelopes are encrypted; no usable initial PIN is
returned or mailed. Only a matching signed sandbox payment creates the account,
contact and one-use setup-mail job in one transaction. Normal purchases start a
12-month period at capture; delayed purchases start it on first sign-in. Duplicate
payment delivery cannot create a second account. The preparation envelopes are
cleared after fulfillment or an unpaid terminal outcome.

Account or setup-mail capacity exhaustion returns a retryable service failure
without acknowledging the payment event. Retry that same signed event after
capacity is restored. There is no live payment provider retry service in this
build. A restricted mailer acknowledgement enables the 30-minute setup link;
completing setup changes the PIN once and retains delayed/normal account status.
The protected purchase receipt shows the public Pass token so the owner can use
PIN recovery if setup mail expires. It does not show the owner's email or PIN.

Local migration checks pass purchase fulfillment, replay, wrong-amount refusal,
capacity backpressure/retry, delayed activation and refund holds. The HTTP fixture
passes concurrent form retries, gift ownership, signed payment, setup-link delivery
acknowledgement, one-use PIN selection and first login for normal and delayed
purchases. The mailer fixture passes SMTP acceptance and rejection for setup mail.
Browser qualification is still pending in CI. Donations, ad fulfillment and
production provider reconciliation remain open.
