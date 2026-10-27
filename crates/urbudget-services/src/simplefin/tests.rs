use super::*;
use crate::bank_import::{apply, Choice};
use urbudget_core::test_support::sample;
use base64::engine::general_purpose::STANDARD;
use chrono::FixedOffset;
use std::io::{BufRead, BufReader, Write};
use std::sync::Arc;

fn at(s: &str) -> i64 {
    DateTime::parse_from_rfc3339(s).unwrap().timestamp()
}
fn set(v: Value) -> AccountSet {
    serde_json::from_value(v).unwrap()
}
fn budget() -> Budget {
    let mut b = sample();
    b.currency = "USD".into();
    b
}
fn linked(mut b: Budget, account: &str, link: Value) -> Budget {
    let a = b.accounts.iter_mut().find(|a| a.id == account).unwrap();
    a.extra.insert("simplefin".into(), link);
    b
}
fn utc() -> FixedOffset {
    FixedOffset::east_opt(0).unwrap()
}
/// The demo's shape: a checking account and a card at one bank, ids repeating across accounts.
fn demo() -> AccountSet {
    set(json!({
        "errlist": [],
        "connections": [{"conn_id": "CON-1", "name": "Demo", "org_id": "demo", "org_name": "First Demo Bank", "sfin_url": "https://example.com/simplefin"}],
        "accounts": [
            {"id": "ACT-checking", "name": "Everyday Checking", "conn_id": "CON-1", "currency": "USD", "balance": "1520.15", "balance-date": at("2026-10-18T06:00:00Z"),
             "transactions": [
                {"id": "1", "posted": at("2026-10-10T12:00:00Z"), "amount": "-40.00", "description": "Grocery store", "payee": "Grocery store", "memo": "LOCAL GROCER STORE #1133"},
                {"id": "2", "posted": at("2026-10-12T12:00:00Z"), "amount": "2500.00", "description": "Pay day!", "payee": "You", "memo": "PAY DAY - FROM YER JOB"},
                {"id": "3", "posted": 0, "amount": "-9.99", "description": "Pending coffee", "pending": true},
                {"id": "4", "posted": at("2026-10-16T12:00:00Z"), "amount": "-12.5", "description": "Bus pass", "pending": true}
             ]},
            {"id": "ACT-card", "name": "Rewards Card", "conn_id": "CON-1", "currency": "USD", "balance": "-212.34", "balance-date": at("2026-10-18T06:00:00Z"),
             "transactions": [
                {"id": "1", "posted": at("2026-10-11T12:00:00Z"), "amount": "-62.34", "description": "Restaurant"},
                {"id": "2", "posted": at("2026-10-13T12:00:00Z"), "amount": "150.00", "description": "Payment, thank you"}
             ]}
        ]
    }))
}

#[test]
fn compact_native_reviews_preserve_desktop_rows_timestamps_and_account_metadata() {
    let b = linked(budget(), "cash", json!({"id":"ACT-checking","since":"2026-10-01"}));
    let set = demo();
    let fetched = at("2026-10-18T12:00:00Z");
    let expected = serde_json::to_value(reviews(&b, &set, fetched, &utc())).unwrap();
    let mut compact = serde_json::to_value(reviews_compact(&b, &set, fetched, &utc())).unwrap();
    for account in compact.as_array_mut().unwrap() {
        let sets = account.as_object_mut().unwrap().remove("candidateSets").unwrap();
        for row in account["rows"].as_array_mut().unwrap() {
            let index = row.as_object_mut().unwrap().remove("candidateSet").unwrap().as_u64().unwrap() as usize;
            row["candidates"] = sets[index]["candidates"].clone();
            for field in ["candidateDetails", "previouslyMatchedCandidates"] {
                if !sets[index][field].as_array().unwrap().is_empty() {
                    row[field] = sets[index][field].clone();
                }
            }
        }
    }
    assert_eq!(compact, expected);
    assert!(compact[0]["rows"].as_array().unwrap().iter().all(|r| r["at"].is_string()));
}

