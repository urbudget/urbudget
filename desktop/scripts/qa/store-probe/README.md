# Windows Store update diagnostics

These probes inspect the Store protocol without opening a budget, signing in, or installing an
update. They are developer tools, not an alternate updater for customers.

Build the low-level WinRT probe from the repository root:

```powershell
cargo build --manifest-path desktop/scripts/qa/store-probe/Cargo.toml
```

Run it with Windows' supported package debugging command. Use a new output file in `reports/`;
the probe overwrites that one file. Create its parent directory first.

```powershell
$package = Get-AppxPackage *urbudget* | Select-Object -First 1
$probe = (Resolve-Path desktop/scripts/qa/store-probe/target/debug/urbudget-store-probe.exe).Path
$output = Join-Path (Get-Location) 'reports/store-query.jsonl'
Invoke-CommandInDesktopPackage -PackageFamilyName $package.PackageFamilyName -AppId UrBudget -Command $probe -Args $output
```

Paths containing spaces must be enclosed in double quotes within the `-Args` string. The executable
runs with no visible console and writes JSON lines with its package identity, matching queue entries,
and the number of available updates. Wait for the `done` or `error` event before interpreting results.

The default is query-only. Append `--download-only` to the argument string to exercise downloading
an offered update. That changes the Store queue, so first finish work and close the installed app;
do not run a second updater against the same package simultaneously. This mode never calls an
installation method. It pumps the owning STA's Windows messages, records progress and completion
callbacks, and polls completion independently. After 150 seconds it requests cancellation and
records whether Windows acknowledges it. `--drop-context` is a diagnostic comparison that deliberately
releases the StoreContext before completion; it must not be used as production updater design.

The installed package's version determines what Windows offers. A current package returning no
updates does **not** validate a download or installation. Never downgrade a customer's live package
to manufacture an update. Use a Store package flight with two versions in a disposable Windows
user or VM for that test. A package debug token also differs from a normal activation token, so
these results supplement a genuine packaged application upgrade rather than replace it.

## Exercise the production commands

The separate Tauri example uses the actual `src-tauri/src/updates.rs` implementation. It creates
one hidden `about:blank` window and an isolated WebView profile beside the output file, without
initializing the budget, vault, account, or assistant services.

```powershell
cargo build --manifest-path desktop/src-tauri/Cargo.toml --example store_update_probe
$probe = (Resolve-Path desktop/src-tauri/target/debug/examples/store_update_probe.exe).Path
$output = Join-Path (Get-Location) 'reports/store-native-query.jsonl'
Invoke-CommandInDesktopPackage -PackageFamilyName $package.PackageFamilyName -AppId UrBudget -Command $probe -Args $output
```

It runs five sequential checks and ten concurrent checks. Sequential checks should all finish;
overlapping checks should return the busy error while one proceeds. The lease must be released
when that operation ends. Optional `--download-only` then runs the production download command,
which returns `none` when the installed package is current. It never invokes apply/install.

The hidden window exits when the probe finishes. Retain the JSON logs under `reports/` and follow
`docs/ARTIFACTS.md` before removing profiles or other QA evidence.

References: [Microsoft's package debugging command](https://learn.microsoft.com/en-us/powershell/module/appx/invoke-commandindesktoppackage)
and [Store update APIs](https://learn.microsoft.com/en-us/windows/uwp/packaging/self-install-package-updates).
