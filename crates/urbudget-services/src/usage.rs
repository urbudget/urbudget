//! Anonymous usage counts, so the team knows how many copies are in use and how many people come
//! back. A copy counts itself on urbudget.app at most once a day, saying only what's new since it
//! was last counted: opened for the first time ever, or for the first time today, this week (Monday
//! to Sunday, UTC) or this month. The days it was last counted stay in `usage.json`; nothing that
//! identifies the copy, the computer or the person is sent, so the server only adds to daily totals
//! (web/src/usage.ts). People can turn it off in Settings → Privacy & data.
use chrono::{Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct State {
    /// Opened for the first time and not counted as such yet.
    first: bool,
    /// The Monday of the week this copy was first opened, shared by every copy first opened that
    /// week; empty for a copy that was in use before counting began.
    cohort: String,
    /// The last day, week (its Monday) and month this copy was counted.
    day: String,
    week: String,
    month: String,
}

/// What a report says is new.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Report {
    first: bool,
    day: bool,
    week: bool,
    month: bool,
}

fn monday(day: NaiveDate) -> NaiveDate {
    day - Duration::days(day.weekday().num_days_from_monday().into())
}
fn month(day: NaiveDate) -> String {
    day.format("%Y-%m").to_string()
}

/// What's new on `today` since `state` was counted, or None when there's nothing to say.
fn due(state: &State, today: NaiveDate) -> Option<Report> {
    let report = Report {
        first: state.first,
        day: state.day != today.to_string(),
        week: state.week != monday(today).to_string(),
        month: state.month != month(today),
    };
    (report.first || report.day || report.week || report.month).then_some(report)
}

/// The state once `today` has been counted.
fn counted(state: &State, today: NaiveDate) -> State {
    State {
        first: false,
        cohort: state.cohort.clone(),
        day: today.to_string(),
        week: monday(today).to_string(),
        month: month(today),
    }
}

pub struct Usage {
    path: PathBuf,
    /// Held across a report, so two at once can't both count the same day.
    state: tokio::sync::Mutex<State>,
    client: reqwest::Client,
}

impl Usage {
    /// Reads what was last counted. Call it before anything else writes to `data_dir`: an empty or
    /// missing folder is a copy opening for the first time, and one with budgets in it was in use
    /// before counting began.
    pub fn open(data_dir: &Path) -> Self {
        Self::open_on(data_dir, Utc::now().date_naive())
    }

