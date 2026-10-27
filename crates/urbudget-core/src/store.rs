use crate::domain::*;
use crate::patch::{Collection, Keyed, Patch};
use chrono::Utc;
use rusqlite::{params, params_from_iter, types::Value as SqlValue, Connection, TransactionBehavior};
use serde::{Serialize, Serializer};
use serde_json::{Map, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

type Result<T> = std::result::Result<T, String>;
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let temporary = path.with_extension("json.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(error)?;
    file.write_all(bytes).map_err(error)?;
    file.sync_all().map_err(error)?;
    drop(file);
    std::fs::rename(temporary, path).map_err(error)
}

fn is_backup_name(name: &str) -> bool {
    let Some(stamp) = name.strip_prefix("budget-").and_then(|s| s.strip_suffix(".sqlite3").or_else(|| s.strip_suffix(".json"))) else {
        return false;
    };
    let parts: Vec<_> = stamp.split('-').collect();
    (parts.len() == 2 || parts.len() == 3)
        && parts[0].len() == 8 && parts[1].len() == 6
        && (parts.len() == 2 || parts[2].len() == 9)
        && parts.iter().all(|s| s.bytes().all(|b| b.is_ascii_digit()))
}

/// Extras that are kept for audit and export but never shown or edited in the UI.
/// They stay in the database and in exports; they are not sent to the window.
pub const HEAVY_EXTRAS: &[&str] = &["sourceExport"];

/// Transaction fields the window never shows or edits: an import's own copy of each transaction
/// (YNAB's), most of an imported budget's size. They are left out of what the window receives
/// and put back on every transaction it sends (see `Cache::restore`).
pub const HIDDEN_FIELDS: &[&str] = &["source"];

fn has_hidden(t: &Transaction) -> bool {
    HIDDEN_FIELDS.iter().any(|k| t.extra.contains_key(*k))
}
/// Drops `t`'s hidden fields. The other extras are copied; the hidden ones never are.
fn hide(t: &mut Transaction) {
    let imported = t.extra.contains_key("source");
    t.extra.remove("hasImportSource");
    if has_hidden(t) {
        t.extra = t
            .extra
            .iter()
            .filter(|(k, _)| !HIDDEN_FIELDS.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Map<_, _>>()
            .into();
    }
    if imported {
        t.extra.insert("hasImportSource".into(), Value::Bool(true));
    }
}
/// Remembers the extras of the transactions that have hidden fields, by id.
fn keep_hidden<'a>(kept: &mut HashMap<String, Extra>, transactions: impl IntoIterator<Item = &'a Transaction>) {
    for t in transactions {
        if has_hidden(t) {
            kept.insert(t.id.clone(), t.extra.clone());
        }
    }
}
/// Gives the transactions a command added (in `after`, not `before`) the time they were added,
/// when the command didn't. The window gets them in the command's patch, so it has the same time.
fn stamp_added(before: &Budget, after: &mut Budget) {
    let known: HashSet<&str> = before.transactions.iter().map(|t| t.id.as_str()).collect();
    let now = stamp_now();
    for t in &mut after.transactions {
        if t.created_at().is_none() && !known.contains(t.id.as_str()) {
            t.set_created_at(&now);
        }
    }
}
/// The budget as the window receives it: without heavy extras or hidden transaction fields.
pub fn for_window(b: &Budget) -> Budget {
    let mut light = b.clone();
    for k in HEAVY_EXTRAS {
        light.extra.remove(*k);
    }
    light.transactions.iter_mut().for_each(hide);
    light
}
fn serialize_light<S: Serializer>(b: &Budget, s: S) -> std::result::Result<S::Ok, S::Error> {
    for_window(b).serialize(s)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(serialize_with = "serialize_light")]
    pub budget: Budget,
    pub token: String,
    pub saved_at: String,
    pub database_path: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub token: String,
    /// The revision captured with this snapshot or save acknowledgement.
    #[serde(skip)]
    pub revision: i64,
    pub saved_at: String,
    pub database_path: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    /// The draft was applied, but its recovery copy could not be written.
    pub recovery_error: Option<String>,
}

/// The saved budget and the window's working draft, both without heavy extras.
struct Cache {
    token: String,
    persisted: Budget,
    draft: Budget,
    heavy: Map<String, Value>,
    /// `heavy` serialized once as `"key":value,...`, spliced into extras writes.
    heavy_json: String,
    /// `heavy` changed since the last commit.
    heavy_dirty: bool,
    /// The extras of every transaction the window has received without its hidden fields, by
    /// id. Kept after a delete, so undoing one puts its fields back too.
    hidden: HashMap<String, Extra>,
}
impl Cache {
    /// Puts back the hidden fields the window left out of `t`.
    fn restore(&self, t: &mut Transaction) {
        // A window-only hint, reconstructed from the actual import metadata on every read.
        t.extra.remove("hasImportSource");
        let Some(kept) = self.hidden.get(&t.id) else {
            return;
        };
        for k in HIDDEN_FIELDS {
            if let (false, Some(v)) = (t.extra.contains_key(*k), kept.get(*k)) {
                t.extra.insert((*k).into(), v.clone());
            }
        }
    }
    /// Restores the transactions in a patch from the window.
    fn restore_patch(&mut self, patch: &mut Patch) {
        if let Some(c) = &mut patch.transactions {
            for t in &mut c.upsert {
                self.restore(t);
            }
            keep_hidden(&mut self.hidden, &c.upsert);
        }
    }
}