#[test]
fn reads_amounts_exactly_in_cents() {
    for (s, n) in [
        ("-40.00", -4000),
        ("2500", 250_000),
        ("+0.5", 50),
        ("-12.5", -1250),
        (".25", 25),
        ("7.", 700),
        ("12.3400", 1234),
        ("0", 0),
        (" -1.01 ", -101),
    ] {
        assert_eq!(cents(s), Some(n), "{s}");
    }
    for s in ["", "-", ".", "12.345", "1,234.00", "1e3", "$5", "--5", "5-", "NaN", "9999999999999"] {
        assert_eq!(cents(s), None, "{s}");
    }
}

#[test]
fn shows_only_plain_text_from_simplefin() {
    assert_eq!(clean("  Sign in\nagain  "), "Sign in again");
    assert_eq!(clean("<b>Bank</b> <script>x()</script>needs you"), "Bank x() needs you");
    assert_eq!(clean("Balance < 0 and 2 > 1"), "Balance < 0 and 2 > 1");
    assert_eq!(clean("evil\u{202E}txt.exe\u{0007}"), "evil txt.exe");
    assert_eq!(clean(&"a".repeat(400)).chars().count(), 300);
}

#[test]
fn decodes_setup_tokens_and_refuses_anything_else() {
    let url = "https://beta-bridge.simplefin.org/simplefin/claim/DEMO-v2-FB8ACBDFD76100EE2EE5";
    let token = STANDARD.encode(url);
    assert_eq!(claim_url(&token).unwrap().as_str(), url);
    // Pasted with a line break, or without padding.
    let (a, b) = token.split_at(20);
    assert_eq!(claim_url(&format!(" {a}\n{b} ")).unwrap().as_str(), url);
    assert_eq!(claim_url(token.trim_end_matches('=')).unwrap().as_str(), url);
    for bad in ["", "not a token", &STANDARD.encode("hello"), &STANDARD.encode("http://example.com/claim/x"), &STANDARD.encode("ftp://example.com/x")] {
        assert_eq!(claim_url(bad).unwrap_err(), bad_token(), "{bad}");
    }
    // Plain http reaches only this computer, and only in tests and debug builds.
    assert!(claim_url(&STANDARD.encode("http://127.0.0.1:9/claim/x")).is_ok());
}

#[test]
fn splits_the_access_url_into_address_and_credentials() {
    let a = Access::parse("https://demo:s%40cret@beta-bridge.simplefin.org/simplefin\n").unwrap();
    assert_eq!((a.user.as_str(), a.password.as_str()), ("demo", "s@cret"));
    assert_eq!(a.accounts(), "https://beta-bridge.simplefin.org/simplefin/accounts");
    assert!(!a.base.as_str().contains("cret"));
    assert!(Access::parse("https://beta-bridge.simplefin.org/simplefin").is_err());
    assert!(Access::parse("http://demo:x@example.com/simplefin").is_err());
    assert!(Access::parse("garbage").is_err());
}

#[test]
fn dates_a_transaction_on_the_users_calendar_day() {
    let b = linked(budget(), "cash", json!({"id": "A", "from": "2026-10-01"}));
    // 02:30 UTC on the 15th is still the 14th in California and already the 15th in Riyadh.
    let one = |t: Value| {
        set(json!({"accounts": [{"id": "A", "name": "Checking", "currency": "USD", "balance": "0", "transactions": [t]}]}))
    };
    let posted = one(json!({"id": "t", "posted": at("2026-10-15T02:30:00Z"), "amount": "-5.00", "description": "Shop"}));
    let date = |s: &AccountSet, tz: FixedOffset| reviews(&b, s, at("2026-10-18T12:00:00Z"), &tz)[0].rows[0].date.clone();
    let california = FixedOffset::west_opt(7 * 3600).unwrap();
    let riyadh = FixedOffset::east_opt(3 * 3600).unwrap();
    assert_eq!(date(&posted, california), "2026-10-14");
    assert_eq!(date(&posted, riyadh), "2026-10-15");
    assert_eq!(date(&posted, utc()), "2026-10-15");
    // When it happened beats when it posted.
    let both = one(json!({"id": "t", "posted": at("2026-10-15T02:30:00Z"), "transacted_at": at("2026-10-13T20:00:00Z"), "amount": "-5.00", "description": "Shop"}));
    assert_eq!(date(&both, riyadh), "2026-10-13");
    assert_eq!(date(&both, california), "2026-10-13");
    // A day boundary in the user's time zone starts the request, not one in UTC.
    assert_eq!(midnight(NaiveDate::from_ymd_opt(2026, 10, 15).unwrap(), &riyadh), at("2026-10-14T21:00:00Z"));
    assert_eq!(day(at("2026-10-14T21:00:00Z"), &riyadh).to_string(), "2026-10-15");
}

