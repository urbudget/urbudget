//! Does the assistant's review catch a template's wrong reading? (docs/plans/sms-templates.md, §9)
//!
//! Synthetic bank texts in formats like the bank's (§1 of that plan), each with its answer known.
//! Every format has a right template and a wrong one with a planted mistake: money in for money
//! out, the day and month swapped, the balance read as the amount, a transfer's accounts swapped,
//! a transfer read as a purchase, a purchase skipped as an offer, and miles read as money. Two
//! rounds send each format once through its right template and once through its wrong one, through
//! the real service and model, in a folder of its own: its own budget and key, and nothing of the
//! apps' read or written. It reports, per mistake, how many rows the review put right, and how
//! many right rows it changed.
//!
//!   cargo run -p urbudget-services --example sms_review_eval -- dry
//!   URBUDGET_EVAL_PROVIDER=anthropic URBUDGET_EVAL_KEY=… cargo run -p urbudget-services --example sms_review_eval
//!
//! `dry` checks the eval itself, without a model: each right template reads its texts as their
//! answers say, and each wrong one gets its mistake. URBUDGET_EVAL_PROVIDER is anthropic or
//! openai (a ChatGPT sign-in isn't used here); URBUDGET_EVAL_MODEL picks a model, the provider's
//! default otherwise; URBUDGET_EVAL_TEXTS sets the texts per format and round (default 6).
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, Instant};
use urbudget_core::sms_import::{Item, Message};
use urbudget_core::sms_templates::{self, Outcome, Template};
use urbudget_services::runtime::Runtime;

const SENDER: &str = "ExampleBank";

/// What a text says, as the review should end up with it.
struct Answer {
    skip: bool,
    kind: &'static str,
    amount: String,
    date: String,
    account: Option<&'static str>,
    other: Option<&'static str>,
}

struct Text {
    body: String,
    answer: Answer,
    day: String,
    time: String,
}

struct Format {
    name: &'static str,
    /// The mistake the wrong template makes, as the report names it.
    mistake: &'static str,
    right: (&'static [&'static str], Outcome),
    wrong: (&'static [&'static str], Outcome),
    payee: Option<&'static str>,
    note: &'static str,
    make: fn(usize, usize) -> Text,
}

fn money(kind: &str, sign: &str) -> Outcome {
    Outcome { disposition: Some("transaction".into()), kind: Some(kind.into()), sign: Some(sign.into()), ..Outcome::default() }
}
fn skip(reason: &str) -> Outcome {
    Outcome { skip: Some(reason.into()), ..Outcome::default() }
}

// Values vary by text and round; days stay under 13, and off the 10th, so a swapped day and month
// is still a date and a different one.
const DAYS: [u32; 11] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12];
const PAYEES: [&str; 6] = ["Coffee House", "Book Corner", "Fresh Market", "City Pharmacy", "Fuel Station", "Hardware Hub"];
const PAYEES_AR: [&str; 6] = ["مخبز الحي", "صيدلية المدينة", "سوق الخضار", "مكتبة النور", "محطة الوقود", "مطعم البيت"];
fn day(i: usize, round: usize) -> u32 {
    DAYS[(i * 4 + round * 5) % DAYS.len()]
}
fn amount(i: usize, round: usize, salt: usize) -> String {
    format!("{}.{:02}", 12 + (i * 53 + round * 17 + salt * 31) % 900, (i * 29 + round * 7 + salt * 13) % 100)
}
fn balance(i: usize, round: usize) -> String {
    format!("{}.{:02}", 2_000 + (i * 311 + round * 97) % 7_000, (i * 3 + round) % 100)
}
fn time(i: usize) -> String {
    format!("{:02}:{:02}", 8 + i % 12, (i * 7) % 60)
}
fn iso(d: u32) -> String {
    format!("2026-10-{d:02}")
}
fn spent(kind: &'static str, amount: &str, d: u32, account: &'static str, other: Option<&'static str>) -> Answer {
    Answer { skip: false, kind, amount: format!("-{amount}"), date: iso(d), account: Some(account), other }
}

