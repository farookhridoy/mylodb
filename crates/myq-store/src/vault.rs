//! An encrypted file for secrets, for people who do not want them in the macOS Keychain.
//!
//! The file holds one JSON map (`"<connection id>:<slot>" -> secret`), encrypted with XChaCha20-Poly1305. The key comes from a master
//! password through Argon2id with a random salt. Every write uses a fresh random nonce and replaces the file atomically (write, then
//! rename). There is no way to recover the contents without the password.

use crate::{Error, Result, SecretStore, Slot};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{aead::{Aead, KeyInit}, XChaCha20Poly1305, XNonce};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::{Path, PathBuf}, sync::Mutex};
use zeroize::Zeroizing;

/// Argon2id cost. The default takes about a third of a second on a laptop and uses 64 MiB; tests use a tiny cost.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cost { pub m_kib: u32, pub t: u32, pub p: u32 }
pub const DEFAULT_COST: Cost = Cost { m_kib: 65_536, t: 3, p: 1 };
/// Only for tests: the smallest cost Argon2 accepts.
pub const FAST_COST: Cost = Cost { m_kib: 8, t: 1, p: 1 };
/// Refuse a file that asks for an absurd cost (a damaged or hostile file must not hang or exhaust memory).
const MAX_M_KIB: u32 = 1 << 20;
const MAX_T: u32 = 10;

#[derive(Serialize, Deserialize)]
struct FileBody { v: u32, kdf: String, m: u32, t: u32, p: u32, salt: String, nonce: String, data: String }

struct Unlocked { key: Zeroizing<[u8; 32]>, salt: [u8; 16], cost: Cost, map: BTreeMap<String, String> }

pub struct Vault { path: PathBuf, cost: Cost, state: Mutex<Option<Unlocked>> }

fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }
fn unhex(s: &str) -> Result<Vec<u8>> {
    if s.len() % 2 != 0 { return Err(Error::Vault("the secrets file is damaged".into())); }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| Error::Vault("the secrets file is damaged".into()))).collect()
}
fn derive(password: &str, salt: &[u8], c: Cost) -> Result<Zeroizing<[u8; 32]>> {
    let params = Params::new(c.m_kib, c.t, c.p, Some(32)).map_err(|e| Error::Vault(format!("bad key settings: {e}")))?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|e| Error::Vault(format!("could not derive the key: {e}")))?;
    Ok(key)
}
pub(crate) fn key(slot: Slot, id: i64) -> String { format!("{id}:{}", match slot { Slot::DbPassword => "db", Slot::SshPassword => "ssh-password", Slot::SshPassphrase => "ssh-passphrase" }) }

impl Vault {
    pub fn new(path: &Path) -> Vault { Vault::with_cost(path, DEFAULT_COST) }
    pub fn with_cost(path: &Path, cost: Cost) -> Vault { Vault { path: path.to_path_buf(), cost, state: Mutex::new(None) } }

    pub fn exists(&self) -> bool { self.path.exists() }
    pub fn is_unlocked(&self) -> bool { self.state.lock().unwrap().is_some() }
    pub fn lock(&self) { *self.state.lock().unwrap() = None; }

