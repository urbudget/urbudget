//! Interactive Brokers, through its own Flex Web Service
//! (https://www.interactivebrokers.com/docs/web-api). The user makes a Flex Query and a token in
//! Client Portal and pastes the two here; from then on a sync is two plain HTTPS requests to
//! Interactive Brokers and nothing in between: one asks for the statement, one collects it. The
//! token only reads statements. It is a password all the same, so it lives with the app's other
//! secrets (vault.rs) on this device and never in the budget, which encrypted sync uploads.
//!
//! What a sync brings (what each account holds, its value day by day, its deposits) goes into
//! the budget through `investments`, so devices without the token show the same holdings. A
//! statement saved from Client Portal as a file reads the same way, for someone who would rather
//! not turn the web service on.
//!
//! Someone with more than one login at Interactive Brokers connects each the same way: a query
//! and a token of its own. A sync asks them all at once and puts their accounts together, so
//! the rest of the app sees one statement. A login that fails (an expired token, say) is named
//! with what went wrong, and the others still come in.
//!
//! Interactive Brokers allows ten requests a minute for a token, and makes the day's statement
//! once, overnight, so the app asks only when the user presses Sync.
use crate::t;
use crate::{
    bank_import::{self, Row},
    domain::Budget,
    flex, investments,
    patch::Patch,
    vault, Database,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::Mutex, time::Duration};

const SERVICE: &str = "com.localbudget.desktop.ibkr";
const HOST: &str = "https://ndcdyn.interactivebrokers.com/AccountManagement/FlexWebService";
/// Interactive Brokers' own page on turning the Flex Web Service on and making a token.
const GUIDE: &str = "https://www.ibkrguides.com/clientportal/performanceandstatements/flex-web-service.htm";
/// How often the statement is asked for while Interactive Brokers prepares it, and how long
/// between: with the first request, under its ten a minute.
const POLLS: u32 = 9;
const PAUSE: Duration = Duration::from_secs(6);
/// How many logins one device keeps: more than anyone has, few enough to ask all at once.
const MAX_LOGINS: usize = 6;
/// "Not ready yet" answers: asking again is what Interactive Brokers says to do.
const NOT_READY: [u32; 5] = [1001, 1004, 1009, 1019, 1021];

fn offline() -> String {
    t!("Couldn't reach Interactive Brokers. Check your connection and try again.")
}
fn not_connected() -> String {
    t!("Connect Interactive Brokers first, in Settings › Import.")
}
fn nothing_fetched() -> String {
    t!("Sync with Interactive Brokers first.")
}
fn stopped() -> String {
    t!("The Interactive Brokers sync stopped. Try again.")
}

/// The credential's name. A QA run (debug builds on a scratch data folder) keeps its own, so a
/// demo connection never replaces a real one.
fn service() -> String {
    if cfg!(debug_assertions) && std::env::var_os("LOCAL_BUDGET_DATA_DIR").is_some() {
        format!("{SERVICE}.qa")
    } else {
        SERVICE.into()
    }
}
/// Where the web service is: Interactive Brokers, or in a debug build a stand-in on this
/// computer named by URBUDGET_IBKR_URL.
fn host() -> String {
    if cfg!(debug_assertions) {
        if let Some(url) = std::env::var("URBUDGET_IBKR_URL").ok().filter(|u| u.starts_with("http://127.0.0.1:") || u.starts_with("http://localhost:")) {
            return url;
        }
    }
    HOST.into()
}

/// What the user pastes: the token and the Flex Query's id.
#[derive(Clone, Debug, PartialEq)]
pub struct Credential {
    token: String,
    query: String,
}
impl Credential {
    /// Reads the two as pasted, with any spaces or line breaks from copying, in either box: the
    /// query id is the short number and the token the long one, so swapped ones are put right.
    pub fn parse(token: &str, query: &str) -> Result<Credential, String> {
        let tidy = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        let (mut token, mut query) = (tidy(token), tidy(query));
        if query.len() > token.len() {
            std::mem::swap(&mut token, &mut query);
        }
        if query.is_empty() || query.len() > 12 || !query.bytes().all(|b| b.is_ascii_digit()) {
            return Err(t!("That isn't a Flex Query ID. It's the number beside your query's name in Client Portal, under Performance & Reports › Flex Queries."));
        }
        if token.len() < 12 || token.len() > 80 || !token.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(t!("That isn't a Flex Web Service token. Copy the whole token from Client Portal, under Flex Web Service Configuration, and paste it again."));
        }
        Ok(Credential { token, query })
    }
}