fn formats() -> Vec<Format> {
    const ONLINE: &[&str] = &["Online Purchase", "VISA Card : {ref}", "Amount : {currency} {amount}", "Balance : SAR {balance}", "At : {payee}", "On : {date:dd/mm/yy} {time}"];
    const POS_AR: &[&str] = &["شراء-POS", "بـ : {currency} {amount}", "من : {payee}", "مدى أثير : {ref}", "في : {date:dd-mm-yy} {time}"];
    const POS_AR_SWAPPED: &[&str] = &["شراء-POS", "بـ : {currency} {amount}", "من : {payee}", "مدى أثير : {ref}", "في : {date:mm-dd-yy} {time}"];
    const POS: &[&str] = &["PoS Purchase", "Card : {ref}", "Amount : {currency} {amount}", "Balance : SAR {balance}", "At : {payee}", "On : {date:dd/mm/yy} {time}"];
    const POS_BALANCE: &[&str] = &["PoS Purchase", "Card : {ref}", "Amount : SAR {balance}", "Balance : {currency} {amount}", "At : {payee}", "On : {date:dd/mm/yy} {time}"];
    const MURABAHA: &[&str] = &["عزيزي العميل،", "تم خصم مبلغ {currency} {amount}", "من حسابك رقم {ref}", "الى حسابك الخاص بالمرابحة رقم {other}", "في تاريخ {date:dd-mm-yy} {time}"];
    const MURABAHA_SWAPPED: &[&str] = &["عزيزي العميل،", "تم خصم مبلغ {currency} {amount}", "من حسابك رقم {other}", "الى حسابك الخاص بالمرابحة رقم {ref}", "في تاريخ {date:dd-mm-yy} {time}"];
    const INTERNAL: &[&str] = &["Internal Transfer", "From : {ref}", "To : {other}", "Amount : {currency} {amount}", "On : {date:dd/mm/yy} {time}"];
    const APPROVED: &[&str] = &["Purchase approved", "Card ending {ref}", "{currency} {amount}", "Merchant : {payee}", "Date : {date:dd/mm/yyyy}"];
    const MILES: &[&str] = &["You earned {n} miles", "From your card spend on {date:dd/mm/yy}", "Miles balance : {n}"];
    const MILES_MONEY: &[&str] = &["You earned {amount} miles", "From your card spend on {date:dd/mm/yy}", "Miles balance : {n}"];
    vec![
        Format { name: "online purchase", mistake: "money in for money out", right: (ONLINE, money("expense", "-")), wrong: (ONLINE, money("income", "+")),
            payee: None, note: "Purchase at {payee}", make: |i, r| {
                let (d, a) = (day(i, r), amount(i, r, 1));
                Text { body: format!("Online Purchase\nVISA Card:5521\nAmount:SAR {a}\nBalance:SAR {}\nAt:{}\nOn:{d:02}/10/26 {}", balance(i, r), PAYEES[i % 6], time(i)),
                    answer: spent("expense", &a, d, "5521", None), day: iso(d), time: time(i) }
            } },
        Format { name: "Arabic card purchase", mistake: "day and month swapped", right: (POS_AR, money("expense", "-")), wrong: (POS_AR_SWAPPED, money("expense", "-")),
            payee: None, note: "Purchase at {payee}", make: |i, r| {
                let (d, a) = (day(i, r), amount(i, r, 2));
                Text { body: format!("شراء-POS\nبـ : SAR {a}\nمن : {}\nمدى أثير : 8830\nفي : {d:02}-10-26 {}", PAYEES_AR[i % 6], time(i)),
                    answer: spent("expense", &a, d, "8830", None), day: iso(d), time: time(i) }
            } },
        Format { name: "card purchase", mistake: "the balance read as the amount", right: (POS, money("expense", "-")), wrong: (POS_BALANCE, money("expense", "-")),
            payee: None, note: "Purchase at {payee}", make: |i, r| {
                let (d, a) = (day(i, r), amount(i, r, 3));
                Text { body: format!("PoS Purchase\nCard:3107\nAmount:SAR {a}\nBalance:SAR {}\nAt:{}\nOn:{d:02}/10/26 {}", balance(i, r), PAYEES[(i + 2) % 6], time(i)),
                    answer: spent("expense", &a, d, "3107", None), day: iso(d), time: time(i) }
            } },
        Format { name: "Arabic transfer to a deposit", mistake: "the accounts swapped", right: (MURABAHA, money("transfer", "-")), wrong: (MURABAHA_SWAPPED, money("transfer", "-")),
            payee: Some("Murabaha deposit"), note: "Transfer to a deposit", make: |i, r| {
                let (d, a) = (day(i, r), amount(i, r, 4));
                Text { body: format!("عزيزي العميل،\nتم خصم مبلغ SAR {a}\nمن حسابك رقم 1204\nالى حسابك الخاص بالمرابحة رقم 9921\nفي تاريخ {d:02}-10-26 {}", time(i)),
                    answer: spent("transfer", &a, d, "1204", Some("9921")), day: iso(d), time: time(i) }
            } },
        Format { name: "transfer between accounts", mistake: "a transfer read as a purchase", right: (INTERNAL, money("transfer", "-")), wrong: (INTERNAL, money("expense", "-")),
            payee: Some("Transfer to savings"), note: "Transfer to savings", make: |i, r| {
                let (d, a) = (day(i, r), amount(i, r, 5));
                Text { body: format!("Internal Transfer\nFrom:6650\nTo:2087\nAmount:SAR {a}\nOn:{d:02}/10/26 {}", time(i)),
                    answer: spent("transfer", &a, d, "6650", Some("2087")), day: iso(d), time: time(i) }
            } },
        Format { name: "approved purchase", mistake: "a purchase skipped as an offer", right: (APPROVED, money("expense", "-")), wrong: (APPROVED, skip("marketing")),
            payee: None, note: "Purchase at {payee}", make: |i, r| {
                let (d, a) = (day(i, r), amount(i, r, 6));
                Text { body: format!("Purchase approved\nCard ending 7419\nSAR {a}\nMerchant: {}\nDate: {d:02}/10/2026", PAYEES[(i + 4) % 6]),
                    answer: spent("expense", &a, d, "7419", None), day: iso(d), time: time(i) }
            } },
        Format { name: "miles", mistake: "miles read as money", right: (MILES, skip("rewards")), wrong: (MILES_MONEY, money("income", "+")),
            payee: Some("Miles"), note: "Miles", make: |i, r| {
                let d = day(i, r);
                Text { body: format!("You earned {} miles\nFrom your card spend on {d:02}/10/26\nMiles balance: {}", 20 + (i * 13 + r * 7) % 300, 5_000 + (i * 97 + r * 41) % 9_000),
                    answer: Answer { skip: true, kind: "", amount: String::new(), date: iso(d), account: None, other: None }, day: iso(d), time: time(i) }
            } },
    ]
}

