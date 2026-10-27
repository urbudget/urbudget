// The Arabic catalog against the code: every string the code translates has Arabic, and every
// Arabic entry fits its English.
import { describe, expect, it, vi } from "vitest";
import ts from "typescript";
import { ar } from "./ar";
import type { PluralForms } from "./index";

type Fs = {
  readdirSync(path: string, options: { withFileTypes: true }): { name: string; isDirectory(): boolean }[];
  readFileSync(path: string, encoding: "utf8"): string;
};
const fs = await vi.importActual<Fs>("node:fs");

const FORMS = ["zero", "one", "two", "few", "many", "other"] as const;
/** The app's own source files (not tests, not the catalog), from the desktop folder the tests run in. */
function sources(dir = "src"): { name: string; text: string }[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = `${dir}/${entry.name}`;
    if (entry.isDirectory()) return entry.name === "i18n" ? [] : sources(path);
    if (!/\.tsx?$/.test(entry.name) || /\.test\.|\.d\.ts$/.test(entry.name)) return [];
    return [{ name: path, text: fs.readFileSync(path, "utf8") }];
  });
}

type Use = { key: string; plural: boolean; where: string };
/** Every literal key passed to t, tc, msg, msgc (strings) and tn, tnc (plurals). */
function uses(): { found: Use[]; dynamic: string[] } {
  const found: Use[] = [];
  const dynamic: string[] = [];
  for (const file of sources()) {
    const sf = ts.createSourceFile(file.name, file.text, ts.ScriptTarget.Latest, true, file.name.endsWith("x") ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
    const literal = (n: ts.Node | undefined) =>
      n && (ts.isStringLiteral(n) || ts.isNoSubstitutionTemplateLiteral(n)) ? n.text : null;
    const visit = (node: ts.Node) => {
      if (ts.isCallExpression(node) && ts.isIdentifier(node.expression)) {
        const name = node.expression.text;
        const args = node.arguments;
        const where = `${file.name}:${sf.getLineAndCharacterOfPosition(node.getStart()).line + 1}`;
        const keyArgs: Record<string, number[]> = { t: [0], msg: [0], tc: [1], msgc: [1], tn: [1, 2], tnc: [2, 3] };
        for (const i of keyArgs[name] ?? []) if (args[i] && ts.isTemplateExpression(args[i])) dynamic.push(where);
        const context = name === "tc" || name === "msgc" || name === "tnc" ? literal(args[0]) : null;
        const withContext = (key: string | null) => (key === null ? null : context === null ? key : `${key} (${context})`);
        if (name === "t" || name === "msg" || name === "tc" || name === "msgc") {
          const key = withContext(literal(args[context === null ? 0 : 1]));
          if (key !== null) found.push({ key, plural: false, where });
        }
        if (name === "tn" || name === "tnc") {
          const key = withContext(literal(args[name === "tn" ? 2 : 3]));
          if (key !== null) found.push({ key, plural: true, where });
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(sf);
  }
  return { found, dynamic };
}

const placeholders = (text: string) => new Set([...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]));
const sorted = (set: Set<string>) => [...set].sort();
const { found, dynamic } = uses();

describe("the Arabic catalog", () => {
  it("finds the app's strings", () => {
    // A guard for the extractor itself: the app has a couple of thousand strings.
    expect(found.length).toBeGreaterThan(1500);
    expect(found.some((u) => u.key === "Save" && !u.plural)).toBe(true);
    expect(found.some((u) => u.key === "{count} transactions" && u.plural)).toBe(true);
  });

  it("has Arabic for every string the code translates", () => {
    const missing = found.filter((u) => !(u.key in ar)).map((u) => `${u.where} ${JSON.stringify(u.key)}`);
    expect(missing).toEqual([]);
  });

  it("has plural forms exactly where the code counts", () => {
    const wrong = found
      .filter((u) => u.key in ar && (typeof ar[u.key] === "object") !== u.plural)
      .map((u) => `${u.where} ${JSON.stringify(u.key)} ${u.plural ? "needs plural forms" : "is not a count"}`);
    expect(wrong).toEqual([]);
  });

  it("only uses literal keys, so every string can be found and translated", () => {
    expect(dynamic).toEqual([]);
  });

  it("has nothing the code no longer uses", () => {
    const used = new Set(found.map((u) => u.key));
    expect(Object.keys(ar).filter((key) => !used.has(key))).toEqual([]);
  });

  it("has no empty translations and keeps each string's placeholders", () => {
    const problems: string[] = [];
    for (const [key, entry] of Object.entries(ar)) {
      const english = placeholders(key);
      if (typeof entry === "string") {
        if (!entry.trim()) problems.push(`empty: ${key}`);
        else if (sorted(placeholders(entry)).join() !== sorted(english).join()) problems.push(`placeholders: ${key} -> ${entry}`);
        continue;
      }
      for (const form of FORMS) {
        const text = (entry as PluralForms)[form];
        if (!text?.trim()) {
          problems.push(`empty ${form}: ${key}`);
          continue;
        }
        const got = placeholders(text);
        // "One transaction" and "two transactions" are words in Arabic: {count} may go there.
        const needed = [...english].filter((name) => name !== "count" || !["zero", "one", "two"].includes(form));
        const extra = [...got].filter((name) => !english.has(name));
        if (extra.length || needed.some((name) => !got.has(name))) problems.push(`placeholders (${form}): ${key} -> ${text}`);
      }
    }
    expect(problems).toEqual([]);
  });

  it("gives every count all of Arabic's plural forms", () => {
    const categories = new Set<string>();
    const rules = new Intl.PluralRules("ar");
    for (let n = 0; n <= 1000; n++) categories.add(rules.select(n));
    categories.add(rules.select(1.5));
    expect(sorted(categories)).toEqual(sorted(new Set(FORMS)));
    const plurals = Object.entries(ar).filter(([, entry]) => typeof entry === "object");
    expect(plurals.length).toBeGreaterThan(50);
    for (const [key, entry] of plurals)
      expect(Object.keys(entry).sort(), key).toEqual([...FORMS].sort());
  });

  it("writes Arabic punctuation, not English, between Arabic words", () => {
    const english = Object.entries(ar)
      .flatMap(([key, entry]) => (typeof entry === "string" ? [[key, entry]] : Object.values(entry).map((text) => [key, text])))
      .filter(([, text]) => /[؀-ۿ] ?[,;?]|[,;?] ?[؀-ۿ]/.test(text))
      .map(([key, text]) => `${key} -> ${text}`);
    expect(english).toEqual([]);
  });
});