/// One login at Interactive Brokers kept on this device: its query and token, and the accounts
/// its last statement named, so a login is known by what it brings without asking again.
#[derive(Clone, Debug, PartialEq)]
struct Login {
    credential: Credential,
    accounts: Vec<String>,
}
impl Login {
    /// Every login in what the vault holds: a line each of token, query id and account ids, with
    /// tabs between and commas among the accounts. Before there could be several, the one login
    /// was kept as the token and the query id on two lines, which still reads.
    fn parse_all(stored: &str) -> Vec<Login> {
        if !stored.contains('\t') {
            return stored
                .split_once('\n')
                .filter(|(token, query)| !token.trim().is_empty() && !query.trim().is_empty())
                .map(|(token, query)| vec![Login { credential: Credential { token: token.trim().into(), query: query.trim().into() }, accounts: vec![] }])
                .unwrap_or_default();
        }
        stored
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\t');
                let (token, query) = (parts.next()?.trim(), parts.next()?.trim());
                (!token.is_empty() && !query.is_empty()).then(|| Login {
                    credential: Credential { token: token.into(), query: query.into() },
                    accounts: parts.next().unwrap_or_default().split(',').map(str::trim).filter(|a| !a.is_empty()).map(str::to_owned).collect(),
                })
            })
            .collect()
    }
    fn stored_all(logins: &[Login]) -> String {
        let plain = |id: &str| id.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>();
        logins
            .iter()
            .map(|l| format!("{}\t{}\t{}", l.credential.token, l.credential.query, l.accounts.iter().map(|a| plain(a)).collect::<Vec<_>>().join(",")))
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn all() -> Vec<Login> {
        vault::read(&service()).map(|stored| Login::parse_all(&stored)).unwrap_or_default()
    }
    fn save_all(logins: &[Login]) -> Result<(), String> {
        if logins.is_empty() {
            return vault::delete(&service()).map(|_| ());
        }
        vault::save(&service(), &Login::stored_all(logins))
    }
}
/// The logins once a new one is kept. It takes the place of any with the same query, and of any
/// whose accounts its statement also names (a new token, or a new query, for a login already
/// here), where the first of those stood; otherwise it goes last.
fn with_login(mut logins: Vec<Login>, new: Login) -> Vec<Login> {
    let same = |l: &Login| l.credential.query == new.credential.query || l.accounts.iter().any(|a| new.accounts.contains(a));
    let at = logins.iter().position(same).unwrap_or(logins.len());
    logins.retain(|l| !same(l));
    logins.insert(at.min(logins.len()), new);
    logins
}
fn ids(statement: &flex::Statement) -> Vec<String> {
    statement.accounts.iter().map(|a| a.id.clone()).collect()
}

pub fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        // A redirect could take the token somewhere else.
        .redirect(reqwest::redirect::Policy::none())
        .https_only(!cfg!(any(test, debug_assertions)))
        // Interactive Brokers refuses requests that don't say what sends them.
        .user_agent(concat!("UrBudget/", env!("CARGO_PKG_VERSION")))
        // Requests come seconds apart while a statement is prepared. A connection kept open
        // between them may have been closed at the other end by then, so each gets its own.
        .pool_max_idle_per_host(0)
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|_| t!("Couldn't start the Interactive Brokers sync."))
}

