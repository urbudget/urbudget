//! The list of budgets. Each budget is its own SQLite file (with its recovery file,
//! backups, imports and exports beside it). The first budget is the original
//! `<data>/budget.sqlite3`, adopted in place; new budgets live in `<data>/budgets/<id>/`.
use crate::t;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, String>;
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    /// Folder relative to the data directory; "" is the data directory itself.
    pub folder: String,
    #[serde(default)]
    pub last_opened: Option<String>,
}
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Registry {
    budgets: Vec<Entry>,
    current: Option<String>,
}
/// A budget as the budget switcher shows it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub id: String,
    pub name: String,
    pub currency: String,
    pub current: bool,
    pub last_opened: Option<String>,
    pub path: String,
    pub missing: bool,
}

pub struct Budgets {
    data_dir: PathBuf,
    registry: Registry,
}

/// A new budget's id: when it was made, then random bits. The time alone isn't enough: a clock
/// that counts in microseconds (macOS) gives two installs, or two budgets made one after the
/// other, the same id, and sync would then take two unrelated budgets for one. Ids are opaque
/// everywhere (the sync server takes up to 64 letters and digits).
pub fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static MADE: AtomicU64 = AtomicU64::new(0);
    // RandomState's keys come from the system's random source, and differ for every state made.
    let mut random = std::collections::hash_map::RandomState::new().build_hasher();
    random.write_u64(MADE.fetch_add(1, Ordering::Relaxed));
    format!(
        "b{:x}{:08x}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        random.finish() as u32
    )
}