#[test]
fn invalid_bank_dates_and_invalid_drafts_produce_review_errors() {
    let b = linked(budget(), "cash", json!({"id": "ACT-checking", "from": "2026-10-01"}));
    let mut set = demo();
    set.accounts[0].transactions[0].transacted_at = Some(NaiveDate::MAX.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp());
    let r = reviews(&b, &set, at("2026-10-18T12:00:00Z"), &utc());
    assert!(r[0].rows.is_empty());
    assert!(r[0].error.as_deref().is_some_and(|e| e.contains("invalid date")));
    let mut invalid = b;
    let mut transfer = urbudget_core::test_support::purchase();
    transfer.transfer_account_id = Some("cash".into());
    transfer.transfer_status = Some(crate::domain::Cleared::Uncleared);
    transfer.amount = i64::MIN;
    transfer.splits[0].amount = i64::MIN;
    invalid.transactions.push(transfer);
    let r = reviews(&invalid, &demo(), at("2026-10-18T12:00:00Z"), &utc());
    assert!(r[0].rows.is_empty());
    assert!(r[0].error.is_some(), "invalid drafts must not enter the transfer match index");
}

#[test]
fn an_imported_transaction_was_added_when_the_bank_says_it_happened() {
    let b = linked(budget(), "cash", json!({"id": "A", "from": "2026-10-01"}));
    let s = set(json!({"accounts": [{"id": "A", "name": "Checking", "currency": "USD", "balance": "0", "transactions": [
        {"id": "1", "posted": at("2026-10-15T23:00:00Z"), "transacted_at": at("2026-10-15T18:30:00Z"), "amount": "-5.00", "description": "Bought first"},
        {"id": "2", "posted": at("2026-10-15T22:00:00Z"), "amount": "-7.00", "description": "Posted only"}
    ]}]}));
    let r = reviews(&b, &s, at("2026-10-18T12:00:00Z"), &utc());
    // The rows go through the window and back, as JSON.
    let choices = r[0]
        .rows
        .iter()
        .map(|row| Choice { row: serde_json::from_value(serde_json::to_value(row).unwrap()).unwrap(), action: "new".into(), match_id: None })
        .collect();
    let b = apply(b, "cash", choices).unwrap();
    let added = |payee: &str| b.transactions.iter().find(|t| t.payee == payee).and_then(|t| t.created_at());
    assert_eq!(added("Bought first"), Some("2026-10-15T18:30:00.000Z"));
    assert_eq!(added("Posted only"), Some("2026-10-15T22:00:00.000Z"));
    // On their day, the one that happened later is on top.
    let mut list: Vec<&crate::domain::Transaction> = b.transactions.iter().collect();
    crate::domain::sort_newest_first(&mut list);
    assert_eq!(list[0].payee, "Posted only");
}

#[test]
fn keeps_the_budgets_signs_for_cash_and_credit_accounts() {
    let b = linked(budget(), "cash", json!({"id": "ACT-checking", "from": "2026-10-01"}));
    let b = linked(b, "card", json!({"id": "ACT-card", "from": "2026-10-01"}));
    let r = reviews(&b, &demo(), at("2026-10-18T12:00:00Z"), &utc());
    let amounts = |i: usize| r[i].rows.iter().map(|row| row.amount).collect::<Vec<_>>();
    // Deposits are positive; on the card a purchase is money out and a payment money in, as in
    // the budget, where a card's balance owed is negative.
    assert_eq!((r[0].account_id.as_str(), amounts(0)), ("cash", vec![-4000, 250_000]));
    assert_eq!((r[1].account_id.as_str(), amounts(1)), ("card", vec![-6234, 15_000]));
    assert_eq!((r[0].balance, r[1].balance), (Some(152_015), Some(-21_234)));
    assert_eq!(r[1].balance_date.as_deref(), Some("2026-10-18"));
    // A bank that sends a card's purchases as deposits can be read the other way round.
    let flipped = linked(b, "card", json!({"id": "ACT-card", "from": "2026-10-01", "flipSigns": true}));
    let r = reviews(&flipped, &demo(), at("2026-10-18T12:00:00Z"), &utc());
    assert_eq!(r[1].rows.iter().map(|row| row.amount).collect::<Vec<_>>(), vec![6234, -15_000]);
    assert_eq!(r[1].balance, Some(21_234));
}

