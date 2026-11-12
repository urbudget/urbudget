// Builds the site's images from the app screenshots and the app icon.
//
//   node scripts/images.mjs        (from web/)
//
// Input:  screenshots/<name>-<light|dark>.png  (2× captures from scripts/screenshots/shoot.mjs:
//         a 1280×800 CSS-pixel window, captured at 2560×1600)
//         ../desktop/assets/icon.svg
// Output: public/img/  WebP screenshots and crops at 1× and 2×, favicon.svg,
//         apple-touch-icon.png, icon-192.png, icon-512.png and og.png.
//         public/**/*.html  screenshot dimensions/srcsets, and every /img/ link's ?v= hash.
//
// Every crop is listed in CROPS in the screenshot's own 2× pixels. Each one is written at its
// requested widths (default: native and half), as <name>-<theme>-<width>.webp.

import { mkdir, copyFile, readFile, readdir, stat, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const shots = join(root, "screenshots");
const out = join(root, "public", "img");
const iconSvg = resolve(root, "..", "desktop", "assets", "icon.svg");

const THEMES = ["light", "dark"];
const WEBP = { quality: 82, effort: 6, smartSubsample: true };

/**
 * name: output name; src: screenshot; region: { left, top, width, height } in 2× pixels
 * (omit for the whole window); widths: output widths (default: native and half).
 */
const CROPS = [
  // Hero, download page and last card: the whole window.
  { name: "budget", src: "budget", widths: [800, 1280, 2560] },
  // The tour's pictures, each run off the right and bottom of its card, so a crop can end
  // mid-column. Budget: the month, its totals and the envelopes with their targets, up to the
  // table's edge.
  { name: "budget-table", src: "budget", region: { left: 496, top: 72, width: 1456, height: 1528 }, widths: [720, 1440] },
  // Register: net worth, the toolbar with the filter search, the scheduled rows and the rest.
  { name: "register", src: "register", region: { left: 496, top: 72, width: 1544, height: 1528 }, widths: [720, 1440] },
  // The car loan's payoff planner, just inside the dialog's edge.
  { name: "loans", src: "loans", region: { left: 428, top: 54, width: 1704, height: 1492 }, widths: [720, 1440] },
  // The reports widget's picture for when its script doesn't run.
  { name: "report-reports", src: "reports", region: { left: 512, top: 280, width: 2016, height: 1136 } },
];

const kb = (n) => `${(n / 1024).toFixed(0)} KB`;
let total = 0;

async function writeWebp(pipeline, file) {
  await pipeline.webp(WEBP).toFile(file);
  const { size } = await stat(file);
  total += size;
  return size;
}

async function screenshots() {
  for (const crop of CROPS) {
    for (const theme of THEMES) {
      const input = join(shots, `${crop.src}-${theme}.png`);
      if (!existsSync(input)) throw new Error(`Missing ${input}`);
      const meta = await sharp(input).metadata();
      const native = crop.region?.width ?? meta.width;
      const widths = crop.widths ?? [Math.round(native / 2), native];
      for (const width of widths) {
        let img = sharp(input);
        if (crop.region) img = img.extract(crop.region);
        if (width !== native) img = img.resize({ width, kernel: "lanczos3" });
        const file = join(out, `${crop.name}-${theme}-${width}.webp`);
        const size = await writeWebp(img, file);
        const height = Math.round(((crop.region?.height ?? meta.height) * width) / native);
        console.log(`${crop.name}-${theme}-${width}.webp  ${width}×${height}  ${kb(size)}`);
      }
    }
  }
}

async function icons() {
  await copyFile(iconSvg, join(out, "favicon.svg"));
  const svg = await readFile(iconSvg);
  // The icon's rounded square leaves a margin; Apple's touch icon wants a full square, so it
  // gets the brand purple behind it.
  await sharp(svg, { density: 72 * 4 })
    .resize(180, 180)
    .flatten({ background: "#6d4ae8" })
    .png({ compressionLevel: 9 })
    .toFile(join(out, "apple-touch-icon.png"));
  for (const size of [192, 512]) {
    await sharp(svg, { density: 72 * 4 })
      .resize(size, size)
      .png({ compressionLevel: 9 })
      .toFile(join(out, `icon-${size}.png`));
  }
  console.log("favicon.svg, apple-touch-icon.png, icon-192.png, icon-512.png");
}

/** Rounds an image's corners and gives it a hairline border. */
async function rounded(buffer, width, height, radius) {
  const mask = Buffer.from(
    `<svg width="${width}" height="${height}"><rect width="${width}" height="${height}" rx="${radius}" ry="${radius}"/></svg>`,
  );
  const border = Buffer.from(
    `<svg width="${width}" height="${height}"><rect x="0.75" y="0.75" width="${width - 1.5}" height="${height - 1.5}" rx="${radius}" ry="${radius}" fill="none" stroke="rgba(255,255,255,0.35)" stroke-width="1.5"/></svg>`,
  );
  return sharp(buffer)
    .composite([
      { input: mask, blend: "dest-in" },
      { input: border, blend: "over" },
    ])
    .png()
    .toBuffer();
}

async function ogImage() {
  const W = 1200;
  const H = 630;
  // Where the screenshot starts; the words on the left must end before it and its shadow.
  const left = 580;
  const top = 168;
  const font = `'Segoe UI', 'Segoe UI Variable Display', system-ui, -apple-system, 'Helvetica Neue', Arial, sans-serif`;
  const background = Buffer.from(`
    <svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">
      <rect width="${W}" height="${H}" fill="#5b3bd6"/>
      <text x="72" y="234" font-family="${font}" font-size="64" font-weight="700" fill="#ffffff" letter-spacing="-1">UrBudget</text>
      <text x="72" y="312" font-family="${font}" font-size="42" font-weight="600" fill="#ffffff">Your budget,</text>
      <text x="72" y="362" font-family="${font}" font-size="42" font-weight="600" fill="#ffffff">on your computer.</text>
      <text x="72" y="416" font-family="${font}" font-size="25" fill="#e6defd">Envelope budgeting for</text>
      <text x="72" y="450" font-family="${font}" font-size="25" fill="#e6defd">Windows and macOS.</text>
      <text x="72" y="592" font-family="${font}" font-size="22" font-weight="600" fill="#ffffff">urbudget.app</text>
    </svg>`);

  // Fonts differ between machines, so check that no word reaches the screenshot's shadow.
  const { data, info } = await sharp(background).raw().toBuffer({ resolveWithObject: true });
  for (let y = 0; y < H; y++) {
    for (let x = left - 48; x < W; x++) {
      const i = (y * W + x) * info.channels;
      if (data[i] !== 0x5b || data[i + 1] !== 0x3b || data[i + 2] !== 0xd6) {
        throw new Error(`og.png: the text reaches x=${x} (y=${y}) and runs under the screenshot`);
      }
    }
  }

  const icon = await sharp(await readFile(iconSvg), { density: 72 * 2 })
    .resize(96, 96)
    .png()
    .toBuffer();

  // The budget screenshot, its left part (sidebar and envelopes) running off the right edge.
  const shotW = 760;
  const shotH = 520;
  const shot = await sharp(join(shots, "budget-light.png"))
    .resize({ width: 1300, kernel: "lanczos3" })
    .extract({ left: 0, top: 0, width: shotW, height: shotH })
    .png()
    .toBuffer();
  const framed = await rounded(shot, shotW, shotH, 18);
  const shadow = await sharp({
    create: { width: shotW + 80, height: shotH + 80, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } },
  })
    .composite([
      {
        input: Buffer.from(
          `<svg width="${shotW + 80}" height="${shotH + 80}"><rect x="40" y="48" width="${shotW}" height="${shotH}" rx="18" fill="rgba(0,0,0,0.35)"/></svg>`,
        ),
      },
    ])
    .blur(18)
    .png()
    .toBuffer();

  // Composed on a larger canvas so the screenshot can run off the right and bottom edges,
  // then cut to 1200×630.
  const canvas = await sharp(background)
    .extend({ right: 400, bottom: 200, background: { r: 0, g: 0, b: 0, alpha: 0 } })
    .composite([
      { input: icon, left: 72, top: 64 },
      { input: shadow, left: left - 40, top: top - 40 },
      { input: framed, left, top },
    ])
    .png()
    .toBuffer();
  await sharp(canvas)
    .extract({ left: 0, top: 0, width: W, height: H })
    .flatten({ background: "#5b3bd6" })
    .png({ compressionLevel: 9 })
    .toFile(join(out, "og.png"));
  const { size } = await stat(join(out, "og.png"));
  console.log(`og.png  1200×630  ${kb(size)}`);
}