/// What the web service answered.
#[derive(Debug, PartialEq)]
enum Answer {
    /// The statement itself.
    Statement(String),
    /// The statement is being made: collect it with this code.
    Reference(String),
    Problem { code: u32, message: String },
}
fn element<'a>(body: &'a str, name: &str) -> Option<&'a str> {
    let start = body.find(&format!("<{name}>"))? + name.len() + 2;
    let end = body[start..].find(&format!("</{name}>"))?;
    Some(body[start..start + end].trim())
}
fn answer(body: String) -> Answer {
    if flex::sniff(&body) {
        return Answer::Statement(body);
    }
    let code = element(&body, "ErrorCode").and_then(|c| c.parse().ok());
    match (element(&body, "ReferenceCode").filter(|r| !r.is_empty()), code) {
        (Some(reference), None) => Answer::Reference(reference.to_owned()),
        (_, code) => Answer::Problem {
            code: code.unwrap_or(0),
            message: flex::clean(element(&body, "ErrorMessage").unwrap_or_default()),
        },
    }
}
/// What to tell the user about a refusal, by Interactive Brokers' error code.
fn problem(code: u32, message: &str) -> String {
    match code {
        1012 => t!("Your Flex Web Service token has expired. Make a new one in Client Portal, then connect again here."),
        1015 => t!("Interactive Brokers doesn't know this token. Copy it again from Client Portal, under Flex Web Service Configuration."),
        1014 => t!("Interactive Brokers doesn't know this Flex Query ID. Copy the number beside your query's name in Client Portal."),
        1011 => t!("The Flex Web Service is turned off for this account. Turn it on in Client Portal, under Flex Web Service Configuration."),
        1013 => t!("This token only works from certain internet addresses. Make one without an IP address restriction in Client Portal."),
        1018 | 1025 => t!("Interactive Brokers has been asked too often. Wait a few minutes, then sync again."),
        1003 | 1005..=1008 => t!("Interactive Brokers hasn't finished today's statement yet. Sync again later."),
        _ if NOT_READY.contains(&code) => t!("Interactive Brokers is still preparing the statement. Try again in a few minutes."),
        _ if message.is_empty() => t!("Interactive Brokers sent an answer UrBudget couldn't read. Nothing was changed."),
        _ => t!("Interactive Brokers couldn't send the statement: {message} (code {code})", message, code),
    }
}
async fn get(http: &reqwest::Client, url: String, query: &[(&str, &str)]) -> Result<Answer, String> {
    // One that fails before an answer comes is tried once more: reading a statement changes
    // nothing at Interactive Brokers, so asking again is safe.
    let send = || http.get(url.as_str()).query(query).send();
    let response = match send().await {
        Ok(response) => response,
        Err(_) => send().await.map_err(|_| offline())?,
    };
    match response.status().as_u16() {
        200..=299 => Ok(answer(response.text().await.map_err(|_| offline())?)),
        n => Err(t!("Interactive Brokers couldn't send the statement (HTTP {n}). Try again later.", n)),
    }
}
/// In a debug build, keeps a copy of the statement as it arrived where URBUDGET_IBKR_DUMP says,
/// for working out why one reads wrongly. A statement is the user's own financial record, so a
/// release build never writes one anywhere.
fn keep_copy(statement: &str) {
    if cfg!(debug_assertions) {
        if let Some(path) = std::env::var_os("URBUDGET_IBKR_DUMP") {
            let _ = std::fs::write(path, statement);
        }
    }
}
/// Asks for the query's statement and collects it once it's ready.
pub async fn download(http: &reqwest::Client, host: &str, credential: &Credential, pause: Duration) -> Result<String, String> {
    let ask = [("t", credential.token.as_str()), ("q", credential.query.as_str()), ("v", "3")];
    let reference = match get(http, format!("{host}/SendRequest"), &ask).await? {
        Answer::Statement(statement) => return Ok(statement),
        Answer::Reference(reference) => reference,
        Answer::Problem { code, message } => return Err(problem(code, &message)),
    };
    let collect = [("t", credential.token.as_str()), ("q", reference.as_str()), ("v", "3")];
    for attempt in 0..POLLS {
        tokio::time::sleep(if attempt == 0 { pause / 3 } else { pause }).await;
        match get(http, format!("{host}/GetStatement"), &collect).await? {
            Answer::Statement(statement) => return Ok(statement),
            Answer::Problem { code, .. } if NOT_READY.contains(&code) => {}
            Answer::Problem { code, message } => return Err(problem(code, &message)),
            Answer::Reference(_) => {}
        }
    }
    Err(problem(1019, ""))
}

/// An account in the last statement, for choosing which budget account it fills.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    /// Interactive Brokers' account id (U1234567).
    pub id: String,
    /// The alias the user gave it there, or empty.
    pub alias: String,
    /// The account's own currency, or empty when the statement doesn't say.
    pub currency: String,
    /// What it's worth in that currency, in hundredths.
    pub value: i64,
    /// The day the statement's holdings are from.
    pub as_of: String,
    pub positions: usize,
    /// Sections the Flex Query leaves out, by the name Client Portal shows.
    pub missing: Vec<String>,
    /// The statement carries the days' exchange rates (Include Currency Rates is on).
    pub rates: bool,
    /// When Interactive Brokers made the statement, on its own clock, or empty.
    pub generated: String,
}
fn found(statement: &flex::Statement) -> Vec<Found> {
    statement
        .accounts
        .iter()
        .map(|a| Found {
            id: a.id.clone(),
            alias: a.alias.clone(),
            currency: a.base.clone(),
            value: investments::value_in_base(a).cents(),
            as_of: investments::as_of(a),
            positions: a.positions.len(),
            missing: a.missing.iter().map(|m| (*m).to_owned()).collect(),
            rates: !a.rates.is_empty(),
            generated: a.generated.clone(),
        })
        .collect()
}