    /// Create (or replace) the file with `password` and the given secrets, and leave it unlocked.
    pub fn create(&self, password: &str, map: BTreeMap<String, String>) -> Result<()> {
        if password.chars().count() < 8 { return Err(Error::Vault("The master password needs at least 8 characters.".into())); }
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);
        let k = derive(password, &salt, self.cost)?;
        let u = Unlocked { key: k, salt, cost: self.cost, map };
        self.write(&u)?;
        *self.state.lock().unwrap() = Some(u);
        Ok(())
    }

    pub fn unlock(&self, password: &str) -> Result<()> {
        let raw = std::fs::read_to_string(&self.path).map_err(|_| Error::Vault("There is no secrets file to unlock.".into()))?;
        let body: FileBody = serde_json::from_str(&raw).map_err(|_| Error::Vault("the secrets file is damaged".into()))?;
        if body.v != 1 || body.kdf != "argon2id" { return Err(Error::Vault("the secrets file is from a newer MyLoDB".into())); }
        let cost = Cost { m_kib: body.m, t: body.t, p: body.p };
        if cost.m_kib > MAX_M_KIB || cost.t > MAX_T || cost.p == 0 || cost.p > 16 { return Err(Error::Vault("the secrets file is damaged".into())); }
        let salt: [u8; 16] = unhex(&body.salt)?.try_into().map_err(|_| Error::Vault("the secrets file is damaged".into()))?;
        let nonce = unhex(&body.nonce)?;
        if nonce.len() != 24 { return Err(Error::Vault("the secrets file is damaged".into())); }
        let k = derive(password, &salt, cost)?;
        let plain = XChaCha20Poly1305::new(k.as_ref().into())
            .decrypt(XNonce::from_slice(&nonce), unhex(&body.data)?.as_ref())
            .map_err(|_| Error::WrongPassword)?;
        let map: BTreeMap<String, String> = serde_json::from_slice(&plain).map_err(|_| Error::Vault("the secrets file is damaged".into()))?;
        *self.state.lock().unwrap() = Some(Unlocked { key: k, salt, cost, map });
        Ok(())
    }

    /// Re-encrypt under a new master password. The vault must be unlocked (so the caller has proved the old password).
    pub fn change_password(&self, new_password: &str) -> Result<()> {
        if new_password.chars().count() < 8 { return Err(Error::Vault("The master password needs at least 8 characters.".into())); }
        let mut g = self.state.lock().unwrap();
        let u = g.as_mut().ok_or(Error::Locked)?;
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);
        let next = Unlocked { key: derive(new_password, &salt, self.cost)?, salt, cost: self.cost, map: std::mem::take(&mut u.map) };
        self.write(&next)?;
        *g = Some(next);
        Ok(())
    }

    /// Delete the file and forget the key.
    pub fn destroy(&self) -> Result<()> {
        *self.state.lock().unwrap() = None;
        match std::fs::remove_file(&self.path) { Ok(()) => Ok(()), Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()), Err(e) => Err(Error::Vault(e.to_string())) }
    }

    fn write(&self, u: &Unlocked) -> Result<()> {
        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut nonce);
        let plain = Zeroizing::new(serde_json::to_vec(&u.map)?);
        let data = XChaCha20Poly1305::new(u.key.as_ref().into())
            .encrypt(XNonce::from_slice(&nonce), plain.as_ref())
            .map_err(|_| Error::Vault("could not encrypt".into()))?;
        let body = FileBody { v: 1, kdf: "argon2id".into(), m: u.cost.m_kib, t: u.cost.t, p: u.cost.p, salt: hex(&u.salt), nonce: hex(&nonce), data: hex(&data) };
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec(&body)?).map_err(|e| Error::Vault(e.to_string()))?;
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)); }
        std::fs::rename(&tmp, &self.path).map_err(|e| Error::Vault(e.to_string()))
    }
}

