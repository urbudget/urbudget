use super::*;
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

/// Placeholder names in a message: `{name}` and the Arabic plural form `{name|…}`.
fn placeholders(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut rest = text;
    while let Some(i) = rest.find('{') {
        let tail = &rest[i..];
        if tail.starts_with("{{") {
            rest = &tail[2..];
            continue;
        }
        let Some(end) = tail.find('}') else { break };
        let inner = &tail[1..end];
        names.insert(inner.split('|').next().unwrap_or_default().to_owned());
        rest = &tail[end + 1..];
    }
    names
}

/// The machine-readable code an app reads at the start of a message, such as `CONFLICT: `.
fn code(text: &str) -> Option<&str> {
    let end = text.find(": ")?;
    let head = &text[..end];
    (head.len() >= 3 && head.bytes().all(|b| b.is_ascii_uppercase() || b == b'_' || b == b' ')).then(|| &text[..end + 2])
}

fn is_arabic_letter(c: char) -> bool {
    ('\u{0600}'..='\u{06FF}').contains(&c)
}

#[test]
fn every_arabic_entry_keeps_the_english_placeholders_and_codes() {
    let mut seen = HashSet::new();
    for (english, arabic) in arabic::MESSAGES.iter().chain(arabic::SERVER) {
        assert!(seen.insert(*english), "duplicate entry: {english:?}");
        assert_eq!(placeholders(english), placeholders(arabic), "placeholders differ:\n{english}\n{arabic}");
        assert_eq!(code(english), code(arabic), "the leading code must stay as in English:\n{english}\n{arabic}");
        // Anything with English words in it needs Arabic ones; "“{text}”" only its punctuation.
        let words = english.split('{').map(|part| part.split_once('}').map_or(part, |(_, rest)| rest)).collect::<String>();
        if words.chars().any(|c| c.is_ascii_alphabetic()) {
            assert!(arabic.chars().any(is_arabic_letter), "not translated: {english:?}");
        }
        assert!(!arabic.contains(" ,") && !arabic.contains(" .") && !arabic.contains(" ،") && !arabic.contains(" ؟"), "space before punctuation: {arabic}");
        assert!(!arabic.contains('?'), "use the Arabic question mark: {arabic}");
        // Plural placeholders list all six of Arabic's forms, and English's two.
        for (text, forms) in [(*arabic, 6), (*english, 2)] {
            let mut rest = text;
            while let Some(i) = rest.find('{') {
                let end = rest[i..].find('}').map(|e| i + e).expect("unclosed placeholder");
                let inner = &rest[i + 1..end];
                if inner.contains('|') {
                    assert_eq!(inner.split('|').count(), forms + 1, "a plural needs {forms} forms: {text}");
                }
                rest = &rest[end + 1..];
            }
        }
        // A full stop at the end in one language and not in the other changes how the apps
        // show it next to other text.
        assert_eq!(english.ends_with('.'), arabic.ends_with('.'), "keep the English full stop habit:\n{english}\n{arabic}");
    }
}

#[test]
fn english_is_unchanged_and_values_fill_in() {
    let value = "Visa";
    let count = 3;
    assert_eq!(tr_in(Language::English, "Account not found."), "Account not found.");
    assert_eq!(
        format_in(Language::English, "Credit account '{name}' ({account}) needs one linked payment category. Its payment category is missing from this budget.", &[("name", &value), ("account", &"visa")]),
        "Credit account 'Visa' (visa) needs one linked payment category. Its payment category is missing from this budget."
    );
    assert_eq!(fill("{a} {{literal}} {b}", &[("a", &count), ("b", &"x")], false), "3 {literal} x");
}

#[test]
fn arabic_reorders_values_and_isolates_them() {
    let text = format_in(
        Language::Arabic,
        "Credit account '{name}' ({account}) needs one linked payment category. Its payment category is missing from this budget.",
        &[("name", &"Visa"), ("account", &"visa")],
    );
    assert!(text.starts_with("يحتاج حساب الائتمان «\u{2068}Visa\u{2069}» (\u{2068}visa\u{2069})"), "{text}");
    assert_eq!(tr_in(Language::Arabic, "Account not found."), "الحساب غير موجود.");
    // A message nobody translated yet still reads in English rather than failing.
    assert_eq!(tr_in(Language::Arabic, "Not in the table"), "Not in the table");
}