/// Asks for every login's statement at once and reads each as it comes: a login with its
/// statement, or with what went wrong.
async fn fetch_all(http: &reqwest::Client, host: &str, logins: Vec<Login>, pause: Duration) -> Vec<(Login, Result<flex::Statement, String>)> {
    async fn one(http: reqwest::Client, host: String, credential: Credential, pause: Duration) -> Result<flex::Statement, String> {
        let body = download(&http, &host, &credential, pause).await?;
        keep_copy(&body);
        flex::parse(&body)
    }
    // Each login has its own allowance at Interactive Brokers, so they needn't wait their turn.
    if logins.len() > 1 && tokio::runtime::Handle::try_current().is_ok() {
        let tasks: Vec<_> = logins.iter().map(|l| tokio::spawn(one(http.clone(), host.to_owned(), l.credential.clone(), pause))).collect();
        let mut out = Vec::with_capacity(logins.len());
        for (login, task) in logins.into_iter().zip(tasks) {
            out.push((login, task.await.unwrap_or_else(|_| Err(stopped()))));
        }
        return out;
    }
    let mut out = Vec::with_capacity(logins.len());
    for login in logins {
        let result = one(http.clone(), host.to_owned(), login.credential.clone(), pause).await;
        out.push((login, result));
    }
    out
}

