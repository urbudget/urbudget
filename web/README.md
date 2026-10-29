# urbudget.app

The website, accounts and sync for UrBudget, as one Cloudflare Worker. UrBudget is free and open
source (AGPL-3.0); the only paid thing is Sync, end-to-end encrypted sync through urbudget.app, at
$3 a month. There are no licenses, trials or read-only mode.

- `public/`: the static site (landing page, pricing, download, account, legal pages).
- `src/`: the API under `/api/v1` (see [API.md](API.md)), downloads under `/download/*` and `/releases/*`.
  Payments live in `src/billing/`: `provider.ts` is what the app needs from a payment provider,
  `polar.ts` is Polar's implementation. (A Paddle implementation is in commit `3cd665f`.)
- `migrations/`: the D1 (SQLite) schema.

The sync and billing lifecycle fixes accompanying desktop 1.0.6 require `0009_lifecycle_guards.sql` before the
updated Worker is deployed. It preserves existing sync objects and initializes their key
generation to zero, adds durable object-cleanup and billing lifecycle state, and prevents old
uploads from surviving a passphrase reset. Updated apps send `X-Sync-Generation`; older apps
can use generation zero but must be updated after a passphrase reset. The normal deployment
workflow applies pending migrations before publishing the Worker.
- `scripts/`: Polar setup, releases, fake Polar events for local testing, screenshots.

## How it fits together

