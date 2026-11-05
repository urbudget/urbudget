import assert from "node:assert/strict";
import { execFile, execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync, renameSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import test from "node:test";
import { changedPaths, selectChecks, successfulAncestor } from "./app-checks.mjs";

const none = { frontend: false, desktop: false };
const all = { frontend: true, desktop: true };

test("UI, phone UI, tests, and frontend build config use the shared checks", () => {
  for (const path of ["desktop/src/Register.tsx", "desktop/src/PhoneBudget.test.tsx", "desktop/src/budget.css",
    "desktop/vite.config.ts", "desktop/tsconfig.json", "desktop/index.html", "desktop/public/icon.svg"]) {
    assert.deepEqual(selectChecks([path]), { ...none, frontend: true }, path);
  }
});

test("shared native code, dependencies, scripts, CI changes and unknown app paths check every platform", () => {
  for (const path of ["desktop/src-tauri/src/vault.rs", "desktop/src-tauri/Cargo.lock",
    "desktop/src-tauri/tauri.conf.json", "desktop/package-lock.json", "desktop/package.json",
    "desktop/scripts/tauri.mjs", "desktop/new-build-setting.json", ".github/workflows/app.yml",
    "crates/urbudget-core/src/engine.rs", "crates/urbudget-core/Cargo.toml",
    "crates/urbudget-services/src/cloud/sync.rs", "crates/urbudget-services/src/vault.rs", "crates/urbudget-services/Cargo.toml", "crates/Cargo.lock", "crates/Cargo.toml",
    ".github/workflows/release.yml", ".github/scripts/app-checks.mjs", "shared/fixtures/patch-cases.json"]) {
    assert.deepEqual(selectChecks([path]), all, path);
  }
});

test("phone apps are checked by their own workflows", () => {
  assert.deepEqual(selectChecks(["shared/android/src/main/java/app/urbudget/SecretStore.kt"]), none);
  assert.deepEqual(selectChecks(["crates/urbudget-android/src/lib.rs", "android/app/build.gradle.kts"]), none, "Compose-only changes run the separate Native Android workflow");
  assert.deepEqual(selectChecks(["crates/urbudget-ios/src/lib.rs", "ios/UrBudget/App/UrBudgetApp.swift"]), none, "SwiftUI-only changes run the separate Native iOS workflow");
  assert.deepEqual(selectChecks(["desktop/src-tauri/tauri.windows.conf.json"]), { ...none, frontend: true, desktop: true });
  assert.deepEqual(selectChecks(["desktop/src-tauri/gen/schemas/desktop-schema.json"]), all, "Desktop builds read the generated schemas");
});

test("unrelated publishing/site changes and documentation do not rebuild apps", () => {
  assert.deepEqual(selectChecks(["desktop/README.md", "desktop/CHANGELOG.md", "web/src/index.ts",
    ".github/workflows/publish.yml", ".github/workflows/store.yml", ".github/workflows/deploy.yml",
    // The native apps' release workflows build android/ and ios/, not the Tauri app.
    ".github/workflows/android-release.yml", ".github/workflows/ios-release.yml", "scripts/versions.mjs"]), none);
  assert.deepEqual(selectChecks([], true), all);
});

test("a later UI push still validates native changes since the last successful run, including deleted/renamed paths", () => {
  const cwd = mkdtempSync(join(tmpdir(), "urbudget-ci-"));
  const git = (...args) => execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  const file = (path, text) => { mkdirSync(join(cwd, path, ".."), { recursive: true }); writeFileSync(join(cwd, path), text); };
  const commit = () => { git("add", "."); git("commit", "-m", "fixture"); return git("rev-parse", "HEAD"); };
  try {
    git("init"); git("config", "user.name", "CI test"); git("config", "user.email", "ci@example.invalid");
    file("desktop/src-tauri/src/old name.rs", "native");
    const green = commit();
    mkdirSync(join(cwd, "desktop/src"), { recursive: true });
    renameSync(join(cwd, "desktop/src-tauri/src/old name.rs"), join(cwd, "desktop/src/moved.ts"));
    const cancelled = commit();
    file("desktop/src/Register.tsx", "UI"); commit();
    const base = successfulAncestor([
      { conclusion: "cancelled", event: "push", head_sha: cancelled },
      { conclusion: "success", event: "pull_request", head_sha: cancelled },
      { conclusion: "success", event: "push", head_sha: "f".repeat(40) },
      { conclusion: "success", event: "push", head_sha: green },
    ], "HEAD", cwd);
    assert.equal(base, green);
    const paths = changedPaths(base, "HEAD", cwd);
    assert(paths.includes("desktop/src-tauri/src/old name.rs"));
    assert.deepEqual(selectChecks(paths), all);
    assert.equal(successfulAncestor([], "HEAD", cwd), null);
  } finally { rmSync(cwd, { recursive: true, force: true }); }
});

test("manual runs, missing history, and denied history access all run every check", async () => {
  const cwd = mkdtempSync(join(tmpdir(), "urbudget-ci-event-"));
  let status = 200;
  let requests = 0;
  const server = createServer((_request, response) => {
    requests++;
    response.writeHead(status, { "Content-Type": "application/json" });
    response.end(JSON.stringify({ workflow_runs: [] }));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const event = join(cwd, "event.json");
    const output = join(cwd, "output");
    writeFileSync(event, "{}");
    for (const eventName of ["workflow_dispatch", "push", "push"]) {
      writeFileSync(output, "");
      await promisify(execFile)(process.execPath, [fileURLToPath(new URL("./app-checks.mjs", import.meta.url))], {
        cwd,
        env: { ...process.env, GITHUB_EVENT_NAME: eventName, GITHUB_EVENT_PATH: event,
          GITHUB_OUTPUT: output, GITHUB_TOKEN: "fixture", GITHUB_REPOSITORY: "test/fixture",
          GITHUB_REF_NAME: "main", GITHUB_API_URL: `http://127.0.0.1:${server.address().port}` },
      });
      assert.equal(readFileSync(output, "utf8"), "frontend=true\ndesktop=true\n");
      if (eventName === "workflow_dispatch") assert.equal(requests, 0);
      if (eventName === "push") status = 403;
    }
    assert.equal(requests, 2);
  } finally {
    await new Promise((resolve) => server.close(resolve));
    rmSync(cwd, { recursive: true, force: true });
  }
});