/**
 * Browsers keep /img/* for a day (public/_headers), and Discord, Slack, X and the rest keep a
 * link preview's image by its URL, so a retaken image at the old URL goes unseen for a while.
 * Every page links each image with a hash of its contents instead (?v=…).
 */
async function stampImageVersions() {
  const versions = new Map();
  for (const file of await readdir(out)) {
    // An SVG is text, so a Windows checkout has CRLF where CI (and the deployed copy) has LF.
    let data = await readFile(join(out, file));
    if (file.endsWith(".svg")) data = Buffer.from(data.toString("utf8").replace(/\r\n/g, "\n"));
    versions.set(file, createHash("sha256").update(data).digest("hex").slice(0, 8));
  }
  const link = /((?:https:\/\/urbudget\.app)?\/img\/)([\w.-]+)(\?v=[0-9a-f]+)?/g;
  const pages = (await readdir(join(root, "public"), { recursive: true })).filter((f) => f.endsWith(".html"));
  let stamped = 0;
  for (const page of pages) {
    const file = join(root, "public", page);
    const html = await readFile(file, "utf8");
    const next = html.replace(link, (match, dir, name) => (versions.has(name) ? `${dir}${name}?v=${versions.get(name)}` : match));
    if (next !== html) {
      await writeFile(file, next);
      stamped++;
    }
  }
  console.log(`Image links stamped with their hashes in ${stamped} changed page(s)`);
}

