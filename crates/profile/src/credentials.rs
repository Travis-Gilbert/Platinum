//! Secrets live in the OS vault. The browser database holds only account labels.
//! This module has no logging, search, plugin or agent export interface.
use super::{ProfileStore, web_url};
use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use anyhow::{Context, Result, ensure};
use rusqlite::{OptionalExtension as _, params};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Login {
    pub origin: String,
    pub username: String,
    pub password: Zeroizing<String>,
}

pub trait Vault {
    fn put(&self, account: &str, secret: &[u8]) -> Result<()>;
    fn get(&self, account: &str) -> Result<Zeroizing<Vec<u8>>>;
    fn remove(&self, account: &str) -> Result<()>;
}

pub struct Keychain {
    service: String,
}
impl Keychain {
    pub fn for_profile(store: &ProfileStore) -> Self {
        // Name items by installation so test/development stores cannot collide.
        let namespace = namespace(store).unwrap_or_else(|_| digest(&store.root));
        Self {
            service: format!("Frame.credentials.{namespace}.{}", store.profile.id),
        }
    }
}

/// What the installation at `store.root` names its Keychain items by. It is
/// fixed in the catalog the first time it is asked for, so the folder can
/// be moved or renamed after, and it opens the same through a link to it.
/// The `Frame` folder became `Platinum` before this was kept; such a store
/// goes on with the name its old path gave its items.
fn namespace(store: &ProfileStore) -> Result<String> {
    let db = super::catalog(&store.root)?;
    let kept = || {
        db.query_row("SELECT namespace FROM vault", [], |r| r.get(0))
            .optional()
    };
    if let Some(namespace) = kept()? {
        return Ok(namespace);
    }
    let legacy = store.root.with_file_name(super::LEGACY_NAME);
    let same = matches!(
        (legacy.canonicalize(), store.root.canonicalize()),
        (Ok(a), Ok(b)) if a == b
    );
    let named_by = if same { &legacy } else { &store.root };
    db.execute(
        "INSERT OR IGNORE INTO vault(id,namespace) VALUES(1,?1)",
        [digest(named_by)],
    )?;
    // Another process may have fixed it first.
    kept()?.context("The Keychain namespace was not kept")
}

fn digest(path: &std::path::Path) -> String {
    format!("{:x}", Sha256::digest(path.to_string_lossy().as_bytes()))
}
impl Vault for Keychain {
    fn put(&self, account: &str, secret: &[u8]) -> Result<()> {
        #[cfg(target_os = "macos")]
        return security_framework::passwords::set_generic_password(&self.service, account, secret)
            .map_err(|_| anyhow::anyhow!("Keychain access was declined or unavailable"));
        #[cfg(not(target_os = "macos"))]
        anyhow::bail!("Credential storage requires macOS Keychain")
    }
    fn remove(&self, account: &str) -> Result<()> {
        #[cfg(target_os = "macos")]
        return security_framework::passwords::delete_generic_password(&self.service, account)
            .map_err(|_| anyhow::anyhow!("Could not remove password from Keychain"));
        #[cfg(not(target_os = "macos"))]
        anyhow::bail!("Credential storage requires macOS Keychain")
    }

    fn get(&self, account: &str) -> Result<Zeroizing<Vec<u8>>> {
        #[cfg(target_os = "macos")]
        return security_framework::passwords::get_generic_password(&self.service, account)
            .map(Zeroizing::new)
            .map_err(|_| anyhow::anyhow!("Keychain access was declined or unavailable"));
        #[cfg(not(target_os = "macos"))]
        anyhow::bail!("Credential storage requires macOS Keychain")
    }
}

pub fn origin(value: &str) -> Result<String> {
    let url = web_url(value).context("Login must belong to a web origin")?;
    Ok(url.origin().ascii_serialization())
}

pub fn fill_allowed(value: &str) -> bool {
    web_url(value).is_some_and(|u| {
        u.scheme() == "https" || matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
    })
}

fn account(origin: &str, username: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(origin);
    digest.update([0]);
    digest.update(username);
    format!("{:x}", digest.finalize())
}

pub fn save(
    store: &ProfileStore,
    vault: &impl Vault,
    url: &str,
    username: &str,
    password: &str,
) -> Result<()> {
    let origin = origin(url)?;
    ensure!(!password.is_empty(), "Empty passwords are not saved");
    // Vault first: an interrupted import cannot leave a usable-looking row
    // without a secret. Retrying repairs either side without duplicates.
    vault.put(&account(&origin, username), password.as_bytes())?;
    store.connect()?.execute(
        "INSERT OR IGNORE INTO credentials VALUES(?1,?2)",
        params![origin, username],
    )?;
    Ok(())
}

