//! Secrets the app keeps between launches (the account's session, the sync key, the assistant's
//! ChatGPT sign-in and API keys, the YNAB token, the SimpleFIN Access URL) are all in one sealed
//! file in the data folder, `secrets.bin` (XChaCha20-Poly1305, as sync uses). Its decrypted map
//! stays in memory while the file is unchanged. Only the file's key is kept by the system:
//!
//! - macOS and iOS: one Keychain item. The Keychain asks per item and per app signature, so one
//!   item read once per launch asks at most once, and a Developer ID build doesn't ask at all.
//! - Windows: `secrets.key` beside it, sealed with DPAPI for this Windows user.
//! - Android: sealed under a key in the Android Keystore (keystore.rs).
//! - Debug builds on a computer: `secrets-dev.bin`, with its key in a plain file beside it, so a
//!   rebuilt app never asks for the Keychain and never takes the installed app's secrets.
//!
//! Earlier versions kept each secret as its own item; one moves into the file the first time it's
//! read, and the item is removed.
use crate::cloud::crypto::{self, Key};
use crate::{i18n::tr, t};
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// What protects the secrets, as the window names it, or None where they can't be kept.
pub const STORE: Option<&str> = if cfg!(any(target_os = "macos", target_os = "ios")) {
    Some("Keychain")
} else if cfg!(windows) {
    Some("Windows encrypted storage")
} else if cfg!(target_os = "android") {
    Some("Android Keystore")
} else {
    None
};
/// `STORE` as a message names it, in the app's language ("The system" where there is none).
/// The services' status fields keep `STORE` itself.
pub fn store_name() -> &'static str {
    match STORE {
        Some("Keychain") => tr("Keychain"),
        Some("Windows encrypted storage") => tr("Windows encrypted storage"),
        Some("Android Keystore") => tr("Android Keystore"),
        Some(other) => other,
        None => tr("The system"),
    }
}
const AAD: &[u8] = b"urbudget:secrets";

// Debug builds on a computer, and tests, keep their own file with a plain key (`plain`).
#[cfg(any(test, all(debug_assertions, any(target_os = "macos", windows))))]
use plain as keeper;
#[cfg(not(any(test, all(debug_assertions, any(target_os = "macos", windows)))))]
use system as keeper;

static VAULT: OnceLock<Vault> = OnceLock::new();

/// Keeps secrets in `data_dir` from now on. Until then (and in `--mcp`) there are none.
pub fn init(data_dir: &Path) {
    let _ = VAULT.set(Vault::new(data_dir));
}
pub fn read(service: &str) -> Option<String> {
    VAULT.get()?.read(service)
}
pub fn save(service: &str, secret: &str) -> Result<(), String> {
    VAULT
        .get()
        .ok_or(())
        .and_then(|v| v.save(service, secret))
        .map_err(|_| t!("{store} couldn't save this.", store = store_name()))
}
/// Removes a secret; Ok(false) when there was none.
pub fn delete(service: &str) -> Result<bool, String> {
    match VAULT.get() {
        Some(v) => v.delete(service).map_err(|_| t!("{store} couldn't remove it.", store = store_name())),
        None => Ok(false),
    }
}

pub struct Vault {
    dir: PathBuf,
    state: Mutex<State>,
}
#[derive(Default)]
struct State {
    /// What the file holds, once read.
    secrets: Option<BTreeMap<String, String>>,
    /// Comparing the small encrypted file detects replacements even with identical timestamps
    /// and lengths, without reopening the system key store or decrypting unchanged contents.
    sealed: Option<Vec<u8>>,
    key: Option<Key>,
    /// The file is there but can't be opened now (Keychain access was denied), so nothing is
    /// written over it.
    locked: bool,
    /// Services already looked for among the earlier one-item-per-secret entries.
    checked: HashSet<String>,
}