#[test]
fn takes_the_tidy_payee_and_keeps_the_banks_words_in_the_memo() {
    let b = linked(budget(), "cash", json!({"id": "ACT-checking", "from": "2026-10-01"}));
    let r = reviews(&b, &demo(), at("2026-10-18T12:00:00Z"), &utc());
    let rows: Vec<_> = r[0].rows.iter().map(|row| (row.payee.as_str(), row.memo.as_str())).collect();
    assert_eq!(rows, vec![("Grocery store", "LOCAL GROCER STORE #1133"), ("You", "Pay day!")]);
    let card = linked(budget(), "card", json!({"id": "ACT-card", "from": "2026-10-01"}));
    let r = reviews(&card, &demo(), at("2026-10-18T12:00:00Z"), &utc());
    assert_eq!((r[0].rows[0].payee.as_str(), r[0].rows[0].memo.as_str()), ("Restaurant", ""));
}

#[test]
fn leaves_out_pending_transactions_and_ones_posted_after_the_download() {
    let b = linked(budget(), "cash", json!({"id": "ACT-checking", "from": "2026-10-01"}));
    let r = reviews(&b, &demo(), at("2026-10-18T12:00:00Z"), &utc());
    assert!(r[0].rows.iter().all(|row| !row.payee.contains("Pending") && row.payee != "Bus pass"));
    assert_eq!(r[0].rows.len(), 2);
    // The demo server ignores end-date and sends tomorrow's transactions; they wait for the next sync.
    let mut early = demo();
    early.accounts[0].transactions.push(
        serde_json::from_value(json!({"id": "9", "posted": at("2026-10-19T09:00:00Z"), "amount": "-1.00", "description": "Tomorrow"})).unwrap(),
    );
    let r = reviews(&b, &early, at("2026-10-18T12:00:00Z"), &utc());
    assert_eq!(r[0].rows.len(), 2);
    let r = reviews(&b, &early, at("2026-10-19T12:00:00Z"), &utc());
    assert_eq!(r[0].rows.last().map(|row| row.payee.as_str()), Some("Tomorrow"));
}

#[test]
fn refuses_an_account_in_another_currency() {
    let b = linked(budget(), "cash", json!({"id": "E", "from": "2026-10-01"}));
    let b = linked(b, "card", json!({"id": "C", "from": "2026-10-01"}));
    let s = set(json!({"accounts": [
        {"id": "E", "name": "Euro Account", "currency": "EUR", "balance": "1", "transactions": [{"id": "t", "posted": at("2026-10-10T12:00:00Z"), "amount": "-1.00", "description": "x"}]},
        {"id": "C", "name": "Points", "currency": "https://bank.example/points", "balance": "1"}
    ]}));
    let r = reviews(&b, &s, at("2026-10-18T12:00:00Z"), &utc());
    assert_eq!(r[0].error.as_deref(), Some("Euro Account is in EUR, but this budget uses USD."));
    assert!(r[0].rows.is_empty());
    assert_eq!(r[1].error.as_deref(), Some("Points uses a currency of its own, but this budget uses USD."));
    // A lower-case code is the same currency.
    let lower = set(json!({"accounts": [{"id": "E", "name": "Checking", "currency": "usd", "balance": "1", "transactions": [{"id": "t", "posted": at("2026-10-10T12:00:00Z"), "amount": "-1.00", "description": "x"}]}]}));
    assert_eq!(reviews(&b, &lower, at("2026-10-18T12:00:00Z"), &utc())[0].rows.len(), 1);
}

