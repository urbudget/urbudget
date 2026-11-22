<p align="center">
  <img src="desktop/assets/icon.svg" width="112" height="112" alt="">
</p>

<h1 align="center">UrBudget</h1>

<p align="center">
  <b>Zero-based envelope budgeting for all your devices.</b><br>
  Free and open source. Your budget stays on your own devices, not on someone else's server.
</p>

<p align="center">
  <a href="https://urbudget.app">Website</a> ·
  <a href="#download">Download</a> ·
  <a href="https://apps.apple.com/app/id6825535486">App Store</a> ·
  <a href="https://urbudget.app/ynab/">Switch from YNAB</a> ·
  <a href="docs/README.md">Docs</a>
</p>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="web/public/img/budget-dark-1280.webp">
  <img src="web/public/img/budget-light-1280.webp" alt="UrBudget on a computer: a month's budget, with envelopes grouped under Credit Card Payments, Bills and Everyday, each with what's assigned, spent and available">
</picture>

UrBudget is an envelope budgeting app, the zero-based method that YNAB made popular: you give
every dollar you have a job, and spending comes out of the envelope you gave it to. Each budget is
a file on your own device. It works offline, needs no account and no bank connection, and every
feature is free. The only paid part is optional end-to-end encrypted sync between your devices, at
$3 a month.

## Download

