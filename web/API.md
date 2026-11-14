# UrBudget API

Served by the same Worker as the site, under `https://urbudget.app/api/v1`. Every response is JSON
unless noted. Errors are `{ "error": "<code>", "message": "<sentence a person can read>" }` with a
4xx/5xx status.

UrBudget is free; the only thing sold is Sync ($3 a month), so there are no license, trial or
one-time purchase endpoints.

Two kinds of session:

- **Web**: an `HttpOnly` cookie (`ub_session`) set by `auth/verify` with `client: "web"`. Requests
  that change something must send `Content-Type: application/json` and come from the site's own
  origin.
- **Desktop**: `Authorization: Bearer <token>` from `auth/verify` with `client: "desktop"`.

## Public

| Method & path | Body | Response |
| --- | --- | --- |
| `GET /config` | | `{ checkout: { ready, environment: "sandbox" \| "production" }, prices: { sync: "$3" }, supportEmail }` |
| `GET /releases/latest` | | `{ version, date, notes, platforms: { windows: Platform \| null, macos: Platform \| null } }`, 404 before the first release. `Platform = { file, size, sha256, url }` |
| `GET /releases/update/:target/:arch/:current` | | The app's updater (tauri-plugin-updater, dynamic format). `target` is `windows` or `darwin`, `arch` is `x86_64`, `aarch64`…, `current` is the running version. `{ version, notes, pub_date, url, signature }` when a newer signed package exists for that system, 204 otherwise, 400 for a malformed version |
| `GET /download/windows`, `GET /download/macos` (no `/api/v1`), optional `?ref=<tag>` | | 302 to the newest installer, counted per day, version and source (the tag, else the page or site the link was on), or to `/download/?unavailable=<platform>` |
| `GET /download/store` (no `/api/v1`), optional `?ref=<tag>` | | 302 to UrBudget in the Microsoft Store, the main Windows button. Counted like the installers, as a Windows download with the version `Microsoft Store` |
| `POST /usage` | `{ platform: "windows" \| "macos" \| "linux", channel: "direct" \| "store", version, first?, day?, week?, month?, cohort? }` | 204. The desktop app's anonymous count of copies in use (`src/usage.ts`, migration 0011): at most once a day, what's new since the copy was last counted. `cohort` (the Monday of the week the copy was first opened) only counts with `week`. Only from `User-Agent: UrBudget/…`; 400 `invalid_usage` for anything else malformed, 429 past 20 a day from one address (hashed with the day) |
| `POST /billing/webhook` | Polar event (Standard Webhooks) | `{ ok: true }`; signature checked. Handles `subscription.*` for the Sync product; anything else (orders included) is acknowledged and ignored |

## Accounts (email codes, no passwords)

| Method & path | Body | Response |
| --- | --- | --- |
| `POST /auth/start` | `{ email }` | `{ ok: true }` (also when the address is new; it becomes an account on verify). In local development it adds `devCode`. Errors: `invalid_email`, `rate_limited` |
| `POST /auth/verify` | `{ email, code, client: "web" \| "desktop", deviceName? }` | web: `{ user }` and the cookie; desktop: `{ user, token }`. Errors: `invalid_code`, `expired_code`, `too_many_attempts` |
| `POST /auth/logout` | | `{ ok: true }` |
| `POST /auth/web-link` (desktop) | `{ next }` | `{ url }`: opening it in a browser signs that browser in and goes to `next` (a path on the site). Valid 5 minutes, once |
| `GET /me` | | `{ user: { id, email, createdAt }, subscription: null \| { provider: "polar" \| "google" \| "apple" \| "comp", status, renewsAt, endsAt }, sync: { active, budgets, bytes } }` |
| `DELETE /me` | `{ confirm: "<the account's email>" }` | `{ ok: true }`. Cancels any Polar or Google Play Sync subscription, then deletes the account and its synced data. Only the subscriber can cancel an App Store subscription, so it keeps renewing until they do |
| `POST /billing/checkout` | `{ plan?: "sync" }` | `{ url }` of Polar's hosted checkout for Sync, tied to the signed-in account (401 signed out); the browser goes there. Polar returns the subscriber to `/account/?checkout_id=…`. Any other `plan` is 400 `invalid_plan`. Error `not_configured` until Polar is set up |
| `POST /billing/portal` | | `{ url }` of Polar's customer portal (update card, cancel), Google Play's subscriptions page for a Play subscriber, or Apple's (`https://apps.apple.com/account/subscriptions`) for an App Store one. Error `no_customer` |

`subscription.status` is Polar's: `active`, `trialing`, `past_due`, `unpaid`, `paused`,
`incomplete` or `canceled` (Play subscriptions are `active` or `canceled`; App Store ones are
`active`, `past_due` in a billing grace period, `unpaid` in billing retry after it, or `canceled`).
Sync works while it is `active`, `trialing` or `past_due`.

