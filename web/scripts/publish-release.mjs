// Publishes a version: uploads the installers behind the download buttons and the update packages
// installed copies fetch to the R2 bucket, then points latest.json at them. Build first
// (`npm run tauri -- build` in desktop/ on each system, which signs the update packages).
//
//   node scripts/publish-release.mjs [--windows <setup.exe>] [--macos <app.dmg>] [--linux <app.AppImage>]
//                                    [--linux-deb <app.deb>] [--notes "…"] [--local] [--version <x.y.z>]
//
// Without paths it looks for this version's installers in desktop/src-tauri/target/release/bundle
// (nsis/*.exe on Windows, dmg/*.dmg on macOS, appimage/*.AppImage and deb/*.deb on Linux). On Windows
// and Linux the installer is also the update package; on macOS the package is the *.app.tar.gz
// beside the dmg (as downloaded from a GitHub release by .github/workflows/publish.yml) or in the
// build's macos/ folder. A Linux copy updates with a package of its own kind: one that came as an
// AppImage gets the AppImage, one installed from the .deb gets the .deb. Each package's `.sig` must verify against
// the updater key in tauri.conf.json and name this version, or nothing is uploaded. A platform with
// no file keeps the installer and update it had.
// --local writes to the `wrangler dev` bucket instead of the real one. Needs `npx wrangler login`.
// --version publishes as another version than desktop/package.json's, for a build made with
// `--config` (see desktop/README.md › Trying an update locally).
import { createHash, createPublicKey, verify } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const BUCKET = "urbudget-releases";
const args = process.argv.slice(2);
const option = (name) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : undefined;
};
const fail = (message) => {
  console.error(message);
  process.exit(1);
};
const where = args.includes("--local") ? "--local" : "--remote";
const desktop = fileURLToPath(new URL("../../desktop/", import.meta.url));
const version = option("version") ?? JSON.parse(readFileSync(join(desktop, "package.json"), "utf8")).version;
const pubkey = JSON.parse(readFileSync(join(desktop, "src-tauri/tauri.conf.json"), "utf8")).plugins.updater.pubkey;
const bundle = join(desktop, "src-tauri/target/release/bundle");

const find = (folder, extension) => {
  const dir = join(bundle, folder);
  if (!existsSync(dir)) return undefined;
  const file = readdirSync(dir).find((f) => f.endsWith(extension) && f.includes(version));
  return file && join(dir, file);
};
const files = {
  windows: option("windows") ?? find("nsis", ".exe"),
  macos: option("macos") ?? find("dmg", ".dmg"),
  linux: option("linux") ?? find("appimage", ".AppImage"),
  "linux-deb": option("linux-deb") ?? find("deb", ".deb"),
};
if (!Object.values(files).some(Boolean))
  fail(`No installers for ${version}. Build them, or pass --windows / --macos / --linux / --linux-deb paths.`);

// The processor in an installer's name, and Tauri's names for the ones it runs on.
const ARCHES = { x64: ["x86_64"], amd64: ["x86_64"], arm64: ["aarch64"], aarch64: ["aarch64"], x86: ["i686"], universal: ["aarch64", "x86_64"] };
// What installed copies ask for: Tauri's name for the system and, on Linux, for the package.
const TARGETS = { windows: ["windows", ""], macos: ["darwin", ""], linux: ["linux", "-appimage"], "linux-deb": ["linux", "-deb"] };

/**
 * Checks a minisign signature, as Tauri writes it, against the updater public key. Returns the
 * version it was signed for ("" when it names none), or null when it doesn't verify.
 */
function signedVersion(bytes, signature) {
  const lines = (text) => text.split(/\r?\n/);
  const [, sigLine, trusted, globalLine] = lines(Buffer.from(signature, "base64").toString("utf8"));
  const key = Buffer.from(lines(Buffer.from(pubkey, "base64").toString("utf8"))[1], "base64");
  const sig = Buffer.from(sigLine ?? "", "base64");
  const comment = (trusted ?? "").replace(/^trusted comment: /, "");
  if (sig.length !== 74 || key.length !== 42 || !sig.subarray(2, 10).equals(key.subarray(2, 10))) return null;
  const publicKey = createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x: key.subarray(10).toString("base64url") }, format: "jwk" });
  // "ED" signs the file's BLAKE2b-512 hash; legacy "Ed" signs the bytes themselves.
  const message = sig.subarray(0, 2).toString() === "ED" ? createHash("blake2b512").update(bytes).digest() : bytes;
  const valid =
    verify(null, message, publicKey, sig.subarray(10)) &&
    verify(null, Buffer.concat([sig.subarray(10), Buffer.from(comment)]), publicKey, Buffer.from(globalLine ?? "", "base64"));
  return valid ? (comment.match(/(?:^|\t)version:(\S+)/)?.[1] ?? "") : null;
}