| Device | Get UrBudget | Needs |
| --- | --- | --- |
| Windows | [Microsoft Store](https://apps.microsoft.com/detail/9PMQMBZJG06R) (recommended) or the [installer (.exe)](https://urbudget.app/download/windows) | Windows 10 or 11, 64-bit |
| Mac | [Download (.dmg)](https://urbudget.app/download/macos), one app for Apple silicon and Intel | macOS 12 Monterey or later |
| Linux | [AppImage](https://urbudget.app/download/linux) or [.deb for Ubuntu and Debian](https://urbudget.app/download/linux-deb) | 64-bit Intel or AMD, a distribution from 2022 or later |
| iPhone and iPad | [App Store](https://apps.apple.com/app/id6825535486) | iOS and iPadOS 17 or later |
| Android | [Closed test on Google Play](https://urbudget.app/android/), open to anyone | Android 8.0 or later |

Every download, with checksums, install steps and what's new, is on
[urbudget.app/download](https://urbudget.app/download/). The desktop installers are also attached
to each [GitHub release](https://github.com/urbudget/urbudget/releases/latest). The direct
installers update themselves from inside the app; the stores update theirs.

## What it does

- **Envelope budgeting.** Assign every dollar to a category, set targets by the month, the week
  or a due date, and roll what's left into next month. Credit card payment
  categories set aside what you owe as you spend. Quick assign fills categories from their
  targets or from last month.
- **Accounts and transactions.** Checking, savings, cash, credit cards and loans, with a register
  you can search and filter, split transactions, transfers and reconciliation.
- **Scheduled transactions and loans.** Bills enter themselves when they're due, and the loan
  planner shows the payoff date and interest with extra payments.
- **Twelve reports.** From spending by category to net worth and a forecast of your balances, each
  filtered by date, account and category, and exported to CSV. A month in review as a PDF, or an
  Excel workbook of transactions and budgets.
- **Bring your YNAB budget over.** Paste a YNAB personal access token and import the whole budget,
  with targets, notes and scheduled transactions. UrBudget checks every month against YNAB's own
  totals. [How it works](https://urbudget.app/ynab/)
- **Your bank, your choice.** Import your bank's CSV, OFX, QFX or QIF files, with duplicates
  skipped and matches paired. Or connect supported US and Canadian banks through optional
  [SimpleFIN bank sync](https://urbudget.app/bank-sync/) (paid separately to SimpleFIN), and
  review what comes in before it's saved.
- **An optional assistant.** Connect an AI service and ask how the month is going, cover
  overspending, or drop in a receipt or a statement. Every change it proposes is a card you apply
  or discard, and an applied one can be undone. On Android it can turn your bank's text messages
  into transactions to review.
- **Sync, if you want it.** Keep your computers, iPhone and iPad in step for $3 a month. The budget
  is encrypted with your passphrase before it leaves your device, so urbudget.app can't read it.
- **English and Arabic**, with any currency.

## Private by design

- Each budget is a SQLite file on your own device, with automatic backups beside it. You can
  export it as a file at any time.
- No account is needed to budget. The app goes online to check for updates (the desktop
  versions from this site), and for what you turn on: sync, bank sync and the assistant.
- Once a day each copy tells urbudget.app it was opened, with no identifier, so we can count
  copies in use. You can turn that off in Settings, under Privacy & data.
- With sync, budgets are end-to-end encrypted with your passphrase, so the server stores them in a
  form it can't read.
- The [privacy policy](https://urbudget.app/privacy/) lists everything urbudget.app receives.

## Pricing

The apps are free, with every feature and no time limit. Optional sync between your devices is
$3 a month, bought on [urbudget.app](https://urbudget.app/pricing/) or in the phone apps. SimpleFIN and the AI service you connect charge separately, if you use them.

## How UrBudget compares

UrBudget isn't affiliated with YNAB or any other product. These pages compare it honestly, with
prices checked on the date each one gives:

- [UrBudget vs YNAB](https://urbudget.app/vs/ynab/) and [YNAB alternatives](https://urbudget.app/ynab-alternatives/)
- [Mint alternatives](https://urbudget.app/mint-alternatives/)
- [All comparisons](https://urbudget.app/vs/): Actual Budget, Goodbudget, Monarch, Copilot,
  EveryDollar, Quicken, Moneydance and more

What it doesn't do: there's no web version, no shared budget for several people, and no
automatic background bank sync. You press Sync and review.

## For developers

UrBudget is one Rust engine that does the accounting everywhere, with each app drawing its own
interface.

| Folder | What it is |
| --- | --- |
| [`desktop/`](desktop/README.md) | The Windows, macOS and Linux app: Tauri, with a React window |
| [`ios/`](ios/README.md) | The iPhone and iPad app: SwiftUI, an Xcode project |
| [`android/`](android/README.md) | The Android app: Kotlin and Jetpack Compose, a Gradle project |
| [`crates/`](crates/) | The shared Rust engine. [`urbudget-core`](crates/urbudget-core/) does the accounting, SQLite storage, imports, reports and sync merges; [`urbudget-services`](crates/urbudget-services/) the account, encrypted sync, sealed vault, bank and YNAB connections and the assistant. [`urbudget-ios`](crates/urbudget-ios/) and [`urbudget-android`](crates/urbudget-android/) let the phone apps call them |
| [`shared/`](shared/) | Kotlin for Android's Keystore and Google Play billing, and test fixtures |
| [`web/`](web/README.md) | The website and API at urbudget.app (one Cloudflare Worker): accounts, the sync subscription, sync storage |
| [`scripts/`](scripts/) | Repository tools, such as [`versions.mjs`](scripts/versions.mjs) for each app's version |
| [`docs/`](docs/README.md) | Guides, release status and history |

```text
desktop/   React window  -> Tauri commands ----------+
ios/       SwiftUI screens -> crates/urbudget-ios ----+--> crates/urbudget-services
android/   Compose screens -> crates/urbudget-android +    crates/urbudget-core
```

The [mobile guide](docs/MOBILE.md) explains how the phone apps build the Rust engine into
themselves and where their code is. Desktop, Android and iOS each have their own version, tag and
release workflow: see [RELEASING.md](RELEASING.md). Start with the
[documentation index](docs/README.md) for everything else.

To run the desktop app:

```sh
cd desktop
npm ci
npm run desktop
```

For the phone apps, open `ios/UrBudget.xcodeproj` in Xcode or the `android/` folder in Android
Studio and run; each README lists what to install first.

Private YNAB exports, `migration-*` fixtures and local `reports/` evidence are not application
assets or source documentation. See [artifact retention](docs/ARTIFACTS.md) before cleaning them.

Questions and bug reports: [GitHub issues](https://github.com/urbudget/urbudget/issues) or
[support@urbudget.app](mailto:support@urbudget.app).

## License

Copyright (C) 2026 Ahmad Horanieh.

UrBudget is free software: you can redistribute it and/or modify it under the terms of the
[GNU Affero General Public License](LICENSE) as published by the Free Software Foundation, either
version 3 of the License, or (at your option) any later version. It is distributed in the hope that
it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY
or FITNESS FOR A PARTICULAR PURPOSE. Running a modified version for others over a network means
offering them its source, as the license's section 13 describes.

"UrBudget" and its icon identify the official apps and urbudget.app; forks should use their own
name.
