//! Where saved passwords live: the macOS Keychain (the default) or an encrypted file locked by a master password.
//! The choice is a small file next to the database, so it is known before anything connects.

use crate::{vault::Vault, Error, Result, SecretStore, Slot};
use std::{collections::BTreeMap, path::{Path, PathBuf}, sync::RwLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode { Keychain, File }

pub struct SecretRouter {
    keychain: Box<dyn SecretStore>,
    vault: Vault,
    mode: RwLock<Mode>,
    mode_path: PathBuf,
}

impl SecretRouter {
    /// `dir` is the app data folder. `vault` is the file store (a cheap Argon2 cost in tests).
    pub fn open(dir: &Path, keychain: Box<dyn SecretStore>, vault: Vault) -> SecretRouter {
        let mode_path = dir.join("secrets-mode");
        let mode = if std::fs::read_to_string(&mode_path).map(|s| s.trim() == "file").unwrap_or(false) && vault.exists() { Mode::File } else { Mode::Keychain };
        SecretRouter { keychain, vault, mode: RwLock::new(mode), mode_path }
    }

    pub fn mode(&self) -> Mode { *self.mode.read().unwrap() }
    pub fn file_exists(&self) -> bool { self.vault.exists() }
    pub fn unlocked(&self) -> bool { self.vault.is_unlocked() }
    pub fn unlock(&self, password: &str) -> Result<()> { self.vault.unlock(password) }
    pub fn lock(&self) { self.vault.lock() }

    fn remember(&self, m: Mode) -> Result<()> {
        match m {
            Mode::File => std::fs::write(&self.mode_path, "file").map_err(|e| Error::Vault(e.to_string()))?,
            Mode::Keychain => { let _ = std::fs::remove_file(&self.mode_path); }
        }
        *self.mode.write().unwrap() = m;
        Ok(())
    }

    /// Move every secret from the Keychain into a new encrypted file, then remove them from the Keychain.
    /// Nothing is removed unless the file was written and the mode switched.
    pub fn use_file(&self, password: &str, keys: &[(i64, Slot)]) -> Result<usize> {
        if self.mode() == Mode::File { return Err(Error::Vault("The passwords are already in the encrypted file.".into())); }
        let mut map = BTreeMap::new();
        let mut found = Vec::new();
        for &(id, slot) in keys {
            if let Some(v) = self.keychain.get(id, slot)? { map.insert(crate::vault_key(slot, id), v); found.push((id, slot)); }
        }
        self.vault.create(password, map)?;
        if let Err(e) = self.remember(Mode::File) { let _ = self.vault.destroy(); return Err(e); }
        for (id, slot) in &found { let _ = self.keychain.delete(*id, *slot); }
        Ok(found.len())
    }

    /// Move every secret from the (unlocked) file back into the Keychain and delete the file.
    pub fn use_keychain(&self, keys: &[(i64, Slot)]) -> Result<usize> {
        if self.mode() == Mode::Keychain { return Err(Error::Vault("The passwords are already in the Keychain.".into())); }
        if !self.vault.is_unlocked() { return Err(Error::Locked); }
        let mut n = 0;
        for &(id, slot) in keys {
            if let Some(v) = self.vault.get(id, slot)? { self.keychain.set(id, slot, &v)?; n += 1; }
        }
        self.remember(Mode::Keychain)?;
        self.vault.destroy()?;
        Ok(n)
    }

    pub fn change_password(&self, new_password: &str) -> Result<()> { self.vault.change_password(new_password) }

    /// Forget the encrypted file (after a forgotten master password). Saved passwords are lost; connections stay and ask again.
    pub fn reset(&self) -> Result<()> {
        self.vault.destroy()?;
        self.remember(Mode::Keychain)
    }

    fn current(&self) -> &dyn SecretStore {
        match self.mode() { Mode::Keychain => self.keychain.as_ref(), Mode::File => &self.vault }
    }
}

impl SecretStore for SecretRouter {
    fn set(&self, id: i64, slot: Slot, secret: &str) -> Result<()> { self.current().set(id, slot, secret) }
    fn get(&self, id: i64, slot: Slot) -> Result<Option<String>> { self.current().get(id, slot) }
    fn delete(&self, id: i64, slot: Slot) -> Result<()> { self.current().delete(id, slot) }
}

impl SecretStore for std::sync::Arc<SecretRouter> {
    fn set(&self, id: i64, slot: Slot, secret: &str) -> Result<()> { (**self).set(id, slot, secret) }
    fn get(&self, id: i64, slot: Slot) -> Result<Option<String>> { (**self).get(id, slot) }
    fn delete(&self, id: i64, slot: Slot) -> Result<()> { (**self).delete(id, slot) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::FAST_COST;
    use std::{collections::HashMap, sync::{Arc, Mutex}};

    #[derive(Default, Clone)]
    struct Mem(Arc<Mutex<HashMap<(i64, String), String>>>);
    impl SecretStore for Mem {
        fn set(&self, id: i64, slot: Slot, s: &str) -> Result<()> { self.0.lock().unwrap().insert((id, slot.account(id)), s.into()); Ok(()) }
        fn get(&self, id: i64, slot: Slot) -> Result<Option<String>> { Ok(self.0.lock().unwrap().get(&(id, slot.account(id))).cloned()) }
        fn delete(&self, id: i64, slot: Slot) -> Result<()> { self.0.lock().unwrap().remove(&(id, slot.account(id))); Ok(()) }
    }

    fn router(d: &Path, kc: &Mem) -> SecretRouter {
        SecretRouter::open(d, Box::new(kc.clone()), Vault::with_cost(&d.join("secrets.json"), FAST_COST))
    }
    const KEYS: [(i64, Slot); 3] = [(1, Slot::DbPassword), (1, Slot::SshPassword), (2, Slot::DbPassword)];

    #[test]
    fn moving_to_the_file_copies_every_secret_and_empties_the_keychain() {
        let d = tempfile::tempdir().unwrap();
        let kc = Mem::default();
        kc.set(1, Slot::DbPassword, "one").unwrap();
        kc.set(2, Slot::DbPassword, "two").unwrap();
        let r = router(d.path(), &kc);
        assert_eq!(r.use_file("master pass", &KEYS).unwrap(), 2);
        assert_eq!(r.mode(), Mode::File);
        assert_eq!(r.get(1, Slot::DbPassword).unwrap().as_deref(), Some("one"));
        assert_eq!(kc.get(1, Slot::DbPassword).unwrap(), None, "no copy is left in the Keychain");
        // a new run: the mode is remembered and the file is locked until the password is typed
        let next = router(d.path(), &kc);
        assert_eq!(next.mode(), Mode::File);
        assert!(matches!(next.get(1, Slot::DbPassword), Err(Error::Locked)));
        assert!(matches!(next.unlock("nope nope nope"), Err(Error::WrongPassword)));
        next.unlock("master pass").unwrap();
        assert_eq!(next.get(2, Slot::DbPassword).unwrap().as_deref(), Some("two"));
    }

    #[test]
    fn new_passwords_go_to_the_file_while_it_is_the_mode() {
        let d = tempfile::tempdir().unwrap();
        let kc = Mem::default();
        let r = router(d.path(), &kc);
        r.use_file("master pass", &KEYS).unwrap();
        r.set(3, Slot::DbPassword, "three").unwrap();
        assert_eq!(kc.get(3, Slot::DbPassword).unwrap(), None);
        assert_eq!(r.get(3, Slot::DbPassword).unwrap().as_deref(), Some("three"));
    }

    #[test]
    fn going_back_to_the_keychain_restores_everything_and_removes_the_file() {
        let d = tempfile::tempdir().unwrap();
        let kc = Mem::default();
        kc.set(1, Slot::DbPassword, "one").unwrap();
        let r = router(d.path(), &kc);
        r.use_file("master pass", &KEYS).unwrap();
        r.lock();
        assert!(matches!(r.use_keychain(&KEYS), Err(Error::Locked)), "needs the master password first");
        r.unlock("master pass").unwrap();
        assert_eq!(r.use_keychain(&KEYS).unwrap(), 1);
        assert_eq!(r.mode(), Mode::Keychain);
        assert_eq!(kc.get(1, Slot::DbPassword).unwrap().as_deref(), Some("one"));
        assert!(!d.path().join("secrets.json").exists() && !d.path().join("secrets-mode").exists());
    }

    #[test]
    fn a_failed_switch_leaves_the_keychain_alone() {
        let d = tempfile::tempdir().unwrap();
        let kc = Mem::default();
        kc.set(1, Slot::DbPassword, "one").unwrap();
        let r = router(d.path(), &kc);
        assert!(r.use_file("short", &KEYS).is_err());
        assert_eq!(r.mode(), Mode::Keychain);
        assert_eq!(kc.get(1, Slot::DbPassword).unwrap().as_deref(), Some("one"));
    }

    #[test]
    fn reset_forgets_the_file_when_the_password_is_lost() {
        let d = tempfile::tempdir().unwrap();
        let kc = Mem::default();
        let r = router(d.path(), &kc);
        r.use_file("master pass", &KEYS).unwrap();
        r.lock();
        r.reset().unwrap();
        assert_eq!(r.mode(), Mode::Keychain);
        assert!(!r.file_exists());
    }

    #[test]
    fn a_mode_file_without_a_vault_falls_back_to_the_keychain() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("secrets-mode"), "file").unwrap();
        assert_eq!(router(d.path(), &Mem::default()).mode(), Mode::Keychain);
    }
}