/// One round's texts, each with its answer and its format, and the templates the round uses:
/// in round 0 the even formats' wrong ones, in round 1 the odd formats'.
fn round(formats: &[Format], r: usize, per: usize) -> (Vec<Message>, Vec<(usize, Answer)>, Vec<Template>) {
    let sender = sms_templates::sender_key(SENDER);
    let (mut messages, mut answers, mut templates) = (vec![], vec![], vec![]);
    for (f, format) in formats.iter().enumerate() {
        let (lines, outcome) = if planted(f, r) { &format.wrong } else { &format.right };
        let lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        templates.push(Template { id: sms_templates::template_id(&sender, &lines), sender: sender.clone(), lines, outcome: outcome.clone(),
            note: format.note.into(), payee: format.payee.map(Into::into) });
        for i in 0..per {
            let text = (format.make)(i, r);
            messages.push(Message { id: format!("sms:{r}-{f}-{i}"), sender: SENDER.into(), received_date: text.day, received_time: text.time, body: text.body });
            answers.push((f, text.answer));
        }
    }
    (messages, answers, templates)
}
fn planted(f: usize, r: usize) -> bool {
    (f + r) % 2 == 0
}

/// Where an item differs from a text's answer, by field.
fn differences(item: Option<&Item>, answer: &Answer) -> Vec<&'static str> {
    let Some(item) = item else { return vec!["missing"] };
    if answer.skip {
        return if item.disposition == "ignore" { vec![] } else { vec!["skip"] };
    }
    let Some(t) = item.transaction.as_ref().filter(|_| item.disposition == "transaction") else { return vec!["disposition"] };
    let cents = |s: &str| s.replace(',', "").parse::<f64>().ok().map(|v| (v * 100.0).round() as i64);
    let digits = |s: Option<&str>| s.map(|s| s.chars().filter(char::is_ascii_digit).collect::<String>()).filter(|d| !d.is_empty());
    let mut wrong = vec![];
    if t.kind != answer.kind { wrong.push("kind") }
    if t.amount.as_deref().and_then(cents) != cents(&answer.amount) { wrong.push("amount") }
    if t.date != answer.date { wrong.push("date") }
    if digits(t.account_ref.as_deref()) != digits(answer.account) { wrong.push("account") }
    if answer.other.is_some() && digits(t.other_ref.as_deref()) != digits(answer.other) { wrong.push("other account") }
    wrong
}