struct Fetched {
    statement: flex::Statement,
    at: String,
    /// From a file the user chose rather than the web service.
    file: bool,
}
pub struct Ibkr {
    /// The last statement, so accounts can be chosen and deposits reviewed without asking
    /// Interactive Brokers again. With several logins it holds all their accounts together.
    last: Mutex<Option<Fetched>>,
    /// What went wrong for a login the last time it was asked, by its query id.
    problems: Mutex<HashMap<String, String>>,
}
impl Default for Ibkr {
    fn default() -> Self {
        Ibkr::new()
    }
}
impl Ibkr {
    pub fn new() -> Self {
        Ibkr { last: Mutex::new(None), problems: Mutex::new(HashMap::new()) }
    }
    fn keep(&self, statement: flex::Statement, file: bool) -> Result<(), String> {
        *self.last.lock().map_err(|_| stopped())? = Some(Fetched { statement, at: crate::domain::stamp(Utc::now()), file });
        Ok(())
    }
    /// Takes these accounts out of the last statement, and then adds `more`'s: a login's own
    /// accounts give way to its new statement, and leave with it when it is removed. A statement
    /// from a file isn't a login's, so one that arrives from the web service replaces it whole.
    fn replace(&self, gone: &[String], more: Option<flex::Statement>) -> Result<(), String> {
        let mut last = self.last.lock().map_err(|_| stopped())?;
        let mut accounts = match last.take() {
            Some(before) if !before.file => before.statement.accounts,
            Some(before) if more.is_none() => {
                *last = Some(before);
                return Ok(());
            }
            _ => vec![],
        };
        let incoming = more.map(|statement| statement.accounts).unwrap_or_default();
        accounts.retain(|a| !gone.contains(&a.id) && !incoming.iter().any(|new| new.id == a.id));
        accounts.extend(incoming);
        if !accounts.is_empty() {
            *last = Some(Fetched { statement: flex::Statement { accounts }, at: crate::domain::stamp(Utc::now()), file: false });
        }
        Ok(())
    }
    /// Puts what every login's fetch came to together: the statements that arrived as one, each
    /// account once, and for a login that failed, why, with the accounts it brought last time
    /// left as they were. Gives the logins back with the accounts each one named, to be kept.
    /// When none arrived nothing changes and the first refusal is the answer.
    fn gather(&self, done: Vec<(Login, Result<flex::Statement, String>)>) -> Result<Vec<Login>, String> {
        let mut problems = HashMap::new();
        let mut logins = Vec::with_capacity(done.len());
        let mut arrived: Vec<flex::Account> = vec![];
        let mut failed: Vec<String> = vec![];
        let mut first = None;
        for (mut login, result) in done {
            match result {
                Ok(statement) => {
                    login.accounts = ids(&statement);
                    arrived.extend(statement.accounts.into_iter().filter(|a| !arrived.iter().any(|known| known.id == a.id)).collect::<Vec<_>>());
                }
                Err(e) => {
                    failed.extend(login.accounts.iter().cloned());
                    first.get_or_insert_with(|| e.clone());
                    problems.insert(login.credential.query.clone(), e);
                }
            }
            logins.push(login);
        }
        if let Ok(mut kept) = self.problems.lock() {
            *kept = problems;
        }
        if arrived.is_empty() {
            return Err(first.unwrap_or_else(not_connected));
        }
        let mut last = self.last.lock().map_err(|_| stopped())?;
        // What a failed login brought before stays, so its accounts don't look unlinked.
        let stale = match last.take() {
            Some(before) if !before.file => before.statement.accounts.into_iter().filter(|a| failed.contains(&a.id) && !arrived.iter().any(|new| new.id == a.id)).collect(),
            _ => vec![],
        };
        arrived.extend::<Vec<_>>(stale);
        *last = Some(Fetched { statement: flex::Statement { accounts: arrived }, at: crate::domain::stamp(Utc::now()), file: false });
        Ok(logins)
    }
    fn status(&self) -> Status {
        let last = self.last.lock().ok();
        let last = last.as_ref().and_then(|l| l.as_ref());
        let problems = self.problems.lock().ok();
        let logins: Vec<LoginStatus> = Login::all()
            .into_iter()
            .map(|l| LoginStatus {
                error: problems.as_ref().and_then(|p| p.get(&l.credential.query).cloned()),
                query: l.credential.query,
                accounts: l.accounts,
            })
            .collect();
        Status {
            store: vault::store(),
            connected: !logins.is_empty(),
            logins,
            accounts: last.map(|l| found(&l.statement)).unwrap_or_default(),
            fetched_at: last.map(|l| l.at.clone()),
            from_file: last.is_some_and(|l| l.file),
        }
    }
    /// What the assistant may know about the connection: whether there is one, and the accounts
    /// the last statement named. Never the token.
    pub fn snapshot(&self) -> Value {
        let s = self.status();
        serde_json::json!({
            "available": s.store.is_some(),
            "connected": s.connected,
            "logins": s.logins.len(),
            "fetchedAt": s.fetched_at,
            "accounts": s.accounts,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// What protects the token (`vault::store()`), or None where the connection isn't offered.
    pub store: Option<&'static str>,
    pub connected: bool,
    /// The logins kept on this device, in the order they were connected. Never their tokens.
    pub logins: Vec<LoginStatus>,
    /// The accounts in the last statement this session; empty until there is one.
    pub accounts: Vec<Found>,
    pub fetched_at: Option<String>,
    pub from_file: bool,
}

/// A login kept on this device, as the screens may know it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStatus {
    /// Its Flex Query's id, which names it: the number means nothing without the token.
    pub query: String,
    /// The Interactive Brokers accounts its last statement named.
    pub accounts: Vec<String>,
    /// Why it couldn't be fetched the last time it was asked, if it couldn't.
    pub error: Option<String>,
}

pub fn ibkr_status(state: &Ibkr) -> Status {
    state.status()
}
/// Checks the token and query by fetching the statement, and only then keeps them: as a login of
/// their own beside any already here, or in place of the one they renew (see `with_login`).
pub async fn ibkr_connect(state: &Ibkr, token: String, query: String) -> Result<Status, String> {
    vault::store().ok_or_else(|| t!("The Interactive Brokers connection needs a system credential store, which this computer doesn't have. You can still bring in a statement file."))?;
    let credential = Credential::parse(&token, &query)?;
    let before = Login::all();
    let body = download(&http()?, &host(), &credential, PAUSE).await?;
    keep_copy(&body);
    let statement = flex::parse(&body)?;
    let login = Login { credential, accounts: ids(&statement) };
    let logins = with_login(before.clone(), login.clone());
    if logins.len() > MAX_LOGINS {
        return Err(t!("UrBudget keeps up to {count} Interactive Brokers logins on a device. Remove one, then connect this one.", count = MAX_LOGINS));
    }
    Login::save_all(&logins).map_err(|_| t!("{store} couldn't keep the connection. Try connecting again.", store = vault::store_name()))?;
    // The logins it took the place of take their accounts with them.
    let gone: Vec<String> = before.iter().filter(|l| !logins.contains(l)).flat_map(|l| l.accounts.iter().cloned()).collect();
    state.replace(&gone, Some(statement))?;
    if let Ok(mut problems) = state.problems.lock() {
        problems.remove(&login.credential.query);
    }
    Ok(state.status())
}
/// Fetches every login's statement again with the tokens that are kept.
pub async fn ibkr_fetch(state: &Ibkr) -> Result<Status, String> {
    let before = Login::all();
    if before.is_empty() {
        return Err(not_connected());
    }
    let logins = state.gather(fetch_all(&http()?, &host(), before.clone(), PAUSE).await)?;
    // A login's accounts are only written again when its statement named different ones.
    if logins != before {
        let _ = Login::save_all(&logins);
    }
    Ok(state.status())
}
/// Reads a statement the user saved from Client Portal, in place of a fetch.
pub fn ibkr_file(state: &Ibkr, content: String) -> Result<Status, String> {
    state.keep(flex::parse(&content)?, true)?;
    Ok(state.status())
}
/// Forgets one login's token, by its query id, or every login's when none is named. Accounts
/// keep what they hold; a token stays valid at Interactive Brokers until it's turned off or
/// replaced there.
pub fn ibkr_disconnect(state: &Ibkr, query: Option<String>) -> Result<Status, String> {
    let before = Login::all();
    let query = query.map(|q| q.trim().to_owned()).filter(|q| !q.is_empty());
    let (gone, kept): (Vec<Login>, Vec<Login>) = before.into_iter().partition(|l| query.as_ref().is_none_or(|q| *q == l.credential.query));
    if kept.is_empty() {
        vault::delete(&service())?;
        if let Ok(mut last) = state.last.lock() {
            *last = None;
        }
    } else {
        Login::save_all(&kept)?;
        state.replace(&gone.iter().flat_map(|l| l.accounts.iter().cloned()).collect::<Vec<_>>(), None)?;
    }
    if let Ok(mut problems) = state.problems.lock() {
        problems.retain(|q, _| kept.iter().any(|l| l.credential.query == *q));
    }
    Ok(state.status())
}

/// Which budget account an Interactive Brokers account fills.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    /// The Interactive Brokers account id.
    pub source: String,
    /// The budget account: an existing tracking account's id, or the id for a new one.
    pub account: String,
    /// Add `account` as a new Investment account.
    #[serde(default)]
    pub create: bool,
    /// The new account's name; left out, the alias at Interactive Brokers or its last digits.
    pub name: Option<String>,
    /// One unit of the account's currency in the budget's, when the statement has no rate.
    pub rate: Option<String>,
}

/// Brings the last statement into the budget: every account already linked, and the ones in
/// `links`. An account that can't be brought in (it needs an exchange rate, say) is left as it
/// was, and `ibkr_review` says why.
pub fn ibkr_apply(db: &Database, state: &Ibkr, patch: Option<Patch>, links: Vec<Link>) -> Result<Patch, String> {
    let last = state.last.lock().map_err(|_| stopped())?;
    let statement = &last.as_ref().ok_or_else(nothing_fetched)?.statement;
    db.change(patch, |b| apply(b, statement, &links))
}
fn apply(mut b: Budget, statement: &flex::Statement, links: &[Link]) -> Result<Budget, String> {
    for link in links {
        let source = statement.accounts.iter().find(|a| a.id == link.source).ok_or_else(|| {
            t!("The statement doesn't include the account {account}.", account = link.source)
        })?;
        let mut next = b.clone();
        if link.create && !next.accounts.iter().any(|a| a.id == link.account) {
            next.accounts.push(investments::new_account(&next, &link.account, source, link.name.as_deref()));
        }
        // Moving a broker account to another budget account releases the one it filled: its
        // value lines go with the broker, so it isn't counted in both.
        for held in investments::read(&next).iter().filter(|h| h.source_account == source.id && h.account_id != link.account) {
            investments::release(&mut next, &held.account_id, &link.account);
        }
        b = investments::apply(next, &link.account, source, link.rate.as_deref())?;
    }
    for held in investments::read(&b) {
        if held.source != "ibkr" || links.iter().any(|l| l.account == held.account_id) {
            continue;
        }
        if let Some(source) = statement.accounts.iter().find(|a| a.id == held.source_account) {
            if let Ok(next) = investments::apply(b.clone(), &held.account_id, source, None) {
                b = next;
            }
        }
    }
    Ok(b)
}

/// One Interactive Brokers account in the last statement, against the budget.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountReview<R = Row> {
    #[serde(flatten)]
    pub found: Found,
    /// The budget account it fills, if any.
    pub account_id: Option<String>,
    /// The currencies an exchange rate is needed between before it can be brought in.
    pub needs_rate: Option<(String, String)>,
    /// Its deposits and withdrawals, to review like a bank's rows.
    pub rows: Vec<R>,
    /// The rows' matching choices, shared between rows, in the phones' compact review.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_sets: Option<Vec<bank_import::CandidateSet>>,
    /// Why it couldn't be brought in.
    pub error: Option<String>,
    /// Read from what the budget keeps, not from a statement this session: nothing was fetched
    /// here yet, or its login is on another device.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub kept: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review<R = Row> {
    pub accounts: Vec<AccountReview<R>>,
    pub fetched_at: String,
}
/// The last statement's accounts against the budget: which are linked, which need an exchange
/// rate, and each linked account's deposits, withdrawals and transfers to review. Nothing
/// changes. With no statement this session (a device without the token, or before the first
/// sync), the accounts the budget already holds from Interactive Brokers, with the rows their
/// last sync on any device left to review.
pub fn ibkr_review(db: &Database, state: &Ibkr, patch: Option<Patch>) -> Result<Review, String> {
    let budget = db.snapshot(patch)?;
    let last = state.last.lock().map_err(|_| stopped())?;
    reviewed(&budget, last.as_ref(), |account, raw| bank_import::review_near(&budget, account, raw).map(|rows| (rows, None)))
}
/// The same review as the phones read it, with the rows' matching choices shared (see
/// `bank_import::review_compact`).
pub fn ibkr_review_compact(db: &Database, state: &Ibkr, patch: Option<Patch>) -> Result<Review<bank_import::CompactRow>, String> {
    let budget = db.snapshot(patch)?;
    let last = state.last.lock().map_err(|_| stopped())?;
    reviewed(&budget, last.as_ref(), |account, raw| {
        bank_import::review_compact_near(&budget, account, raw).map(|review| (review.items, Some(review.candidate_sets)))
    })
}
/// The statement's accounts, then the ones the budget keeps that it doesn't list (`kept`): an
/// account another device fills, whose login isn't here, still has rows to review.
fn reviewed<R>(b: &Budget, last: Option<&Fetched>, rows: impl Fn(&str, Vec<(bank_import::BankTransaction, i64)>) -> Rows<R>) -> Result<Review<R>, String> {
    let Some(last) = last else { return kept(b, rows, &[]) };
    let mut accounts = review_with(b, &last.statement, &rows);
    let here: Vec<&str> = last.statement.accounts.iter().map(|a| a.id.as_str()).collect();
    accounts.extend(kept(b, &rows, &here).map(|more| more.accounts).unwrap_or_default());
    Ok(Review { accounts, fetched_at: last.at.clone() })
}
#[cfg(test)]
fn review(b: &Budget, statement: &flex::Statement) -> Vec<AccountReview> {
    review_with(b, statement, |account, raw| bank_import::review_near(b, account, raw).map(|rows| (rows, None)))
}
#[cfg(test)]
fn review_compact(b: &Budget, statement: &flex::Statement) -> Vec<AccountReview<bank_import::CompactRow>> {
    review_with(b, statement, |account, raw| {
        bank_import::review_compact_near(b, account, raw).map(|review| (review.items, Some(review.candidate_sets)))
    })
}
/// What a review's rows come as: plain, or with their choices shared.
type Rows<R> = Result<(Vec<R>, Option<Vec<bank_import::CandidateSet>>), String>;
/// The review without a statement: each account the budget holds from Interactive Brokers, as
/// its last sync left it, apart from the ones `listed` in a statement at hand. Refused when
/// there is none, since there is nothing to show.
fn kept<R>(b: &Budget, rows: impl Fn(&str, Vec<(bank_import::BankTransaction, i64)>) -> Rows<R>, listed: &[&str]) -> Result<Review<R>, String> {
    let held: Vec<investments::Holdings> = investments::read(b).into_iter().filter(|h| h.source == "ibkr" && !listed.contains(&h.source_account.as_str())).collect();
    if held.is_empty() {
        return Err(nothing_fetched());
    }
    let fetched_at = held.iter().map(|h| h.synced_at.clone()).max().unwrap_or_default();
    let accounts = held
        .into_iter()
        .map(|h| {
            let (rows, candidate_sets, error) = match rows(&h.account_id, investments::pending(b, &h.account_id)) {
                Ok((rows, sets)) => (rows, sets, None),
                Err(e) => (vec![], None, Some(e)),
            };
            // What it's worth in its own currency, from the budget's figure and the rate it came at.
            let rate = flex::Dec::parse(&h.rate).filter(|r| !r.is_zero());
            let value = flex::Dec::parse(&format!("{}{}.{:02}", if h.total < 0 { "-" } else { "" }, h.total.abs() / 100, h.total.abs() % 100));
            let found = Found {
                id: h.source_account.clone(),
                alias: String::new(),
                currency: h.currency.clone(),
                value: value.zip(rate).and_then(|(v, r)| v.over(r)).map(|v| v.cents()).unwrap_or(h.total),
                as_of: h.as_of.clone(),
                positions: h.positions.len(),
                missing: h.missing.clone(),
                rates: true,
                generated: String::new(),
            };
            AccountReview { found, account_id: Some(h.account_id), needs_rate: None, rows, candidate_sets, error, kept: true }
        })
        .collect();
    Ok(Review { accounts, fetched_at })
}
fn review_with<R>(
    b: &Budget,
    statement: &flex::Statement,
    rows: impl Fn(&str, Vec<(bank_import::BankTransaction, i64)>) -> Rows<R>,
) -> Vec<AccountReview<R>> {
    let held = investments::read(b);
    found(statement)
        .into_iter()
        .zip(&statement.accounts)
        .map(|(found, source)| {
            let link = held.iter().find(|h| h.source == "ibkr" && h.source_account == source.id);
            let manual = link.and_then(|h| h.manual_rate.as_deref());
            let needs_rate = investments::needs_rate(source, &b.currency, manual).filter(|(from, _)| !from.is_empty());
            let (rows, candidate_sets, error) = match link {
                Some(link) if needs_rate.is_none() => {
                    // What bringing it in would say, without changing anything.
                    let refused = investments::apply(b.clone(), &link.account_id, source, None).err();
                    // The rows are what the budget keeps from the last time it was brought in.
                    match rows(&link.account_id, investments::pending(b, &link.account_id)) {
                        Ok((rows, sets)) => (rows, sets, refused),
                        Err(e) => (vec![], None, Some(refused.unwrap_or(e))),
                    }
                }
                _ => (vec![], None, None),
            };
            AccountReview { found, account_id: link.map(|h| h.account_id.clone()), needs_rate, rows, candidate_sets, error, kept: false }
        })
        .collect()
}