#[test]
fn says_when_simplefin_leaves_a_linked_account_out() {
    let b = linked(budget(), "cash", json!({"id": "ACT-gone", "from": "2026-10-01"}));
    let r = reviews(&b, &demo(), at("2026-10-18T12:00:00Z"), &utc());
    assert!(r[0].error.as_deref().unwrap().starts_with("SimpleFIN didn't send Cash this time."));
    // An unreadable amount stops that account, not the others, and says which one.
    let b = linked(b, "card", json!({"id": "ACT-card", "from": "2026-10-01"}));
    let mut s = demo();
    s.accounts[1].transactions[0].amount = "12,34".into();
    let r = reviews(&b, &s, at("2026-10-18T12:00:00Z"), &utc());
    assert_eq!(r[1].error.as_deref(), Some("SimpleFIN sent an amount UrBudget can't read for Rewards Card: 12,34"));
}

#[test]
fn a_second_sync_finds_what_the_first_imported() {
    let b = linked(budget(), "cash", json!({"id": "ACT-checking", "from": "2026-10-01"}));
    let b = linked(b, "card", json!({"id": "ACT-card", "from": "2026-10-01"}));
    let first = reviews(&b, &demo(), at("2026-10-18T12:00:00Z"), &utc());
    assert!(first.iter().flat_map(|r| &r.rows).all(|row| !row.duplicate));
    // The same ids at two accounts make different rows.
    assert_ne!(first[0].rows[0].id, first[1].rows[0].id);
    let mut b = b;
    for r in &first {
        let choices = r.rows.iter().map(|row| Choice { row: row.clone(), action: "new".into(), match_id: None }).collect();
        b = mark_synced(apply(b, &r.account_id, choices).unwrap(), &r.account_id, "2026-10-18").unwrap();
    }
    assert_eq!(b.transactions.len(), 4);
    let link = b.accounts[0].extra["simplefin"].clone();
    assert_eq!((link["lastSync"].as_str(), link["from"].as_str()), (Some("2026-10-18"), Some("2026-10-01")));
    // The next sync brings the same transactions and one new one: only that one is new.
    let mut again = demo();
    again.accounts[0].transactions.push(
        serde_json::from_value(json!({"id": "5", "posted": at("2026-10-19T12:00:00Z"), "amount": "-3.00", "description": "Newsstand"})).unwrap(),
    );
    let second = reviews(&b, &again, at("2026-10-20T12:00:00Z"), &utc());
    let fresh: Vec<_> = second.iter().flat_map(|r| &r.rows).filter(|row| !row.duplicate).map(|row| row.payee.as_str()).collect();
    assert_eq!(fresh, vec!["Newsstand"]);
    // Importing the whole review again adds only the new one.
    let choices = second[0].rows.iter().map(|row| Choice { row: row.clone(), action: "new".into(), match_id: None }).collect();
    assert_eq!(apply(b.clone(), "cash", choices).unwrap().transactions.len(), 5);
    // A transaction typed in by hand is offered as the match for the bank's.
    let mut manual = budget();
    let mut transaction = crate::domain::Transaction { account_id: "cash".into(), date: "2026-10-09".into(), amount: -4000, ..urbudget_core::test_support::purchase() };
    transaction.splits[0].amount = transaction.amount;
    manual.transactions.push(transaction);
    let manual = linked(manual, "cash", json!({"id": "ACT-checking", "from": "2026-10-01"}));
    crate::domain::validate(&manual).unwrap();
    let r = reviews(&manual, &demo(), at("2026-10-18T12:00:00Z"), &utc());
    assert!(r[0].error.is_none(), "{:?}", r[0].error);
    assert_eq!(r[0].rows[0].candidates, vec!["tx"]);
}