#[derive(Default)]
struct Tally {
    planted: usize,
    put_right: usize,
    right: usize,
    changed: usize,
    left: BTreeMap<&'static str, usize>,
}

fn sample_budget() -> urbudget_core::domain::Budget {
    let mut b = urbudget_core::test_support::sample();
    b.currency = "SAR".into();
    b
}

fn main() {
    let dry = std::env::args().nth(1).as_deref() == Some("dry");
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let per: usize = env("URBUDGET_EVAL_TEXTS").and_then(|n| n.parse().ok()).unwrap_or(6);
    let formats = formats();
    if dry {
        return check(&formats, per);
    }
    let (Some(provider), Some(key)) = (env("URBUDGET_EVAL_PROVIDER"), env("URBUDGET_EVAL_KEY")) else {
        eprintln!("Set URBUDGET_EVAL_PROVIDER (anthropic or openai) and URBUDGET_EVAL_KEY, or run with `dry`.");
        std::process::exit(2);
    };
    if provider != "anthropic" && provider != "openai" {
        eprintln!("URBUDGET_EVAL_PROVIDER is anthropic or openai.");
        std::process::exit(2);
    }
    let model = env("URBUDGET_EVAL_MODEL");
    // A folder of its own: its own budget, settings and sealed key.
    let folder = tempfile::tempdir().expect("a temporary folder");
    let rt = Runtime::new(folder.path()).expect("the services must start");
    let call = |method: &str, params: Value| urbudget_services::block_on(rt.dispatch(method, params));
    call("save_budget", json!({"budget": sample_budget(), "expectedToken": ""})).expect("a budget");
    call("assistant_set_key", json!({"provider": provider, "key": key})).expect("the key");
    call("assistant_settings", json!({"change": {"provider": provider, "model": model.clone().unwrap_or_default()}})).expect("the provider");
    let budget_id = call("load_budget", json!({})).expect("the budget")["databasePath"].as_str().unwrap_or_default().to_owned();

    let mut tallies: Vec<Tally> = formats.iter().map(|_| Tally::default()).collect();
    for r in 0..2 {
        let (messages, answers, templates) = round(&formats, r, per);
        let request = json!({"budgetId": budget_id, "provider": provider, "model": model, "messages": messages, "note": "",
            "templates": templates, "payees": [], "trusted": []});
        let began = Instant::now();
        let started = call("assistant_sms_start", json!({"request": request})).unwrap_or_else(|e| panic!("round {r} didn't start: {e}"));
        let id = started["id"].as_str().unwrap_or_default().to_owned();
        let reply = urbudget_services::block_on(async {
            loop {
                tokio::time::sleep(Duration::from_millis(800)).await;
                let reply = rt.dispatch("assistant_sms_poll", json!({"id": id})).await?;
                if reply["running"] != json!(true) { return Ok::<Value, String>(reply) }
            }
        }).unwrap_or_else(|e| panic!("round {r} failed: {e}"));
        let items: Vec<Item> = serde_json::from_value(reply["analysis"]["items"].clone()).expect("the analysis");
        let learning = &reply["learning"];
        println!("Round {}: {} texts, {} reviewed, {} read in full, {:.0} s", r + 1, messages.len(), learning["reviewed"], learning["read"], began.elapsed().as_secs_f64());
        for (m, (f, answer)) in messages.iter().zip(&answers) {
            let item = items.iter().find(|item| item.source_ids.contains(&m.id));
            let wrong = differences(item, answer);
            let t = &mut tallies[*f];
            if planted(*f, r) {
                t.planted += 1;
                if wrong.is_empty() { t.put_right += 1 } else { for w in wrong { *t.left.entry(w).or_default() += 1 } }
            } else {
                t.right += 1;
                if !wrong.is_empty() { t.changed += 1; for w in wrong { *t.left.entry(w).or_default() += 1 } }
            }
        }
        let rewritten = learning["templates"].as_array().map_or(0, |t| t.iter().filter(|l| !l["replaces"].is_null()).count());
        if rewritten > 0 { println!("  the assistant rewrote {rewritten} template(s)") }
    }
    println!();
    println!("{:<32} {:<34} {:>12} {:>14}  still wrong", "Format", "Planted mistake", "Put right", "Right changed");
    let (mut planted, mut put_right, mut right, mut changed) = (0, 0, 0, 0);
    for (format, t) in formats.iter().zip(&tallies) {
        let left = t.left.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", ");
        println!("{:<32} {:<34} {:>12} {:>14}  {left}", format.name, format.mistake, format!("{}/{}", t.put_right, t.planted), format!("{}/{}", t.changed, t.right));
        planted += t.planted; put_right += t.put_right; right += t.right; changed += t.changed;
    }
    println!();
    println!("Planted mistakes put right: {put_right} of {planted} ({}%)", put_right * 100 / planted.max(1));
    println!("Right readings changed: {changed} of {right}");
}

