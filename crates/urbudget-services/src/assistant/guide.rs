//! How UrBudget itself works, for the assistant's directions. `guide.md` has one entry per task, with
//! the steps on the desktop, Android and iPhone apps in each app's own words. `get_app_guide` looks
//! entries up for the app the user is in (or another they ask about), and the prompt carries that
//! app's short map. Tests check that every entry covers every app and that each **label** it quotes
//! is really in that app's text, so a renamed button fails a test rather than misleading someone.

use serde_json::{json, Value};
use std::sync::OnceLock;

/// The app the user is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Desktop,
    Android,
    Ios,
}

impl Platform {
    pub const ALL: [Platform; 3] = [Platform::Desktop, Platform::Android, Platform::Ios];

    /// The platform a client names ("android", "ios"). The desktop app names none.
    pub fn of(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "android" => Platform::Android,
            "ios" | "iphone" | "ipad" => Platform::Ios,
            _ => Platform::Desktop,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Platform::Desktop => "desktop",
            Platform::Android => "android",
            Platform::Ios => "ios",
        }
    }

    /// How the prompt names the app.
    pub fn app(self) -> &'static str {
        match self {
            Platform::Desktop => "the UrBudget desktop app for Windows and macOS",
            Platform::Android => "the UrBudget Android app",
            Platform::Ios => "the UrBudget iPhone and iPad app",
        }
    }

    fn index(self) -> usize {
        match self {
            Platform::Desktop => 0,
            Platform::Android => 1,
            Platform::Ios => 2,
        }
    }
}

/// One task in the guide.
#[derive(Debug)]
pub struct Topic {
    pub id: String,
    pub title: String,
    pub keywords: Vec<String>,
    /// What the feature is, the same on every app.
    pub what: String,
    steps: [String; 3],
}

impl Topic {
    /// The steps on [platform], or why it isn't there.
    pub fn steps(&self, platform: Platform) -> &str {
        &self.steps[platform.index()]
    }
}

const GUIDE: &str = include_str!("guide.md");

/// Every entry in the guide, read once.
pub fn topics() -> &'static [Topic] {
    static TOPICS: OnceLock<Vec<Topic>> = OnceLock::new();
    TOPICS.get_or_init(|| parse(GUIDE))
}

/// Reads `guide.md`: each entry starts `## id: Title`, then `keywords:`, `what:`, `desktop:`,
/// `android:` and `ios:` lines; a line that doesn't start a field continues the one before, on its own line.
fn parse(text: &str) -> Vec<Topic> {
    let mut topics = vec![];
    for block in text.split("\n## ").skip(1) {
        let mut lines = block.lines();
        let Some((id, title)) = lines.next().and_then(|h| h.split_once(':')) else { continue };
        let mut fields: Vec<(String, String)> = vec![];
        for line in lines {
            let line = line.trim();
            if line.is_empty() || line.starts_with("<!--") {
                continue;
            }
            match line.split_once(':') {
                Some((key, value)) if ["keywords", "what", "desktop", "android", "ios"].contains(&key.trim()) => {
                    fields.push((key.trim().to_owned(), value.trim().to_owned()))
                }
                _ => {
                    // Lists and numbered steps keep their lines.
                    if let Some((_, value)) = fields.last_mut() {
                        value.push('\n');
                        value.push_str(line);
                    }
                }
            }
        }
        let field = |key: &str| fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap_or_default();
        topics.push(Topic {
            id: id.trim().to_owned(),
            title: title.trim().to_owned(),
            keywords: field("keywords").split(',').map(|k| k.trim().to_lowercase()).filter(|k| !k.is_empty()).collect(),
            what: field("what"),
            steps: [field("desktop"), field("android"), field("ios")],
        });
    }
    topics
}

/// The app's layout in a few sentences, for the prompt.
pub fn map(platform: Platform) -> &'static str {
    topics().iter().find(|t| t.id == "map").map(|t| t.steps(platform)).unwrap_or_default()
}

/// Words too common to tell entries apart.
const COMMON: [&str; 24] = [
    "the", "and", "how", "can", "you", "your", "for", "with", "what", "where", "this", "that", "from", "into", "does", "doesn't",
    "want", "need", "app", "urbudget", "use", "using", "get", "make",
];

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| w.chars().count() >= 3 && !COMMON.contains(w))
        .map(str::to_owned)
        .collect()
}

/// How well [topic] answers [query]: its keywords count most, then its title, then what it says.
fn score(topic: &Topic, word: &str) -> usize {
    let title = words(&topic.title);
    let what = words(&topic.what);
    let id = topic.id.replace('-', " ");
    let keyword = topic.keywords.iter().any(|k| k == word || k.split(' ').any(|p| p == word || (word.len() >= 5 && p.starts_with(word))));
    let stem = |list: &[String]| list.iter().any(|t| t == word || (word.len() >= 5 && (t.starts_with(word) || word.starts_with(t.as_str()))));
    (if keyword { 6 } else { 0 }) + (if stem(&title) || id.contains(word) { 3 } else { 0 }) + (if stem(&what) { 1 } else { 0 })
}

