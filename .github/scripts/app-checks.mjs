// Select native builds conservatively; ordinary UI edits need only the shared web checks.
// Phones are the native apps in android/ and ios/, checked by their own workflows.
import { appendFileSync, readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { pathToFileURL } from "node:url";

export function selectChecks(paths, full = false) {
  const checks = { frontend: full, desktop: full };
  const all = () => Object.keys(checks).forEach((key) => { checks[key] = true; });
  for (const path of paths) {
    if (path.endsWith(".md")) continue;
    if (path.startsWith(".github/scripts/") ||
        /^\.github\/workflows\/(app|release)\.yml$/.test(path)) {
      all();
    } else if (path.startsWith("crates/") && !path.startsWith("crates/urbudget-android/") && !path.startsWith("crates/urbudget-ios/")) {
      // Core, services and their workspace lockfile are built into the desktop app.
      all();
    } else if (path.startsWith("shared/fixtures/")) {
      // Read by desktop's shared-cases test and by core's tests alike.
      all();
    } else if (path.startsWith("desktop/")) {
      checks.frontend = true;
      if (path.startsWith("desktop/src/") || path.startsWith("desktop/public/") ||
          /^desktop\/(index\.html|(?:vite|vitest)\.config\.[cm]?[jt]s|tsconfig[^/]*\.json)$/.test(path)) {
        continue;
      } else if (path === "desktop/src-tauri/tauri.windows.conf.json" ||
                 path === "desktop/src-tauri/capabilities/desktop.json" ||
                 path.startsWith("desktop/msix/")) {
        checks.desktop = true;
      } else {
        // Rust, dependencies, build scripts, shared configuration, and unknown new
        // paths run every platform. Never infer platform safety from a Rust filename.
        all();
      }
    }
  }
  return checks;
}

export function changedPaths(base, head = "HEAD", cwd = process.cwd()) {
  // Disable rename detection so moving a file out of a platform also checks its old path.
  return execFileSync("git", ["diff", "--name-only", "--no-renames", "-z", base, head, "--"],
    { cwd, encoding: "utf8" }).split("\0").filter(Boolean);
}

export function successfulAncestor(runs, head = "HEAD", cwd = process.cwd()) {
  for (const run of runs) {
    if (run.conclusion !== "success" || !["push", "workflow_dispatch"].includes(run.event) ||
        !/^[a-f0-9]{40}$/.test(run.head_sha)) continue;
    try {
      execFileSync("git", ["merge-base", "--is-ancestor", run.head_sha, head], { cwd, stdio: "ignore" });
      return run.head_sha;
    } catch { /* A force-pushed or unavailable commit cannot establish a baseline. */ }
  }
  return null;
}

async function main() {
  const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, "utf8"));
  let base;
  let full = process.env.GITHUB_EVENT_NAME === "workflow_dispatch";
  try {
    if (!full && process.env.GITHUB_EVENT_NAME === "pull_request") {
      base = execFileSync("git", ["merge-base", event.pull_request.base.sha, "HEAD"],
        { encoding: "utf8" }).trim();
    } else if (!full && process.env.GITHUB_EVENT_NAME === "push") {
      // Compare with the last green run, not just the previous push: cancelled or
      // failed native changes must still be validated after a subsequent UI-only edit.
      const url = new URL(`${process.env.GITHUB_API_URL}/repos/${process.env.GITHUB_REPOSITORY}/actions/workflows/app.yml/runs`);
      url.search = new URLSearchParams({ branch: process.env.GITHUB_REF_NAME, status: "success", per_page: "100" });
      const response = await fetch(url, { signal: AbortSignal.timeout(30_000), headers: { Authorization: `Bearer ${process.env.GITHUB_TOKEN}`, Accept: "application/vnd.github+json" } });
      if (!response.ok) throw new Error(`Workflow history returned HTTP ${response.status}`);
      base = successfulAncestor((await response.json()).workflow_runs);
    }
    full ||= !base;
    const checks = selectChecks(full ? [] : changedPaths(base), full);
    console.log(JSON.stringify({ base: base ?? null, full, checks }, null, 2));
    appendFileSync(process.env.GITHUB_OUTPUT, Object.entries(checks).map(([key, value]) => `${key}=${value}\n`).join(""));
  } catch (error) {
    // Loss of history or API access must increase coverage, never skip validation.
    console.warn(`Cannot establish changed files; running every check: ${error.message}`);
    appendFileSync(process.env.GITHUB_OUTPUT, Object.keys(selectChecks([])).map((key) => `${key}=true\n`).join(""));
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await main();
