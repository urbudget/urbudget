// Generate the phone apps' "How this works" page from the website's getting-started guide.
// Like the app privacy policy (app-privacy.mjs) it has no site navigation and no links, so an app
// installed from Google Play or the App Store never leads to the website's checkout.
// Run from web/: node scripts/app-guide.mjs. Commit the generated HTML with guide edits.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export function appGuideHtml(source) {
  const article = source.match(/<article class="guide">([\s\S]*?)<\/article>/)?.[1];
  const title = source.match(/<h1 id="page-title">([^<]+)<\/h1>/)?.[1];
  const lead = source.match(/<p class="lead">([^<]+)<\/p>/)?.[1];
  if (!article || !title || !lead) throw new Error("Getting-started guide not found.");
  const content = article
    // The download and pricing buttons, and the list of other pages.
    .replace(/[ \t]*<div class="guide-cta">[\s\S]*?(?=[ \t]*<ul class="related")/, "")
    .replace(/[ \t]*<ul class="related"[\s\S]*?<\/ul>\s*/, "")
    // Sentences that only point at another page of the website.
    .replace(/[ \t]*<p data-website-only>[\s\S]*?<\/p>\r?\n/g, "")
    .replace(/\s*<span data-website-only>[\s\S]*?<\/span>/g, "")
    // Other links keep their words.
    .replace(/<a href="\/[^"]*">([\s\S]*?)<\/a>/g, "$1")
    .trimEnd();
  if (/<a\b|href=/.test(content)) throw new Error("The app guide must not link anywhere.");
  // The website's own icon link, with the ?v= hash scripts/images.mjs keeps current.
  const icon = source.match(/<link rel="icon" href="[^"]+" type="image\/svg\+xml">/)?.[0];
  if (!icon) throw new Error("Getting-started icon link not found.");
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="robots" content="noindex">
  <title>How UrBudget works: getting started</title>
  <meta name="description" content="Envelope budgeting in five steps, a first budget worked through, and a weekly routine.">
  <link rel="canonical" href="https://urbudget.app/getting-started/">
  <meta name="color-scheme" content="light">
  ${icon}
  <link rel="stylesheet" href="/assets/site.css?v=2">
</head>
<body>
  <main id="main" class="container page-body">
    <h1>${title}</h1>
    <p class="lead">${lead}</p>
    <article class="guide">${content}
    </article>
  </main>
</body>
</html>
`;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const guide = join(dirname(fileURLToPath(import.meta.url)), "..", "public", "getting-started");
  mkdirSync(join(guide, "app"), { recursive: true });
  writeFileSync(join(guide, "app", "index.html"), appGuideHtml(readFileSync(join(guide, "index.html"), "utf8")));
}
