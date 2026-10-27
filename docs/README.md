# Documentation

Use the guides below for current behavior. The desktop app is in `desktop/` (Tauri and React);
the phone apps are native, in `ios/` (SwiftUI) and `android/` (Kotlin and Jetpack Compose).
All of them use `urbudget-core` for accounting/storage and `urbudget-services`
for the account, encrypted sync, assistant and bank services.

| Topic | Current guide |
| --- | --- |
| Development, architecture, data, desktop releases | [Desktop guide](../desktop/README.md) |
| Rust accounting, YNAB/Actual comparison and engine audit | [Rust engine](RUST-ENGINE.md) |
| How the phone apps are built, where their code is, secrets, CI and store setup | [Mobile guide](MOBILE.md) |
| Native Kotlin Android client, shared services, billing and feature coverage | [Native Android](../android/README.md) |
| Native SwiftUI iOS client, its Rust adapter and feature coverage | [Native iOS](../ios/README.md) |
| English and Arabic: the language setting, right to left, the Arabic style guide and glossary | [Languages](LOCALIZATION.md) |
| Versions, tags and release workflows for each app | [Releasing](../RELEASING.md) |
| Published versions and remaining release work | [Release status](RELEASE-STATUS.md) |
| Agreed work not done yet | [Follow-ups](FOLLOW-UPS.md) |
| Changes by release | [Changelog](../desktop/CHANGELOG.md) |
| Website/API operation and deployment | [Website guide](../web/README.md), [API reference](../web/API.md) |
| Local test artifacts and retained evidence | [Artifact retention](ARTIFACTS.md) |
| Earlier investigations and research | [Historical archive](archive/README.md) |

When behavior changes, update its guide and user-facing copy in the same change. Record current
release state in RELEASE-STATUS.md. Store listings, screenshots, the Play checklist and marketing
records are kept privately, outside this repository. Keep completed
investigations dated and marked historical; do not rewrite their observations as if they describe
the current build. Version-specific changelog entries and minimum-supported-version statements
should keep their original version numbers.
