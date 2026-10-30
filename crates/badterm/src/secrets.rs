//! Passwords kept in the OS credential store (Keychain on macOS, Credential
//! Manager on Windows, the Secret Service on Linux) instead of the directory
//! file. Where no store is available, passwords stay in the file as before.

use badterm_core::{Directory, Entry};

const SERVICE: &str = "BadTerm BBS client";
/// Where passwords were kept when BadTerm was called Phosphor (to 0.1.16).
const OLD_SERVICE: &str = "Phosphor BBS client";

/// Somewhere to keep passwords; [`Keychain`] in the app, an in-memory map in
/// tests.
pub trait Store {
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    fn set(&self, account: &str, password: &str) -> Result<(), String>;
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// The OS credential store.
pub struct Keychain;

impl Keychain {
    fn entry(account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())
    }
}

impl Store for Keychain {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        match Self::entry(account)?.get_password() {
            Ok(p) => return Ok(Some(p)),
            Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(e.to_string()),
        }
        // Not found: it may still be under the old name; move it over.
        let old = keyring::Entry::new(OLD_SERVICE, account).map_err(|e| e.to_string())?;
        match old.get_password() {
            Ok(p) => {
                if self.set(account, &p).is_ok() {
                    let _ = old.delete_credential();
                }
                Ok(Some(p))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, account: &str, password: &str) -> Result<(), String> {
        Self::entry(account)?.set_password(password).map_err(|e| e.to_string())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        if let Ok(old) = keyring::Entry::new(OLD_SERVICE, account) {
            let _ = old.delete_credential();
        }
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// The credential name for an entry, e.g. `guest@ssh://bbs.example.com:22`.
pub fn account(e: &Entry) -> String {
    format!("{}@{}", e.username, e.system_id())
}

/// The entry's password: from the file, or else from the store.
pub fn password(e: &Entry, store: &dyn Store) -> Result<String, String> {
    if !e.password.is_empty() || !e.keychain {
        return Ok(e.password.clone());
    }
    Ok(store.get(&account(e))?.unwrap_or_default())
}

/// Move passwords held in the file into the store. On failure the rest stay
/// in the file, and the store's error is returned.
pub fn migrate(dir: &mut Directory, store: &dyn Store) -> Result<(), String> {
    for e in dir.entries.iter_mut().filter(|e| !e.password.is_empty()) {
        store.set(&account(e), &e.password)?;
        e.password.clear();
        e.keychain = true;
    }
    Ok(())
}

/// Remove an entry's stored password, if it has one.
pub fn forget(e: &Entry, store: &dyn Store) {
    if e.keychain
        && let Err(err) = store.delete(&account(e))
    {
        log::warn!("removing the password for {} from the keychain: {err}", e.name);
    }
}

#[cfg(test)]
pub mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    /// An in-memory store; `broken` makes every call fail.
    #[derive(Default)]
    pub struct Memory {
        pub map: RefCell<HashMap<String, String>>,
        pub broken: bool,
    }

    impl Store for Memory {
        fn get(&self, account: &str) -> Result<Option<String>, String> {
            if self.broken {
                return Err("no keychain".into());
            }
            Ok(self.map.borrow().get(account).cloned())
        }

        fn set(&self, account: &str, password: &str) -> Result<(), String> {
            if self.broken {
                return Err("no keychain".into());
            }
            self.map.borrow_mut().insert(account.into(), password.into());
            Ok(())
        }

        fn delete(&self, account: &str) -> Result<(), String> {
            if self.broken {
                return Err("no keychain".into());
            }
            self.map.borrow_mut().remove(account);
            Ok(())
        }
    }

    fn ssh(user: &str, password: &str) -> Entry {
        let mut e = Entry::from_address(&format!("ssh://{user}@bbs.example.com")).unwrap();
        e.password = password.into();
        e
    }

    #[test]
    fn passwords_move_into_the_store() {
        let store = Memory::default();
        let mut dir = Directory { entries: vec![ssh("guest", "pw"), ssh("other", "")], ..Directory::default() };
        migrate(&mut dir, &store).unwrap();
        let e = &dir.entries[0];
        assert!(e.password.is_empty() && e.keychain);
        assert_eq!(store.map.borrow()["guest@ssh://bbs.example.com:22"], "pw");
        assert_eq!(password(e, &store).unwrap(), "pw");
        // An entry without a password is left alone.
        assert!(!dir.entries[1].keychain);
        assert_eq!(password(&dir.entries[1], &store).unwrap(), "");

        forget(e, &store);
        assert!(store.map.borrow().is_empty());
    }

    #[test]
    fn without_a_store_passwords_stay_in_the_file() {
        let store = Memory { broken: true, ..Memory::default() };
        let mut dir = Directory { entries: vec![ssh("guest", "pw")], ..Directory::default() };
        assert!(migrate(&mut dir, &store).is_err());
        assert_eq!(dir.entries[0].password, "pw");
        assert!(!dir.entries[0].keychain);
        // Reading never needs the store then.
        assert_eq!(password(&dir.entries[0], &store).unwrap(), "pw");
    }
}
