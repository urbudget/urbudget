//! Exports are saved where you choose: a save panel opens in Downloads with a suggested name.
//! The panel opens from here, so the window never names a path; it only hears where the file went.
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

/// The files saved this way while the app runs, which Show in folder may reveal.
#[derive(Default)]
pub struct Saved(Mutex<HashSet<PathBuf>>);

impl Saved {
    /// Whether `path` (canonical) was saved this way.
    pub fn contains(&self, path: &Path) -> bool {
        self.0.lock().is_ok_and(|saved| saved.contains(path))
    }
}

/// What's saved, for the panel's file types.
#[derive(Clone, Copy)]
pub enum Kind {
    Csv,
    Json,
    Workbook,
    /// A folder of files (the YNAB export).
    Folder,
}

/// `stem` as a file name, with the date and time: "Groceries 2026-10-24 153012.csv". Characters
/// a file name can't have on Windows or macOS become "_".
pub fn dated(stem: &str, extension: &str) -> String {
    let stem: String = stem
        .chars()
        .map(|c| if c.is_control() || r#"\/:*?"<>|"#.contains(c) { '_' } else { c })
        .collect();
    let stem = stem.trim().trim_start_matches('.');
    let stamp = chrono::Local::now().format("%Y-%m-%d %H%M%S");
    match extension {
        "" => format!("{stem} {stamp}"),
        _ => format!("{stem} {stamp}.{extension}"),
    }
}

/// Asks where to save `name`, starting in Downloads. `None` when the panel is cancelled.
pub fn ask(window: &WebviewWindow, name: &str, kind: Kind) -> Option<PathBuf> {
    let mut panel = window.dialog().file().set_parent(window).set_file_name(name);
    if let Ok(downloads) = window.path().download_dir() {
        panel = panel.set_directory(downloads);
    }
    let panel = match kind {
        Kind::Csv => panel.add_filter("CSV", &["csv"]),
        Kind::Json => panel.add_filter("JSON", &["json"]),
        Kind::Workbook => panel.add_filter(urbudget_services::i18n::tr("Excel workbook"), &["xlsx"]),
        Kind::Folder => panel,
    };
    panel.blocking_save_file()?.into_path().ok()
}

/// Asks where to save `name`, writes `contents` there and returns its path; `None` when the panel
/// is cancelled.
pub fn file(window: &WebviewWindow, name: &str, kind: Kind, contents: &[u8]) -> Result<Option<String>, String> {
    let Some(path) = ask(window, name, kind) else {
        return Ok(None);
    };
    write(&path, contents)?;
    Ok(Some(saved(window, &path)))
}

/// Writes an export, readable only by you where the system has such permissions.
pub fn write(path: &Path, contents: &[u8]) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|e| urbudget_services::t!("Couldn't save {path}: {e}", path = path.display(), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Where the drive allows it: a USB stick or network share may not, and the file is saved.
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Remembers `path` as saved, so Show in folder can reveal it, and returns it for the window.
pub fn saved(window: &WebviewWindow, path: &Path) -> String {
    if let (Ok(canonical), Ok(mut saved)) = (path.canonicalize(), window.state::<Saved>().0.lock()) {
        saved.insert(canonical);
    }
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggested_names_are_plain_and_dated() {
        let name = dated("Visa: Joint/Bills", "csv");
        assert!(name.starts_with("Visa_ Joint_Bills 20"), "{name}");
        assert!(name.ends_with(".csv") && name.len() == "Visa_ Joint_Bills 2026-10-24 153012.csv".len(), "{name}");
        assert!(dated("Family budget (QA) for YNAB", "").starts_with("Family budget (QA) for YNAB 20"));
        assert!(dated(".hidden", "json").starts_with("hidden 20"));
        assert!(!dated("For YNAB", "").contains('.'));
    }
}