// Keep intrinsic sizes and responsive width descriptors tied to the actual exports.
// A new screenshot crop can change its aspect ratio even when its filename stays the same.
async function stampScreenshotSizes() {
  const dimensions = new Map();
  for (const file of await readdir(out)) {
    if (file.endsWith(".webp")) dimensions.set(`/img/${file}`, await sharp(join(out, file)).metadata());
  }
  for (const page of (await readdir(join(root, "public"), { recursive: true })).filter(f => f.endsWith(".html"))) {
    const file = join(root, "public", page);
    const html = await readFile(file, "utf8");
    const next = html.replace(/<(?:img|source)\b[^>]*>/g, tag => {
      let updated = tag.replace(/(\/img\/[^\s,"<>?]+\.webp)(\?v=[0-9a-f]+)?\s+\d+w/g, (match, url, version = "") => {
        const meta = dimensions.get(url);
        return meta ? `${url}${version} ${meta.width}w` : match;
      });
      const source = updated.match(/\bsrc="([^"?]+)/);
      const meta = source && dimensions.get(source[1]);
      if (meta && updated.startsWith("<img")) {
        updated = updated.replace(/\bwidth="\d+"/, `width="${meta.width}"`)
          .replace(/\bheight="\d+"/, `height="${meta.height}"`);
      }
      return updated;
    });
    if (next !== html) await writeFile(file, next);
  }
}

/**
 * The home page's Android section: the phone's screen from the Play listing's Bank messages
 * screenshot (docs/play-listing/graphics), without its caption, with the screen's own round
 * corners cut out. Its region and radius are in that 1440×2560 image's pixels.
 */
const PHONE = {
  name: "phone-bank-messages",
  src: resolve(root, "..", "docs", "play-listing", "graphics", "2-bank-messages.png"),
  region: { left: 268, top: 568, width: 904, height: 1935 },
  radius: 88,
  widths: [360, 720],
};

async function phoneShots() {
  const { region, radius } = PHONE;
  const mask = Buffer.from(
    `<svg width="${region.width}" height="${region.height}"><rect width="${region.width}" height="${region.height}" rx="${radius}" ry="${radius}"/></svg>`,
  );
  const screen = await sharp(PHONE.src).extract(region).ensureAlpha()
    .composite([{ input: mask, blend: "dest-in" }]).png().toBuffer();
  for (const width of PHONE.widths) {
    const file = join(out, `${PHONE.name}-${width}.webp`);
    const size = await writeWebp(sharp(screen).resize({ width, kernel: "lanczos3" }), file);
    console.log(`${PHONE.name}-${width}.webp  ${width}×${Math.round((region.height * width) / region.width)}  ${kb(size)}`);
  }
}

await mkdir(out, { recursive: true });
// `node scripts/images.mjs phone` remakes only the phone pictures, without the desktop screenshots.
if (process.argv[2] !== "phone") {
  await icons();
  await screenshots();
  await ogImage();
}
await phoneShots();
await stampScreenshotSizes();
await stampImageVersions();
console.log(`WebP screenshots in total: ${kb(total)}`);
