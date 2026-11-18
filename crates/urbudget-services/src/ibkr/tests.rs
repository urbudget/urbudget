use super::*;
use crate::bank_import::Choice;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::Arc;
use urbudget_core::test_support::{sample, FLEX_SAMPLE};

fn budget() -> Budget {
    let mut b = sample();
    b.currency = "USD".into();
    b
}
fn statement() -> flex::Statement {
    flex::parse(FLEX_SAMPLE).unwrap()
}
fn link(account: &str, create: bool) -> Link {
    Link { source: "U1234567".into(), account: account.into(), create, name: None, rate: None }
}
fn run<T>(f: impl std::future::Future<Output = T>) -> T {
    crate::block_on(f)
}

#[test]
fn the_token_and_query_read_however_they_were_pasted() {
    let c = Credential::parse(" 1234 5678 9012 3456 7890 \n", "\t987654 ").unwrap();
    assert_eq!((c.token.as_str(), c.query.as_str()), ("12345678901234567890", "987654"));
    // Pasted into each other's box: the long one is the token.
    assert_eq!(Credential::parse("987654", "12345678901234567890").unwrap(), c);
    assert!(Credential::parse("12345678901234567890", "").unwrap_err().contains("Flex Query ID"));
    assert!(Credential::parse("12345678901234567890", "my query").unwrap_err().contains("Flex Query ID"));
    assert!(Credential::parse("https://example.com/a?b=c", "987654").unwrap_err().contains("token"));
    assert!(Credential::parse("1234567", "987654").unwrap_err().contains("token"));
}

#[test]
fn reads_what_the_web_service_answers() {
    let accepted = "<FlexStatementResponse timestamp='13 November, 2026 06:30 AM EST'><Status>Success</Status><ReferenceCode>4455667788</ReferenceCode><url>https://example.com/ignored</url></FlexStatementResponse>";
    assert_eq!(answer(accepted.into()), Answer::Reference("4455667788".into()));
    let waiting = "<FlexStatementResponse><Status>Warn</Status><ErrorCode>1019</ErrorCode><ErrorMessage>Statement generation in progress. Please try again shortly.</ErrorMessage></FlexStatementResponse>";
    assert!(matches!(answer(waiting.into()), Answer::Problem { code: 1019, .. }));
    assert!(matches!(answer(FLEX_SAMPLE.into()), Answer::Statement(_)));
    assert_eq!(answer("<html>Maintenance</html>".into()), Answer::Problem { code: 0, message: String::new() });
    // Each refusal says what to do about it; one the app doesn't know is passed on as sent.
    assert!(problem(1012, "Token has expired.").contains("expired"));
    assert!(problem(1015, "Token is invalid.").contains("doesn't know this token"));
    assert!(problem(1014, "Query is invalid.").contains("Flex Query ID"));
    assert!(problem(1013, "IP restriction.").contains("IP address"));
    assert!(problem(1018, "Too many requests.").contains("too often"));
    assert_eq!(problem(1099, "Something new."), "Interactive Brokers couldn't send the statement: Something new. (code 1099)");
    assert!(problem(0, "").contains("couldn't read"));
}

// A stand-in for the Flex Web Service on this computer.
struct Fake {
    url: String,
    seen: Arc<Mutex<Vec<(String, bool)>>>,
}
fn serve(answer: impl Fn(&str, usize) -> String + Send + 'static) -> Fake {
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
            let mut named = false;
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                    break;
                }
                named |= header.to_ascii_lowercase().starts_with("user-agent: urbudget/");
            }
            let request = request.trim().to_string();
            let count = {
                let mut log = log.lock().unwrap();
                log.push((request.clone(), named));
                log.len()
            };
            let body = answer(&request, count);
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/plain;charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    Fake { url: format!("http://127.0.0.1:{port}"), seen }
}
fn query(request: &str) -> HashMap<String, String> {
    let path = request.split(' ').nth(1).unwrap();
    reqwest::Url::parse(&format!("http://x{path}")).unwrap().query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
}
fn credential() -> Credential {
    Credential::parse("12345678901234567890", "987654").unwrap()
}

