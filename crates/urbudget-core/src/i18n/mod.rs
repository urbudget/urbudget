//! The language of the messages people see: errors, sync and import status, notices.
//!
//! English is the source. Every message is written in English in the code, and `arabic.rs`
//! maps that exact English text to its Arabic. The language is process-wide: each app calls
//! `set_language` with the language it resolved ("ar" or "en") at startup and whenever it
//! changes. Data is never translated: names the engine stores or syncs, file formats, CSV
//! columns, API fields and anything another device reads stay as they are (see
//! docs/LOCALIZATION.md).
//!
//! Writing a message:
//! - `t!("Choose an account.")` returns the text in the current language as a `String`.
//! - `t!("Couldn't read {name}: {error}", name = file.name, error = e)` fills named
//!   placeholders; `t!("… {count} …", count)` uses a variable of the same name. Arabic may move
//!   placeholders wherever its sentence needs them. Values are shown with `Display`, so format
//!   anything else (`{:?}`, `{:.2}`) into a string first.
//! - `tr("…")` gives a `&'static str` for constants and labels without placeholders.
//! - Add the Arabic for each new English text to `arabic.rs`; a test fails until you do.
//!
//! A placeholder can also choose a plural form, `#` standing for the number: English gives
//! one and other (`{count|# edit|# edits}`), Arabic all six of its forms, zero, one, two, few,
//! many and other (`{count|لا تعديلات|تعديل واحد|تعديلان|# تعديلات|# تعديلًا|# تعديل}`).
//!
//! A machine-readable code that begins a message (`CONFLICT: `) stays in English in both
//! languages, because the apps read it.
mod arabic;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    English,
    Arabic,
}
impl Language {
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Arabic => "ar",
        }
    }
}

static LANGUAGE: AtomicU8 = AtomicU8::new(0);

/// "ar" selects Arabic; anything else selects English.
pub fn set_language(code: &str) {
    let arabic = code == "ar";
    LANGUAGE.store(u8::from(arabic), Ordering::Relaxed);
}

pub fn language() -> Language {
    if LANGUAGE.load(Ordering::Relaxed) == 1 { Language::Arabic } else { Language::English }
}

fn table() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| arabic::MESSAGES.iter().chain(arabic::SERVER).copied().collect())
}

/// Text that arrived in English from elsewhere, such as urbudget.app's error messages, in the
/// current language when the table has it (`arabic::SERVER`), and as it came otherwise.
pub fn translate(english: &str) -> String {
    match language() {
        Language::English => english.to_owned(),
        Language::Arabic => arabic(english).unwrap_or(english).to_owned(),
    }
}

/// The Arabic for an English message, if there is one.
pub fn arabic(english: &str) -> Option<&'static str> {
    table().get(english).copied()
}

/// A message without placeholders, in the current language.
pub fn tr(english: &'static str) -> &'static str {
    tr_in(language(), english)
}

/// A message without placeholders, in the given language.
pub fn tr_in(language: Language, english: &'static str) -> &'static str {
    match language {
        Language::English => english,
        Language::Arabic => arabic(english).unwrap_or(english),
    }
}

/// A message with named `{placeholders}`, in the current language. Use `t!` rather than
/// calling this directly.
pub fn format(english: &'static str, args: &[(&str, &dyn Display)]) -> String {
    format_in(language(), english, args)
}

/// A message with named `{placeholders}`, in the given language.
pub fn format_in(language: Language, english: &'static str, args: &[(&str, &dyn Display)]) -> String {
    match language {
        Language::English => fill(english, args, false),
        Language::Arabic => match arabic(english) {
            Some(text) => fill(text, args, true),
            None => fill(english, args, false),
        },
    }
}

/// What separates the items of a list within a sentence: "; " or Arabic's "؛ ".
pub fn list_separator() -> &'static str {
    match language() {
        Language::English => "; ",
        Language::Arabic => "؛ ",
    }
}

/// A comma between items: ", " or Arabic's "، ".
pub fn comma() -> &'static str {
    match language() {
        Language::English => ", ",
        Language::Arabic => "، ",
    }
}

const ARABIC_MONTHS: [&str; 12] = [
    "يناير", "فبراير", "مارس", "أبريل", "مايو", "يونيو", "يوليو", "أغسطس", "سبتمبر", "أكتوبر", "نوفمبر", "ديسمبر",
];