## Google Play (Android)

`GET /billing/google/context` and `POST /billing/google/verify` require the signed-in app
account. Context returns `{ accountId, syncId: "urbudget_sync", basePlanId: "monthly" }`; the app
passes the obscured account ID to Google's purchase flow. Verification accepts
`{ token, products: ["urbudget_sync"] }` (any other product is 400 `invalid_purchase`) and returns
`{ active, pending }`. A pending receipt is stored without granting access or acknowledging payment. Verification is limited to 240 requests per account
per hour; tokens are bounded to 4,096 characters and request bodies to 8 KiB.

`POST /billing/google/notifications` accepts the standard Pub/Sub message envelope. It requires
a Google-signed Bearer token for the configured push identity and audience, not an app session.
Known purchase tokens are rechecked with Google. Successful, test and unknown-token deliveries
return 204. Invalid authentication returns 401, invalid payloads 400, oversized bodies 413,
and temporary provider/configuration failures 503 so Pub/Sub can retry. Notifications are
bounded to 32 KiB and never logged with purchase tokens. See README for deployment setup.

## App Store (iOS)

`GET /billing/apple/context` and `POST /billing/apple/verify` require the signed-in app account.
Context returns `{ appAccountToken, productId: "app.urbudget.sync.monthly" }`; the app passes the
token, a UUID derived from the account id, to StoreKit's purchase as `appAccountToken`.
Verification accepts `{ transaction }`, a StoreKit 2 transaction's JWS (`jwsRepresentation`),
after a purchase, a restore or a `Transaction.updates` delivery, and returns `{ active }`. The JWS
must be signed through a chain to Apple Root CA - G3 for the bundle `app.urbudget`, the Sync
product and the `Production` or `Sandbox` environment, else 400 `invalid_purchase`. A transaction
made with another account's token, or for a subscription another account holds, is also 400
`invalid_purchase`, with a message that says so; the app finishes the transaction either way. One
without a token (Apple sends none for some purchases started outside the app) goes to the first
account that posts it. Verification is limited to 240 requests per account per hour and bodies to
32 KiB.

`POST /billing/apple/notifications` takes App Store Server Notifications V2 (`{ signedPayload }`),
verified the same way, with the bundle id and, in Production, the app's Apple ID (6825535486).
They update only subscriptions an account already claimed, by the signed transaction and renewal
info they carry rather than by their type, so late, repeated or out-of-order deliveries change
nothing. Handled, test, summary and unknown-subscription deliveries return 200; an invalid one
400; a temporary failure 5xx, so Apple retries. Bodies are bounded to 128 KiB.

## Assistant reports

`POST /assistant-reports` requires no account and accepts only `{ text, reason? }` as JSON.
The trimmed reply must contain 1–4,000 UTF-16 code units and the reason at most 1,000. The
request body is limited to 32 KiB including streamed bytes. Success returns 201 `{ id }`.
Limits are five requests per hour and 20 per day per daily IP hash, plus 1,000 globally per
day. Only the submitted text, optional reason, random reference and received time are stored;
no account, attachments or other conversation context is collected. Daily cleanup deletes
reports older than 90 days. A deployment must apply migration 0007 before accepting reports.

## Admin (behind Cloudflare Access)

For the page at `/admin/`. Every request needs the `Cf-Access-Jwt-Assertion` header Cloudflare Access adds, signed for
`ACCESS_AUD` by `ACCESS_TEAM_DOMAIN`, for an email in `ADMIN_EMAILS`; otherwise 403 `not_admin`. Changes must come from
the site's own origin (403 `bad_origin`). Each change is written to the admin log with the admin's email.

