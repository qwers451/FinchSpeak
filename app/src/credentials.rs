//! Storage for session tokens, keyed per server and user.
//! The production store is the Windows Credential Manager (via `keyring`).

use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "FinchSpeak";

pub trait TokenStore: Send + Sync {
    fn load(&self, key: &str) -> Option<String>;
    fn save(&self, key: &str, token: &str) -> Result<(), String>;
    fn delete(&self, key: &str);
}

/// Windows Credential Manager, service "FinchSpeak".
pub struct KeyringStore;

impl TokenStore for KeyringStore {
    fn load(&self, key: &str) -> Option<String> {
        // Any keyring error (missing entry, locked store, ...) means "no token".
        keyring::Entry::new(SERVICE, key).ok()?.get_password().ok()
    }

    fn save(&self, key: &str, token: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, key)
            .and_then(|entry| entry.set_password(token))
            .map_err(|e| e.to_string())
    }

    fn delete(&self, key: &str) {
        if let Ok(entry) = keyring::Entry::new(SERVICE, key) {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(e) => tracing::warn!("Failed to delete stored credential: {e}"),
            }
        }
    }
}

/// In-memory store for tests.
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

impl TokenStore for MemoryStore {
    fn load(&self, key: &str) -> Option<String> {
        self.0.lock().ok()?.get(key).cloned()
    }

    fn save(&self, key: &str, token: &str) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|e| e.to_string())?
            .insert(key.to_string(), token.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) {
        if let Ok(mut map) = self.0.lock() {
            map.remove(key);
        }
    }
}

/// `"{url without trailing /}|{username}"`.
pub fn credential_key(server_url: &str, username: &str) -> String {
    format!("{}|{}", server_url.trim_end_matches('/'), username)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_key_trims_trailing_slash() {
        assert_eq!(credential_key("https://a:8443/", "bob"), "https://a:8443|bob");
    }

    #[test]
    fn memory_store_round_trip() {
        let store = MemoryStore::default();
        store.save("k", "t").unwrap();
        assert_eq!(store.load("k"), Some("t".to_string()));
        store.delete("k");
        assert_eq!(store.load("k"), None);
    }
}