/// The entries that best fit [query]. A word few entries share ("reconcile") counts for more than
/// one many do ("account").
fn rank<'a>(topics: &'a [Topic], query: &[String]) -> Vec<(f64, &'a Topic)> {
    let candidates: Vec<&Topic> = topics.iter().filter(|t| t.id != "map").collect();
    let mut totals = vec![0.0; candidates.len()];
    for word in query {
        let scores: Vec<usize> = candidates.iter().map(|t| score(t, word)).collect();
        let shared = scores.iter().filter(|s| **s > 0).count();
        if shared == 0 {
            continue;
        }
        let weight = (1.0 + candidates.len() as f64 / shared as f64).ln();
        for (total, s) in totals.iter_mut().zip(&scores) {
            *total += *s as f64 * weight;
        }
    }
    let mut ranked: Vec<(f64, &Topic)> = totals.into_iter().zip(candidates).filter(|(s, _)| *s > 0.0).collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    ranked
}

/// What `get_app_guide` answers: the entries that fit [args] `question` (or the `topic` id), with the
/// steps for the app the user is in, another app (`platform`), or every app (`platform: "all"`).
pub fn lookup(args: &Value, current: Platform) -> Value {
    let text = |key: &str| args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let all = text("platform").is_some_and(|p| p.eq_ignore_ascii_case("all"));
    let platform = text("platform").filter(|_| !all).map(Platform::of).unwrap_or(current);
    let topics = topics();
    let found: Vec<&Topic> = match text("topic").and_then(|id| topics.iter().find(|t| t.id == id)) {
        Some(topic) => vec![topic],
        None => {
            let query = words(text("question").or(text("topic")).unwrap_or_default());
            let ranked = rank(topics, &query);
            let best = ranked.first().map(|(s, _)| *s).unwrap_or(0.0);
            // The best entries only: one clear answer, or a few close ones.
            ranked.into_iter().filter(|(s, _)| *s * 2.0 >= best).take(3).map(|(_, t)| t).collect()
        }
    };
    let entry = |t: &Topic| {
        let steps = if all {
            json!({ "desktop": t.steps(Platform::Desktop), "android": t.steps(Platform::Android), "ios": t.steps(Platform::Ios) })
        } else {
            json!(t.steps(platform))
        };
        json!({ "topic": t.id, "title": t.title, "what": t.what, "steps": steps })
    };
    if found.is_empty() {
        return json!({
            "app": current.key(),
            "found": [],
            "note": "No guide entry matched. Ask again with one of these topic ids, or say you aren't sure where it is rather than guessing.",
            "topics": topics.iter().filter(|t| t.id != "map").map(|t| json!({ "topic": t.id, "title": t.title })).collect::<Vec<_>>(),
        });
    }
    json!({
        "app": current.key(),
        "stepsFor": if all { "all".to_owned() } else { platform.key().to_owned() },
        "found": found.into_iter().map(entry).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    /// Topics every app's users ask about, so none is dropped from the guide by accident.
    const EXPECTED: [&str; 51] = [
        "map", "assign-money", "move-money", "quick-assign", "targets", "categories", "groups", "reorder-categories", "month-navigation",
        "notes", "budget-views", "expected-income", "add-transaction", "split-transaction", "transfer", "edit-delete-transaction",
        "approve-clear", "search-filter", "bulk-edit", "scheduled", "export-csv", "add-account", "credit-cards", "reconcile",
        "close-account", "loans", "credit-limit", "net-worth", "import-bank-file", "bank-connections", "ynab-import", "bank-texts",
        "payees-rules", "reports", "report-export", "account-sign-in", "sync-setup", "sync-subscription", "sync-conflicts", "budgets",
        "trash", "backups", "json-export-import", "fresh-start", "language", "theme", "formats", "flag-names", "assistant-setup",
        "assistant-history", "undo",
    ];

    #[test]
    fn every_task_is_covered_on_every_app() {
        let topics = topics();
        let ids: Vec<&str> = topics.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids.len(), ids.iter().collect::<HashSet<_>>().len(), "duplicate topic ids");
        for id in EXPECTED {
            assert!(ids.contains(&id), "the guide has no {id} entry");
        }
        for t in topics {
            assert!(!t.title.is_empty() && !t.what.is_empty() && !t.keywords.is_empty() || t.id == "map", "{} needs a title, keywords and what", t.id);
            for p in Platform::ALL {
                assert!(t.steps(p).len() > 10, "{} has no {} steps (or a 'Not available on …' line)", t.id, p.key());
            }
        }
        for p in Platform::ALL {
            assert!(!map(p).is_empty());
        }
    }

    #[test]
    fn questions_find_their_entry_for_the_users_app() {
        let first = |q: &str, p: Platform| lookup(&json!({ "question": q }), p)["found"][0]["topic"].as_str().unwrap_or("").to_owned();
        assert_eq!(first("How do I reconcile my checking account?", Platform::Android), "reconcile");
        assert_eq!(first("where are bank text messages set up", Platform::Android), "bank-texts");
        assert_eq!(first("how to undo", Platform::Ios), "undo");
        assert_eq!(first("move money to cover overspending", Platform::Desktop), "move-money");
        for (question, topic) in [
            ("How do I sign in with ChatGPT?", "assistant-setup"),
            ("delete a budget permanently", "trash"),
            ("switch the app to Arabic", "language"),
            ("turn on dark mode", "theme"),
            ("split a receipt between groceries and household", "split-transaction"),
            ("export my transactions to excel", "export-csv"),
            ("pay off my credit card", "credit-cards"),
            ("connect my bank so transactions download", "bank-connections"),
            ("set a savings goal for a category", "targets"),
            ("restore yesterday's backup", "backups"),
            ("my mortgage interest rate changed", "loans"),
            ("make this payment repeat every month", "scheduled"),
        ] {
            assert_eq!(first(question, Platform::Desktop), topic, "{question}");
        }
        let ios = lookup(&json!({ "topic": "bank-texts" }), Platform::Ios);
        assert!(ios["found"][0]["steps"].as_str().unwrap().contains("Not available on iPhone"));
        let all = lookup(&json!({ "topic": "reconcile", "platform": "all" }), Platform::Ios);
        assert!(all["found"][0]["steps"]["desktop"].is_string() && all["found"][0]["steps"]["android"].is_string());
        let other = lookup(&json!({ "topic": "reconcile", "platform": "desktop" }), Platform::Android);
        assert_eq!(other["stepsFor"], "desktop");
        let none = lookup(&json!({ "question": "zzzz qqqq" }), Platform::Desktop);
        assert!(none["found"].as_array().unwrap().is_empty() && none["topics"].as_array().unwrap().len() > 40);
    }

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn files(dir: &Path, ext: &[&str], out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())).flatten() {
            let path = entry.path();
            if path.is_dir() {
                files(&path, ext, out);
            } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| ext.contains(&e)) {
                out.push(path);
            }
        }
    }

    /// The quoted strings in source code: each "…" and '…' on a line, and JSX text between tags.
    fn literals(code: &str, labels: &mut HashSet<String>) {
        for line in code.lines() {
            for quote in ['"', '\'', '`'] {
                let parts: Vec<&str> = line.split(quote).collect();
                for (i, part) in parts.iter().enumerate() {
                    if i % 2 == 1 {
                        labels.insert(part.replace("\\'", "'").replace("\\\"", "\""));
                    }
                }
            }
            for piece in line.split('>').skip(1) {
                let text = piece.split('<').next().unwrap_or("").trim();
                if !text.is_empty() {
                    labels.insert(text.to_owned());
                }
            }
        }
    }

    fn unescape_xml(s: &str) -> String {
        s.replace("\\'", "'").replace("\\\"", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&")
    }

    /// Everything an app shows in English, as whole strings.
    fn labels(p: Platform) -> HashSet<String> {
        let root = repo();
        let mut labels = HashSet::new();
        let mut code = vec![];
        match p {
            Platform::Android => {
                let strings = std::fs::read_to_string(root.join("android/app/src/main/res/values/strings.xml")).unwrap();
                for piece in strings.split('<').filter(|s| s.starts_with("string ") || s.starts_with("item ")) {
                    if let Some((_, value)) = piece.split_once('>') {
                        labels.insert(unescape_xml(value.trim()));
                    }
                }
                files(&root.join("android/app/src/main/java"), &["kt"], &mut code);
            }
            Platform::Ios => {
                let catalog: Value = serde_json::from_str(&std::fs::read_to_string(root.join("ios/UrBudget/Localizable.xcstrings")).unwrap()).unwrap();
                labels.extend(catalog["strings"].as_object().unwrap().keys().cloned());
                files(&root.join("ios/UrBudget"), &["swift"], &mut code);
            }
            Platform::Desktop => files(&root.join("desktop/src"), &["ts", "tsx"], &mut code),
        }
        for path in code {
            literals(&std::fs::read_to_string(path).unwrap(), &mut labels);
        }
        labels
    }

    /// The **labels** in a line of steps.
    fn bold(text: &str) -> Vec<String> {
        text.split("**").enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, s)| s.to_owned()).collect()
    }

    #[test]
    fn every_quoted_label_is_in_that_apps_own_text() {
        // The check tells the apps apart: one app's words aren't taken as another's.
        assert!(labels(Platform::Android).contains("Bank messages") && !labels(Platform::Desktop).contains("Bank messages"));
        assert!(labels(Platform::Desktop).contains("Manage budgets…") && !labels(Platform::Ios).contains("Manage budgets…"));
        let mut missing = vec![];
        for p in Platform::ALL {
            let known = labels(p);
            for t in topics() {
                for label in bold(t.steps(p)) {
                    if !known.contains(&label) {
                        missing.push(format!("{} / {}: **{label}**", p.key(), t.id));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "labels the apps don't show (renamed, or never there):\n{}", missing.join("\n"));
    }
}