impl Budgets {
    /// Reads `<data>/budgets.json`, creating it on first run with the existing budget.
    pub fn load(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir).map_err(error)?;
        let file = data_dir.join("budgets.json");
        let mut registry: Registry = if file.exists() {
            serde_json::from_slice(&std::fs::read(&file).map_err(error)?).map_err(error)?
        } else {
            Registry::default()
        };
        if registry.budgets.is_empty() {
            registry.budgets.push(Entry {
                id: new_id(),
                folder: String::new(),
                last_opened: None,
            });
        }
        let current_known = registry
            .current
            .as_ref()
            .is_some_and(|id| registry.budgets.iter().any(|b| &b.id == id));
        if !current_known {
            registry.current = Some(registry.budgets[0].id.clone());
        }
        let budgets = Self {
            data_dir: data_dir.to_path_buf(),
            registry,
        };
        budgets.save()?;
        Ok(budgets)
    }
    /// The open budget's id and database, read without writing anything (for another process).
    pub fn peek(data_dir: &Path) -> Result<(String, PathBuf)> {
        let raw = std::fs::read(data_dir.join("budgets.json"))
            .map_err(|_| t!("Open UrBudget once before connecting to it."))?;
        let registry: Registry = serde_json::from_slice(&raw).map_err(error)?;
        let entry = registry
            .current
            .as_ref()
            .and_then(|id| registry.budgets.iter().find(|b| &b.id == id))
            .or(registry.budgets.first())
            .ok_or_else(|| t!("UrBudget has no budgets yet."))?;
        Ok((entry.id.clone(), data_dir.join(&entry.folder).join("budget.sqlite3")))
    }
    fn save(&self) -> Result<()> {
        let file = self.data_dir.join("budgets.json");
        let temporary = file.with_extension("json.tmp");
        std::fs::write(
            &temporary,
            serde_json::to_vec_pretty(&self.registry).map_err(error)?,
        )
        .map_err(error)?;
        std::fs::rename(temporary, file).map_err(error)
    }
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
    fn entry(&self, id: &str) -> Result<&Entry> {
        self.registry
            .budgets
            .iter()
            .find(|b| b.id == id)
            .ok_or_else(|| t!("That budget no longer exists."))
    }
    pub fn current(&self) -> &str {
        self.registry.current.as_deref().unwrap_or_default()
    }
    pub fn database(&self, id: &str) -> Result<PathBuf> {
        Ok(self
            .data_dir
            .join(&self.entry(id)?.folder)
            .join("budget.sqlite3"))
    }
    /// Registers a new, empty budget folder and returns its id.
    pub fn create(&mut self) -> Result<String> {
        let id = new_id();
        let folder = format!("budgets/{id}");
        std::fs::create_dir_all(self.data_dir.join(&folder)).map_err(error)?;
        self.registry.budgets.push(Entry {
            id: id.clone(),
            folder,
            last_opened: None,
        });
        self.save()?;
        Ok(id)
    }
    /// Registers an empty folder for a budget that already has an id elsewhere (one synced from
    /// another computer), so both computers know it by the same id.
    pub fn adopt(&mut self, id: &str) -> Result<()> {
        if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(t!("That budget id isn't valid."));
        }
        if self.registry.budgets.iter().any(|b| b.id == id) {
            return Err(t!("That budget is already on this computer."));
        }
        let folder = format!("budgets/{id}");
        std::fs::create_dir_all(self.data_dir.join(&folder)).map_err(error)?;
        self.registry.budgets.push(Entry {
            id: id.to_owned(),
            folder,
            last_opened: None,
        });
        self.save()
    }
    pub fn contains(&self, id: &str) -> bool {
        self.registry.budgets.iter().any(|b| b.id == id)
    }
    /// Undoes `create` when the new budget could not be written. Only for brand-new folders.
    pub fn forget_new(&mut self, id: &str) -> Result<()> {
        let entry = self.entry(id)?.clone();
        if !entry.folder.starts_with("budgets/") || self.current() == id {
            return Err(t!("Only a budget that was just created can be discarded."));
        }
        self.registry.budgets.retain(|b| b.id != id);
        self.save()?;
        let _ = std::fs::remove_dir_all(self.data_dir.join(&entry.folder));
        Ok(())
    }
    pub fn set_current(&mut self, id: &str) -> Result<()> {
        self.entry(id)?;
        self.registry.current = Some(id.into());
        let now = chrono::Utc::now().to_rfc3339();
        if let Some(e) = self.registry.budgets.iter_mut().find(|b| b.id == id) {
            e.last_opened = Some(now);
        }
        self.save()
    }
    /// Moves a budget that is not open into `<data>/trash/`. Nothing is deleted.
    pub fn remove(&mut self, id: &str) -> Result<()> {
        if self.current() == id {
            return Err(t!("Open another budget before deleting this one."));
        }
        if self.registry.budgets.len() <= 1 {
            return Err(t!("You need at least one budget."));
        }
        let entry = self.entry(id)?.clone();
        let trash = self.data_dir.join("trash").join(format!(
            "{id}-{}",
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        ));
        std::fs::create_dir_all(&trash).map_err(error)?;
        let source = self.data_dir.join(&entry.folder);
        if entry.folder.is_empty() {
            // The original budget shares the data directory: move only its own files.
            for name in [
                "budget.sqlite3",
                "budget.sqlite3-wal",
                "budget.sqlite3-shm",
                "recovery.json",
            ] {
                let file = source.join(name);
                if file.exists() {
                    std::fs::rename(&file, trash.join(name)).map_err(error)?;
                }
            }
        } else if source.exists() {
            std::fs::remove_dir(&trash).map_err(error)?;
            std::fs::rename(&source, &trash).map_err(error)?;
        }
        self.registry.budgets.retain(|b| b.id != id);
        self.save()
    }
    /// Every budget with its saved name and currency, most recently opened first.
    pub fn list(&self) -> Vec<Listing> {
        let mut list: Vec<Listing> = self
            .registry
            .budgets
            .iter()
            .map(|e| {
                let path = self.data_dir.join(&e.folder).join("budget.sqlite3");
                let meta = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .and_then(|c| {
                        c.query_row("SELECT name,currency FROM metadata WHERE id=1", [], |r| {
                            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                        })
                    })
                    .ok();
                Listing {
                    id: e.id.clone(),
                    name: meta
                        .as_ref()
                        .map(|m| m.0.clone())
                        .unwrap_or_else(|| t!("Unavailable budget")),
                    currency: meta.map(|m| m.1).unwrap_or_default(),
                    current: Some(&e.id) == self.registry.current.as_ref(),
                    last_opened: e.last_opened.clone(),
                    missing: !path.exists(),
                    path: path.to_string_lossy().into_owned(),
                }
            })
            .collect();
        list.sort_by(|a, b| b.current.cmp(&a.current).then(b.last_opened.cmp(&a.last_opened)));
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn ids_made_at_once_differ() {
        let ids: std::collections::HashSet<String> = (0..10_000).map(|_| new_id()).collect();
        assert_eq!(ids.len(), 10_000);
        assert!(ids.iter().all(|id| id.len() <= 64 && id.starts_with('b') && id[1..].chars().all(|c| c.is_ascii_hexdigit())));
    }

    #[test]
    fn adopts_the_existing_budget_in_place_and_remembers_the_current_one() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("budget.sqlite3");
        Store::open(&original).unwrap();
        let mut budgets = Budgets::load(dir.path()).unwrap();
        let first = budgets.current().to_string();
        assert_eq!(budgets.database(&first).unwrap(), original);
        let second = budgets.create().unwrap();
        budgets.set_current(&second).unwrap();
        assert!(budgets
            .database(&second)
            .unwrap()
            .starts_with(dir.path().join("budgets")));
        let reloaded = Budgets::load(dir.path()).unwrap();
        assert_eq!(reloaded.current(), second);
        assert_eq!(reloaded.list().len(), 2);
    }

    #[test]
    fn deleting_moves_a_closed_budget_to_trash() {
        let dir = tempfile::tempdir().unwrap();
        let mut budgets = Budgets::load(dir.path()).unwrap();
        let original = budgets.current().to_string();
        Store::open(&budgets.database(&original).unwrap()).unwrap();
        let other = budgets.create().unwrap();
        Store::open(&budgets.database(&other).unwrap()).unwrap();
        assert!(budgets.remove(&original).is_err(), "the open budget stays");
        budgets.set_current(&other).unwrap();
        budgets.remove(&original).unwrap();
        assert!(!dir.path().join("budget.sqlite3").exists());
        let trash: Vec<_> = std::fs::read_dir(dir.path().join("trash")).unwrap().collect();
        assert_eq!(trash.len(), 1);
        assert!(budgets.remove(&other).is_err(), "the last budget stays");
    }
}
