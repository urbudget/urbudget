#!/usr/bin/env node
// Checks the app's String Catalogs: valid JSON, and every string translated into Arabic with the
// same placeholders as the English, all six Arabic plural forms, and names and amounts isolated
// (U+2068 … U+2069) inside Arabic text. Run from anywhere: node ios/scripts/check-translations.mjs
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const folder = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'UrBudget');
const catalogs = fs.readdirSync(folder).filter(name => name.endsWith('.xcstrings'));
const arabicForms = ['zero', 'one', 'two', 'few', 'many', 'other'];
const problems = [];
let total = 0;
let plurals = 0;

// The arguments a format string takes, by position: "%@", "%lld", "%2$@", "%#@name@".
function argumentsOf(text, substitutions = {}) {
  const found = new Map();
  let next = 1;
  for (const match of text.matchAll(/%(?:(\d+)\$)?(#@(\w+)@|lld|llu|ld|lu|lf|d|u|f|@|%)/g)) {
    if (match[2] === '%') continue;
    const position = match[1] ? Number(match[1]) : next++;
    const substitution = match[3] && substitutions[match[3]];
    const specifier = substitution ? substitution.formatSpecifier : match[2];
    found.set(substitution ? substitution.argNum : position, specifier === '@' ? 'object' : /f$/.test(specifier) ? 'float' : 'integer');
  }
  return found;
}

// Each text of a localization: [label, text, substitutions], plural forms included.
function texts(localization, forms) {
  if (!localization) return [];
  if (localization.variations?.plural) {
    const plural = localization.variations.plural;
    for (const form of forms) if (!plural[form]) problems.push(`missing plural form "${form}"`);
    return Object.entries(plural).map(([form, unit]) => [form, unit.stringUnit, {}]);
  }
  const out = [['', localization.stringUnit, localization.substitutions ?? {}]];
  for (const [name, substitution] of Object.entries(localization.substitutions ?? {})) {
    const plural = substitution.variations?.plural ?? {};
    for (const form of forms) if (!plural[form]) problems.push(`substitution "${name}" is missing plural form "${form}"`);
  }
  return out;
}

for (const name of catalogs) {
  let catalog;
  try {
    catalog = JSON.parse(fs.readFileSync(path.join(folder, name), 'utf8'));
  } catch (error) {
    problems.push(`${name}: not valid JSON: ${error.message}`);
    continue;
  }
  if (catalog.sourceLanguage !== 'en') problems.push(`${name}: the source language must be English`);
  for (const [key, entry] of Object.entries(catalog.strings ?? {})) {
    total++;
    const where = `${name} ${JSON.stringify(key)}`;
    const before = problems.length;
    const arabic = entry.localizations?.ar;
    if (!arabic) { problems.push(`${where}: no Arabic`); continue; }
    if (arabic.variations?.plural || arabic.substitutions) plurals++;
    const english = entry.localizations?.en;
    const source = english?.stringUnit?.value ?? english?.variations?.plural?.other?.stringUnit?.value ?? key;
    const expected = argumentsOf(source, english?.substitutions);
    for (const [label, unit, substitutions] of texts(arabic, arabicForms)) {
      const at = label ? `${where} (${label})` : where;
      if (!unit || unit.state !== 'translated') { problems.push(`${at}: Arabic not marked translated`); continue; }
      const value = unit.value ?? '';
      if (!value.trim()) problems.push(`${at}: empty Arabic`);
      if (/[٠-٩]/.test(value)) problems.push(`${at}: Arabic-Indic digits; budgets write 0-9`);
      const given = argumentsOf(value, substitutions);
      for (const [position, kind] of given) {
        if (expected.get(position) !== kind) problems.push(`${at}: argument ${position} (${kind}) doesn't match the English`);
      }
      // Plural forms may leave the count out ("معاملة واحدة"); everything else keeps every argument.
      for (const [position, kind] of expected) {
        if (!given.has(position) && !(label && kind !== 'object')) problems.push(`${at}: argument ${position} is missing`);
      }
      for (const match of value.matchAll(/%(\d+\$)?@/g)) {
        if (value[match.index - 1] !== '⁨' || value[match.index + match[0].length] !== '⁩') {
          problems.push(`${at}: wrap ${match[0]} in U+2068 … U+2069`);
          break;
        }
      }
    }
    if (english) {
      for (const [label, unit] of texts(english, ['one', 'other'])) {
        if (!unit?.value) problems.push(`${where} (${label || 'English'}): empty English`);
      }
    }
    if (problems.length > before && problems.length - before > 20) problems.length = before + 20;
  }
}

for (const problem of problems) console.error(problem);
console.log(`${catalogs.length} catalogs, ${total} strings (${plurals} with plural forms): ${problems.length ? problems.length + ' problems' : 'every one translated into Arabic'}`);
process.exit(problems.length ? 1 : 0);