// Everything is checked before anything is uploaded.
const date = new Date().toISOString();
const notes = option("notes") ?? "";
const publish = [];
for (const [platform, path] of Object.entries(files)) {
  if (!path) continue;
  // Spaces become dots, so the file's URL needs no escaping.
  const name = basename(path).replace(/\s+/g, ".");
  const label = name.match(/_(x64|amd64|arm64|aarch64|x86|universal)(?:-setup)?\.(?:exe|dmg|AppImage|deb)$/)?.[1];
  if (!label) fail(`Can't tell which processor ${name} is for: its name should end like _x64-setup.exe, _aarch64.dmg or _amd64.AppImage.`);
  let pkg = path;
  if (platform === "macos") {
    const near = [dirname(path), join(dirname(path), "..", "macos")].flatMap((dir) =>
      existsSync(dir) ? readdirSync(dir).filter((f) => f.endsWith(".app.tar.gz")).map((f) => join(dir, f)) : [],
    );
    // tauri-action names it UrBudget_universal.app.tar.gz; a local build, UrBudget.app.tar.gz.
    const tarball = near.find((f) => basename(f).includes(`_${label}.`)) ?? (near.length === 1 ? near[0] : undefined);
    if (!tarball) fail(`No *.app.tar.gz for ${name} beside it or in ../macos. Build with the updater key (desktop/README.md › Updates).`);
    pkg = tarball;
  }
  if (!existsSync(`${pkg}.sig`)) fail(`${basename(pkg)} has no .sig. Build with the updater key (desktop/README.md › Updates).`);
  const signature = readFileSync(`${pkg}.sig`, "utf8").trim();
  const signed = signedVersion(readFileSync(pkg), signature);
  if (signed === null) fail(`${basename(pkg)}.sig doesn't verify with the updater key in desktop/src-tauri/tauri.conf.json.`);
  if (signed !== version) fail(`${basename(pkg)} is signed for ${signed ? `version ${signed}` : "no version"}, not ${version}. Build it again.`);
  const update = platform === "macos" ? `UrBudget_${version}_${label}.app.tar.gz` : name;
  publish.push({ platform, path, name, pkg, update, signature, arches: ARCHES[label] });
}

const windows = process.platform === "win32";
// Windows runs npx through the shell, which splits unquoted paths at their spaces.
const quote = (arg) => (windows && /\s/.test(arg) ? `"${arg}"` : arg);
const run = (quiet, ...rest) =>
  execFileSync(windows ? "npx.cmd" : "npx", ["wrangler", ...rest].map(quote), {
    stdio: ["ignore", "pipe", quiet ? "ignore" : "inherit"],
    shell: windows,
  }).toString();
const wrangler = (...rest) => run(false, ...rest);
const upload = (name, path) =>
  wrangler("r2", "object", "put", `${BUCKET}/files/${name}`, "--file", path, "--content-type", "application/octet-stream", where);

let previous = { platforms: {} };
try {
  const out = join(mkdtempSync(join(tmpdir(), "urbudget-")), "latest.json");
  run(true, "r2", "object", "get", `${BUCKET}/latest.json`, "--file", out, where);
  previous = JSON.parse(readFileSync(out, "utf8"));
} catch {
  /* first release */
}

const platforms = { ...previous.platforms };
const updates = { ...previous.updates };
for (const { platform, path, name, pkg, update, signature, arches } of publish) {
  const bytes = readFileSync(path);
  platforms[platform] = {
    file: name,
    size: statSync(path).size,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
  console.log(`Uploading ${platform}: ${name} (${(bytes.length / 1e6).toFixed(1)} MB)`);
  upload(name, path);
  if (pkg !== path) {
    console.log(`Uploading ${platform} update: ${update} (${(statSync(pkg).size / 1e6).toFixed(1)} MB)`);
    upload(update, pkg);
  }
  const [target, kind] = TARGETS[platform];
  for (const arch of arches) updates[`${target}-${arch}${kind}`] = { version, date, notes, file: update, signature };
}

const latest = {
  version,
  date: date.slice(0, 10),
  notes,
  platforms,
  updates,
};
const manifest = join(mkdtempSync(join(tmpdir(), "urbudget-")), "latest.json");
writeFileSync(manifest, JSON.stringify(latest, null, 2));
wrangler("r2", "object", "put", `${BUCKET}/latest.json`, "--file", manifest, "--content-type", "application/json", where);
console.log(`Published UrBudget ${version}:`, Object.keys(platforms).join(", "));
console.log("Installed copies update from it:", Object.keys(updates).join(", "));