impl Vault {
    pub fn new(dir: &Path) -> Self {
        Self { dir: dir.to_owned(), state: Mutex::default() }
    }
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn file(&self) -> PathBuf {
        self.dir.join(keeper::FILE)
    }
    fn key(&self, state: &mut State) -> Result<Option<Key>, ()> {
        if state.key.is_none() {
            state.key = keeper::load_key(&self.dir)?;
        }
        Ok(state.key)
    }
    /// Refreshes a changed file while keeping the system key and unchanged plaintext cached.
    fn load<'a>(&self, state: &'a mut State) -> &'a BTreeMap<String, String> {
        let read = std::fs::read(self.file());
        if !state.locked {
            match &read {
                Ok(sealed) if state.sealed.as_ref() == Some(sealed) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && state.sealed.is_none() => {}
                _ => state.secrets = None,
            }
        } else {
            // A temporarily unavailable key or file may be accessible on the next attempt.
            state.secrets = None;
        }
        if state.secrets.is_none() {
            state.locked = false;
            state.sealed = read.as_ref().ok().cloned();
            let secrets = match read {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
                Ok(sealed) => match self.key(state) {
                    Ok(None) => {
                        // Missing is not permission to replace an existing vault. It may be
                        // recoverable after a restore or once the system key is accessible.
                        state.locked = true;
                        BTreeMap::new()
                    }
                    Ok(Some(key)) => match open(&key, &sealed) {
                        Some(secrets) => secrets,
                        None => {
                            // A damaged file or mismatched key must not silently erase every
                            // other credential when the next sign-in saves its token.
                            state.locked = true;
                            // A different process may have replaced the system key. Retry it
                            // after failure, while keeping successful reads cached.
                            state.key = None;
                            BTreeMap::new()
                        }
                    },
                    Err(()) => {
                        state.locked = true;
                        BTreeMap::new()
                    }
                },
                Err(_) => {
                    state.locked = true;
                    BTreeMap::new()
                }
            };
            state.secrets = Some(secrets);
        }
        state.secrets.get_or_insert_default()
    }
    fn lock_file(&self) -> Result<std::fs::File, ()> {
        std::fs::create_dir_all(&self.dir).map_err(|_| ())?;
        // A process-local Mutex is insufficient when two installed copies share the vault.
        // Keep a separate, stable lock file across the entire read/modify/rename operation,
        // including creation of the encryption key. Dropping it releases the OS lock.
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let guard = options.open(self.file().with_extension("lock")).map_err(|_| ())?;
        lock_exclusive(&guard).map_err(|_| ())?;
        Ok(guard)
    }
    /// The caller holds the file lock through both persistence and legacy-item cleanup.
    fn change_locked(&self, state: &mut State, change: impl FnOnce(&mut BTreeMap<String, String>) -> bool) -> Result<bool, ()> {
        state.secrets = None;
        let mut secrets = self.load(state).clone();
        if state.locked {
            return Err(());
        }
        if !change(&mut secrets) {
            return Ok(false);
        }
        let key = match self.key(state)? {
            Some(key) => key,
            None => {
                let key = crypto::random().map_err(|_| ())?;
                keeper::store_key(&self.dir, &key)?;
                state.key = Some(key);
                key
            }
        };
        let plain = serde_json::to_vec(&secrets).map_err(|_| ())?;
        let sealed = crypto::seal(&key, AAD, &plain).map_err(|_| ())?;
        std::fs::create_dir_all(&self.dir).map_err(|_| ())?;
        write_private(&self.file(), &sealed).map_err(|_| ())?;
        state.sealed = Some(sealed);
        state.secrets = Some(secrets);
        Ok(true)
    }

    pub fn read(&self, service: &str) -> Option<String> {
        let mut state = self.lock();
        if let Some(secret) = self.load(&mut state).get(service) {
            return Some(secret.clone());
        }
        if !state.checked.insert(service.to_owned()) {
            return None;
        }
        self.migrate(&mut state, service)
    }
    fn migrate(&self, state: &mut State, service: &str) -> Option<String> {
        let _guard = self.lock_file().ok()?;
        // Another copy may have migrated, refreshed or deleted this item while we waited.
        // Read legacy storage only inside the same lock used by every save and deletion.
        state.secrets = None;
        if let Some(secret) = self.load(state).get(service) {
            return Some(secret.clone());
        }
        // Kept as its own item by an earlier version: moved here, then removed there. An item
        // that can't be read now (access denied) stays for the next launch.
        let mut secret = keeper::legacy_read(service)
            .ok()
            .flatten()
            .and_then(|b| String::from_utf8(b).ok())
            .filter(|s| !s.is_empty())?;
        let moved = self.change_locked(state, |s| {
            // Key access may have recovered since the first load. An already persisted
            // value always wins over an earlier version's credential.
            if let Some(current) = s.get(service) {
                secret = current.clone();
                return false;
            }
            s.insert(service.to_owned(), secret.clone());
            true
        });
        if moved.is_ok() {
            keeper::legacy_delete(service);
        }
        Some(secret)
    }
    pub fn save(&self, service: &str, secret: &str) -> Result<(), ()> {
        let mut state = self.lock();
        let _guard = self.lock_file()?;
        self.change_locked(&mut state, |s| s.insert(service.to_owned(), secret.to_owned()).as_deref() != Some(secret))?;
        // An earlier version's item would only be out of date now.
        state.checked.insert(service.to_owned());
        keeper::legacy_delete(service);
        Ok(())
    }
    /// Whether there was one to remove.
    pub fn delete(&self, service: &str) -> Result<bool, ()> {
        let mut state = self.lock();
        let _guard = self.lock_file()?;
        let removed = self.change_locked(&mut state, |s| s.remove(service).is_some())?;
        state.checked.insert(service.to_owned());
        // A previous read may have failed to migrate this item; `checked` only records a
        // read attempt, so it cannot prove the earlier credential has been removed.
        let earlier = keeper::legacy_delete(service);
        Ok(removed || earlier)
    }
}