#[test]
fn asks_for_the_statement_then_collects_it_when_its_ready() {
    let fake = serve(|request, count| {
        if request.starts_with("GET /SendRequest?") {
            "<FlexStatementResponse><Status>Success</Status><ReferenceCode>REF-1</ReferenceCode></FlexStatementResponse>".into()
        } else if count < 4 {
            "<FlexStatementResponse><Status>Warn</Status><ErrorCode>1019</ErrorCode><ErrorMessage>Statement generation in progress.</ErrorMessage></FlexStatementResponse>".into()
        } else {
            FLEX_SAMPLE.into()
        }
    });
    let body = run(download(&http().unwrap(), &fake.url, &credential(), Duration::from_millis(3))).unwrap();
    assert_eq!(flex::parse(&body).unwrap().accounts[0].id, "U1234567");
    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen.len(), 4);
    // Every request says what sends it, as Interactive Brokers requires.
    assert!(seen.iter().all(|(_, named)| *named));
    let asked = query(&seen[0].0);
    assert_eq!((asked["t"].as_str(), asked["q"].as_str(), asked["v"].as_str()), ("12345678901234567890", "987654", "3"));
    for (request, _) in &seen[1..] {
        assert!(request.starts_with("GET /GetStatement?"), "{request}");
        let collected = query(request);
        assert_eq!((collected["t"].as_str(), collected["q"].as_str(), collected["v"].as_str()), ("12345678901234567890", "REF-1", "3"));
    }
}

#[test]
fn a_refusal_stops_at_once_and_a_statement_never_ready_gives_up() {
    let expired = serve(|_, _| "<FlexStatementResponse><Status>Fail</Status><ErrorCode>1012</ErrorCode><ErrorMessage>Token has expired.</ErrorMessage></FlexStatementResponse>".into());
    let error = run(download(&http().unwrap(), &expired.url, &credential(), Duration::ZERO)).unwrap_err();
    assert!(error.contains("expired"), "{error}");
    assert_eq!(expired.seen.lock().unwrap().len(), 1);
    let busy = serve(|request, _| {
        if request.starts_with("GET /SendRequest?") {
            "<FlexStatementResponse><Status>Success</Status><ReferenceCode>R</ReferenceCode></FlexStatementResponse>".into()
        } else {
            "<FlexStatementResponse><Status>Warn</Status><ErrorCode>1019</ErrorCode><ErrorMessage>In progress.</ErrorMessage></FlexStatementResponse>".into()
        }
    });
    let error = run(download(&http().unwrap(), &busy.url, &credential(), Duration::ZERO)).unwrap_err();
    assert!(error.contains("still preparing"), "{error}");
    // Ten requests at most: Interactive Brokers allows ten a minute.
    assert_eq!(busy.seen.lock().unwrap().len(), 1 + POLLS as usize);
    let down = run(download(&http().unwrap(), "http://127.0.0.1:1", &credential(), Duration::ZERO)).unwrap_err();
    assert_eq!(down, offline());
}

#[test]
fn a_request_that_fails_before_an_answer_is_tried_once_more() {
    // The first connection is dropped without a word, as one closed at the other end would be.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let connections = Arc::new(Mutex::new(0));
    let count = connections.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let n = {
                let mut count = count.lock().unwrap();
                *count += 1;
                *count
            };
            if n == 1 {
                continue;
            }
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 && line != "\r\n" {
                line.clear();
            }
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{FLEX_SAMPLE}", FLEX_SAMPLE.len());
        }
    });
    let body = run(download(&http().unwrap(), &format!("http://127.0.0.1:{port}"), &credential(), Duration::ZERO)).unwrap();
    assert!(flex::sniff(&body));
    assert_eq!(*connections.lock().unwrap(), 2);
}

#[test]
fn a_new_broker_account_becomes_an_investment_account_and_later_syncs_fill_it() {
    let s = statement();
    let b = apply(budget(), &s, &[link("new-account", true)]).unwrap();
    let account = b.accounts.iter().find(|a| a.id == "new-account").unwrap();
    assert_eq!((account.name.as_str(), account.extra["accountType"].as_str()), ("Long term", Some("investment")));
    assert_eq!(investments::find(&b, "new-account").unwrap().total, 2_701_250);
    // The next statement fills it without being told where it goes.
    let mut later = s.clone();
    later.accounts[0].values.push(("2026-11-16".into(), flex::Dec::parse("28000").unwrap()));
    let b = apply(b, &later, &[]).unwrap();
    assert_eq!(investments::find(&b, "new-account").unwrap().total, 2_800_000);
    assert_eq!(b.accounts.len(), 3);
    // Moved to another account, the first stops following the broker, and the value lines it
    // had go with it, so the broker's value is counted once.
    let b = apply(b, &later, &[link("second", true)]).unwrap();
    assert!(investments::find(&b, "new-account").is_none());
    assert!(!b.transactions.iter().any(|t| investments::generated(t) && t.account_id == "new-account"));
    let held = |b: &Budget, account: &str| b.transactions.iter().filter(|t| t.account_id == account).map(|t| t.amount).sum::<i64>();
    assert_eq!((held(&b, "new-account"), held(&b, "second")), (0, 2_800_000));
    assert_eq!(investments::find(&b, "second").unwrap().source_account, "U1234567");
    assert!(apply(budget(), &s, &[Link { source: "U999".into(), ..link("x", true) }]).unwrap_err().contains("U999"));
    assert!(apply(budget(), &s, &[link("cash", false)]).unwrap_err().contains("tracking account"));
}