/// Without a model: every text goes to review, each right template reads its texts as their
/// answers say, and each wrong one gets its mistake.
fn check(formats: &[Format], per: usize) {
    let b = sample_budget();
    let mut ok = true;
    for r in 0..2 {
        let (messages, answers, templates) = round(formats, r, per);
        let lanes = sms_templates::lanes(&b, &messages, &templates, &[], &HashSet::new());
        if !lanes.read.is_empty() || !lanes.junk.is_empty() {
            ok = false;
            for i in lanes.read.iter().chain(lanes.junk.iter().map(|(i, _)| i)) {
                println!("round {} {}: text {i} isn't read by its template", r + 1, formats[answers[*i].0].name);
            }
        }
        for (i, item) in &lanes.review {
            let (f, answer) = &answers[*i];
            let wrong = differences(Some(item), answer);
            if planted(*f, r) == wrong.is_empty() {
                ok = false;
                println!("round {} {}: text {i} reads {wrong:?}, planted: {}", r + 1, formats[*f].name, planted(*f, r));
            }
        }
        println!("Round {}: {} texts, {} reviewed with a planted mistake", r + 1, messages.len(),
            lanes.review.iter().filter(|(i, _)| planted(answers[*i].0, r)).count());
    }
    if !ok { std::process::exit(1) }
    println!("Every right template reads its texts right, and every wrong one makes its mistake.");
}