#[test]
fn a_sync_after_the_first_starts_a_week_before_the_last() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 18).unwrap();
    let link = |v: Value| serde_json::from_value::<Link>(v).unwrap();
    let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
    assert_eq!(since(&link(json!({"id": "a", "from": "2026-09-01"})), today), d("2026-09-01"));
    assert_eq!(since(&link(json!({"id": "a", "from": "2026-09-01", "lastSync": "2026-10-15"})), today), d("2026-10-08"));
    assert_eq!(since(&link(json!({"id": "a"})), today), d("2026-09-18"));
    // No further back than a year, and never in the future.
    assert_eq!(since(&link(json!({"id": "a", "from": "2020-01-01"})), today), d("2025-10-18"));
    assert_eq!(since(&link(json!({"id": "a", "from": "2027-01-01"})), today), today);
    // Rows from before an account's own start are left out, even when another account made
    // the download reach further back.
    let b = linked(budget(), "cash", json!({"id": "ACT-checking", "from": "2026-10-11"}));
    let r = reviews(&b, &demo(), at("2026-10-18T12:00:00Z"), &utc());
    assert_eq!(r[0].rows.iter().map(|row| row.date.as_str()).collect::<Vec<_>>(), vec!["2026-10-12"]);
}

#[test]
fn one_request_covers_every_account_and_long_ones_are_split() {
    let now = at("2026-10-18T15:00:00Z");
    // Two accounts synced lately: one request from the earlier one's start.
    let b = linked(budget(), "cash", json!({"id": "A", "from": "2026-01-01", "lastSync": "2026-10-17"}));
    let b = linked(b, "card", json!({"id": "B", "from": "2026-01-01", "lastSync": "2026-10-15"}));
    assert_eq!(plan(&b, now, &utc()).unwrap(), vec![(at("2026-10-08T00:00:00Z"), now)]);
    // A first sync a year back takes one request per 45 days, back to back.
    let b = linked(b, "card", json!({"id": "B", "from": "2025-10-18"}));
    let w = plan(&b, now, &utc()).unwrap();
    assert_eq!(w.len(), 9);
    assert_eq!((w[0].0, w.last().unwrap().1), (at("2025-10-18T00:00:00Z"), now));
    assert!(w.windows(2).all(|p| p[0].1 == p[1].0));
    assert!(w.iter().all(|(s, e)| e - s <= WINDOW_DAYS * 86_400 && e > s));
    // Closed accounts and accounts without a link don't count.
    let mut closed = b.clone();
    closed.accounts[1].archived = true;
    assert_eq!(plan(&closed, now, &utc()).unwrap().len(), 1);
    assert!(plan(&budget(), now, &utc()).unwrap_err().starts_with("Link an account"));
}

#[test]
fn passes_on_simplefins_messages_once_and_says_whose() {
    let s = set(json!({
        "errlist": [
            {"code": "con.auth", "msg": "Sign in to <b>your bank</b> again.", "conn_id": "CON-1"},
            {"code": "act.missingdata", "msg": "Some transactions are missing.", "account_id": "ACT-card"},
            {"code": "gen.api", "msg": "You're close to\n today's request limit."},
            {"code": "gen.api", "msg": "You're close to today's request limit."}
        ],
        "errors": ["You're close to today's request limit.", "Over quota: requests may be refused."],
        "connections": [{"conn_id": "CON-1", "name": "Demo", "org_name": "First Demo Bank"}],
        "accounts": [{"id": "ACT-card", "name": "Rewards Card", "conn_id": "CON-1", "currency": "USD", "balance": "0"}]
    }));
    assert_eq!(
        messages(&s),
        vec![
            "First Demo Bank: Sign in to your bank again.",
            "Rewards Card: Some transactions are missing.",
            "You're close to today's request limit.",
            "Over quota: requests may be refused.",
        ]
    );
    let l = listed(&s, &utc());
    assert_eq!((l[0].org.as_str(), l[0].currency.as_deref(), l[0].balance), ("First Demo Bank", Some("USD"), Some(0)));
}