#[test]
fn the_review_says_what_is_linked_what_needs_a_rate_and_which_deposits_are_new() {
    let s = statement();
    let unlinked = review(&budget(), &s);
    assert_eq!(unlinked.len(), 1);
    let first = &unlinked[0];
    assert_eq!((first.found.id.as_str(), first.found.alias.as_str(), first.found.currency.as_str(), first.found.value, first.found.as_of.as_str(), first.found.positions),
        ("U1234567", "Long term", "USD", 2_701_250, "2026-11-13", 3));
    assert!(first.account_id.is_none() && first.needs_rate.is_none() && first.rows.is_empty() && first.error.is_none());
    // A budget in a currency the statement has no rate for says so before anything is linked.
    let mut yen = budget();
    yen.currency = "JPY".into();
    assert_eq!(review(&yen, &s)[0].needs_rate, Some(("USD".into(), "JPY".into())));
    let with_rate = apply(yen, &s, &[Link { rate: Some("150".into()), ..link("broker", true) }]).unwrap();
    assert!(review(&with_rate, &s)[0].needs_rate.is_none());

    let b = apply(budget(), &s, &[link("broker", true)]).unwrap();
    let linked = review(&b, &s);
    assert_eq!(linked[0].account_id.as_deref(), Some("broker"));
    let rows = &linked[0].rows;
    assert_eq!(rows.iter().map(|r| (r.date.as_str(), r.amount, r.duplicate)).collect::<Vec<_>>(), vec![("2026-10-05", 100_000, false), ("2026-11-02", 40_000, false)]);
    // Importing them takes them out of the months' market changes; the value stays the broker's.
    let change = |b: &Budget, month: &str| b.transactions.iter().find(|t| t.id == format!("investment-change:broker:{month}")).unwrap().amount;
    // The gain is the same before and after; what changes is the stand-in for the deposits.
    let standing = |b: &Budget| b.transactions.iter().filter(|t| t.id.starts_with("investment-moved:broker:")).map(|t| t.amount).sum::<i64>();
    assert_eq!((change(&b, "2026-10"), change(&b, "2026-11"), standing(&b)), (28_000, 111_250, 140_000));
    let choices: Vec<Choice> = rows.iter().cloned().map(|row| Choice { row, action: "new".into(), match_id: None }).collect();
    let mut imported = bank_import::apply(b, "broker", choices).unwrap();
    investments::revalue(&mut imported, "broker").unwrap();
    crate::domain::validate(&imported).unwrap();
    assert_eq!((change(&imported, "2026-10"), change(&imported, "2026-11"), standing(&imported)), (28_000, 111_250, 0));
    let balance: i64 = imported.transactions.iter().filter(|t| t.account_id == "broker").map(|t| t.amount).sum();
    assert_eq!(balance, 2_701_250);
    // Seen again, they're known by Interactive Brokers' ids.
    assert!(review(&imported, &s)[0].rows.iter().all(|r| r.duplicate));
    // The phones' compact review says the same of the same rows.
    let fresh = apply(budget(), &s, &[link("broker", true)]).unwrap();
    let compact = review_compact(&fresh, &s);
    assert_eq!(compact[0].rows.iter().map(|r| (r.row.date.as_str(), r.row.amount, r.row.duplicate)).collect::<Vec<_>>(), vec![("2026-10-05", 100_000, false), ("2026-11-02", 40_000, false)]);
    assert!(compact[0].candidate_sets.as_ref().is_some_and(|sets| compact[0].rows.iter().all(|r| r.candidate_set < sets.len())));
    assert!(review_compact(&imported, &s)[0].rows.iter().all(|r| r.row.duplicate));
}