| | |
| --- | --- |
| **The app** | Free and open source, with every feature. Nothing online is needed to use it. |
| **Accounts** | Optional and passwordless: an emailed six-digit code signs in a browser (cookie) or the app (token in the app's encrypted vault; its key is protected by the platform). Needed only for Sync. |
| **Sync ($3/month)** | A Polar subscription bought signed in on `/account/`, or a Google Play or App Store one bought in the Android or iOS app. Each device encrypts whole budgets (Argon2id key from a sync passphrase, XChaCha20-Poly1305) before upload; the server keeps ciphertext and a revision per budget in R2 and never sees a key. Independent edits merge automatically; competing edits to the same field are shown in the app to settle. |
| **Admin** | `/admin/`, behind Cloudflare Access: look up an account for support, and see Sync subscribers and revenue, user download totals from urbudget.app and the Microsoft Store, with unfiltered GitHub file activity shown separately, copies of the app in use and how many come back, page views, link tags, the funnel from a visit to a Sync checkout, each week's numbers for the weekly log, and problems (failed emails and webhooks, rate limits). Everything counted is a daily total, nothing about who: downloads per installer and linking page, GitHub's and Partner Center's download totals (migration 0011), page views, update checks, the app's anonymous usage counts (`app_usage`, migration 0011), and sign-ins, emails, checkouts and sync uploads (`events`, migration 0006). |
| **Payments** | Polar is the merchant of record for Sync: checkout, tax and VAT, receipts, refunds, cancellations (its customer portal). The website loads no payment scripts; checkout happens on Polar's site. In the Android and iOS apps, Google Play and the App Store sell it instead. |

The `licenses` table and its columns (migrations 0001–0006) are no longer read or written; they
stay in the schema because migrations are never edited or rolled back.

## Privacy and assistant reports

The privacy policy is maintained in `public/privacy/index.html`. After editing it, run
`node scripts/app-privacy.mjs` and commit `public/privacy/app/index.html` too. The generated
Android destination shares the policy text but has no site navigation or account/checkout
links. The site tests check that both versions stay in sync.

Assistant reports use migration `0007_assistant_reports.sql`. Apply the migration and deploy
the Worker before releasing an app that submits reports. Admins can review and delete them
under **Health → Assistant reports**; no email is sent automatically. Daily scheduled cleanup
removes reports older than 90 days. A report reference lets support locate a deletion request
without collecting the reporter's account or conversation. Review this inbox as part of
normal support, investigate harmful output, and remove text when it is no longer needed.

## Google Play purchases

On Android, Sync is bought with Google's native billing sheet and verified on the server. On the
website it's bought through Polar. There is no one-time product: the app is free. This code does
not configure a Play Console account, publish products, create credentials or enable a production
subscription by itself.

Before releasing the Android purchase flow:

1. Create `app.urbudget` in Play Console. Configure the subscription `urbudget_sync` with the
   auto-renewing base plan `monthly` at $3 a month (and local prices). Activate the intended
   countries, prices and offers. Don't add one-time products or unsupported base plans; the
   server refuses any product but `urbudget_sync`.
2. Enable the Android Publisher API and authorize a dedicated service account in Play Console
   to read subscriptions and acknowledge/cancel them. Set its JSON as the Worker secret
   `GOOGLE_PLAY_SERVICE_ACCOUNT`. It belongs only on the server, never in the Android bundle or
   repository.
3. Apply migration `0008_google_play.sql` before deploying the Worker. The table retains token
   ownership after account deletion, so another account cannot reclaim a receipt. Normal
   sign-in, payment and refund behavior still needs testing against real Play test purchases.
4. Create a Pub/Sub topic and grant publisher access to
   `google-play-developer-notifications@system.gserviceaccount.com`. Connect the topic in
   Play Console and enable subscription and voided-purchase notifications.
5. Create an **authenticated** Pub/Sub push subscription to
   `https://urbudget.app/api/v1/billing/google/notifications`. Set its push service-account
   email as `GOOGLE_PLAY_PUSH_EMAIL`, and set the exact configured audience as
   `GOOGLE_PLAY_PUSH_AUDIENCE` (normally the endpoint URL). Grant the Pub/Sub service agent
   permission to create OpenID tokens for that push service account. Restrict who can use
   that identity or publish to the topic. Configure retry/dead-letter handling and monitor
   undelivered messages; HTTP 503 means retry is needed.
6. Send Play Console's test notification, then exercise subscribing, pending-payment
   completion, restore, renewal/cancellation, grace period, account hold, expiry,
   refund/revocation and account deletion on an internal Play test track. Use Play's license
   testers (Google's name for test accounts) and test payment methods. A sideloaded QA package
   cannot prove these store flows.

Notifications verify the Google signature, issuer, expiry, audience and verified service-account
email. A known receipt is then checked using the Android Publisher API; notification payloads
never grant access. Unknown receipts are ignored. The app must register pending receipts so a
later notification can finish verification while the app is closed. Access is persisted before
acknowledgment and retrying is idempotent.

The account page and sync requests refresh stale purchase status with a five-minute cache. The
hourly scheduled job also refreshes up to 100 of the oldest receipts as a fallback, so configure
and monitor notifications rather than relying on the sweep for timely lifecycle changes. Sync
access also checks the verified expiry even during a provider outage.

Primary setup references: [Google Play notifications](https://developer.android.com/google/play/billing/getting-ready#configure-rtdn),
[authenticated Pub/Sub push](https://docs.cloud.google.com/pubsub/docs/authenticate-push-subscriptions),
and [subscription lifecycle](https://developer.android.com/google/play/billing/lifecycle/subscriptions).

For App Store and Play review, optionally configure a dedicated account containing synthetic data only. (The setting names say Play, but the same account works for both stores' reviewers, because the apps sign in the same way. The iOS code field shows a number pad, so use an all-digit code: 24 or more digits pass the format check below.)
Set `PLAY_REVIEW_EMAIL` to that account's lowercase email and `PLAY_REVIEW_CODE_HASH` to
SHA-256 of `review:<lowercase email>:<random access code>`. Generate a high-entropy code of
24–128 characters using letters, digits, `_` or `-`. Store the hash as a Worker secret, and
put the actual code and review instructions only in Play Console's private App access details
or an approved private credential store. Never use a customer's account, commit the code,
or enable production `DEV_LOGIN_CODES` for review. The configured address gets a reusable
app-only review sign-in; other addresses keep normal email OTP. This creates no paid access.
To give the account Sync, add a complimentary subscription row. Its id starts with `comp:`, which
no provider bills, cancels or manages, so reviewers can still delete the account. The account must
already exist (`userFor` makes it at the first sign-in, or insert it the same way):

```sh
npx wrangler d1 execute urbudget --remote --command "INSERT INTO subscriptions
  (id, user_id, customer_id, status, current_period_end, updated_at, event_at)
  SELECT 'comp:review', id, 'comp', 'active', NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%SZ', 'now') FROM users WHERE email = 'review@urbudget.app'"
```

If a reviewer deletes the account, the row loses its user and grants nothing. Delete it and add
it again once the account signs in afresh. Verify the full reviewer journey,
including any synthetic Sync passphrase, and maintain access while review is needed. When
retiring the credential, remove/rotate the configuration and revoke that account's existing
sessions; changing the credential alone does not invalidate sessions already issued.

## App Store purchases

On iPhone and iPad, Sync is an auto-renewable App Store subscription bought with StoreKit 2 and
checked on the server; the app's word never grants it. This code doesn't set anything up in App
Store Connect, and the Worker needs no Apple key or secret: it never calls Apple, it only checks
what Apple signed.

Before releasing the iOS purchase flow:

1. In App Store Connect → UrBudget → Monetization → Subscriptions, make the subscription group
   **UrBudget Sync** and, in it, the auto-renewable subscription `app.urbudget.sync.monthly`
   with a one-month duration. Give it a price, an English display name and description, and a
   review screenshot of Account & sync's subscribe section. Leave Family Sharing off: one
   subscription belongs to one account, and the server refuses shared copies and any other product.
2. In App Information → App Store Server Notifications, set both the Production and the Sandbox
   server URL to `https://urbudget.app/api/v1/billing/apple/notifications`, Version 2.
3. Apply migration `0010_app_store.sql` before deploying the Worker.
4. Try a TestFlight build, which buys in Sandbox: subscribe, restore on another device, turn off
   renewal and let it lapse, and delete the account. Sandbox renews a monthly subscription every
   few minutes, so renewals and expiry show up quickly. The Worker answers notifications with 200;
   App Store Connect lists recent notifications and their responses for each environment.

How it works: the app asks `GET /billing/apple/context` for the account's `appAccountToken`
(a UUID derived from the account id) and buys with it. It posts each transaction's JWS to
`POST /billing/apple/verify` after a purchase or a restore, and whatever `Transaction.updates`
delivers (renewals, Ask to Buy approvals, refunds), finishing a transaction only once the server
has answered. The Worker checks the `x5c` chain up to Apple Root CA - G3, pinned by its SHA-256
fingerprint in `src/billing/apple-jws.ts`; Apple's marker extensions on the intermediate and
leaf; each certificate's validity when Apple signed the data; the ES256 signature; and the bundle
id `app.urbudget`, the product and the environment: `Production`, or `Sandbox` for TestFlight and
App Review. Xcode's local StoreKit testing signs with a local certificate, so the server refuses
those purchases. Subscriptions are kept in `apple_subscriptions` by original transaction id, and
the subscription row's id is `apple:<originalTransactionId>`.

Notifications (renewals, billing retry and grace periods, auto-renew changes, expiry, refunds and
their reversals, revocation) are verified the same way and update only subscriptions an account
claimed from the app. What they carry decides, not their type: a newer purchase replaces an older
one, a later signature of the same transaction (a refund, or its reversal) replaces an earlier one,
and renewal info goes by its own signing time, so late or repeated deliveries change nothing. Sync
access also checks the end of the paid period or grace period, and the hourly job and `/me` settle
lapsed subscriptions even if no notification arrives. The App Store Server API could look a
subscription up when a notification is missed; that would need an In-App Purchase key (not the
App Store Connect API key) as a Worker secret, and isn't used today.

The server can't cancel an App Store subscription. Account deletion leaves it to the subscriber's
Apple Account settings, and the app's delete-account sheet says it keeps renewing until cancelled
there. The record stays without an account, so no other account can claim it; renewals keep it
current. A later purchase on the same Apple Account made for another account (with that
account's token) moves the subscription to it.

The review account's complimentary Sync keeps working. The app still offers the subscription to an
account with complimentary Sync, so App Review can buy it in Sandbox with that account; the
purchase then shows as the App Store's.

## Local setup

```bash
cd web
npm install
cp .dev.vars.example .dev.vars   # already done on this computer
npm run db:migrate:local
npm run dev                      # http://localhost:8787
```

With `DEV_LOGIN_CODES=1` (in `.dev.vars`), sign-in codes are printed in the terminal and filled in on the page, and no email is sent. Pretend to be Polar:

```bash
node scripts/dev-webhook.mjs subscribe you@example.com           # Sync plan starts
node scripts/dev-webhook.mjs cancel you@example.com
node scripts/publish-release.mjs --windows path/to/setup.exe --local   # fills the download buttons and offers the update
```

Give each marketing link its own tag, `urbudget.app/download/?ref=hn` (lowercase letters, digits and `-`, up to 32); the download buttons pass it on, and it's counted as the source `ref:hn`. `node scripts/downloads.mjs` prints downloads per week, system and source (the admin page shows them too). Only daily totals are kept, nothing about who downloaded. People are counted apart from bots (crawlers, scripts, headless and AI agents' browsers, and bare requests with no referrer) and from the team: a browser that opened the admin page gets an `ub_team` cookie, sent only to `/download/`, so your own test downloads don't count.

To try Polar's real (sandbox) checkout locally, put a sandbox `POLAR_ACCESS_TOKEN` and the sandbox Sync product id (`POLAR_PRODUCT_SYNC`) in `.dev.vars`.

Point a debug build of the app at it (the app's own data stays untouched with a separate folder):

```bash
cd ../desktop
URBUDGET_API=http://127.0.0.1:8787 LOCAL_BUDGET_DATA_DIR=../migration-cloud-test npm run desktop
```

To watch the app update itself from this server, see [desktop/README.md › Trying an update locally](../desktop/README.md#trying-an-update-locally).

Tests: `npm run check` (types and the API tests, on an in-memory D1 and R2).

Sync between two installs, end to end against this server: with `npm run dev` running,
`cargo run -p urbudget-services --example sync_qa` (from `crates/`) signs two devices in to a new
account, starts its sync plan with `dev-webhook.mjs`, and runs them through edits apart and together,
conflicts, offline edits, races, schedules, a new passphrase, removing and re-adding a budget,
signing out, a 5,000-transaction budget and a random run that must end with both the same.
`-- stress 1500` runs only the random run, longer; other names run only those scenarios.

## Going live

Accounts needed: Cloudflare (hosting), Resend (email) and Polar (payments). For signed installers: an Apple Developer membership and a Windows code-signing certificate.

### 1. Domain on Cloudflare

1. Create a Cloudflare account, choose **Add a domain**, enter `urbudget.app`, pick the Free plan. Cloudflare shows two nameservers.
2. In Namecheap: **Domain List → urbudget.app → Manage → Nameservers → Custom DNS**, paste the two nameservers, save. It takes minutes to a few hours; Cloudflare emails you when the domain is active.
3. Enable R2 once: **R2 → Get started** (Cloudflare asks for a payment method; the free tier covers a small app).

### 2. Database, storage and first deploy

```bash
cd web
npx wrangler login                          # opens the browser to authorize Wrangler
npx wrangler d1 create urbudget             # put the database_id it prints into wrangler.jsonc
npx wrangler r2 bucket create urbudget-sync
npx wrangler r2 bucket create urbudget-releases
npm run db:migrate:remote
npm run deploy
```

The Worker answers on `https://urbudget.app`. For `www`, add a DNS record `www` (AAAA, `100::`, proxied) and **Rules → Redirect Rules → "Redirect from WWW to root"**.

### 3. Email: Resend

1. Create a Resend account, **Domains → Add domain** → `urbudget.app`. Its DNS records can be added for you with Resend's Cloudflare option, or by hand in **Cloudflare → DNS → Records**.
2. **API Keys → Create** (sending access), then `npx wrangler secret put RESEND_API_KEY`.
3. For mail to `support@` and `hello@`: **Cloudflare → Email → Email Routing**, forward both to your own inbox.

### 4. Payments: Polar

1. Sign up at [sandbox.polar.sh](https://sandbox.polar.sh) and at [polar.sh](https://polar.sh), and create an organization in each. In the live one, finish **Finance → Account**: product details, identity check and payout account, then submit for review. Until Polar approves the account, checkout is in test mode and only takes free or 100%-off orders.
2. In the sandbox: **Settings → Developers → New token** with the products, checkouts, customer sessions, subscriptions and webhooks scopes, and `metrics:read` for the admin page's sales numbers. Then:

   ```bash
   npm run polar:setup -- --sandbox    # paste the token when asked
   npm run deploy
   ```

   It creates the one product, "UrBudget Sync" at $3 a month (an existing one at another price is moved to $3), and the webhook to `https://urbudget.app/api/v1/billing/webhook` for subscription events, writes the product id and `BILLING_ENV` into `wrangler.jsonc`, and stores the token and webhook secret as Worker secrets.
3. Test with Polar's test card: sign in and subscribe on `/account/`, turn on sync in the app, then cancel in Polar's customer portal (opened from `/account/`) and check sync stops at the end of the period.
4. Go live: make a token in the live organization and run `npm run polar:setup -- --production`, then `npm run deploy`.

### 5. Publish the app

For a new desktop release, follow [RELEASING.md](../RELEASING.md): `node scripts/versions.mjs desktop 1.2.3` sets the version in every desktop file, and a `desktop-v1.2.3` tag builds the installers ([desktop release procedure](../desktop/README.md#releases)). Installed copies only take a higher version. The phone apps have their own versions and go to their stores, not here. [Release status](../docs/RELEASE-STATUS.md) records what is already published.

```bash
cd desktop && npm run tauri -- build     # Windows: bundle/nsis/UrBudget_<version>_x64-setup.exe (+ .sig)
                                         # macOS (on a Mac): bundle/dmg/UrBudget_<version>_<arch>.dmg,
                                         #   bundle/macos/UrBudget.app.tar.gz (+ .sig)
cd ../web && npm run release -- --notes "What's new"
```

`npm run release` finds this version's installers in `desktop/src-tauri/target/release/bundle` (or pass `--windows` / `--macos` paths), uploads them, and points the download buttons at them. A system you don't pass keeps its previous installer.

It also publishes the update that installed copies offer (they look at launch and daily; Settings › General › Install & restart). On Windows the update package is the installer itself; on macOS it's the `.app.tar.gz` from the same build, uploaded as `UrBudget_<version>_<arch>.app.tar.gz`. Before uploading anything, the script checks each package's `.sig` against the updater public key in `desktop/src-tauri/tauri.conf.json` and that it was signed for this version, so a package installed copies would refuse never goes out. `latest.json` then lists, per Tauri target and architecture (`windows-x86_64`, `darwin-aarch64`, …), the version, date, notes, package file and signature, and `/api/v1/releases/update/<target>/<arch>/<running version>` answers from it: 204 when that copy is up to date. Each entry keeps its own version, so a system you don't publish again isn't offered the other's version.

When a GitHub release is published, `.github/workflows/publish.yml` downloads its installers, update packages and `.sig` files and runs the same script.

**The updater key.** The build signs update packages with the private key in `~/.tauri/urbudget-updater.key`, which is outside the repository. GitHub's release workflow reads it from the `TAURI_SIGNING_PRIVATE_KEY` secret. To build on another computer, copy it there or set `TAURI_SIGNING_PRIVATE_KEY` (see [desktop/README.md › The updater signing key](../desktop/README.md#the-updater-signing-key)). Back it up somewhere safe: without it, installed copies can't be updated, and everyone has to download the next version by hand.

Unsigned installers work, but Windows SmartScreen warns and macOS blocks the app until it's allowed in System Settings. Before launch, sign Windows builds with a code-signing certificate and macOS builds with an Apple Developer ID (notarized); Tauri's docs cover both under "Distribute → Sign".

### 6. Admin page

`/admin/` and its API (`/api/v1/admin/*`) answer only to someone Cloudflare Access has signed in, and the Worker checks Access's signed token itself, so nothing opens until all of this is done:

1. In Cloudflare, open **Zero Trust**. The first time, it asks for a team name (say `urbudget`) and a plan: Free covers up to 50 people. **Settings → Authentication** has "One-time PIN" (a code sent to your email) on by default; add Google there if you'd rather.
2. **Access → Applications → Add an application → Self-hosted.** Name it "UrBudget admin" and add two public hostnames: `urbudget.app` with path `admin`, and `urbudget.app` with path `api/v1/admin`. Add a policy: action **Allow**, include **Emails** → your address. Save.
3. Open the application and copy its **Application Audience (AUD) Tag**.
4. In `wrangler.jsonc`, set `ACCESS_TEAM_DOMAIN` (`urbudget.cloudflareaccess.com`) and `ACCESS_AUD` (the tag). Push to `main` to deploy.
5. Say who may use the admin page as a Worker secret, so no address is in the (public) repository: `npx wrangler secret put ADMIN_EMAILS` and enter your address (comma-separate more). Until it's set, nobody gets in. A Worker that still has `ADMIN_EMAILS` as a plain variable from an older `wrangler.jsonc` refuses a secret of the same name: deploy the current `wrangler.jsonc` first, which drops the variable, then put the secret.
6. Download counts from GitHub releases work without a token, since the repository is public, but unauthenticated requests share GitHub's low per-address rate limit. To lift it, make a [fine-grained token](https://github.com/settings/personal-access-tokens/new) with **Public repositories (read-only)** access, then `npx wrangler secret put GITHUB_TOKEN`. The hourly cron notes each file's raw count, so file activity adds up per day from the first note on. GitHub cannot distinguish users from release workflows, QA or other automation; its counts are excluded from user totals, platform totals, funnels, weekly exports and conversion rates. No guessed automation deduction is applied, and historical snapshots are preserved.
7. For the Microsoft Store's own acquisitions and installs, give the Worker the "UrBudget releases" Microsoft Entra application the Store workflow uses (Partner Center → Account settings → User management → Microsoft Entra applications, Manager role). Its tenant and client IDs are on that page; GitHub doesn't show the existing key, so add a new key there. Then `npx wrangler secret put` each of `MSSTORE_TENANT_ID`, `MSSTORE_CLIENT_ID` and `MSSTORE_CLIENT_SECRET`. The cron fetches the last 30 days from Partner Center's analytics API once a day; Downloads on the admin page says when it last answered. Keys expire: when it can't sign in, add a new key and replace the secret (and the GitHub secret, for the Store workflow). Until it's set, clicks on the site's Store button stand in for the Store's downloads.
8. Revenue numbers need the Polar token to have `metrics:read`. If it was made without it, make a new token with it and run `npm run go-live` again.

Locally, `npm run dev` serves `http://localhost:8787/admin/` without Access (only with `DEV_LOGIN_CODES=1` and a localhost `APP_URL`, as in `.dev.vars`).

### 7. Deployment verification checklist

Use this checklist for a new environment or an operational audit. Unchecked items are checks
to perform, not a claim that the existing service has never been configured. Record dated
results in local evidence; [release status](../docs/RELEASE-STATUS.md) tracks publication.

- [x] The operator on `/privacy/`, `/terms/` and `/refunds/`: Ahmad Horanieh, an independent developer, with the support address and no postal address (privacy law asks for the controller's identity and a contact). If UrBudget becomes a registered business, put its legal name in each page's "Who we are" box instead.
- [ ] Polar live: account details submitted, `polar:setup -- --production` run, deployed.
- [ ] A real Sync subscription with your own card, then a cancellation and a refund, all behave.
- [ ] Sign-in emails arrive (check spam once; Resend's DNS records fix most of it).
- [ ] The updater signing key (`~/.tauri/urbudget-updater.key`) is backed up.
- [ ] Signed installers published with `npm run release`.
- [ ] `ADMIN_EMAILS` is set as a Worker secret, and the admin page asks for your Access sign-in and shows your email at the top once you're in.

## Running it

- Deploying: a push to `main` that changes `web/` (other than the READMEs and `scripts/`) runs the tests, applies new migrations and deploys (`.github/workflows/deploy.yml`). Nothing else is built. By hand: `npm run deploy`. Its Cloudflare token comes from `node scripts/cloudflare-token.mjs`.
- Logs: `npx wrangler tail`
- Look at data: `npx wrangler d1 execute urbudget --remote --command "SELECT id, status, current_period_end FROM subscriptions ORDER BY updated_at DESC LIMIT 20"`
- Something not arriving (a sign-in email, a webhook)? `/admin/` → Health lists what failed in the last 90 days, and how many emails went out against Resend's free plan (100 a day, 3,000 a month).
- Someone writes in about their account or sync: `/admin/` → Accounts shows their sync plan, signed-in devices and synced budgets (sizes only).
- The weekly numbers: `/admin/` → Overview → Week by week, **Copy** on last week's row, and paste it into the weekly log, which is kept with the private marketing files (not in this repository); add the search and Store columns there.
- New screenshots after UI changes: follow the screenshot guide, kept with the private marketing files (not in this repository). `scripts/screenshots/shoot.mjs` fails on missing navigation instead of saving the wrong screen.

## Costs

- **Cloudflare:** Workers, D1 and R2 free tiers cover a small app. Move to Workers Paid ($5/month) if you hit the free plan's request or CPU limits.
- **Resend:** free up to 3,000 emails a month.
- **Polar:** a percentage plus a fixed fee per transaction; check [Polar's fees page](https://polar.sh/docs/merchant-of-record/fees) for the current rates. The fixed part weighs most on a small monthly price like Sync's $3; a yearly price would cut it.
