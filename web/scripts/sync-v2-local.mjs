// Explicitly local D1/R2/workerd setup shared by the independent-process suite.
import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { createServer } from "node:net";
export const web = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const config = "wrangler.sync-v2-local.jsonc";
const cli = resolve(web, "node_modules/wrangler/bin/wrangler.js");
const env = { ...process.env, WRANGLER_SEND_METRICS: "false", CI: "1" };
const delay = ms => new Promise(r => setTimeout(r, ms));
async function stopOwnedWorker(child, port) {
  if (child.exitCode === null) {
    if (process.platform === "win32") {
      // Wrangler owns a workerd child; terminating only its Node wrapper can leak it.
      const killer = spawn("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], { windowsHide: true, stdio: "ignore" });
      await new Promise((done, reject) => { killer.on("exit", done); killer.on("error", reject); });
    } else {
      try { process.kill(-child.pid, "SIGTERM"); } catch (error) { if (error.code !== "ESRCH") throw error; }
    }
  }
  for (let i = 0; i < 100; i++) {
    const free = await new Promise(resolve => {
      const server = createServer(); server.once("error", () => resolve(false));
      server.listen(port, "127.0.0.1", () => server.close(() => resolve(true)));
    });
    if (free) return;
    await delay(50);
  }
  throw new Error(`Owned QA Worker port ${port} was not released`);
}
async function command(args) {
  const child = spawn(process.execPath, [cli, ...args], { cwd: web, env, windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
  let output = "";
  child.stdout.on("data", data => { output += data; }); child.stderr.on("data", data => { output += data; }); child.stdin.end("y\n");
  const code = await new Promise((ok, fail) => { child.on("exit", ok); child.on("error", fail); });
  if (code) throw new Error(`Local Wrangler command failed (${code}): ${output}`);
  return output;
}
export async function startLocal({ port = 8788, stamp = Date.now().toString() } = {}) {
  const persist = resolve(web, ".wrangler", "sync-v2-qa", stamp);
  await mkdir(persist, { recursive: true });
  const base = ["--config", config, "--local", "--persist-to", persist];
  await command(["d1", "migrations", "apply", "urbudget-sync-v2-local", ...base]);
  const accountId = `usr_qa_${stamp}`, email = `sync-v2-${stamp}@example.test`, token = `local-only-${stamp}`;
  const sql = `INSERT INTO users(id,email,created_at) VALUES ('${accountId}','${email}','2026-01-01');
INSERT INTO sessions(id,user_id,kind,created_at,last_seen_at,expires_at) VALUES ('${createHash("sha256").update(token).digest("hex")}','${accountId}','desktop','2026-01-01','2099-01-01','2099-12-31');
INSERT INTO subscriptions(id,user_id,customer_id,status,updated_at,event_at) VALUES ('comp:${stamp}','${accountId}','synthetic','active','2026-01-01','2026-01-01');`;
  const seed = resolve(persist, "seed.sql"); await writeFile(seed, sql);
  await command(["d1", "execute", "urbudget-sync-v2-local", ...base, "--file", seed]);
  const child = spawn(process.execPath, [cli, "dev", "--config", config, "--local", "--persist-to", persist, "--port", String(port), "--ip", "127.0.0.1", "--show-interactive-dev-session", "false"], { cwd: web, env, windowsHide: true, detached: process.platform !== "win32", stdio: ["ignore", "pipe", "pipe"] });
  let output = ""; child.stdout.on("data", data => { output += data; }); child.stderr.on("data", data => { output += data; });
  const url = `http://127.0.0.1:${port}`;
  for (let i = 0; i < 100; i++) {
    if (child.exitCode !== null) throw new Error(`Local Worker exited: ${output}`);
    try { const response = await fetch(`${url}/api/v2/sync/bootstrap`, { headers: { Authorization: `Bearer ${token}` } }); if (response.ok) return { url, accountId, email, token, persist, child,
      execute: sql => command(["d1", "execute", "urbudget-sync-v2-local", ...base, "--command", sql]),
      logs: () => output, stop: () => stopOwnedWorker(child, port) }; } catch {}
    await delay(200);
  }
  await stopOwnedWorker(child, port); throw new Error(`Local Worker did not start: ${output}`);
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const local = await startLocal();
  console.log(JSON.stringify({ url: local.url, accountId: local.accountId, email: local.email, token: local.token, persist: local.persist }));
  const stop = async () => { await local.stop(); process.exit(); };
  process.on("SIGINT", stop); process.on("SIGTERM", stop);
}