/// A month with its year in words: "October 2026", or "أكتوبر 2026" in Arabic.
pub fn month_year(date: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    match language() {
        Language::English => date.format("%B %Y").to_string(),
        Language::Arabic => format!("{} {}", ARABIC_MONTHS[date.month0() as usize], date.year()),
    }
}

/// A month with its year, short: "Oct 2026", or "أكتوبر 2026" in Arabic, which has no short names.
pub fn short_month_year(date: chrono::NaiveDate) -> String {
    match language() {
        Language::English => date.format("%b %Y").to_string(),
        Language::Arabic => month_year(date),
    }
}

/// A day with its month, short: "Oct 5", or "5 أكتوبر" in Arabic.
pub fn day_month(date: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    match language() {
        Language::English => date.format("%b %-d").to_string(),
        Language::Arabic => format!("{} {}", date.day(), ARABIC_MONTHS[date.month0() as usize]),
    }
}

/// A full date, short: "Oct 5, 2026", or "5 أكتوبر 2026" in Arabic.
pub fn day_month_year(date: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    match language() {
        Language::English => date.format("%b %-d, %Y").to_string(),
        Language::Arabic => format!("{} {}", day_month(date), date.year()),
    }
}

const ISOLATE: char = '\u{2068}';
const POP_ISOLATE: char = '\u{2069}';

/// Replaces `{name}` with its value; `{{` and `}}` are literal braces. In Arabic each value is
/// wrapped in a bidi isolate, so an amount, a date or an English name keeps its own direction
/// inside the right-to-left sentence.
fn fill(text: &str, args: &[(&str, &dyn Display)], isolate: bool) -> String {
    let mut out = String::with_capacity(text.len() + 32);
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        let end = match (tail.starts_with('{'), tail.find('}')) {
            (true, Some(end)) => end,
            _ => {
                out.push_str(&tail[..1]);
                rest = &tail[1..];
                continue;
            }
        };
        let inner = &tail[1..end];
        let (name, forms) = match inner.split_once('|') {
            Some((name, forms)) => (name, Some(forms)),
            None => (inner, None),
        };
        match args.iter().find(|(n, _)| *n == name) {
            Some((_, value)) => {
                let value = value.to_string();
                match forms {
                    Some(forms) => out.push_str(&plural(&value, forms)),
                    None if isolate => {
                        out.push(ISOLATE);
                        out.push_str(&value);
                        out.push(POP_ISOLATE);
                    }
                    None => out.push_str(&value),
                }
            }
            None => {
                debug_assert!(false, "no value for {{{name}}} in {text:?}");
                out.push_str(&tail[..=end]);
            }
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Arabic's plural categories (CLDR): 0, 1, 2, 3–10, 11–99 (by the last two digits), other.
fn arabic_plural(n: u64) -> usize {
    match (n, n % 100) {
        (0, _) => 0,
        (1, _) => 1,
        (2, _) => 2,
        (_, 3..=10) => 3,
        (_, 11..=99) => 4,
        _ => 5,
    }
}

/// `{n|one|other}` in English, `{n|zero|one|two|few|many|other}` in Arabic; `#` is the number.
fn plural(value: &str, forms: &str) -> String {
    let forms: Vec<&str> = forms.split('|').collect();
    let n = value.trim_start_matches('-').parse::<u64>().ok();
    let index = match (forms.len(), n) {
        (2, Some(1)) => 0,
        (2, _) => 1,
        (_, Some(n)) => arabic_plural(n),
        (_, None) => 5,
    };
    let form = forms.get(index).or(forms.last()).copied().unwrap_or("#");
    form.replace('#', value)
}

/// The message in the current language: `t!("English text")`, or with named placeholders
/// `t!("Couldn't read {name}.", name = file.name)` (`t!("…{name}…", name)` for a variable of
/// the same name). Returns a `String`.
#[macro_export]
macro_rules! t {
    ($text:literal $(,)?) => {
        $crate::i18n::tr($text).to_owned()
    };
    ($text:literal, $($name:ident $(= $value:expr)?),+ $(,)?) => {
        $crate::i18n::format(
            $text,
            &[$((stringify!($name), &$crate::__t_value!($name $(, $value)?) as &dyn ::std::fmt::Display)),+],
        )
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __t_value {
    ($name:ident) => {
        $name
    };
    ($name:ident, $value:expr) => {
        $value
    };
}