pub fn delete(store: &ProfileStore, vault: &impl Vault, url: &str, username: &str) -> Result<()> {
    let origin = origin(url)?;
    vault.remove(&account(&origin, username))?;
    store.connect()?.execute(
        "DELETE FROM credentials WHERE origin=?1 AND username=?2",
        params![origin, username],
    )?;
    Ok(())
}

pub fn accounts(store: &ProfileStore, url: &str) -> Result<Vec<String>> {
    let origin = origin(url)?;
    let db = store.connect()?;
    Ok(db
        .prepare("SELECT username FROM credentials WHERE origin=?1 ORDER BY username")?
        .query_map([origin], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

pub fn password(vault: &impl Vault, url: &str, username: &str) -> Result<Zeroizing<String>> {
    ensure!(
        fill_allowed(url),
        "Passwords can only be filled on HTTPS or localhost"
    );
    let bytes = vault.get(&account(&origin(url)?, username))?;
    Ok(Zeroizing::new(String::from_utf8(bytes.to_vec()).map_err(
        |_| anyhow::anyhow!("Invalid saved credential"),
    )?))
}

pub fn chrome_key(service: &str, account: &str) -> Result<Zeroizing<[u8; 16]>> {
    #[cfg(target_os = "macos")]
    {
        let passphrase=Zeroizing::new(security_framework::passwords::get_generic_password(service,account)
            .map_err(|_|anyhow::anyhow!("Source Keychain access was declined or unavailable; use password CSV import"))?);
        Ok(derive_chrome_key(&passphrase))
    }
    #[cfg(not(target_os = "macos"))]
    anyhow::bail!("Direct password import requires macOS; use password CSV import")
}

pub fn derive_chrome_key(passphrase: &[u8]) -> Zeroizing<[u8; 16]> {
    let mut key = Zeroizing::new([0; 16]);
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(passphrase, b"saltysalt", 1003, key.as_mut());
    key
}

pub fn decrypt_chrome(key: &[u8; 16], encrypted: &[u8]) -> Result<Zeroizing<String>> {
    ensure!(
        encrypted.starts_with(b"v10"),
        "Unsupported password encryption; use password CSV import"
    );
    let clear = Zeroizing::new(
        cbc::Decryptor::<aes::Aes128>::new(key.into(), (&[b' '; 16]).into())
            .decrypt_padded_vec_mut::<Pkcs7>(&encrypted[3..])
            .map_err(|_| anyhow::anyhow!("Could not decrypt password; use password CSV import"))?,
    );
    Ok(Zeroizing::new(
        std::str::from_utf8(&clear)
            .context("Invalid password encoding")?
            .to_owned(),
    ))
}

/// Verify vault storage without exposing credentials or invoking form filling.
/// Returns (readable, unavailable); HTTP logins remain ineligible for filling.
pub fn verify_saved(store: &ProfileStore, vault: &impl Vault) -> Result<(usize, usize)> {
    let db = store.connect()?;
    let mut readable = 0;
    let mut unavailable = 0;
    for row in db
        .prepare("SELECT origin,username FROM credentials")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
    {
        let (origin, username) = row?;
        match vault.get(&account(&origin, &username)) {
            Ok(secret) if !secret.is_empty() && std::str::from_utf8(&secret).is_ok() => {
                readable += 1
            }
            _ => unavailable += 1,
        }
    }
    Ok((readable, unavailable))
}

/// Top-document only, exact origin and same-origin form action, rechecked in
/// the renderer when it executes. No auto-submit, clipboard, or hidden fields.
pub fn fill_script(url: &str, username: &str, password: &str) -> Result<Zeroizing<String>> {
    ensure!(fill_allowed(url), "Passwords require HTTPS");
    let origin = serde_json::to_string(&origin(url)?)?;
    let username = serde_json::to_string(username)?;
    let password = Zeroizing::new(serde_json::to_string(password)?);
    Ok(Zeroizing::new(format!(
        r#"(() => {{
      if (window !== top || location.origin !== {origin}) return false;
      const visible = e => !e.disabled && !e.readOnly && e.getClientRects().length > 0 &&
        !['hidden','collapse'].includes(getComputedStyle(e).visibility) && getComputedStyle(e).opacity !== '0';
      const p = [...document.querySelectorAll('input[type="password"]')].find(e=>visible(e) && e.autocomplete !== 'new-password');
      const fields = [...(p && p.form ? p.form : document).querySelectorAll('input')];
      const u = fields.find(e => visible(e) && e.autocomplete === 'username') ||
        fields.find(e => visible(e) && ['email','text'].includes(e.type));
      const form = p ? p.form : (u ? u.form : null);
      if ((!p && !u) || (form && new URL(form.action || location.href, location.href).origin !== location.origin)) return false;
      const set = (e,v) => {{
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(e,v);
        e.dispatchEvent(new Event('input',{{bubbles:true}}));
        e.dispatchEvent(new Event('change',{{bubbles:true}}));
      }};
      if (u) set(u,{username}); if (p) set(p,{password}); (p || u).focus(); return true;
    }})()"#,
        password = password.as_str()
    )))
}