pub struct Store {
    connection: Connection,
    path: PathBuf,
    /// Budget id, part of every save token so a window can never commit into another budget.
    id: String,
    cache: Option<Cache>,
    /// Device-local policy shared by the library's open and background sync stores.
    backup_preferences: PathBuf,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct BackupPreferences {
    keep: usize,
}

fn split(mut budget: Budget) -> (Budget, Map<String, Value>) {
    let mut heavy = Map::new();
    for k in HEAVY_EXTRAS {
        if let Some(v) = budget.extra.remove(*k) {
            heavy.insert((*k).into(), v);
        }
    }
    (budget, heavy)
}
fn fragment(heavy: &Map<String, Value>) -> Result<String> {
    let s = serde_json::to_string(heavy).map_err(error)?;
    Ok(s[1..s.len() - 1].to_owned())
}
fn extras_json(extra: &Map<String, Value>, heavy_json: &str) -> Result<String> {
    let mut s = serde_json::to_string(extra).map_err(error)?;
    if !heavy_json.is_empty() {
        s.pop();
        if s.len() > 1 {
            s.push(',');
        }
        s.push_str(heavy_json);
        s.push('}');
    }
    Ok(s)
}
/// The revision part of a `{budget}:{revision}:{data_version}` token.
fn revision(token: &str) -> &str {
    token.split(':').nth(1).unwrap_or_default()
}

/// A table row stored as `(id, position, ...indexed columns, data)`.
trait Row: Keyed + Serialize {
    const TABLE: &'static str;
    const COLUMNS: &'static [&'static str];
    fn values(&self, position: i64) -> Result<Vec<SqlValue>>;
}
impl Row for Account {
    const TABLE: &'static str = "accounts";
    const COLUMNS: &'static [&'static str] = &["id", "position", "data"];
    fn values(&self, position: i64) -> Result<Vec<SqlValue>> {
        Ok(vec![
            self.id.clone().into(),
            position.into(),
            serde_json::to_string(self).map_err(error)?.into(),
        ])
    }
}
impl Row for Category {
    const TABLE: &'static str = "categories";
    const COLUMNS: &'static [&'static str] = &["id", "position", "data"];
    fn values(&self, position: i64) -> Result<Vec<SqlValue>> {
        Ok(vec![
            self.id.clone().into(),
            position.into(),
            serde_json::to_string(self).map_err(error)?.into(),
        ])
    }
}
impl Row for Transaction {
    const TABLE: &'static str = "transactions";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "position",
        "account_id",
        "transfer_account_id",
        "date",
        "data",
    ];
    fn values(&self, position: i64) -> Result<Vec<SqlValue>> {
        Ok(vec![
            self.id.clone().into(),
            position.into(),
            self.account_id.clone().into(),
            self.transfer_account_id.clone().into(),
            self.date.clone().into(),
            serde_json::to_string(self).map_err(error)?.into(),
        ])
    }
}
fn upsert_sql<T: Row>(reposition: bool) -> String {
    let placeholders: Vec<String> = (1..=T::COLUMNS.len()).map(|i| format!("?{i}")).collect();
    let updates: Vec<String> = T::COLUMNS
        .iter()
        .filter(|c| **c != "id" && (reposition || **c != "position"))
        .map(|c| format!("{c}=excluded.{c}"))
        .collect();
    format!(
        "INSERT INTO {}({}) VALUES ({}) ON CONFLICT(id) DO UPDATE SET {}",
        T::TABLE,
        T::COLUMNS.join(","),
        placeholders.join(","),
        updates.join(",")
    )
}
fn delete_rows<T: Row>(tx: &Connection, change: &Collection<T>) -> Result<()> {
    let mut stmt = tx
        .prepare_cached(&format!("DELETE FROM {} WHERE id=?1", T::TABLE))
        .map_err(error)?;
    for id in &change.delete {
        stmt.execute([id]).map_err(error)?;
    }
    Ok(())
}
/// Writes changed rows. Untouched rows keep their stored position unless records moved.
fn write_rows<T: Row>(tx: &Connection, before: &[T], after: &[T], change: &Collection<T>) -> Result<()> {
    let previous: HashMap<&str, usize> = before.iter().enumerate().map(|(i, x)| (x.key(), i)).collect();
    let mut last = None;
    let mut appended = false;
    let in_place = after.iter().all(|x| match previous.get(x.key()) {
        Some(&i) => {
            let ordered = !appended && last.is_none_or(|l| i > l);
            last = Some(i);
            ordered
        }
        None => {
            appended = true;
            true
        }
    });
    if in_place {
        let mut next: i64 = tx
            .query_row(
                &format!("SELECT COALESCE(MAX(position),-1)+1 FROM {}", T::TABLE),
                [],
                |r| r.get(0),
            )
            .map_err(error)?;
        let mut stmt = tx.prepare_cached(&upsert_sql::<T>(false)).map_err(error)?;
        for x in &change.upsert {
            let position = if previous.contains_key(x.key()) {
                0 // Ignored: existing rows keep their position.
            } else {
                next += 1;
                next - 1
            };
            stmt.execute(params_from_iter(x.values(position)?))
                .map_err(error)?;
        }
    } else {
        let changed: HashSet<&str> = change.upsert.iter().map(Keyed::key).collect();
        let mut upsert = tx.prepare_cached(&upsert_sql::<T>(true)).map_err(error)?;
        let mut reposition = tx
            .prepare_cached(&format!("UPDATE {} SET position=?2 WHERE id=?1", T::TABLE))
            .map_err(error)?;
        for (i, x) in after.iter().enumerate() {
            if changed.contains(x.key()) {
                upsert
                    .execute(params_from_iter(x.values(i as i64)?))
                    .map_err(error)?;
            } else {
                reposition.execute(params![x.key(), i as i64]).map_err(error)?;
            }
        }
    }
    Ok(())
}
/// Writes the rows `patch` changes from `before` to `after`, then bumps the revision.
fn write_changes(tx: &Connection, before: &Budget, after: &Budget, patch: &Patch, heavy_json: &str, heavy_dirty: bool) -> Result<()> {
    tx.execute_batch("PRAGMA defer_foreign_keys=ON;").map_err(error)?;
    // Dependents first: transactions reference accounts, assignments reference categories.
    if let Some(c) = &patch.transactions {
        delete_rows(tx, c)?;
    }
    if let Some(a) = &patch.assignments {
        let mut stmt = tx
            .prepare_cached("DELETE FROM assignments WHERE month=?1 AND category_id=?2")
            .map_err(error)?;
        for k in &a.delete {
            stmt.execute([&k.month, &k.category_id]).map_err(error)?;
        }
    }
    if let Some(c) = &patch.accounts {
        write_rows(tx, &before.accounts, &after.accounts, c)?;
    }
    if let Some(c) = &patch.categories {
        write_rows(tx, &before.categories, &after.categories, c)?;
    }
    if let Some(c) = &patch.transactions {
        write_rows(tx, &before.transactions, &after.transactions, c)?;
    }
    if let Some(a) = &patch.assignments {
        let mut stmt = tx
            .prepare_cached("INSERT INTO assignments VALUES (?1,?2,?3) ON CONFLICT(month,category_id) DO UPDATE SET amount=excluded.amount")
            .map_err(error)?;
        for x in &a.upsert {
            stmt.execute(params![x.month, x.category_id, x.amount])
                .map_err(error)?;
            tx.execute("INSERT INTO assignment_extras(month,category_id,data) VALUES (?1,?2,?3) ON CONFLICT(month,category_id) DO UPDATE SET data=excluded.data",
                params![x.month, x.category_id, serde_json::to_string(&x.extra).map_err(error)?]).map_err(error)?;
        }
    }
    if let Some(c) = &patch.accounts {
        delete_rows(tx, c)?;
    }
    if let Some(c) = &patch.categories {
        delete_rows(tx, c)?;
    }
    if patch.extras.is_some() || heavy_dirty {
        tx.execute(
            "UPDATE budget_extras SET data=?1 WHERE id=1",
            [extras_json(&after.extra, heavy_json)?],
        )
        .map_err(error)?;
    }
    tx.execute(
        "UPDATE metadata SET revision=revision+1,saved_at=?1,name=?2,currency=?3 WHERE id=1",
        params![Utc::now().to_rfc3339(), after.name, after.currency],
    )
    .map_err(error)?;
    Ok(())
}
fn write_sync_base(tx: &Connection, revision: u64, base: &Budget) -> Result<()> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&serde_json::to_vec(base).map_err(error)?)
        .map_err(error)?;
    let data = encoder.finish().map_err(error)?;
    tx.execute(
        "INSERT INTO sync_base VALUES (1,?1,?2) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,data=excluded.data",
        params![revision as i64, data],
    )
    .map_err(error)?;
    Ok(())
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_budget(path, "")
    }
    pub fn open_budget(path: &Path, id: &str) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(error)?;
        }
        let connection = Connection::open(path).map_err(error)?;
        connection
            .busy_timeout(Duration::from_secs(3))
            .map_err(error)?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
            )
            .map_err(error)?;
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(error)?;
        if version > SCHEMA_VERSION {
            return Err("This database was created by a newer UrBudget version.".into());
        }
        connection.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS metadata (id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL, saved_at TEXT NOT NULL, name TEXT NOT NULL, currency TEXT NOT NULL);
            INSERT OR IGNORE INTO metadata VALUES (1,0,'','My budget','SAR');
            CREATE TABLE IF NOT EXISTS accounts (id TEXT PRIMARY KEY, position INTEGER NOT NULL, data TEXT NOT NULL CHECK(json_valid(data)));
            CREATE TABLE IF NOT EXISTS categories (id TEXT PRIMARY KEY, position INTEGER NOT NULL, data TEXT NOT NULL CHECK(json_valid(data)));
            CREATE TABLE IF NOT EXISTS transactions (id TEXT PRIMARY KEY, position INTEGER NOT NULL, account_id TEXT NOT NULL REFERENCES accounts(id), transfer_account_id TEXT REFERENCES accounts(id), date TEXT NOT NULL, data TEXT NOT NULL CHECK(json_valid(data)));
            CREATE INDEX IF NOT EXISTS transaction_account_date ON transactions(account_id, date);
            CREATE TABLE IF NOT EXISTS assignments (month TEXT NOT NULL, category_id TEXT NOT NULL REFERENCES categories(id), amount INTEGER NOT NULL, PRIMARY KEY(month, category_id));
            CREATE TABLE IF NOT EXISTS assignment_extras (month TEXT NOT NULL, category_id TEXT NOT NULL, data TEXT NOT NULL CHECK(json_valid(data)), PRIMARY KEY(month,category_id), FOREIGN KEY(month,category_id) REFERENCES assignments(month,category_id) ON DELETE CASCADE);
            CREATE TABLE IF NOT EXISTS budget_extras (id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL CHECK(json_valid(data))); INSERT OR IGNORE INTO budget_extras VALUES (1,'{}');
            CREATE TABLE IF NOT EXISTS sync_base (id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL, data BLOB NOT NULL);
            PRAGMA user_version=2; COMMIT;").map_err(error)?;
        Ok(Self {
            connection,
            path: path.to_path_buf(),
            id: id.to_owned(),
            cache: None,
            backup_preferences: path.with_file_name("backup-settings.json"),
        })
    }

    pub fn with_backup_preferences(mut self, data_dir: &Path) -> Self {
        self.backup_preferences = data_dir.join("backup-settings.json");
        self
    }

    /// Migrates the old web preference once. Later launches use the native policy even when
    /// localStorage was cleared; background sync never needs a window to enforce retention.
    pub fn initialize_backup_retention(&self, legacy_keep: usize) -> Result<usize> {
        if self.backup_preferences.exists() {
            return self.backup_retention();
        }
        let keep = if [0, 5, 10, 30].contains(&legacy_keep) { legacy_keep } else { 0 };
        self.set_backup_retention(keep)?;
        Ok(keep)
    }

    pub fn backup_retention(&self) -> Result<usize> {
        let bytes = match std::fs::read(&self.backup_preferences) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(error(e)),
        };
        let preferences: BackupPreferences = serde_json::from_slice(&bytes).map_err(error)?;
        if ![0, 5, 10, 30].contains(&preferences.keep) {
            return Err("Invalid backup retention setting.".into());
        }
        Ok(preferences.keep)
    }

    pub fn set_backup_retention(&self, keep: usize) -> Result<()> {
        if ![0, 5, 10, 30].contains(&keep) {
            return Err("Choose all backups, or the newest 5, 10 or 30.".into());
        }
        write_atomic(&self.backup_preferences, &serde_json::to_vec(&BackupPreferences { keep }).map_err(error)?)
    }

    fn retain_backups(&self) -> Result<()> {
        let keep = self.backup_retention()?;
        if keep > 0 { self.prune_backups(keep)?; }
        Ok(())
    }

    fn status_from(connection: &Connection, path: &Path, id: &str) -> Result<Status> {
        let (revision, saved_at): (i64, String) = connection
            .query_row(
                "SELECT revision,saved_at FROM metadata WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(error)?;
        // data_version detects writes by other connections even if they do not update revision.
        let data_version: i64 = connection
            .pragma_query_value(None, "data_version", |r| r.get(0))
            .map_err(error)?;
        Ok(Status {
            token: format!("{id}:{revision}:{data_version}"),
            revision,
            saved_at,
            database_path: path.to_string_lossy().into_owned(),
        })
    }
    pub fn status(&self) -> Result<Status> {
        Self::status_from(&self.connection, &self.path, &self.id)
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    /// The budget's folder: backups, exports, imports and the recovery file live here.
    pub fn folder(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }
    /// The saved revision, which every save increments.
    pub fn revision(&self) -> Result<i64> {
        self.connection
            .query_row("SELECT revision FROM metadata WHERE id=1", [], |r| r.get(0))
            .map_err(error)
    }
    /// The saved budget with heavy extras, read without touching the window's draft.
    pub fn saved(&mut self) -> Result<(Budget, Status)> {
        self.read()
    }
    /// The window has changes in the draft that aren't saved yet.
    pub fn has_unsaved_draft(&self) -> bool {
        self.cache.as_ref().is_some_and(|c| c.draft != c.persisted || c.heavy_dirty)
    }

    fn read(&mut self) -> Result<(Budget, Status)> {
        let tx = self.connection.transaction().map_err(error)?;
        let status = Self::status_from(&tx, &self.path, &self.id)?;
        let (name, currency) = tx
            .query_row("SELECT name,currency FROM metadata WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(error)?;
        fn records<T: serde::de::DeserializeOwned>(c: &Connection, table: &str) -> Result<Vec<T>> {
            let mut stmt = c
                .prepare(&format!("SELECT data FROM {table} ORDER BY position"))
                .map_err(error)?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(error)?;
            rows.map(|r| serde_json::from_str(&r.map_err(error)?).map_err(error))
                .collect()
        }
        let assignments = {
            let mut stmt = tx
                .prepare(
                    "SELECT a.month,a.category_id,a.amount,COALESCE(e.data,'{}') FROM assignments a LEFT JOIN assignment_extras e ON a.month=e.month AND a.category_id=e.category_id ORDER BY a.month,a.category_id",
                )
                .map_err(error)?;
            let values = stmt
                .query_map([], |r| {
                    Ok(Assignment {
                        extra: serde_json::from_str(&r.get::<_, String>(3)?).map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e)))?,
                        month: r.get(0)?,
                        category_id: r.get(1)?,
                        amount: r.get(2)?,
                    })
                })
                .map_err(error)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(error)?;
            values
        };
        let mut budget = Budget {
            schema_version: SCHEMA_VERSION,
            name,
            currency,
            accounts: records(&tx, "accounts")?,
            categories: records(&tx, "categories")?,
            transactions: records(&tx, "transactions")?,
            assignments,
            extra: serde_json::from_str(
                &tx.query_row("SELECT data FROM budget_extras WHERE id=1", [], |r| {
                    r.get::<_, String>(0)
                })
                .map_err(error)?,
            )
            .map_err(error)?,
        };
        crate::ynab::normalize_import_metadata(&mut budget);
        validate(&budget)?;
        tx.commit().map_err(error)?;
        Ok((budget, status))
    }
    fn remember(&mut self, full: Budget, token: String) -> Result<&mut Cache> {
        let (light, heavy) = split(full);
        let heavy_json = fragment(&heavy)?;
        let mut hidden = HashMap::new();
        keep_hidden(&mut hidden, &light.transactions);
        Ok(self.cache.insert(Cache {
            token,
            persisted: light.clone(),
            draft: light,
            heavy,
            heavy_json,
            heavy_dirty: false,
            hidden,
        }))
    }
    /// Reads the complete budget, including heavy extras, and resets the draft to it.
    pub fn load(&mut self) -> Result<Snapshot> {
        let (budget, status) = self.read()?;
        self.remember(budget.clone(), status.token.clone())?;
        Ok(Snapshot {
            budget,
            token: status.token,
            saved_at: status.saved_at,
            database_path: status.database_path,
        })
    }
    /// Like `load`, without heavy extras: what the window receives.
    pub fn load_light(&mut self) -> Result<Snapshot> {
        let (budget, status) = self.read()?;
        let budget = self.remember(budget, status.token.clone())?.draft.clone();
        Ok(Snapshot {
            budget,
            token: status.token,
            saved_at: status.saved_at,
            database_path: status.database_path,
        })
    }
    /// Discards only the in-memory working draft. Native transactional editors call this after
    /// a failed commit, so later actions cannot accidentally include that rejected edit. The
    /// recovery file remains available for an explicit recovery/export action.
    pub fn invalidate_draft(&mut self) {
        self.cache = None;
    }
    fn cache(&mut self) -> Result<&mut Cache> {
        if self.cache.is_none() {
            self.load_light()?;
        }
        Ok(self.cache.as_mut().expect("cache loaded"))
    }
    /// Runs `f` on the draft, or on a copy of it with `patch` applied.
    pub fn read_draft<T>(&mut self, patch: Option<Patch>, f: impl FnOnce(&Budget) -> Result<T>) -> Result<T> {
        let cache = self.cache()?;
        match patch.filter(|p| !p.is_empty()) {
            Some(mut p) => {
                cache.restore_patch(&mut p);
                let mut copy = cache.draft.clone();
                p.apply(&mut copy);
                f(&copy)
            }
            None => f(&cache.draft),
        }
    }
    /// A stable read snapshot. Callers can release the Store lock before deriving reports.
    /// Applying the window's relative patch here never changes the persisted or recovery draft.
    pub fn snapshot_draft(&mut self, patch: Option<Patch>) -> Result<Budget> {
        let cache = self.cache()?;
        let mut snapshot = cache.draft.clone();
        if let Some(mut p) = patch.filter(|p| !p.is_empty()) {
            cache.restore_patch(&mut p);
            p.apply(&mut snapshot);
        }
        Ok(snapshot)
    }
    /// Runs `f` on a copy of the draft (plus `patch`) and returns what it changed, as the
    /// window receives it. The draft itself is unchanged.
    pub fn change_draft(&mut self, patch: Option<Patch>, f: impl FnOnce(Budget) -> Result<Budget>) -> Result<Patch> {
        let mut changes = self.read_draft(patch, |before| {
            let mut after = f(before.clone())?;
            stamp_added(before, &mut after);
            Ok(Patch::diff(before, &after))
        })?;
        if let Some(c) = &mut changes.transactions {
            keep_hidden(&mut self.cache()?.hidden, &c.upsert);
            c.upsert.iter_mut().for_each(hide);
        }
        Ok(changes)
    }
    /// Puts back the hidden fields of a transaction the window sends on its own.
    pub fn restore_transaction(&mut self, t: &mut Transaction) -> Result<()> {
        self.cache()?.restore(t);
        Ok(())
    }
    /// The draft (plus `patch`) with heavy extras restored, for exports.
    pub fn full_draft(&mut self, patch: Option<Patch>) -> Result<Budget> {
        let cache = self.cache()?;
        let mut budget = cache.draft.clone();
        if let Some(mut p) = patch {
            cache.restore_patch(&mut p);
            p.apply(&mut budget);
        }
        for (k, v) in &cache.heavy {
            budget.extra.entry(k.clone()).or_insert_with(|| v.clone());
        }
        Ok(budget)
    }
    /// A fresh start must use the same source snapshot the window used for its balances.
    pub fn fresh_start_source(&mut self, patch: Option<Patch>, expected_token: &str) -> Result<Budget> {
        if self.status()?.token != expected_token || self.cache()?.token != expected_token {
            return Err("CONFLICT: The source budget changed. Fetch it and try the fresh start again.".into());
        }
        self.full_draft(patch)
    }
    pub fn apply_patch(&mut self, mut patch: Patch) -> Result<Applied> {
        let cache = self.cache()?;
        cache.restore_patch(&mut patch);
        patch.apply(&mut cache.draft);
        Ok(self.after_draft_change())
    }
    pub fn replace_draft(&mut self, mut budget: Budget) -> Result<Applied> {
        let cache = self.cache()?;
        for t in &mut budget.transactions {
            cache.restore(t);
        }
        keep_hidden(&mut cache.hidden, &budget.transactions);
        cache.draft = budget;
        Ok(self.after_draft_change())
    }
    fn after_draft_change(&self) -> Applied {
        Applied {
            recovery_error: self.save_recovery().err(),
        }
    }

    /// Saves the draft: only rows that differ from the database are written,
    /// atomically, after the same validation and conflict checks as a full save.
    pub fn commit(&mut self, expected_token: &str) -> Result<Status> {
        self.cache()?;
        let Store {
            connection,
            path,
            id,
            cache,
            ..
        } = self;
        let cache = cache.as_mut().expect("cache loaded");
        validate(&cache.draft)?;
        // A restored JSON backup can carry heavy extras; they replace the stored copy.
        for k in HEAVY_EXTRAS {
            if let Some(v) = cache.draft.extra.remove(*k) {
                cache.heavy.insert((*k).into(), v);
                cache.heavy_json = fragment(&cache.heavy)?;
                cache.heavy_dirty = true;
            }
        }
        let patch = Patch::diff(&cache.persisted, &cache.draft);
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        let current = Self::status_from(&tx, path, id)?;
        if current.token != expected_token || cache.token != expected_token {
            return Err("CONFLICT: The database changed. Your edits are safe in the recovery draft. Fetch the database or export your draft before continuing.".into());
        }
        if patch.is_empty() && !cache.heavy_dirty {
            return Ok(current);
        }
        write_changes(&tx, &cache.persisted, &cache.draft, &patch, &cache.heavy_json, cache.heavy_dirty)?;
        // Capture our acknowledgement while the write lock is held. Reading it
        // after commit could accidentally acknowledge a competing writer's version.
        let status = Self::status_from(&tx, path, id)?;
        tx.commit().map_err(error)?;
        cache.persisted = cache.draft.clone();
        cache.token = status.token.clone();
        cache.heavy_dirty = false;
        // The save succeeded; a stale recovery file is only offered again if this fails.
        let _ = std::fs::remove_file(path.with_file_name("recovery.json"));
        Ok(status)
    }

    /// Replaces the complete budget (imports). Heavy extras in `budget` are stored too.
    pub fn save(&mut self, budget: &Budget, expected_token: &str) -> Result<Status> {
        validate(budget)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        if Self::status_from(&tx, &self.path, &self.id)?.token != expected_token {
            return Err("CONFLICT: The database changed. Your edits are safe in the recovery draft. Fetch the database or export your draft before continuing.".into());
        }
        // A complete snapshot commits atomically; a crash can never leave half a transfer or split.
        tx.execute_batch("DELETE FROM transactions; DELETE FROM assignments; DELETE FROM accounts; DELETE FROM categories;").map_err(error)?;
        {
            let mut accounts = tx
                .prepare_cached("INSERT INTO accounts VALUES (?1,?2,?3)")
                .map_err(error)?;
            for (i, a) in budget.accounts.iter().enumerate() {
                accounts
                    .execute(params_from_iter(a.values(i as i64)?))
                    .map_err(error)?;
            }
            let mut categories = tx
                .prepare_cached("INSERT INTO categories VALUES (?1,?2,?3)")
                .map_err(error)?;
            for (i, c) in budget.categories.iter().enumerate() {
                categories
                    .execute(params_from_iter(c.values(i as i64)?))
                    .map_err(error)?;
            }
            let mut transactions = tx
                .prepare_cached("INSERT INTO transactions VALUES (?1,?2,?3,?4,?5,?6)")
                .map_err(error)?;
            for (i, t) in budget.transactions.iter().enumerate() {
                transactions
                    .execute(params_from_iter(t.values(i as i64)?))
                    .map_err(error)?;
            }
            let mut assignments = tx
                .prepare_cached("INSERT INTO assignments VALUES (?1,?2,?3)")
                .map_err(error)?;
            for a in &budget.assignments {
                assignments
                    .execute(params![a.month, a.category_id, a.amount])
                    .map_err(error)?;
                tx.execute("INSERT INTO assignment_extras(month,category_id,data) VALUES (?1,?2,?3)",
                    params![a.month,a.category_id,serde_json::to_string(&a.extra).map_err(error)?]).map_err(error)?;
            }
        }
        tx.execute(
            "UPDATE budget_extras SET data=?1 WHERE id=1",
            [serde_json::to_string(&budget.extra).map_err(error)?],
        )
        .map_err(error)?;
        tx.execute(
            "UPDATE metadata SET revision=revision+1,saved_at=?1,name=?2,currency=?3 WHERE id=1",
            params![Utc::now().to_rfc3339(), budget.name, budget.currency],
        )
        .map_err(error)?;
        let status = Self::status_from(&tx, &self.path, &self.id)?;
        tx.commit().map_err(error)?;
        self.remember(budget.clone(), status.token.clone())?;
        Ok(status)
    }

    /// The cloud revision of the version this budget last synced as (see `sync_base`).
    pub fn sync_base_revision(&self) -> Result<Option<u64>> {
        use rusqlite::OptionalExtension;
        let revision: Option<i64> = self
            .connection
            .query_row("SELECT revision FROM sync_base WHERE id=1", [], |r| r.get(0))
            .optional()
            .map_err(error)?;
        Ok(revision.map(|r| r as u64))
    }
    /// The version this budget last synced as and its cloud revision: the base the next sync's
    /// merge compares both sides with. Without heavy extras. One that can't be read is treated
    /// as missing, which only makes the next merge ask more.
    pub fn sync_base(&self) -> Result<Option<(u64, Budget)>> {
        use rusqlite::OptionalExtension;
        use std::io::Read;
        let row: Option<(i64, Vec<u8>)> = self
            .connection
            .query_row("SELECT revision,data FROM sync_base WHERE id=1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .map_err(error)?;
        Ok(row.and_then(|(revision, data)| {
            let mut json = Vec::new();
            flate2::read::GzDecoder::new(&data[..]).read_to_end(&mut json).ok()?;
            Some((revision as u64, serde_json::from_slice(&json).ok()?))
        }))
    }
    pub fn set_sync_base(&self, revision: u64, base: &Budget) -> Result<()> {
        write_sync_base(&self.connection, revision, base)
    }
    /// As if this budget last synced before sync bases were kept.
    #[cfg(any(test, feature = "test-support"))]
    pub fn forget_sync_base(&self) -> Result<()> {
        self.connection.execute("DELETE FROM sync_base", []).map_err(error)?;
        Ok(())
    }
    /// Saves a budget sync merged or downloaded together with its new sync base, in one
    /// transaction, so a merge can never be applied twice. Only rows that differ are written,
    /// and heavy extras stay as they are. Refused while the window has unsaved edits.
    pub fn save_synced(&mut self, budget: &Budget, expected_token: &str, revision: u64, base: &Budget) -> Result<Status> {
        if self.has_unsaved_draft() {
            return Err("Save your changes first.".into());
        }
        validate(budget)?;
        self.cache()?;
        let Store {
            connection,
            path,
            id,
            cache,
            ..
        } = self;
        let cache = cache.as_mut().expect("cache loaded");
        let (budget, _) = split(budget.clone());
        let patch = Patch::diff(&cache.persisted, &budget);
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        if Self::status_from(&tx, path, id)?.token != expected_token || cache.token != expected_token {
            return Err("CONFLICT: The database changed while syncing. Sync tries again shortly.".into());
        }
        if !patch.is_empty() {
            write_changes(&tx, &cache.persisted, &budget, &patch, &cache.heavy_json, false)?;
        }
        write_sync_base(&tx, revision, base)?;
        let status = Self::status_from(&tx, path, id)?;
        tx.commit().map_err(error)?;
        cache.persisted = budget.clone();
        cache.draft = budget;
        cache.token = status.token.clone();
        Ok(status)
    }

    /// Applies due schedules to the saved budget and commits them. The draft must be saved.
    pub fn run_schedules(&mut self, expected_token: &str, through: &str) -> Result<Snapshot> {
        let mut status = self.status()?;
        if status.token != expected_token || self.cache()?.token != expected_token {
            return Err("CONFLICT: Database changed before scheduled posting.".into());
        }
        if self.has_unsaved_draft() {
            return Err("Save your changes before posting scheduled transactions.".into());
        }
        let cache = self.cache()?;
        let mut next = cache.persisted.clone();
        // Transactions dated ahead are scheduled ones; then whatever is due is entered.
        crate::schedules::schedule_future(&mut next, through)?;
        crate::schedules::apply(&mut next, through, None, false)?;
        if next != cache.persisted {
            let previous = std::mem::replace(&mut cache.draft, next);
            match self.commit(expected_token) {
                Ok(saved) => status = saved,
                Err(e) => {
                    self.cache()?.draft = previous;
                    return Err(e);
                }
            }
        }
        let Cache { draft, hidden, .. } = self.cache()?;
        keep_hidden(hidden, &draft.transactions);
        Ok(Snapshot {
            budget: draft.clone(),
            token: status.token,
            saved_at: status.saved_at,
            database_path: status.database_path,
        })
    }

    pub fn backup(&self) -> Result<String> {
        let directory = self
            .path
            .parent()
            .ok_or("Missing database directory")?
            .join("backups");
        std::fs::create_dir_all(&directory).map_err(error)?;
        let path = directory.join(format!(
            "budget-{}.sqlite3",
            Utc::now().format("%Y%m%d-%H%M%S-%f")
        ));
        {
            let mut destination = Connection::open(&path).map_err(error)?;
            let backup =
                rusqlite::backup::Backup::new(&self.connection, &mut destination).map_err(error)?;
            backup
                .run_to_completion(64, Duration::from_millis(5), None)
                .map_err(error)?;
        }
        self.retain_backups()?;
        Ok(path.to_string_lossy().into_owned())
    }

    /// A portable, complete snapshot of the saved budget, accepted by Restore's JSON picker.
    /// Android shares this file; internal pre-import/sync backups remain SQLite snapshots.
    pub fn backup_portable(&mut self) -> Result<String> {
        let (budget, _) = self.saved()?;
        let directory = self.folder().join("backups");
        std::fs::create_dir_all(&directory).map_err(error)?;
        let path = directory.join(format!("budget-{}.json", Utc::now().format("%Y%m%d-%H%M%S-%f")));
        write_atomic(&path, &serde_json::to_vec_pretty(&budget).map_err(error)?)?;
        self.retain_backups()?;
        Ok(path.to_string_lossy().into_owned())
    }

    /// Restoring is a complete replacement, not an edit patch: omitted optional/hidden
    /// fields must not survive from the previous budget. Keep a readable pre-restore copy.
    pub fn restore_backup(&mut self, budget: &Budget, expected_token: &str) -> Result<Snapshot> {
        validate(budget)?;
        if self.status()?.token != expected_token {
            return Err("CONFLICT: The database changed. Fetch the saved budget before restoring.".into());
        }
        self.backup()?;
        self.save(budget, expected_token)?;
        let _ = self.discard_recovery();
        self.load_light()
    }

    /// Deletes all but the newest `keep` backups this app made (timestamped SQLite/JSON files in
    /// the budget's backups folder). Other files there are never touched. Returns how many went.
    pub fn prune_backups(&self, keep: usize) -> Result<usize> {
        if keep == 0 { return Ok(0); }
        let directory = self
            .path
            .parent()
            .ok_or("Missing database directory")?
            .join("backups");
        let Ok(entries) = std::fs::read_dir(&directory) else {
            return Ok(0);
        };
        let mut backups: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(is_backup_name)
            })
            .collect();
        // Timestamped names sort oldest first.
        backups.sort();
        let excess = backups.len().saturating_sub(keep);
        for path in &backups[..excess] {
            match std::fs::remove_file(path) {
                Ok(()) => {},
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
                Err(e) => return Err(error(e)),
            }
        }
        Ok(excess)
    }

    fn recovery_path(&self) -> PathBuf {
        self.path.with_file_name("recovery.json")
    }
    /// Writes the unsaved changes (not the whole budget) next to the database.
    fn save_recovery(&self) -> Result<()> {
        let Some(cache) = &self.cache else {
            return Ok(());
        };
        let path = self.recovery_path();
        let patch = Patch::diff(&cache.persisted, &cache.draft);
        if patch.is_empty() {
            if path.exists() {
                std::fs::remove_file(path).map_err(error)?;
            }
            return Ok(());
        }
        let draft = serde_json::json!({
            "version": 2,
            "baseRevision": revision(&cache.token),
            "patch": patch,
        });
        use std::io::Write;
        let temporary = path.with_extension("json.tmp");
        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(error)?;
        file.write_all(&serde_json::to_vec(&draft).map_err(error)?)
            .map_err(error)?;
        file.sync_all().map_err(error)?;
        std::fs::rename(temporary, path).map_err(error)
    }
    /// Returns `{ budget, matches }` for an unsaved draft left by an earlier session.
    /// `matches` is true when the draft was based on the budget that is saved now.
    pub fn read_recovery(&mut self) -> Result<Option<Value>> {
        let path = self.recovery_path();
        if !path.exists() {
            return Ok(None);
        }
        let value: Value =
            serde_json::from_slice(&std::fs::read(path).map_err(error)?).map_err(error)?;
        if value.get("version").and_then(Value::as_u64) == Some(2) {
            let base = value
                .get("baseRevision")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let patch: Patch =
                serde_json::from_value(value.get("patch").cloned().unwrap_or_default())
                    .map_err(error)?;
            let saved = revision(&self.status()?.token).to_owned();
            let cache = self.cache()?;
            let mut budget = cache.persisted.clone();
            patch.apply(&mut budget);
            let matches = base == saved && revision(&cache.token) == saved;
            keep_hidden(&mut cache.hidden, &budget.transactions);
            return Ok(Some(serde_json::json!({ "budget": for_window(&budget), "matches": matches })));
        }
        // Earlier versions stored the whole draft; its base cannot be verified.
        Ok(Some(serde_json::json!({
            "budget": value.get("budget").cloned().unwrap_or_default(),
            "matches": false,
        })))
    }
    pub fn discard_recovery(&self) -> Result<()> {
        let path = self.recovery_path();
        if path.exists() {
            std::fs::remove_file(path).map_err(error)?;
        }
        Ok(())
    }
}