#[test]
fn stops_before_the_daily_limit_and_starts_again_tomorrow() {
    let dir = tempfile::tempdir().unwrap();
    let state = SimpleFin::new(dir.path());
    state.spend(DAILY_LIMIT - 3).unwrap();
    let wide = state.spend(5).unwrap_err();
    assert!(wide.starts_with("This sync needs 5 requests to SimpleFIN, and only 3 are left today."), "{wide}");
    state.spend(3).unwrap();
    let done = state.spend(1).unwrap_err();
    assert!(done.contains("20 times today"), "{done}");
    assert_eq!(state.used().requests, DAILY_LIMIT);
    // Another day's count doesn't carry over.
    std::fs::write(dir.path().join("simplefin-usage.json"), r#"{"day":"2026-01-01","requests":20}"#).unwrap();
    assert_eq!(state.used().requests, 0);
    state.spend(1).unwrap();
}

// A fake SimpleFIN server on this computer, answering like the real one.
struct Fake {
    url: String,
    seen: Arc<Mutex<Vec<String>>>,
}
fn serve(answer: impl Fn(&str, Option<&str>) -> (u16, String) + Send + 'static) -> Fake {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(vec![]));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            let mut auth = None;
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                    break;
                }
                if let Some((name, value)) = header.split_once(':') {
                    if name.eq_ignore_ascii_case("authorization") {
                        auth = Some(value.trim().to_string());
                    }
                }
            }
            let request = request.trim().to_string();
            log.lock().unwrap().push(request.clone());
            let (status, body) = answer(&request, auth.as_deref());
            let _ = write!(
                stream,
                "HTTP/1.1 {status} Answer\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Fake { url: format!("http://127.0.0.1:{port}"), seen }
}
fn query(request: &str) -> HashMap<String, String> {
    let path = request.split(' ').nth(1).unwrap();
    Url::parse(&format!("http://x{path}")).unwrap().query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
}
use std::collections::HashMap;
fn run<T>(f: impl std::future::Future<Output = T>) -> T {
    crate::block_on(f)
}

#[test]
fn claims_a_setup_token_once() {
    let claimed = Arc::new(Mutex::new(false));
    let fake = {
        let claimed = claimed.clone();
        serve(move |request, _| {
            assert!(request.starts_with("POST /simplefin/claim/TOKEN-1 "), "{request}");
            let mut once = claimed.lock().unwrap();
            if *once {
                return (403, String::new());
            }
            *once = true;
            (200, "http://demo:secret@127.0.0.1:1/simplefin".into())
        })
    };
    let token = STANDARD.encode(format!("{}/simplefin/claim/TOKEN-1", fake.url));
    let http = http().unwrap();
    let access = run(claim(&http, claim_url(&token).unwrap())).unwrap();
    assert_eq!(access, "http://demo:secret@127.0.0.1:1/simplefin");
    // A second claim means someone already has the access: the user is told so.
    assert_eq!(run(claim(&http, claim_url(&token).unwrap())).unwrap_err(), token_used());
    assert_eq!(fake.seen.lock().unwrap().len(), 2);
}

#[test]
fn a_claim_that_answers_with_something_else_is_refused() {
    let fake = serve(|_, _| (200, "<html>Oops</html>".into()));
    let token = STANDARD.encode(format!("{}/simplefin/claim/x", fake.url));
    let e = run(claim(&http().unwrap(), claim_url(&token).unwrap())).unwrap_err();
    assert!(e.starts_with("SimpleFIN sent an access address UrBudget can't use."), "{e}");
    let fake = serve(|_, _| (500, String::new()));
    let token = STANDARD.encode(format!("{}/simplefin/claim/x", fake.url));
    let e = run(claim(&http().unwrap(), claim_url(&token).unwrap())).unwrap_err();
    assert_eq!(e, "SimpleFIN couldn't accept the setup token (HTTP 500). Try again later.");
}

/// A server holding 120 days of one transaction a day at noon, which answers only within the
/// dates asked for, only with the right password, and warns about the quota on the third request.
fn bank() -> Fake {
    let count = Arc::new(Mutex::new(0));
    serve(move |request, auth| {
        if auth != Some(format!("Basic {}", STANDARD.encode("demo:s@cret")).as_str()) {
            return (403, String::new());
        }
        let q = query(request);
        assert_eq!(q.get("version").map(String::as_str), Some("2"));
        assert!(!q.contains_key("pending"), "pending transactions are left out");
        let mut n = count.lock().unwrap();
        *n += 1;
        let (start, end): (i64, i64) = (q["start-date"].parse().unwrap(), q["end-date"].parse().unwrap());
        assert!(end - start <= 90 * 86_400, "SimpleFIN allows at most 90 days a request");
        let first = at("2026-06-20T12:00:00Z");
        let transactions: Vec<Value> = (0..120)
            .map(|i| first + i * 86_400)
            .filter(|t| (start..end).contains(t))
            .map(|t| json!({"id": format!("T{t}"), "posted": t, "amount": "-1.00", "description": "Coffee"}))
            .collect();
        let errlist = if *n == 3 { json!([{"code": "gen.api", "msg": "You're nearing your daily request limit."}]) } else { json!([]) };
        let body = json!({
            "errlist": errlist,
            "connections": [{"conn_id": "C", "name": "Bank", "org_id": "b", "sfin_url": "x"}],
            "accounts": [{"id": "A", "name": "Checking", "conn_id": "C", "currency": "USD", "balance": format!("{}.00", 100 + *n), "balance-date": end, "transactions": transactions}]
        });
        (200, body.to_string())
    })
}