#[test]
fn a_statement_file_stands_in_for_a_fetch_and_the_status_never_holds_the_token() {
    let state = Ibkr::new();
    assert!(ibkr_status(&state).accounts.is_empty());
    assert!(ibkr_file(&state, "not a statement".into()).unwrap_err().contains("Flex"));
    let status = ibkr_file(&state, FLEX_SAMPLE.into()).unwrap();
    assert!(status.from_file && status.fetched_at.is_some());
    assert_eq!(status.accounts.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["U1234567"]);
    let shown = serde_json::to_string(&status).unwrap() + &state.snapshot().to_string();
    assert!(!shown.contains("token") && !shown.contains("Someone"));
}

#[test]
fn without_a_statement_the_review_is_what_the_budget_keeps() {
    let plain = |b: &Budget| kept(b, |account, raw| bank_import::review_near(b, account, raw).map(|rows| (rows, None)), &[]);
    // Nothing from Interactive Brokers in the budget: there is nothing to show, as before.
    assert!(plain(&budget()).is_err_and(|e| e.contains("Sync with Interactive Brokers first")));
    // A device that only has the budget (another one synced) reviews the same rows.
    let b = apply(budget(), &statement(), &[link("broker", true)]).unwrap();
    let review = plain(&b).unwrap();
    let a = &review.accounts[0];
    assert_eq!((a.found.id.as_str(), a.found.currency.as_str(), a.found.value, a.found.as_of.as_str(), a.found.positions, a.account_id.as_deref()),
        ("U1234567", "USD", 2_701_250, "2026-11-13", 3, Some("broker")));
    assert_eq!(a.rows.iter().map(|r| (r.date.as_str(), r.amount, r.duplicate)).collect::<Vec<_>>(), vec![("2026-10-05", 100_000, false), ("2026-11-02", 40_000, false)]);
    assert!(a.error.is_none() && a.needs_rate.is_none() && a.kept && !review.fetched_at.is_empty());
    // Beside a statement that lists the account, there is nothing more to add for it.
    assert!(kept(&b, |account, raw| bank_import::review_near(&b, account, raw).map(|rows| (rows, None)), &["U1234567"]).is_err());
    // A statement at hand from another login doesn't hide it: the phones get its rows after
    // the statement's own accounts.
    let last = Fetched { statement: flex::parse(&FLEX_SAMPLE.replace("U1234567", "U7654321")).unwrap(), at: "now".into(), file: false };
    let both = reviewed(&b, Some(&last), |account, raw| {
        bank_import::review_compact_near(&b, account, raw).map(|review| (review.items, Some(review.candidate_sets)))
    })
    .unwrap();
    assert_eq!(both.accounts.iter().map(|a| (a.found.id.as_str(), a.kept, a.rows.len())).collect::<Vec<_>>(), vec![("U7654321", false, 0), ("U1234567", true, 2)]);
    // And brings them in, with no statement either.
    let choices: Vec<Choice> = a.rows.iter().cloned().map(|row| Choice { row, action: "new".into(), match_id: None }).collect();
    let imported = investments::import(b, "broker", choices).unwrap();
    assert!(plain(&imported).unwrap().accounts[0].rows.iter().all(|r| r.duplicate));
    assert!(!imported.transactions.iter().any(|t| t.id.starts_with("investment-moved:")));
}

fn login(token: &str, query: &str, accounts: &[&str]) -> Login {
    Login { credential: Credential::parse(token, query).unwrap(), accounts: accounts.iter().map(|a| a.to_string()).collect() }
}