#[test]
fn arabic_plurals_use_all_six_forms() {
    let forms = "{n|لا تعديلات|تعديل واحد|تعديلان|# تعديلات|# تعديلًا|# تعديل}";
    let say = |n: i64| fill(forms, &[("n", &n)], true);
    assert_eq!(say(0), "لا تعديلات");
    assert_eq!(say(1), "تعديل واحد");
    assert_eq!(say(2), "تعديلان");
    assert_eq!(say(3), "3 تعديلات");
    assert_eq!(say(10), "10 تعديلات");
    assert_eq!(say(11), "11 تعديلًا");
    assert_eq!(say(99), "99 تعديلًا");
    assert_eq!(say(100), "100 تعديل");
    assert_eq!(say(102), "102 تعديل");
    assert_eq!(say(103), "103 تعديلات");
    assert_eq!(say(111), "111 تعديلًا");
    let english = |n: i64| fill("{n|# edit|# edits} to check", &[("n", &n)], false);
    assert_eq!((english(0), english(1), english(2)), ("0 edits to check".into(), "1 edit to check".into(), "2 edits to check".into()));
}

#[test]
fn language_codes() {
    assert_eq!(Language::Arabic.code(), "ar");
    assert_eq!(Language::English.code(), "en");
}

/// Every Rust source file in the engine's crates.
fn sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut out = vec![];
    for name in ["urbudget-core", "urbudget-services", "urbudget-android", "urbudget-ios"] {
        let src = crates.join(name).join("src");
        if src.is_dir() {
            walk(&src, &mut out);
        }
    }
    // The desktop app's own commands use the same table.
    let desktop = crates.parent().unwrap().join("desktop").join("src-tauri").join("src");
    if desktop.is_dir() {
        walk(&desktop, &mut out);
    }
    out
}

/// Reads the Rust string literal starting at `text` (just after its opening quote).
fn literal(text: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '0' => out.push('\0'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                'u' => {
                    let rest: String = chars.by_ref().take_while(|c| *c != '}').collect();
                    out.push(char::from_u32(u32::from_str_radix(rest.trim_start_matches('{'), 16).ok()?)?);
                }
                '\n' | '\r' => {
                    let rest = chars.as_str().trim_start();
                    chars = rest.chars();
                }
                other => {
                    out.push('\\');
                    out.push(other);
                }
            },
            c => out.push(c),
        }
    }
    None
}

/// The English texts the code passes to `t!` and `tr`.
fn messages_in_code() -> Vec<(String, PathBuf)> {
    let mut found = vec![];
    for path in sources() {
        if path.ends_with(Path::new("i18n").join("tests.rs")) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for marker in ["t!(", "tr("] {
            let mut from = 0;
            while let Some(i) = text[from..].find(marker).map(|i| from + i) {
                from = i + marker.len();
                let before = text[..i].chars().next_back();
                let line = &text[text[..i].rfind('\n').map_or(0, |n| n + 1)..i];
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '!') || line.trim_start().starts_with("//") {
                    continue;
                }
                let after = text[from..].trim_start();
                if let Some(body) = after.strip_prefix('"') {
                    if let Some(message) = literal(body) {
                        found.push((message, path.clone()));
                    }
                }
            }
        }
    }
    found
}

#[test]
fn server_messages_are_still_the_ones_urbudget_app_sends() {
    let web = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src");
    if !web.is_dir() {
        return;
    }
    let mut files = vec![];
    fn walk(dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "ts") {
                out.push(std::fs::read_to_string(&path).unwrap());
            }
        }
    }
    walk(&web, &mut files);
    for (english, _) in arabic::SERVER {
        // A number in the message may be written from a constant ("up to ${MAX_BUDGETS} budgets").
        let (head, tail) = match (english.find(|c: char| c.is_ascii_digit()), english.rfind(|c: char| c.is_ascii_digit())) {
            (Some(first), Some(last)) => (&english[..first], &english[last + 1..]),
            _ => (*english, ""),
        };
        let found = files.iter().any(|f| f.contains(english) || f.lines().any(|l| l.contains(head) && l.contains(tail)));
        assert!(found, "urbudget.app no longer sends {english:?}; update i18n/arabic.rs SERVER");
    }
}

#[test]
fn every_message_in_the_code_has_its_arabic_and_every_entry_is_used() {
    let found = messages_in_code();
    assert!(found.len() > 100, "the scan found only {} messages", found.len());
    let missing: Vec<String> = found.iter().filter(|(m, _)| arabic(m).is_none())
        .map(|(m, path)| format!("{}: {m:?}", path.file_name().unwrap().to_string_lossy())).collect();
    assert!(missing.is_empty(), "add these to i18n/arabic.rs:\n{}", missing.join("\n"));
    let used: HashSet<&str> = found.iter().map(|(m, _)| m.as_str()).collect();
    let unused: Vec<&&str> = arabic::MESSAGES.iter().map(|(e, _)| e).filter(|e| !used.contains(**e)).collect();
    assert!(unused.is_empty(), "no code uses these entries; remove them from i18n/arabic.rs:\n{unused:#?}");
}