/// YNAB's Register and Plan CSVs, with the full JSON beside them: the files of a YNAB export
/// folder, by name.
pub fn export_ynab(budget: &Budget) -> Result<[(&'static str, Vec<u8>); 3]> {
    Ok([
        ("Register.csv", crate::ynab::export_register(budget)?.into_bytes()),
        ("Plan.csv", crate::ynab::export_plan(budget)?.into_bytes()),
        ("UrBudget.json", export_recovery_draft(budget)?),
    ])
}

/// A full backup of the budget as JSON. Only a valid budget is exported this way.
pub fn export(budget: &Budget) -> Result<Vec<u8>> {
    validate(budget)?;
    export_recovery_draft(budget)
}

/// Preserve an unsaved draft even when validation is what prevents recovery.
/// This is explicitly a recovery file, not a validated backup or a database write.
pub fn export_recovery_draft(budget: &Budget) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(budget).map_err(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tests::{purchase, sample};

    #[test]
    fn assignment_extension_metadata_migrates_and_patch_only_edits_survive_reopen() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("budget.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let original = sample();
        let token = store.status().unwrap().token;
        store.save(&original, &token).unwrap();
        // Simulate a database created before assignment metadata had its own optional table.
        store.connection.execute_batch("DROP TABLE assignment_extras").unwrap();
        drop(store);
        let mut store = Store::open(&path).unwrap();
        let before = store.load().unwrap();
        assert_eq!(before.budget, original);
        let mut after = before.budget.clone();
        after.assignments[0].extra.insert("sourceId".into(), serde_json::json!("legacy-row"));
        let patch = Patch::diff(&before.budget, &after);
        assert!(patch.assignments.is_some(), "metadata-only changes are real assignment changes");
        store.apply_patch(patch).unwrap();
        store.commit(&before.token).unwrap();
        drop(store);
        assert_eq!(Store::open(&path).unwrap().load().unwrap().budget.assignments[0].extra["sourceId"], "legacy-row");
    }
    fn transaction(id: &str) -> Transaction {
        let mut t = purchase();
        t.id = id.into();
        t
    }
    #[test]
    fn recovery_export_preserves_invalid_draft_without_changing_saved_data_or_validation() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("budget.db")).unwrap();
        let original = store.load().unwrap();
        let mut invalid = original.budget.clone();
        invalid.name = "Unsaved recovery".into();
        invalid.extra.insert("calculationAsOfMonth".into(), serde_json::json!("not-a-month"));
        invalid.extra.insert("sourceExport".into(), serde_json::json!({"preserve": "full source"}));
        store.replace_draft(invalid.clone()).unwrap();
        assert!(export(&invalid).is_err());
        let full = store.full_draft(None).unwrap();
        let exported: Budget = serde_json::from_slice(&export_recovery_draft(&full).unwrap()).unwrap();
        assert_eq!(exported, invalid);
        assert!(validate(&exported).is_err(), "exporting does not silently repair or certify the draft");
        assert_eq!(store.full_draft(None).unwrap(), invalid);
        let (saved, status) = store.saved().unwrap();
        assert_eq!(saved, original.budget);
        assert_eq!(status.token, original.token);
    }
    /// Loads the budget, applies `edit` to the draft through a patch and commits it.
    fn edit(store: &mut Store, f: impl FnOnce(&mut Budget)) -> Status {
        let before = store.load_light().unwrap();
        let mut after = before.budget.clone();
        f(&mut after);
        let applied = store
            .apply_patch(Patch::diff(&before.budget, &after))
            .unwrap();
        assert!(applied.recovery_error.is_none());
        store.commit(&before.token).unwrap()
    }
    #[test]
    fn scheduled_posting_moves_transactions_dated_ahead_into_scheduled() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("budget.db")).unwrap();
        let token = store.load().unwrap().token;
        let mut budget = sample();
        let ahead = |id: &str| {
            let mut t = transaction(id);
            t.date = "2999-01-01".into();
            t.status = Cleared::Uncleared;
            t
        };
        let mut imported = ahead("imported");
        imported.extra.insert("source".into(), serde_json::json!({ "id": "ynab-imported" }));
        budget.transactions.extend([ahead("typed"), imported, transaction("today")]);
        let saved = store.save(&budget, &token).unwrap();

        let posted = store.run_schedules(&saved.token, "2026-10-19").unwrap();
        let ids = |b: &Budget| b.transactions.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&posted.budget), ["imported", "today"]);
        // Saved: a fresh load has the one-time schedule, and the import keeps its source.
        let loaded = store.load().unwrap();
        let schedules = crate::schedules::read(&loaded.budget).unwrap();
        assert_eq!(schedules.len(), 1);
        assert_eq!((schedules[0].id.as_str(), schedules[0].frequency.as_str()), ("typed", "never"));
        assert_eq!(schedules[0].next_date.as_deref(), Some("2999-01-01"));
        assert!(loaded.budget.transactions.iter().any(|t| t.id == "imported" && t.extra.contains_key("source")));
        assert_eq!(loaded.token, posted.token);
    }
    #[test]
    fn a_transaction_a_command_adds_reaches_the_window_and_the_database_with_when_it_was_added() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("budget.db")).unwrap();
        let token = store.load().unwrap().token;
        let mut budget = sample();
        budget.transactions.push(transaction("old"));
        let saved = store.save(&budget, &token).unwrap();
        let window = store.load_light().unwrap().budget;

        // The command didn't stamp it; the time comes back in the patch. The one already there
        // stays without one.
        let changes = store
            .change_draft(None, |mut b| {
                b.transactions.push(transaction("new"));
                Ok(b)
            })
            .unwrap();
        let upserts = &changes.transactions.as_ref().unwrap().upsert;
        assert_eq!(upserts.len(), 1);
        let added = upserts[0].created_at().unwrap().to_owned();

        // The window applies it and sends it back; saved and loaded again, it's the same.
        let mut next = window.clone();
        changes.apply(&mut next);
        store.apply_patch(Patch::diff(&window, &next)).unwrap();
        store.commit(&saved.token).unwrap();
        let loaded = store.load().unwrap().budget;
        let created = |id: &str| loaded.transactions.iter().find(|t| t.id == id).unwrap().created_at().map(str::to_owned);
        assert_eq!((created("old"), created("new")), (None, Some(added)));
    }
    #[test]
    fn the_window_never_receives_import_source_data_and_never_drops_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("budget.db")).unwrap();
        let token = store.load().unwrap().token;
        let mut budget = sample();
        for id in ["a", "b"] {
            let mut t = transaction(id);
            t.extra.insert("source".into(), serde_json::json!({ "id": format!("ynab-{id}") }));
            t.extra.insert("flag".into(), "red".into());
            budget.transactions.push(t);
        }
        store.save(&budget, &token).unwrap();
        let saved_source = |store: &mut Store, id: &str| {
            let (b, _) = store.saved().unwrap();
            b.transactions.into_iter().find(|t| t.id == id).and_then(|t| t.extra.get("source").cloned())
        };
        let snapshot = store.load_light().unwrap();
        let window: Budget = serde_json::from_value(serde_json::to_value(&snapshot).unwrap()["budget"].clone()).unwrap();
        assert!(window.transactions.iter().all(|t| !t.extra.contains_key("source") && t.extra["flag"] == "red"));
        assert!(window.transactions.iter().all(|t| t.extra["hasImportSource"] == true));

        // An edit from the window keeps the source.
        let mut edited = window.clone();
        edited.transactions.iter_mut().find(|t| t.id == "a").unwrap().memo = "Edited".into();
        store.apply_patch(Patch::diff(&window, &edited)).unwrap();
        let token = store.commit(&snapshot.token).unwrap().token;
        assert_eq!(saved_source(&mut store, "a").unwrap()["id"], "ynab-a");

        // So does deleting a transaction and undoing it.
        let mut deleted = edited.clone();
        deleted.transactions.retain(|t| t.id != "b");
        store.apply_patch(Patch::diff(&edited, &deleted)).unwrap();
        let token = store.commit(&token).unwrap().token;
        assert!(saved_source(&mut store, "b").is_none());
        store.apply_patch(Patch::diff(&deleted, &edited)).unwrap();
        store.commit(&token).unwrap();
        assert_eq!(saved_source(&mut store, "b").unwrap()["id"], "ynab-b");

        // Changes worked out for the window leave it out; a transaction it sends gets it back.
        let changes = store
            .change_draft(None, |mut b| {
                b.transactions.iter_mut().for_each(|t| t.memo = "Changed".into());
                Ok(b)
            })
            .unwrap();
        let upserts = changes.transactions.unwrap().upsert;
        assert!(upserts.len() == 2 && upserts.iter().all(|t| !t.extra.contains_key("source")));
        let mut sent = upserts[0].clone();
        store.restore_transaction(&mut sent).unwrap();
        assert!(sent.extra.contains_key("source"));

        // And so does replacing the whole draft with the window's copy.
        store.replace_draft(edited).unwrap();
        store
            .read_draft(None, |b| {
                assert!(b.transactions.iter().all(|t| t.extra.contains_key("source")));
                Ok(())
            })
            .unwrap();
    }
    #[test]
    fn legacy_import_roles_reach_the_window_without_source_and_explicit_clear_survives_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let mut store = Store::open(&path).unwrap();
        let token = store.load().unwrap().token;
        let mut budget = sample();
        budget.extra.insert("sourceExport".into(), serde_json::json!({"data":{"plan":{
            "categories":[{"id":"rta","internal":true,"name":"Inflow: Ready to Assign"},{"id":"food","name":"Food"}],
            "payees":[{"id":"opening","name":"Starting Balance"}]
        }}}));
        for (id, category) in [("opening", "rta"), ("rta", "rta"), ("cleared", "food"), ("unknown", "unknown"), ("named", "unknown"), ("explicit", "unknown")] {
            let mut t = transaction(id);
            t.splits[0].category_id = None;
            t.extra.insert("source".into(), serde_json::json!({"category_id":category,"payee_id":id}));
            if id == "opening" {
                t.payee = "Starting Balance".into();
                t.splits[0].extra.insert("source".into(), Value::Null);
            }
            if matches!(id, "unknown" | "named" | "explicit") {
                t.payee = "Starting Balance".into();
                t.extra.get_mut("source").unwrap()["payee_name"] = serde_json::json!("Starting Balance");
            }
            if id == "named" {
                t.extra.get_mut("source").unwrap()["category_name"] = serde_json::json!("Inflow: Ready to Assign");
            }
            if id == "explicit" { t.splits[0].extra.insert("unassigned".into(), Value::Bool(true)); }
            budget.transactions.push(t);
        }
        store.save(&budget, &token).unwrap();
        let snapshot = store.load_light().unwrap();
        let window: Budget = serde_json::from_value(serde_json::to_value(&snapshot).unwrap()["budget"].clone()).unwrap();
        assert!(!window.extra.contains_key("sourceExport"));
        assert!(window.transactions.iter().all(|t| !t.extra.contains_key("source")));
        let find = |id: &str| window.transactions.iter().find(|t| t.id == id).unwrap();
        assert_eq!(find("opening").extra["startingBalance"], true);
        assert_eq!(find("opening").splits[0].extra["unassigned"], true);
        assert_eq!(find("rta").splits[0].extra["unassigned"], true);
        assert_eq!(find("cleared").splits[0].extra["unassigned"], false);
        assert!(!find("unknown").splits[0].extra.contains_key("unassigned"));
        assert!(!find("unknown").extra.contains_key("startingBalance"), "a payee alone cannot prove an opening entry");
        assert_eq!(find("named").extra["startingBalance"], true);
        assert_eq!(find("named").splits[0].extra["unassigned"], true);
        assert_eq!(find("explicit").extra["startingBalance"], true);
        let mut edited = window.clone();
        edited.transactions.iter_mut().find(|t| t.id == "rta").unwrap().splits[0].extra.insert("unassigned".into(), Value::Bool(false));
        store.apply_patch(Patch::diff(&window, &edited)).unwrap();
        store.commit(&snapshot.token).unwrap();
        drop(store);
        let mut reopened = Store::open(&path).unwrap();
        let saved = reopened.load().unwrap().budget;
        let cleared = saved.transactions.iter().find(|t| t.id == "rta").unwrap();
        assert_eq!(cleared.extra["source"]["category_id"], "rta");
        assert_eq!(cleared.splits[0].extra["unassigned"], false);
        assert!(!crate::ledger::explicitly_unassigned(cleared, &cleared.splits[0]));
        assert_eq!(saved.transactions.len(), budget.transactions.len());
    }
    #[test]
    fn complete_budget_roundtrips_and_failed_commit_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let mut store = Store::open(&path).unwrap();
        let snapshot = store.load().unwrap();
        let mut budget = crate::domain::tests::sample();
        budget.transactions.push(crate::domain::tests::purchase());
        store.save(&budget, &snapshot.token).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.budget, budget);
        store.connection.execute_batch("CREATE TRIGGER fail_insert BEFORE INSERT ON transactions BEGIN SELECT RAISE(ABORT, 'Simulated disk write failure'); END;").unwrap();
        let before = store.load().unwrap();
        budget.name = "Must not persist".into();
        assert!(store.save(&budget, &before.token).is_err());
        let after = store.load().unwrap();
        assert_eq!(after.budget, loaded.budget);
        assert_eq!(after.token, before.token);
    }
    #[test]
    fn save_survives_reopen_and_backup_is_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let mut store = Store::open(&path).unwrap();
        let first = store.load().unwrap();
        let mut budget = first.budget;
        budget.name = "Persisted".into();
        store.save(&budget, &first.token).unwrap();
        let backup = store.backup().unwrap();
        drop(store);
        assert_eq!(Store::open(&path).unwrap().load().unwrap().budget, budget);
        assert_eq!(
            Store::open(Path::new(&backup))
                .unwrap()
                .load()
                .unwrap()
                .budget,
            budget
        );
    }
    #[test]
    fn pruning_keeps_the_newest_backups_and_leaves_other_files_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let store = Store::open(&path).unwrap();
        let backups = dir.path().join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        for stamp in ["20260101-000000", "20260102-000000", "20260103-000000", "20260104-000000"] {
            std::fs::write(backups.join(format!("budget-{stamp}.sqlite3")), b"x").unwrap();
        }
        std::fs::write(backups.join("notes.txt"), b"mine").unwrap();
        assert_eq!(store.prune_backups(2).unwrap(), 2);
        let mut left: Vec<_> = std::fs::read_dir(&backups)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["budget-20260103-000000.sqlite3", "budget-20260104-000000.sqlite3", "notes.txt"]);
    }
    #[test]
    fn portable_backup_restores_everything_without_old_hidden_or_optional_fields() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("budget.db")).unwrap();
        let mut original = sample();
        original.transactions.push(purchase());
        original.extra.insert("sourceExport".into(), serde_json::json!({"original": "import bytes"}));
        original.extra.insert("schedules".into(), serde_json::json!([]));
        original.transactions[0].extra.insert("source".into(), serde_json::json!({"originalId": "source-1"}));
        let token = store.status().unwrap().token;
        store.save(&original, &token).unwrap();
        let path = store.backup_portable().unwrap();
        assert!(path.ends_with(".json"));
        let portable: Budget = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(portable, original);
        let mut replacement = original.clone();
        replacement.extra.clear();
        replacement.transactions[0].extra.remove("source");
        replacement.name = "Restored clean backup".into();
        let token = store.status().unwrap().token;
        store.restore_backup(&replacement, &token).unwrap();
        assert_eq!(store.saved().unwrap().0, replacement);
        assert!(!store.full_draft(None).unwrap().extra.contains_key("sourceExport"));
        let before = std::fs::read_dir(dir.path().join("backups")).unwrap()
            .map(|e| e.unwrap().path()).find(|p| p.extension().is_some_and(|s| s == "sqlite3")).unwrap();
        assert_eq!(Store::open(&before).unwrap().saved().unwrap().0, original);
        let token = store.status().unwrap().token;
        store.restore_backup(&portable, &token).unwrap();
        assert_eq!(store.saved().unwrap().0, original);
    }
    #[test]
    fn every_new_backup_enforces_the_native_retention_setting_across_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let mut store = Store::open(&path).unwrap();
        assert_eq!(store.initialize_backup_retention(5).unwrap(), 5);
        let folder = dir.path().join("backups");
        let count = || std::fs::read_dir(&folder).unwrap().filter_map(|e| e.ok())
            .filter(|e| is_backup_name(&e.file_name().to_string_lossy())).count();
        for _ in 0..7 { store.backup().unwrap(); }
        assert_eq!(count(), 5);
        drop(store);
        store = Store::open(&path).unwrap();
        assert_eq!(store.initialize_backup_retention(0).unwrap(), 5);
        for _ in 0..7 { store.backup_portable().unwrap(); }
        assert_eq!(count(), 5);
        std::fs::write(folder.join("budget-my-notes.sqlite3"), "personal file").unwrap();
        for keep in [10, 30] {
            store.set_backup_retention(keep).unwrap();
            for _ in 0..keep + 2 { store.backup().unwrap(); }
            assert_eq!(count(), keep);
        }
        store.set_backup_retention(0).unwrap();
        store.backup().unwrap();
        store.backup_portable().unwrap();
        assert_eq!(count(), 32);
        assert_eq!(store.prune_backups(0).unwrap(), 0);
        assert!(folder.join("budget-my-notes.sqlite3").exists());
        assert!(store.set_backup_retention(1).is_err());
        assert_eq!(store.backup_retention().unwrap(), 0);
    }
    #[test]
    fn invalid_or_stale_restore_never_replaces_the_current_budget() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("budget.db")).unwrap();
        let original = store.saved().unwrap().0;
        let token = store.status().unwrap().token;
        let mut invalid = original.clone();
        invalid.schema_version = 9999;
        assert!(store.restore_backup(&invalid, &token).is_err());
        assert!(store.restore_backup(&sample(), "stale").is_err());
        assert_eq!(store.saved().unwrap().0, original);
        assert!(!dir.path().join("backups").exists());
    }
    #[test]
    fn competing_writer_is_rejected_without_losing_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let mut a = Store::open(&path).unwrap();
        let mut b = Store::open(&path).unwrap();
        let first = a.load().unwrap();
        let second = b.load().unwrap();
        let mut changed = second.budget;
        changed.name = "Other window".into();
        b.save(&changed, &second.token).unwrap();
        assert!(a
            .save(&first.budget, &first.token)
            .unwrap_err()
            .starts_with("CONFLICT:"));
        assert_eq!(a.load().unwrap().budget.name, "Other window");
    }
    #[test]
    fn raw_sql_change_is_detected_even_without_revision_bump() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let mut a = Store::open(&path).unwrap();
        let first = a.load().unwrap();
        let other = Connection::open(&path).unwrap();
        other
            .execute("UPDATE metadata SET name='Raw writer'", [])
            .unwrap();
        assert_ne!(a.status().unwrap().token, first.token);
        assert!(a.save(&first.budget, &first.token).is_err());
    }
    #[test]
    fn validation_failure_keeps_previous_database_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("budget.db")).unwrap();
        let snapshot = store.load().unwrap();
        let mut bad = snapshot.budget.clone();
        bad.currency = "INVALID".into();
        assert!(store.save(&bad, &snapshot.token).is_err());
        assert_eq!(store.load().unwrap().budget, snapshot.budget);
    }
    #[test]
    fn metadata_round_trips_without_dropping_unknown_ynab_fields() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("budget.db")).unwrap();
        let mut b = crate::domain::tests::sample();
        b.extra.insert(
            "sourceExport".into(),
            serde_json::json!({"futureField":[1,2,3]}),
        );
        b.categories[0]
            .extra
            .insert("note".into(), serde_json::json!("Original note"));
        let token = store.status().unwrap().token;
        store.save(&b, &token).unwrap();
        assert_eq!(store.load().unwrap().budget, b);
    }

    #[test]
    fn commits_write_only_changes_and_reload_identically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let mut store = Store::open(&path).unwrap();
        let token = store.status().unwrap().token;
        let mut b = sample();
        b.transactions = (0..20).map(|i| transaction(&format!("t{i:02}"))).collect();
        store.save(&b, &token).unwrap();
        // Edit, delete, append, assign, rename and add an extra in one commit.
        edit(&mut store, |d| {
            d.transactions[3].memo = "Edited".into();
            d.transactions.retain(|t| t.id != "t07");
            d.transactions.push(transaction("new"));
            d.assignments[0].amount = 123;
            d.name = "Renamed".into();
            d.extra.insert("ynabMonths".into(), serde_json::json!([{"month":"2026-10-01","note":"Hi"}]));
        });
        // Reorder, then delete an account's transactions and the account itself.
        edit(&mut store, |d| {
            d.transactions.reverse();
            d.categories.reverse();
        });
        let expected = store.load().unwrap().budget;
        drop(store);
        let mut reopened = Store::open(&path).unwrap();
        let loaded = reopened.load().unwrap().budget;
        assert_eq!(loaded, expected);
        assert_eq!(loaded.name, "Renamed");
        assert_eq!(loaded.transactions.first().unwrap().id, "new");
        assert!(!loaded.transactions.iter().any(|t| t.id == "t07"));
        assert_eq!(loaded.transactions.iter().find(|t| t.id == "t03").unwrap().memo, "Edited");
        assert_eq!(loaded.categories[0].id, "payment");
        assert_eq!(loaded.assignments[0].amount, 123);
        edit(&mut reopened, |d| {
            d.transactions.retain(|t| t.account_id != "card");
            d.accounts.retain(|a| a.id != "card");
            d.categories.retain(|c| c.id != "payment");
        });
        let after = reopened.load().unwrap().budget;
        assert_eq!(after.accounts.len(), 1);
        assert!(after.transactions.is_empty());
    }

    #[test]
    fn heavy_extras_stay_in_the_database_but_not_in_the_window() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("budget.db")).unwrap();
        let token = store.status().unwrap().token;
        let mut b = sample();
        b.extra.insert("sourceExport".into(), serde_json::json!({"data":{"server_knowledge":7}}));
        store.save(&b, &token).unwrap();
        let light = store.load_light().unwrap();
        assert!(!light.budget.extra.contains_key("sourceExport"));
        assert!(!serde_json::to_string(&store.load().unwrap()).unwrap().contains("server_knowledge"));
        edit(&mut store, |d| {
            d.extra.insert("ynabMonths".into(), serde_json::json!([]));
        });
        let full = store.load().unwrap().budget;
        assert_eq!(full.extra["sourceExport"], serde_json::json!({"data":{"server_knowledge":7}}));
        assert_eq!(store.full_draft(None).unwrap(), full);
    }

    #[test]
    fn commit_rejects_competing_writers_and_keeps_the_draft() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let mut store = Store::open(&path).unwrap();
        let snapshot = store.load_light().unwrap();
        let mut draft = snapshot.budget.clone();
        draft.name = "Mine".into();
        store
            .apply_patch(Patch::diff(&snapshot.budget, &draft))
            .unwrap();
        Connection::open(&path)
            .unwrap()
            .execute("UPDATE metadata SET revision=revision+1", [])
            .unwrap();
        assert!(store.commit(&snapshot.token).unwrap_err().starts_with("CONFLICT:"));
        assert_eq!(store.read_draft(None, |b| Ok(b.name.clone())).unwrap(), "Mine");
        assert_eq!(store.load().unwrap().budget.name, "My budget");
    }

    #[test]
    fn scheduled_posting_never_discards_unsaved_edits() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("budget.db")).unwrap();
        let mut budget = sample();
        budget.extra.insert("schedules".into(), serde_json::json!([{
            "id": "rent", "nextDate": "2026-10-01", "anchorDate": "2026-10-01",
            "frequency": "monthly", "transaction": purchase()
        }]));
        let token = store.status().unwrap().token;
        let token = store.save(&budget, &token).unwrap().token;
        let mut draft = budget.clone();
        draft.name = "Unsaved changes".into();
        store.apply_patch(Patch::diff(&budget, &draft)).unwrap();

        assert!(store.run_schedules(&token, "2026-10-01").is_err());
        assert_eq!(store.full_draft(None).unwrap(), draft);
        assert_eq!(store.saved().unwrap().0, budget);
        assert_eq!(store.read_recovery().unwrap().unwrap()["budget"]["name"], "Unsaved changes");

        let token = store.commit(&token).unwrap().token;
        let posted = store.run_schedules(&token, "2026-10-01").unwrap();
        assert_eq!(posted.budget.name, "Unsaved changes");
        assert_eq!(posted.budget.transactions.len(), 1);
        assert_eq!(posted.token, store.saved().unwrap().1.token);
        let repeated = store.run_schedules(&posted.token, "2026-10-01").unwrap();
        assert_eq!(repeated.token, posted.token);
        assert_eq!(repeated.budget, posted.budget);
    }

    #[test]
    fn snapshot_and_save_revisions_stay_bound_to_their_own_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let mut store = Store::open(&path).unwrap();
        let (initial, initial_status) = store.saved().unwrap();
        let mut next = initial.clone();
        next.name = "First save".into();
        let acknowledged = store.save(&next, &initial_status.token).unwrap();
        let (read, read_status) = store.saved().unwrap();
        let mut writer = Store::open(&path).unwrap();
        let other_token = writer.status().unwrap().token;
        let mut other = next.clone();
        other.name = "Another writer".into();
        writer.save(&other, &other_token).unwrap();

        assert_eq!(read, next);
        assert_eq!(read_status.revision, acknowledged.revision);
        assert_eq!(acknowledged.revision, initial_status.revision + 1);
        assert_eq!(store.revision().unwrap(), acknowledged.revision + 1);
    }

    #[test]
    fn fresh_start_source_checks_the_token_and_restores_hidden_source_without_changing_the_draft() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let mut store = Store::open_budget(&path, "original").unwrap();
        let mut budget = sample();
        let mut t = purchase();
        t.extra.insert("source".into(), serde_json::json!({"debt_transaction_type": "interest"}));
        budget.transactions.push(t);
        let token = store.status().unwrap().token;
        let token = store.save(&budget, &token).unwrap().token;
        let window = for_window(&budget);
        let mut edited = window.clone();
        edited.name = "Current window draft".into();
        let source = store.fresh_start_source(Some(Patch::diff(&window, &edited)), &token).unwrap();
        assert_eq!(source.name, edited.name);
        assert_eq!(source.transactions[0].extra["source"]["debt_transaction_type"], "interest");
        assert_eq!(store.full_draft(None).unwrap(), budget);
        assert!(store.fresh_start_source(None, &token.replacen("original", "other", 1)).is_err());
        Connection::open(&path).unwrap().execute("UPDATE metadata SET revision=revision+1", []).unwrap();
        assert!(store.fresh_start_source(None, &token).is_err());
    }

    #[test]
    fn recovery_holds_unsaved_changes_until_commit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let mut store = Store::open(&path).unwrap();
        let snapshot = store.load_light().unwrap();
        let mut draft = snapshot.budget.clone();
        draft.name = "Unsaved".into();
        draft.extra.insert("note".into(), serde_json::json!("x".repeat(100_000)));
        store
            .apply_patch(Patch::diff(&snapshot.budget, &draft))
            .unwrap();
        // The token is unchanged: recovery never touches the database.
        assert_eq!(store.status().unwrap().token, snapshot.token);
        drop(store);
        let mut reopened = Store::open(&path).unwrap();
        reopened.load_light().unwrap();
        let recovered = reopened.read_recovery().unwrap().unwrap();
        assert_eq!(recovered["budget"]["name"], "Unsaved");
        assert_eq!(recovered["matches"], true);
        // A save elsewhere makes the draft stale.
        let token = reopened.status().unwrap().token;
        let mut other = snapshot.budget.clone();
        other.name = "Elsewhere".into();
        reopened.save(&other, &token).unwrap();
        assert_eq!(reopened.read_recovery().unwrap().unwrap()["matches"], false);
        reopened.discard_recovery().unwrap();
        assert!(reopened.read_recovery().unwrap().is_none());
        // Committing a draft removes its recovery file.
        let s = reopened.load_light().unwrap();
        let mut d = s.budget.clone();
        d.name = "Saved".into();
        reopened.apply_patch(Patch::diff(&s.budget, &d)).unwrap();
        assert!(reopened.read_recovery().unwrap().is_some());
        reopened.commit(&s.token).unwrap();
        assert!(reopened.read_recovery().unwrap().is_none());
    }

    #[test]
    fn tokens_from_one_budget_never_commit_into_another() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = Store::open_budget(&dir.path().join("a.db"), "budget-a").unwrap();
        let mut b = Store::open_budget(&dir.path().join("b.db"), "budget-b").unwrap();
        let token_a = a.load_light().unwrap().token;
        let snapshot_b = b.load_light().unwrap();
        assert_ne!(token_a, snapshot_b.token, "same revision, different budgets");
        let mut draft = snapshot_b.budget.clone();
        draft.name = "Wrong budget".into();
        b.apply_patch(Patch::diff(&snapshot_b.budget, &draft)).unwrap();
        assert!(b.commit(&token_a).unwrap_err().starts_with("CONFLICT:"));
        assert!(b.commit(&snapshot_b.token).is_ok());
    }

    #[test]
    fn version_one_recovery_files_are_still_offered() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let mut store = Store::open(&path).unwrap();
        std::fs::write(
            path.with_file_name("recovery.json"),
            serde_json::to_vec(&serde_json::json!({"budget": {"name": "Old draft"}, "base": "x"})).unwrap(),
        )
        .unwrap();
        let recovered = store.read_recovery().unwrap().unwrap();
        assert_eq!(recovered["budget"]["name"], "Old draft");
        assert_eq!(recovered["matches"], false);
    }

    #[test]
    fn a_synced_save_writes_the_budget_and_its_base_together() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let mut store = Store::open(&path).unwrap();
        let mut b = sample();
        b.extra.insert("sourceExport".into(), serde_json::json!({"data":{"server_knowledge":7}}));
        store.save(&b, &store.status().unwrap().token).unwrap();
        assert!(store.sync_base().unwrap().is_none());
        let (light, _) = split(b.clone());
        let mut merged = light.clone();
        merged.transactions.push(transaction("from-phone"));
        let token = store.status().unwrap().token;
        store.save_synced(&merged, &token, 3, &light).unwrap();
        drop(store);
        let mut reopened = Store::open(&path).unwrap();
        let (saved, _) = reopened.saved().unwrap();
        // Heavy extras stay; the base is kept as given.
        assert_eq!(saved.extra["sourceExport"]["data"]["server_knowledge"], 7);
        assert_eq!(saved.transactions.len(), 1);
        assert_eq!(reopened.sync_base().unwrap(), Some((3, light.clone())));
        assert_eq!(reopened.sync_base_revision().unwrap(), Some(3));
        // A failed write leaves neither the budget nor the base changed.
        reopened.connection.execute_batch("CREATE TRIGGER fail_insert BEFORE INSERT ON transactions BEGIN SELECT RAISE(ABORT, 'Simulated disk write failure'); END;").unwrap();
        let mut more = merged.clone();
        more.transactions.push(transaction("another"));
        let token = reopened.status().unwrap().token;
        assert!(reopened.save_synced(&more, &token, 4, &merged).is_err());
        assert_eq!(reopened.sync_base_revision().unwrap(), Some(3));
        assert_eq!(reopened.saved().unwrap().0.transactions.len(), 1);
        // Never under unsaved edits.
        let snapshot = reopened.load_light().unwrap();
        let mut draft = snapshot.budget.clone();
        draft.name = "Unsaved".into();
        reopened.apply_patch(Patch::diff(&snapshot.budget, &draft)).unwrap();
        assert!(reopened.save_synced(&merged, &snapshot.token, 4, &merged).is_err());
    }

    #[test]
    fn extras_json_splices_heavy_fields() {
        let heavy = serde_json::json!({"sourceExport": {"a": 1}});
        let fragment = fragment(heavy.as_object().unwrap()).unwrap();
        for light in [serde_json::json!({}), serde_json::json!({"x": [1]})] {
            let joined: Value =
                serde_json::from_str(&extras_json(light.as_object().unwrap(), &fragment).unwrap()).unwrap();
            assert_eq!(joined["sourceExport"]["a"], 1);
        }
        assert_eq!(extras_json(&Map::new(), "").unwrap(), "{}");
    }
}