#[test]
fn several_logins_are_kept_a_line_each_and_the_one_from_before_still_reads() {
    let first = login("12345678901234567890", "987654", &["U1234567"]);
    let second = login("ABCDEFGHIJKLMNOPQRST", "111222", &["U7654321", "U7654322"]);
    let stored = Login::stored_all(&[first.clone(), second.clone()]);
    assert_eq!(stored, "12345678901234567890\t987654\tU1234567\nABCDEFGHIJKLMNOPQRST\t111222\tU7654321,U7654322");
    assert_eq!(Login::parse_all(&stored), vec![first.clone(), second.clone()]);
    // The one login kept before there could be several: its token and query on two lines.
    assert_eq!(Login::parse_all("12345678901234567890\n987654"), vec![login("12345678901234567890", "987654", &[])]);
    assert!(Login::parse_all("").is_empty() && Login::parse_all("only one line").is_empty());
    // Another login goes after the ones here.
    let third = login("ZZZZZZZZZZZZZZZZZZZZ", "333444", &["U9999999"]);
    assert_eq!(with_login(vec![first.clone(), second.clone()], third.clone()), vec![first.clone(), second.clone(), third]);
    // A new token for a query already here takes its place, where it stood.
    let renewed = login("NEWTOKENNEWTOKEN1234", "987654", &["U1234567"]);
    assert_eq!(with_login(vec![first.clone(), second.clone()], renewed.clone()), vec![renewed, second.clone()]);
    // So does another query that brings an account a login here already brings.
    let requeried = login("NEWTOKENNEWTOKEN1234", "555666", &["U7654322"]);
    assert_eq!(with_login(vec![first.clone(), second], requeried.clone()), vec![first, requeried]);
}

#[test]
fn every_login_is_asked_and_their_accounts_come_in_together() {
    let other = FLEX_SAMPLE.replace("U1234567", "U7654321");
    let expired = "<FlexStatementResponse><Status>Fail</Status><ErrorCode>1012</ErrorCode><ErrorMessage>Token has expired.</ErrorMessage></FlexStatementResponse>";
    // The stand-in answers each token with its own statement, and the second one's token
    // stops working after the first round.
    let fake = serve(move |request, count| match query(request)["t"].as_str() {
        "12345678901234567890" => FLEX_SAMPLE.into(),
        _ if count <= 2 => other.clone(),
        _ => expired.into(),
    });
    let logins = vec![login("12345678901234567890", "987654", &[]), login("ABCDEFGHIJKLMNOPQRST", "111222", &[])];
    let state = Ibkr::new();
    let fetch = |logins: Vec<Login>| run(fetch_all(&http().unwrap(), &fake.url, logins, Duration::from_millis(3)));
    let held = |state: &Ibkr| state.last.lock().unwrap().as_ref().map(|l| ids(&l.statement)).unwrap_or_default();

    // Both arrive: one statement with both accounts, and each login knows which it brought.
    let logins = state.gather(fetch(logins)).unwrap();
    assert_eq!(logins.iter().map(|l| l.accounts.join(",")).collect::<Vec<_>>(), vec!["U1234567", "U7654321"]);
    assert_eq!(held(&state), vec!["U1234567", "U7654321"]);
    assert!(state.problems.lock().unwrap().is_empty());
    let asked: Vec<String> = fake.seen.lock().unwrap().iter().map(|(request, _)| query(request)["q"].clone()).collect();
    assert_eq!(asked.len(), 2);
    assert!(asked.contains(&"987654".to_string()) && asked.contains(&"111222".to_string()));

    // One fails: the other still comes in, the failure is named by its query, and the account
    // the failed login brought before stays as it was.
    let logins = state.gather(fetch(logins)).unwrap();
    assert_eq!(held(&state), vec!["U1234567", "U7654321"]);
    let problems = state.problems.lock().unwrap().clone();
    assert_eq!(problems.keys().collect::<Vec<_>>(), vec!["111222"]);
    assert!(problems["111222"].contains("expired"));
    assert_eq!(logins[1].accounts, vec!["U7654321"]);

    // Removing that login takes its account out of the statement and leaves the other's.
    state.replace(&logins[1].accounts, None).unwrap();
    assert_eq!(held(&state), vec!["U1234567"]);

    // When every login fails nothing changes, and the refusal is the answer.
    let all = state.gather(fetch(vec![logins[1].clone()])).unwrap_err();
    assert!(all.contains("expired"));
    assert_eq!(held(&state), vec!["U1234567"]);

    // A login connected later adds its accounts; one that renews a login replaces them.
    state.replace(&[], Some(flex::parse(&FLEX_SAMPLE.replace("U1234567", "U5550001")).unwrap())).unwrap();
    assert_eq!(held(&state), vec!["U1234567", "U5550001"]);
    state.replace(&["U5550001".to_string()], Some(statement())).unwrap();
    assert_eq!(held(&state), vec!["U1234567"]);
    // A statement from a file isn't a login's: one from the web service replaces it whole.
    ibkr_file(&state, FLEX_SAMPLE.replace("U1234567", "U5550002")).unwrap();
    state.replace(&[], Some(statement())).unwrap();
    assert_eq!(held(&state), vec!["U1234567"]);
}