pub fn read_script(url: &str) -> Result<String> {
    ensure!(fill_allowed(url), "Passwords require HTTPS");
    let origin = serde_json::to_string(&origin(url)?)?;
    Ok(format!(
        r#"(() => {{
      if (window !== top || location.origin !== {origin}) return null;
      const visible=e=>!e.disabled && e.getClientRects().length>0;
      const p=[...document.querySelectorAll('input[type="password"]')].find(e=>visible(e) && e.value);
      if (!p || (p.form && new URL(p.form.action || location.href,location.href).origin !== location.origin)) return null;
      const f=[...(p.form || document).querySelectorAll('input')];
      const u=f.find(e=>visible(e) && e.autocomplete==='username') || f.find(e=>visible(e) && ['text','email'].includes(e.type));
      return [location.origin,u ? u.value : '',p.value];
    }})()"#
    ))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[test]
    #[ignore = "explicit local OS-Keychain smoke test; creates and removes one synthetic item"]
    fn native_keychain_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProfileStore::open(temp.path()).unwrap();
        let vault = Keychain::for_profile(&store);
        let key = account("https://synthetic.example.test", "synthetic-user");
        vault
            .put(&key, b"synthetic-password")
            .expect("Keychain write");
        let result = vault.get(&key);
        security_framework::passwords::delete_generic_password(&vault.service, &key)
            .expect("clean up synthetic Keychain item");
        assert!(result.is_ok_and(|value| value.as_slice() == b"synthetic-password"));
    }

    #[test]
    fn keychain_names_outlast_the_folder_rename() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join("Frame");
        std::fs::create_dir(&legacy).unwrap();
        // What builds from before the rename named the items by.
        let before = format!("Frame.credentials.{}.1", digest(&legacy));
        let root = crate::app_dir(temp.path());
        assert!(root.ends_with(crate::APP_NAME));
        let service = |root: &std::path::Path| {
            Keychain::for_profile(&ProfileStore::open(root).unwrap()).service
        };
        assert_eq!(service(&root), before);
        assert_eq!(service(&legacy), before, "through the link");
        let moved = temp.path().join("Moved");
        std::fs::rename(&root, &moved).unwrap();
        assert_eq!(service(&moved), before, "after a move");
    }
}

/// An explicit user choice fills one visible field; payment and secret fields
/// never participate in this basic form-information importer.
pub fn form_script(url: &str, name: &str, value: &str) -> Result<String> {
    ensure!(
        fill_allowed(url),
        "Form filling requires HTTPS or localhost"
    );
    ensure!(
        [
            "email",
            "name",
            "first_name",
            "last_name",
            "city",
            "country"
        ]
        .contains(&name.to_ascii_lowercase().as_str()),
        "Unsupported form field"
    );
    let origin = serde_json::to_string(&origin(url)?)?;
    let name = serde_json::to_string(name)?;
    let value = serde_json::to_string(value)?;
    Ok(format!(
        r#"(() => {{
      if (window !== top || location.origin !== {origin}) return false;
      const e=[...document.querySelectorAll('input')].find(e=>e.name==={name} &&
        ['text','email'].includes(e.type) && !e.disabled && !e.readOnly && e.getClientRects().length>0);
      if (!e || (e.form && new URL(e.form.action || location.href,location.href).origin !== location.origin)) return false;
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,{value});
      e.dispatchEvent(new Event('input',{{bubbles:true}}));e.dispatchEvent(new Event('change',{{bubbles:true}}));
      e.focus();return true;
    }})()"#
    ))
}