| Method & path | Body | Response |
| --- | --- | --- |
| `GET /admin/overview` | | `{ admin, environment, accounts, sync, downloads, usage, pageViews, previous30, funnel, countingSince, health, daily, weeks }`: totals; downloads from every channel for the last 30 days (`downloads: { last30, website, github, store, storeSource: "partner-center" \| "clicks", windows, macos, team, bots }`); first opens and copies in use (`usage: { opens30, since, thisWeek, lastWeek, weekBefore, yesterday }`); the 30 days before (`previous30: { checkouts, downloads, opens, pageViews, accounts }`); the funnel from page views to Sync checkouts (`funnel: { views, downloads, opens, checkouts }`); 90 days of daily numbers (`daily: { days, checkouts, signups, views, downloads, website, github, store, opens, active }`); and the last 12 weeks (Monday to Sunday, UTC, newest first: `{ start, views, downloads, website, github, store, tags, opens, active, checkouts, accounts }`) with each week's top download link tags. `health` adds emails sent against Resend's limits and when the daily clean-up last ran |
| `GET /admin/log` | | `{ entries: [{ at, admin, action, detail }] }`, the last 200 |
| `GET /admin/accounts?q=&filter=&page=` | | `{ accounts: [{ id, email, createdAt, subscription, devices, lastSeenAt, budgets }], total, page, pageSize }`, newest first. `q` matches part of an email, or an account or Polar customer id. `filter`: `all`, `sync` (a live subscription), `app` (signed in to the app), `nothing` (never subscribed) |
| `GET /admin/accounts/:id` | | `{ account: { id, email, customerId, createdAt }, subscriptions, sessions: [{ kind, deviceName, createdAt, lastSeenAt, expiresAt }], syncKeySetAt, budgets: [{ id, revision, size, device, updatedAt }] }`. Read-only; no session secret or hash. 404 `not_found` |
| `GET /admin/downloads` | | `{ days, website, github, store, total, opens, versions, sources, others, latest }`, 90 days of each: the site's clicks (`website: { windows, macos, store }`, `store` being its Store button), installers downloaded from GitHub (`github: { windows, macos, since, fetched, releases \| error }`, from the hourly snapshots, with every file's count now), Partner Center's numbers (`store: { configured, partnerCenter, acquisitions, installs, since, fetched }`), every channel added up (`total`, with the Store's acquisitions once Partner Center has answered, its button's clicks before), first opens (`opens: { direct, store }`), and the site's clicks by version and source |
| `GET /admin/usage` | | `{ days, since, daily, opens, weeks, months, cohorts, versions, launches }`: copies in use per day and first opens (`{ direct, store }`, 90 days), per week (12, newest first: `{ start, active, direct, store, windows, macos, opens }`) and per month (6), each week's first opens and how many came back in each of the 8 weeks after (`cohorts: [{ week, opens, back }]`), versions in use this week and last, and update checks by day and version (`launches: { windows, macos, versions }`) |
| `GET /admin/traffic` | | `{ days, views, pages, sources, last30, tags }`: people's page views for 90 days by page and source, and every link tag counted so far with its visits, downloads and first and last day |
| `GET /admin/revenue?days=30\|90\|365` | | `{ environment, interval, start, metrics: [{ slug, name, type }], totals, periods }` from Polar's metrics (currency in cents): revenue, net revenue, orders, monthly recurring revenue, active, new and churned subscriptions, checkouts and their conversion. 502 `missing_scope` when the token lacks `metrics:read` |
| `GET /admin/health` | | `{ activity, emails, countingSince, lastCleanup, problems, webhooks, webhookTypes, settings }`: daily counts of sign-ins, emails, checkouts, sync uploads and account deletions (`[{ name, detail, last7, last30 }]`, see `migrations/0006_events.sql`), emails sent today and this month against Resend's limits, when the daily clean-up last ran, failed emails and webhooks and rate limits hit (90 days), recent webhooks, and which secrets are set |
| `GET /admin/assistant-reports?offset=0` | | `{ reports: [{ id, createdAt, reply, reason }], hasMore }`, 50 reports per page, newest first |
| `DELETE /admin/assistant-reports/:id` | | `{ ok: true }`. Deletes the report and records only its ID in the admin action log |

Older days also count license activations (`activation`) and license checkouts (`checkout` with
detail `license`) from before UrBudget became free; nothing counts them now.

## Sync (desktop, needs an active subscription; otherwise 402 `subscription_required`)

Budgets are encrypted on the device (XChaCha20-Poly1305, key from the sync passphrase with
Argon2id). The server stores opaque bytes and a revision counter per budget.

| Method & path | Body | Response |
| --- | --- | --- |
| `GET /sync/key` | | `{ salt, check, createdAt }` or 404 `no_key`. `check` is a value encrypted with the key, so a device can test a passphrase |
| `PUT /sync/key` | `{ salt, check, reset? }` | `{ ok: true }`. 409 `key_exists` unless `reset: true`, which also deletes every synced budget |
| `GET /sync/budgets` | | `{ budgets: [{ id, revision, size, meta, device, updatedAt }] }`. `meta` is encrypted (the budget's name) |
| `GET /sync/budgets/:id` | | the encrypted bytes (`application/octet-stream`), headers `X-Revision`, `X-Sync-Meta` |
| `PUT /sync/budgets/:id` | encrypted bytes; headers `If-Match: <revision it replaces, 0 for new>`, `X-Sync-Meta`, `X-Device` | `{ revision }`, or 409 `{ error: "conflict", revision }` when another device got there first. 413 above 50 MB |
| `DELETE /sync/budgets/:id` | | `{ ok: true }` |