/// Imports the reviewed deposits, withdrawals and transfers (`investments::import`): each is
/// matched, joined to its other side or added, and the accounts' lines are worked out again,
/// since money now in the budget is no longer stood in for. Needs no statement.
pub fn ibkr_import(db: &Database, patch: Option<Patch>, account: String, choices: Vec<bank_import::Choice>) -> Result<Patch, String> {
    db.change(patch, |b| investments::import(b, &account, choices))
}
/// Opens Interactive Brokers' page on making a Flex Web Service token.
pub fn ibkr_open() -> Result<(), String> {
    crate::host::open_browser(GUIDE)
}

/// Platform-independent command surface. The token never crosses this boundary outward.
pub async fn dispatch(method: &str, p: Value, db: &Database, state: &Ibkr) -> Result<Value, String> {
    let encode = |value| serde_json::to_value(value).map_err(|e| e.to_string());
    let text = |key: &str| p[key].as_str().unwrap_or_default().to_owned();
    match method {
        "ibkr_status" => encode(ibkr_status(state)),
        "ibkr_connect" => encode(ibkr_connect(state, text("token"), text("query")).await?),
        "ibkr_fetch" => encode(ibkr_fetch(state).await?),
        "ibkr_file" => encode(ibkr_file(state, text("content"))?),
        "ibkr_disconnect" => encode(ibkr_disconnect(state, p["query"].as_str().map(str::to_owned))?),
        "ibkr_open" => {
            ibkr_open()?;
            Ok(Value::Null)
        }
        "ibkr_review" => serde_json::to_value(ibkr_review(db, state, None)?).map_err(|e| e.to_string()),
        "ibkr_review_compact" => serde_json::to_value(ibkr_review_compact(db, state, None)?).map_err(|e| e.to_string()),
        "ibkr_apply" => {
            let links = serde_json::from_value(p["links"].clone()).unwrap_or_default();
            serde_json::to_value(ibkr_apply(db, state, None, links)?).map_err(|e| e.to_string())
        }
        "ibkr_import" => {
            let choices = serde_json::from_value(p["choices"].clone()).map_err(|e| format!("Invalid deposit choices: {e}"))?;
            serde_json::to_value(ibkr_import(db, None, text("account"), choices)?).map_err(|e| e.to_string())
        }
        _ => Err(format!("Unknown Interactive Brokers command: {method}")),
    }
}

#[cfg(test)]
mod tests;