/// Rust's File::lock currently returns Unsupported on Android, although Bionic supports
/// flock. Both variants lock the stable sidecar file until its handle is dropped.
pub(crate) fn lock_exclusive(file: &std::fs::File) -> std::io::Result<()> {
    #[cfg(target_os = "android")]
    {
        use std::os::fd::AsRawFd;
        retry_interrupted(|| {
            // SAFETY: file owns a valid descriptor throughout this call. flock takes no
            // pointers, and closing the file releases its advisory lock.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        })
    }
    #[cfg(not(target_os = "android"))]
    file.lock()
}

#[cfg(any(target_os = "android", test))]
fn retry_interrupted(mut operation: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    loop {
        match operation() {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

fn open(key: &Key, sealed: &[u8]) -> Option<BTreeMap<String, String>> {
    serde_json::from_slice(&crypto::open(key, AAD, sealed).ok()?).ok()
}

/// Replaces `path` in one step with a file only this user can read.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let temporary = path.with_file_name(format!("{name}.tmp"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(temporary, path)
}

/// A key in a plain file beside the secrets.
#[cfg(any(test, all(debug_assertions, any(target_os = "macos", windows))))]
mod plain {
    use super::Key;
    use std::path::Path;

    pub const FILE: &str = "secrets-dev.bin";
    const KEY_FILE: &str = "secrets-dev.key";

    pub fn load_key(dir: &Path) -> Result<Option<Key>, ()> {
        match std::fs::read(dir.join(KEY_FILE)) {
            Ok(raw) => raw.try_into().map(Some).map_err(|_| ()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(()),
        }
    }
    pub fn store_key(dir: &Path, key: &Key) -> Result<(), ()> {
        super::write_private(&dir.join(KEY_FILE), key).map_err(|_| ())
    }
    /// Stands in for an earlier version's items in tests.
    #[cfg(test)]
    pub static EARLIER: std::sync::Mutex<std::collections::BTreeMap<String, String>> =
        std::sync::Mutex::new(std::collections::BTreeMap::new());

    // The installed app's items are left alone.
    pub fn legacy_read(_service: &str) -> Result<Option<Vec<u8>>, ()> {
        #[cfg(test)]
        return Ok(EARLIER.lock().unwrap().get(_service).map(|s| s.clone().into_bytes()));
        #[cfg(not(test))]
        Ok(None)
    }
    pub fn legacy_delete(_service: &str) -> bool {
        #[cfg(test)]
        return EARLIER.lock().unwrap().remove(_service).is_some();
        #[cfg(not(test))]
        false
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[cfg_attr(debug_assertions, allow(dead_code))]
mod system {
    use super::Key;
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit, SearchResult};
    use security_framework::passwords::{self, PasswordOptions};
    use std::path::Path;

    pub const FILE: &str = "secrets.bin";
    const KEY_SERVICE: &str = "com.localbudget.desktop.secrets-key";
    const ACCOUNT: &str = "local-budget";
    /// errSecItemNotFound.
    const NOT_FOUND: i32 = -25300;

    pub fn load_key(_: &Path) -> Result<Option<Key>, ()> {
        match passwords::get_generic_password(KEY_SERVICE, ACCOUNT) {
            Ok(raw) => raw.try_into().map(Some).map_err(|_| ()),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(_) => Err(()),
        }
    }
    pub fn store_key(_: &Path, key: &Key) -> Result<(), ()> {
        let mut options = PasswordOptions::new_generic_password(KEY_SERVICE, ACCOUNT);
        options.set_label("UrBudget");
        passwords::set_generic_password_options(key, options).map_err(|_| ())
    }

    fn search(service: &str) -> ItemSearchOptions {
        let mut search = ItemSearchOptions::new();
        search.class(ItemClass::generic_password()).service(service).limit(Limit::Max(1));
        search
    }
    pub fn legacy_read(service: &str) -> Result<Option<Vec<u8>>, ()> {
        match search(service).load_data(true).search() {
            Ok(found) => Ok(found.into_iter().find_map(|r| match r {
                SearchResult::Data(data) => Some(data),
                _ => None,
            })),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(_) => Err(()),
        }
    }
    /// Removes every item for `service`; whether there was one.
    pub fn legacy_delete(service: &str) -> bool {
        let mut search = ItemSearchOptions::new();
        search.class(ItemClass::generic_password()).service(service);
        search.delete().is_ok()
    }
}

#[cfg(windows)]
#[cfg_attr(debug_assertions, allow(dead_code))]
mod system {
    use super::Key;
    use std::path::Path;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Credentials::{CredDeleteW, CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC};
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    pub const FILE: &str = "secrets.bin";
    const KEY_FILE: &str = "secrets.key";

    pub fn load_key(dir: &Path) -> Result<Option<Key>, ()> {
        match std::fs::read(dir.join(KEY_FILE)) {
            // Failed decryption must not replace a key that still protects existing secrets.
            Ok(sealed) => unprotect(&sealed)?.try_into().map(Some).map_err(|_| ()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(()),
        }
    }
    pub fn store_key(dir: &Path, key: &Key) -> Result<(), ()> {
        super::write_private(&dir.join(KEY_FILE), &protect(key)?).map_err(|_| ())
    }

    fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr().cast_mut() }
    }
    /// Copies out what DPAPI returned and frees it.
    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData.cast());
        bytes
    }
    /// Sealed for this Windows user on this computer.
    pub fn protect(plain: &[u8]) -> Result<Vec<u8>, ()> {
        let input = blob(plain);
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: `input` points into `plain`, which outlives the call. On success Windows
        // allocates `out`, which `take` copies and frees.
        unsafe {
            let flags = CRYPTPROTECT_UI_FORBIDDEN;
            if CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), flags, &mut out) == 0 {
                return Err(());
            }
            Ok(take(out))
        }
    }
    pub fn unprotect(sealed: &[u8]) -> Result<Vec<u8>, ()> {
        let input = blob(sealed);
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: as in `protect`.
        unsafe {
            let flags = CRYPTPROTECT_UI_FORBIDDEN;
            if CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), flags, &mut out) == 0 {
                return Err(());
            }
            Ok(take(out))
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    /// A generic credential named by its target (the service), listed under Windows Credentials.
    fn credential(service: &str) -> Option<Vec<u8>> {
        let target = wide(service);
        let mut found: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: `target` is NUL-terminated. On success Windows returns one buffer, copied
        // out and then freed with CredFree.
        unsafe {
            if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut found) == 0 {
                return None;
            }
            let c = &*found;
            let bytes = if c.CredentialBlob.is_null() {
                vec![]
            } else {
                std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec()
            };
            CredFree(found as *const _);
            Some(bytes)
        }
    }
    pub fn legacy_read(service: &str) -> Result<Option<Vec<u8>>, ()> {
        Ok(credential(service))
    }
    pub fn legacy_delete(service: &str) -> bool {
        let target = wide(service);
        // SAFETY: `target` is NUL-terminated.
        unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) != 0 }
    }

    #[cfg(test)]
    mod tests {
        use windows_sys::Win32::Security::Credentials::{CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC};

        /// Writes a generic credential the way an earlier version kept a secret.
        fn write_earlier(service: &str, secret: &str) {
            let (mut target, mut user) = (super::wide(service), super::wide("local-budget"));
            let credential = CREDENTIALW {
                Type: CRED_TYPE_GENERIC,
                TargetName: target.as_mut_ptr(),
                UserName: user.as_mut_ptr(),
                CredentialBlobSize: secret.len() as u32,
                CredentialBlob: secret.as_ptr().cast_mut(),
                Persist: CRED_PERSIST_LOCAL_MACHINE,
                ..Default::default()
            };
            // SAFETY: the strings outlive the call, which copies them.
            assert_ne!(unsafe { CredWriteW(&credential, 0) }, 0);
        }
        #[test]
        fn dpapi_seals_for_this_user() {
            let sealed = super::protect(b"secret").unwrap();
            assert_ne!(sealed, b"secret");
            assert_eq!(super::unprotect(&sealed).unwrap(), b"secret");
            assert!(super::unprotect(b"not sealed").is_err());
        }
        #[test]
        fn earlier_secrets_are_read_from_and_removed_from_credential_manager() {
            // A throwaway target, so the app's own entries are never touched.
            let service = format!("com.localbudget.desktop.vault-test-{}", std::process::id());
            assert_eq!(super::legacy_read(&service), Ok(None));
            write_earlier(&service, "old-token");
            assert_eq!(super::legacy_read(&service), Ok(Some(b"old-token".to_vec())));
            assert!(super::legacy_delete(&service));
            assert!(!super::legacy_delete(&service));
            assert_eq!(super::legacy_read(&service), Ok(None));
        }
    }
}