    fn open_on(data_dir: &Path, today: NaiveDate) -> Self {
        let path = data_dir.join("usage.json");
        let state = match std::fs::read(&path) {
            // Unreadable: counted again as a copy in use, never as a new one.
            Ok(raw) => serde_json::from_slice(&raw).unwrap_or_default(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let fresh = std::fs::read_dir(data_dir).map_or(true, |mut entries| entries.next().is_none());
                let state = State {
                    first: fresh,
                    cohort: if fresh { monday(today).to_string() } else { String::new() },
                    ..State::default()
                };
                // Kept straight away, so a first opening while offline still counts as one later.
                let _ = std::fs::create_dir_all(data_dir);
                let _ = save(&path, &state);
                state
            }
            // There but out of reach for now (a backup or virus scan holding it): left as it is, and
            // taken as counted today rather than counting this copy again.
            Err(_) => counted(&State::default(), today),
        };
        let client = reqwest::Client::builder()
            .user_agent(concat!("UrBudget/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self { path, state: tokio::sync::Mutex::new(state), client }
    }

    /// Counts this copy as in use on `site`, when anything is new since it was last counted.
    /// `platform` is "windows" or "macos", and `channel` "direct" or "store". When the request
    /// fails, nothing is marked counted, so the next call tries again.
    pub async fn report(&self, site: &str, platform: &str, channel: &str, version: &str) -> Result<(), String> {
        self.report_on(site, platform, channel, version, Utc::now().date_naive()).await
    }

    async fn report_on(&self, site: &str, platform: &str, channel: &str, version: &str, today: NaiveDate) -> Result<(), String> {
        let mut state = self.state.lock().await;
        let Some(report) = due(&state, today) else { return Ok(()) };
        let body = json!({
            "platform": platform,
            "channel": channel,
            "version": version,
            "first": report.first,
            "day": report.day,
            "week": report.week,
            "month": report.month,
            // Only with the first count of a week, for how many of each week's new copies come back.
            "cohort": if report.week { state.cohort.as_str() } else { "" },
        });
        let response = self
            .client
            .post(format!("{site}/api/v1/usage"))
            .json(&body)
            .send()
            .await
            .map_err(|_| "Couldn't reach urbudget.app.".to_string())?;
        if !response.status().is_success() {
            return Err(format!("urbudget.app answered {}.", response.status().as_u16()));
        }
        // Counted from here on, even if it can't be kept: a later report in this run mustn't count
        // the same day again.
        *state = counted(&state, today);
        save(&self.path, &state).map_err(|e| e.to_string())
    }
}

/// Keeps `state` whole or not at all: a write cut short never leaves half a file behind.
fn save(path: &Path, state: &State) -> std::io::Result<()> {
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec(state)?)?;
    std::fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn says_only_what_is_new() {
        // Thursday 22 October 2026.
        let state = counted(&State { cohort: "2026-10-19".into(), ..State::default() }, day("2026-10-22"));
        assert_eq!(state.week, "2026-10-19");
        assert_eq!(due(&state, day("2026-10-22")), None);
        assert_eq!(due(&state, day("2026-10-23")), Some(Report { first: false, day: true, week: false, month: false }));
        assert_eq!(due(&state, day("2026-10-26")), Some(Report { first: false, day: true, week: true, month: false }));
        assert_eq!(due(&state, day("2026-11-01")), Some(Report { first: false, day: true, week: true, month: true }));
        // Tuesday 29 September, then Thursday 1 October of the same week.
        let september = counted(&State::default(), day("2026-09-29"));
        assert_eq!(due(&september, day("2026-10-01")), Some(Report { first: false, day: true, week: false, month: true }));
        let first = State { first: true, cohort: "2026-10-19".into(), ..State::default() };
        assert_eq!(due(&first, day("2026-10-22")), Some(Report { first: true, day: true, week: true, month: true }));
        assert_eq!(counted(&first, day("2026-10-22")).cohort, "2026-10-19");
    }

    #[test]
    fn a_new_folder_is_a_first_opening_and_a_used_one_is_not() {
        let fresh = tempfile::tempdir().unwrap();
        let usage = Usage::open_on(fresh.path(), day("2026-10-22"));
        assert_eq!(usage.state.try_lock().unwrap().clone(), State { first: true, cohort: "2026-10-19".into(), ..State::default() });
        // Opening again before it was counted keeps it a first opening.
        let again = Usage::open_on(fresh.path(), day("2026-10-27"));
        assert!(again.state.try_lock().unwrap().first);

        let used = tempfile::tempdir().unwrap();
        std::fs::write(used.path().join("budgets.json"), "{}").unwrap();
        let usage = Usage::open_on(used.path(), day("2026-10-22"));
        assert_eq!(usage.state.try_lock().unwrap().clone(), State::default());
    }

    /// A one-request server that answers `status` and hands back what it was sent.
    fn server(status: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let site = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let n = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length { break; }
                }
                if n == 0 { break; }
            }
            stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        (site, handle)
    }

    #[test]
    fn reports_once_a_day_and_tries_again_after_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let usage = Usage::open_on(dir.path(), day("2026-10-22"));
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();

        let (site, request) = server("503 Service Unavailable");
        assert!(runtime.block_on(usage.report_on(&site, "windows", "store", "1.0.9", day("2026-10-22"))).is_err());
        request.join().unwrap();
        assert!(usage.state.try_lock().unwrap().first, "a failed report isn't counted");

        let (site, request) = server("204 No Content");
        runtime.block_on(usage.report_on(&site, "windows", "store", "1.0.9", day("2026-10-22"))).unwrap();
        let sent = request.join().unwrap();
        assert!(sent.starts_with("POST /api/v1/usage "), "{sent}");
        assert!(sent.to_ascii_lowercase().contains("user-agent: urbudget/"), "{sent}");
        let body: serde_json::Value = serde_json::from_str(&sent[sent.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(
            body,
            json!({ "platform": "windows", "channel": "store", "version": "1.0.9", "first": true, "day": true, "week": true, "month": true, "cohort": "2026-10-19" })
        );

        // The same day again: nothing to say, so nothing is sent (no server is listening).
        runtime.block_on(usage.report_on("http://127.0.0.1:9", "windows", "store", "1.0.9", day("2026-10-22"))).unwrap();
        // And it's remembered across restarts.
        let saved: State = serde_json::from_slice(&std::fs::read(dir.path().join("usage.json")).unwrap()).unwrap();
        assert_eq!(saved, State { first: false, cohort: "2026-10-19".into(), day: "2026-10-22".into(), week: "2026-10-19".into(), month: "2026-10".into() });

        // The next day: only the day is new, and the week the copy was first opened stays home.
        let (site, request) = server("204 No Content");
        runtime.block_on(usage.report_on(&site, "windows", "store", "1.0.9", day("2026-10-23"))).unwrap();
        let sent = request.join().unwrap();
        let body: serde_json::Value = serde_json::from_str(&sent[sent.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body["first"], false);
        assert_eq!(body["day"], true);
        assert_eq!(body["week"], false);
        assert_eq!(body["cohort"], "");
    }

    #[test]
    fn a_report_that_cant_be_kept_still_counts_once() {
        let dir = tempfile::tempdir().unwrap();
        let usage = Usage::open_on(dir.path(), day("2026-10-22"));
        // The file can't be written: its temporary copy's name is taken by a folder.
        std::fs::create_dir(dir.path().join("usage.json.tmp")).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let (site, request) = server("204 No Content");
        assert!(runtime.block_on(usage.report_on(&site, "windows", "direct", "1.0.9", day("2026-10-22"))).is_err());
        request.join().unwrap();
        // Later the same day nothing is sent again (no server is listening).
        runtime.block_on(usage.report_on("http://127.0.0.1:9", "windows", "direct", "1.0.9", day("2026-10-22"))).unwrap();
        // And the first opening the file holds is still there for the next run.
        let saved: State = serde_json::from_slice(&std::fs::read(dir.path().join("usage.json")).unwrap()).unwrap();
        assert!(saved.first);
    }
}