#[test]
fn downloads_a_long_first_sync_in_windows_and_keeps_each_transaction_once() {
    let fake = bank();
    let access = Access::parse(&(fake.url.replace("http://", "http://demo:s%40cret@") + "/simplefin")).unwrap();
    let now = at("2026-10-18T15:00:00Z");
    let b = linked(budget(), "cash", json!({"id": "A", "from": "2026-06-01"}));
    let windows = plan(&b, now, &utc()).unwrap();
    assert_eq!(windows.len(), 4);
    // Overlapping windows, as a merge of two downloads might bring, still give each once.
    let mut overlapping = windows.clone();
    overlapping.insert(1, (windows[0].1 - 5 * 86_400, windows[1].1));
    let all = run(download(&http().unwrap(), &access, &overlapping)).unwrap();
    let seen = fake.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 5);
    assert!(seen.iter().all(|r| r.starts_with("GET /simplefin/accounts?version=2&start-date=")));
    assert_eq!(all.accounts.len(), 1);
    assert_eq!(all.accounts[0].transactions.len(), 120);
    // The balance is the newest request's, and the quota warning reaches the user.
    assert_eq!(all.accounts[0].balance, "105.00");
    assert_eq!(messages(&all), vec!["You're nearing your daily request limit."]);
    let r = reviews(&b, &all, now, &utc());
    assert_eq!(r[0].rows.len(), 120);
    assert_eq!((r[0].rows[0].date.as_str(), r[0].rows[119].date.as_str()), ("2026-06-20", "2026-10-17"));
}

#[test]
fn says_plainly_when_simplefin_refuses() {
    let wrong = bank();
    let access = Access::parse(&(wrong.url.replace("http://", "http://demo:wrong@") + "/simplefin")).unwrap();
    let e = run(download(&http().unwrap(), &access, &[(0, 1)])).err().unwrap();
    assert_eq!(e, "SimpleFIN turned down this connection. It may have been revoked. Remove it here, then connect again with a new setup token.");
    let unpaid = serve(|_, _| (402, String::new()));
    let access = Access::parse(&(unpaid.url.replace("http://", "http://demo:x@") + "/simplefin")).unwrap();
    let e = run(download(&http().unwrap(), &access, &[(0, 1)])).err().unwrap();
    assert_eq!(e, "SimpleFIN needs a payment before it sends more. Check your subscription at SimpleFIN Bridge, then sync again.");
    let broken = serve(|_, _| (200, "{not json".into()));
    let access = Access::parse(&(broken.url.replace("http://", "http://demo:x@") + "/simplefin")).unwrap();
    assert_eq!(run(download(&http().unwrap(), &access, &[(0, 1)])).err().unwrap(), unreadable());
    // Nothing listening.
    let access = Access::parse("http://demo:x@127.0.0.1:9/simplefin").unwrap();
    assert_eq!(run(download(&http().unwrap(), &access, &[(0, 1)])).err().unwrap(), offline());
    // A redirect isn't followed with the credentials.
    let moved = serve(|_, _| (302, String::new()));
    let access = Access::parse(&(moved.url.replace("http://", "http://demo:x@") + "/simplefin")).unwrap();
    let e = run(download(&http().unwrap(), &access, &[(0, 1)])).err().unwrap();
    assert_eq!(e, "SimpleFIN couldn't send your transactions (HTTP 302). Try again later.");
    assert_eq!(moved.seen.lock().unwrap().len(), 1);
}