#[cfg(target_os = "android")]
#[cfg_attr(test, allow(dead_code))]
mod system {
    use super::Key;
    use crate::keystore;
    use std::path::Path;

    pub const FILE: &str = "secrets.bin";
    const KEY: &str = "com.localbudget.desktop.secrets-key";

    pub fn load_key(_: &Path) -> Result<Option<Key>, ()> {
        keystore::read(KEY)?.map(|raw| raw.try_into().map_err(|_| ())).transpose()
    }
    pub fn store_key(_: &Path, key: &Key) -> Result<(), ()> {
        keystore::save(KEY, "", key)
    }
    pub fn legacy_read(service: &str) -> Result<Option<Vec<u8>>, ()> {
        keystore::read(service)
    }
    pub fn legacy_delete(service: &str) -> bool {
        keystore::delete(service, "").is_ok()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", windows)))]
#[allow(dead_code)]
mod system {
    use super::Key;
    use std::path::Path;

    pub const FILE: &str = "secrets.bin";
    pub fn load_key(_: &Path) -> Result<Option<Key>, ()> {
        Err(())
    }
    pub fn store_key(_: &Path, _: &Key) -> Result<(), ()> {
        Err(())
    }
    pub fn legacy_read(_: &str) -> Result<Option<Vec<u8>>, ()> {
        Ok(None)
    }
    pub fn legacy_delete(_: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{plain, Vault};

    #[test]
    fn platform_lock_retries_interruptions_without_ignoring_other_failures() {
        let mut attempts = 0;
        super::retry_interrupted(|| {
            attempts += 1;
            if attempts < 3 {
                Err(std::io::ErrorKind::Interrupted.into())
            } else {
                Ok(())
            }
        }).unwrap();
        assert_eq!(attempts, 3);
        attempts = 0;
        let error = super::retry_interrupted(|| {
            attempts += 1;
            Err(std::io::ErrorKind::PermissionDenied.into())
        }).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(attempts, 1);
    }

    #[test]
    fn unreadable_ciphertext_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        Vault::new(dir.path()).save("a", "keep-me").unwrap();
        let file = dir.path().join(plain::FILE);
        let broken = b"damaged or temporarily unreadable ciphertext";
        std::fs::write(&file, broken).unwrap();
        assert!(Vault::new(dir.path()).save("b", "new-secret").is_err());
        assert_eq!(std::fs::read(file).unwrap(), broken);
    }

    #[test]
    fn deleting_after_failed_migration_also_removes_the_legacy_secret() {
        let service = "test.failed-migration";
        plain::EARLIER.lock().unwrap().insert(service.into(), "old-token".into());
        let dir = tempfile::tempdir().unwrap();
        // A failed write leaves the earlier item in place.
        let file = dir.path().join(plain::FILE);
        std::fs::create_dir(&file).unwrap();
        let vault = Vault::new(dir.path());
        assert_eq!(vault.read(service).as_deref(), Some("old-token"));
        std::fs::remove_dir(file).unwrap();
        assert_eq!(vault.delete(service), Ok(true));
        assert_eq!(Vault::new(dir.path()).read(service), None);
    }

    #[test]
    fn simultaneous_writers_keep_every_secret() {
        let dir = tempfile::tempdir().unwrap();
        Vault::new(dir.path()).save("initial", "kept").unwrap();
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            for writer in 0..8 {
                let (dir, barrier) = (dir.path(), &barrier);
                scope.spawn(move || {
                    let vault = Vault::new(dir);
                    barrier.wait();
                    for i in 0..10 {
                        vault.save(&format!("{writer}:{i}"), "secret").unwrap();
                    }
                });
            }
        });
        let restarted = Vault::new(dir.path());
        for writer in 0..8 {
            for i in 0..10 {
                assert_eq!(restarted.read(&format!("{writer}:{i}")).as_deref(), Some("secret"));
            }
        }
        assert_eq!(restarted.read("initial").as_deref(), Some("kept"));
    }