impl SecretStore for Vault {
    fn set(&self, id: i64, slot: Slot, secret: &str) -> Result<()> {
        let mut g = self.state.lock().unwrap();
        let u = g.as_mut().ok_or(Error::Locked)?;
        let before = u.map.insert(key(slot, id), secret.to_string());
        if let Err(e) = self.write(u) {
            match before { Some(b) => { u.map.insert(key(slot, id), b); } None => { u.map.remove(&key(slot, id)); } }
            return Err(e);
        }
        Ok(())
    }
    fn get(&self, id: i64, slot: Slot) -> Result<Option<String>> {
        let g = self.state.lock().unwrap();
        Ok(g.as_ref().ok_or(Error::Locked)?.map.get(&key(slot, id)).cloned())
    }
    fn delete(&self, id: i64, slot: Slot) -> Result<()> {
        let mut g = self.state.lock().unwrap();
        let u = g.as_mut().ok_or(Error::Locked)?;
        if u.map.remove(&key(slot, id)).is_some() { self.write(u)?; }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> (tempfile::TempDir, Vault) {
        let d = tempfile::tempdir().unwrap();
        let v = Vault::with_cost(&d.path().join("secrets.json"), FAST_COST);
        (d, v)
    }

    #[test]
    fn secrets_survive_a_lock_and_unlock() {
        let (d, v) = vault();
        v.create("correct horse", BTreeMap::new()).unwrap();
        v.set(1, Slot::DbPassword, "hunter2").unwrap();
        v.set(1, Slot::SshPassphrase, "pass phrase").unwrap();
        v.lock();
        assert!(matches!(v.get(1, Slot::DbPassword), Err(Error::Locked)));
        let again = Vault::with_cost(&d.path().join("secrets.json"), FAST_COST);
        again.unlock("correct horse").unwrap();
        assert_eq!(again.get(1, Slot::DbPassword).unwrap().as_deref(), Some("hunter2"));
        assert_eq!(again.get(1, Slot::SshPassphrase).unwrap().as_deref(), Some("pass phrase"));
        assert_eq!(again.get(2, Slot::DbPassword).unwrap(), None);
    }

    #[test]
    fn a_wrong_password_is_refused_and_leaves_it_locked() {
        let (_d, v) = vault();
        v.create("correct horse", BTreeMap::new()).unwrap();
        v.lock();
        assert!(matches!(v.unlock("wrong password"), Err(Error::WrongPassword)));
        assert!(!v.is_unlocked());
    }

    #[test]
    fn the_file_does_not_contain_the_secret_in_the_clear() {
        let (d, v) = vault();
        v.create("correct horse", BTreeMap::new()).unwrap();
        v.set(7, Slot::DbPassword, "s3cr3t-value-xyz").unwrap();
        let raw = std::fs::read_to_string(d.path().join("secrets.json")).unwrap();
        assert!(!raw.contains("s3cr3t-value-xyz") && !raw.contains("correct horse"));
    }

    #[test]
    fn a_changed_file_is_detected() {
        let (d, v) = vault();
        v.create("correct horse", BTreeMap::new()).unwrap();
        v.set(1, Slot::DbPassword, "x").unwrap();
        v.lock();
        let p = d.path().join("secrets.json");
        let mut body: FileBody = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        let mut bytes = unhex(&body.data).unwrap();
        bytes[0] ^= 1;
        body.data = hex(&bytes);
        std::fs::write(&p, serde_json::to_vec(&body).unwrap()).unwrap();
        assert!(matches!(v.unlock("correct horse"), Err(Error::WrongPassword)));
    }

    #[test]
    fn changing_the_password_keeps_the_secrets() {
        let (_d, v) = vault();
        v.create("correct horse", BTreeMap::new()).unwrap();
        v.set(1, Slot::DbPassword, "keep me").unwrap();
        v.change_password("battery staple").unwrap();
        v.lock();
        assert!(matches!(v.unlock("correct horse"), Err(Error::WrongPassword)));
        v.unlock("battery staple").unwrap();
        assert_eq!(v.get(1, Slot::DbPassword).unwrap().as_deref(), Some("keep me"));
    }

    #[test]
    fn every_write_uses_a_new_nonce_and_a_short_password_is_refused() {
        let (d, v) = vault();
        assert!(v.create("short", BTreeMap::new()).is_err());
        v.create("correct horse", BTreeMap::new()).unwrap();
        let p = d.path().join("secrets.json");
        let a = std::fs::read_to_string(&p).unwrap();
        v.set(1, Slot::DbPassword, "x").unwrap();
        v.set(1, Slot::DbPassword, "x").unwrap();
        let nonce = |s: &str| serde_json::from_str::<FileBody>(s).unwrap().nonce;
        assert_ne!(nonce(&a), nonce(&std::fs::read_to_string(&p).unwrap()));
    }

    #[test]
    fn an_absurd_cost_in_the_file_is_refused() {
        let (d, v) = vault();
        v.create("correct horse", BTreeMap::new()).unwrap();
        v.lock();
        let p = d.path().join("secrets.json");
        let mut body: FileBody = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        body.m = u32::MAX;
        std::fs::write(&p, serde_json::to_vec(&body).unwrap()).unwrap();
        assert!(matches!(v.unlock("correct horse"), Err(Error::Vault(_))));
    }

    #[test]
    fn destroy_removes_the_file() {
        let (d, v) = vault();
        v.create("correct horse", BTreeMap::new()).unwrap();
        v.destroy().unwrap();
        assert!(!d.path().join("secrets.json").exists() && !v.is_unlocked());
    }
}