    #[test]
    fn secrets_are_sealed_in_one_file_and_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::new(dir.path());
        assert_eq!(vault.read("one"), None);
        // Nothing is written, and no key made, until there's a secret.
        assert!(!dir.path().join(plain::FILE).exists());
        vault.save("one", "first-secret").unwrap();
        vault.save("two", "second-secret").unwrap();
        vault.save("one", "replaced").unwrap();
        let raw = std::fs::read(dir.path().join(plain::FILE)).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("second-secret"));

        let restarted = Vault::new(dir.path());
        assert_eq!(restarted.read("one").as_deref(), Some("replaced"));
        assert_eq!(restarted.read("two").as_deref(), Some("second-secret"));
        assert_eq!(restarted.delete("one"), Ok(true));
        assert_eq!(restarted.delete("one"), Ok(false));
        assert_eq!(Vault::new(dir.path()).read("one"), None);
        assert_eq!(Vault::new(dir.path()).read("two").as_deref(), Some("second-secret"));
    }

    #[test]
    fn an_earlier_versions_items_move_into_the_file() {
        let earlier = |service: &str| plain::EARLIER.lock().unwrap().contains_key(service);
        for (service, secret) in [("test.read", "old-token"), ("test.saved", "old-key"), ("test.deleted", "old-url")] {
            plain::EARLIER.lock().unwrap().insert(service.into(), secret.into());
        }
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::new(dir.path());
        assert_eq!(vault.read("test.read").as_deref(), Some("old-token"));
        assert!(!earlier("test.read"));
        // Saving or removing one that was never read removes the earlier item too.
        vault.save("test.saved", "new-key").unwrap();
        assert!(!earlier("test.saved"));
        assert_eq!(vault.delete("test.deleted"), Ok(true));
        assert!(!earlier("test.deleted"));

        let restarted = Vault::new(dir.path());
        assert_eq!(restarted.read("test.read").as_deref(), Some("old-token"));
        assert_eq!(restarted.read("test.saved").as_deref(), Some("new-key"));
        assert_eq!(restarted.read("test.deleted"), None);
    }

    #[test]
    fn two_copies_of_the_app_keep_each_others_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let (first, second) = (Vault::new(dir.path()), Vault::new(dir.path()));
        assert_eq!(first.read("a"), None);
        assert_eq!(second.read("a"), None);
        first.save("a", "from-first").unwrap();
        second.save("b", "from-second").unwrap();
        let restarted = Vault::new(dir.path());
        assert_eq!(restarted.read("a").as_deref(), Some("from-first"));
        assert_eq!(restarted.read("b").as_deref(), Some("from-second"));
    }

    #[test]
    fn a_file_without_its_key_is_preserved_and_recovers_when_the_key_returns() {
        let dir = tempfile::tempdir().unwrap();
        Vault::new(dir.path()).save("a", "keep-me").unwrap();
        let file = dir.path().join(plain::FILE);
        let key_file = dir.path().join("secrets-dev.key");
        let (sealed, key) = (std::fs::read(&file).unwrap(), std::fs::read(&key_file).unwrap());
        std::fs::remove_file(&key_file).unwrap();
        let vault = Vault::new(dir.path());
        assert_eq!(vault.read("a"), None);
        assert!(vault.save("b", "new-secret").is_err());
        assert!(vault.delete("a").is_err());
        assert!(!key_file.exists());
        assert_eq!(std::fs::read(&file).unwrap(), sealed);
        std::fs::write(&key_file, &key).unwrap();
        assert_eq!(vault.read("a").as_deref(), Some("keep-me"));
        vault.save("b", "new-secret").unwrap();
        assert_eq!(vault.read("a").as_deref(), Some("keep-me"));
    }

    #[test]
    fn malformed_or_unreadable_keys_never_replace_existing_ciphertext() {
        for malformed in [vec![], vec![0; 31], vec![0; 33]] {
            let dir = tempfile::tempdir().unwrap();
            Vault::new(dir.path()).save("a", "keep-me").unwrap();
            let file = dir.path().join(plain::FILE);
            let key_file = dir.path().join("secrets-dev.key");
            let (sealed, key) = (std::fs::read(&file).unwrap(), std::fs::read(&key_file).unwrap());
            std::fs::write(&key_file, &malformed).unwrap();
            let vault = Vault::new(dir.path());
            assert_eq!(vault.read("a"), None);
            assert!(vault.save("b", "new-secret").is_err());
            assert_eq!(std::fs::read(&file).unwrap(), sealed);
            assert_eq!(std::fs::read(&key_file).unwrap(), malformed);
            // A directory at the key path deterministically makes reads fail on every OS.
            std::fs::remove_file(&key_file).unwrap();
            std::fs::create_dir(&key_file).unwrap();
            assert!(vault.save("b", "new-secret").is_err());
            assert_eq!(std::fs::read(&file).unwrap(), sealed);
            std::fs::remove_dir(&key_file).unwrap();
            std::fs::write(&key_file, key).unwrap();
            assert_eq!(vault.read("a").as_deref(), Some("keep-me"));
        }
    }

    #[test]
    fn cached_key_recovers_after_a_mismatched_key_is_repaired() {
        let dir = tempfile::tempdir().unwrap();
        Vault::new(dir.path()).save("a", "keep-me").unwrap();
        let key_file = dir.path().join("secrets-dev.key");
        let key = std::fs::read(&key_file).unwrap();
        std::fs::write(&key_file, [0; 32]).unwrap();
        let vault = Vault::new(dir.path());
        assert_eq!(vault.read("a"), None);
        assert!(vault.save("b", "new-secret").is_err());
        std::fs::write(&key_file, key).unwrap();
        assert_eq!(vault.read("a").as_deref(), Some("keep-me"));
    }

    #[test]
    fn readers_notice_replacements_deletions_and_new_secrets_without_reopening_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let (writer, reader) = (Vault::new(dir.path()), Vault::new(dir.path()));
        assert_eq!(reader.read("a"), None);
        writer.save("a", "first").unwrap();
        assert_eq!(reader.read("a").as_deref(), Some("first"));
        let file = dir.path().join(plain::FILE);
        let before = std::fs::metadata(&file).unwrap();
        // Both instances have the key now. Successful reads must not reopen its store.
        std::fs::remove_file(dir.path().join("secrets-dev.key")).unwrap();
        writer.save("a", "other").unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().len(), before.len());
        std::fs::OpenOptions::new().write(true).open(&file).unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(before.modified().unwrap())).unwrap();
        assert_eq!(reader.read("a").as_deref(), Some("other"));
        assert_eq!(writer.delete("a"), Ok(true));
        assert_eq!(reader.read("a"), None);
        writer.save("b", "second").unwrap();
        assert_eq!(reader.read("b").as_deref(), Some("second"));
        std::fs::remove_file(file).unwrap();
        assert_eq!(reader.read("b"), None);
        assert!(!dir.path().join("secrets-dev.key").exists());
    }

    #[test]
    fn migration_rechecks_after_another_copy_saves_or_deletes_the_legacy_item() {
        for replace in [true, false] {
            let service = if replace { "test.racing-save" } else { "test.racing-delete" };
            plain::EARLIER.lock().unwrap().insert(service.into(), "old-token".into());
            let dir = tempfile::tempdir().unwrap();
            let (reader, writer) = (Vault::new(dir.path()), Vault::new(dir.path()));
            let mut stale = reader.lock();
            assert!(!reader.load(&mut stale).contains_key(service));
            // Stop precisely between read()'s initial miss and its locked migration.
            // A competing copy finishes a refresh/deletion before migration resumes.
            if replace {
                writer.save(service, "refreshed-token").unwrap();
            } else {
                assert_eq!(writer.delete(service), Ok(true));
            }
            let expected = replace.then_some("refreshed-token");
            assert_eq!(reader.migrate(&mut stale, service).as_deref(), expected);
            assert_eq!(Vault::new(dir.path()).read(service).as_deref(), expected);
            assert!(!plain::EARLIER.lock().unwrap().contains_key(service));
        }
    }

    #[cfg(unix)]
    #[test]
    fn only_this_user_can_read_the_files() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        Vault::new(dir.path()).save("a", "secret").unwrap();
        for name in [plain::FILE, "secrets-dev.key"] {
            let mode = std::fs::metadata(dir.path().join(name)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "{name}");
        }
    }
}
