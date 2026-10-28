//! API keys in the OS keyring (Windows Credential Manager, Linux Secret
//! Service). One entry per AI provider, one per song-search service (`by_name`). Never a plain file: with no keyring the
//! caller gets `SecretsError::Unavailable` and must tell the user.

use std::fmt;

use crate::config::AiConfig;
use crate::music::Service;

const SERVICE: &str = "opencircle";

/// An API key. `Debug` is redacted, there is no `Display`/`Serialize`, and the
/// text is only reachable through `expose()` at the one place that sends it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(key: impl Into<String>) -> Self {
        Secret(key.into().trim().to_string())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretsError {
    /// No usable keyring (e.g. no Secret Service / D-Bus session).
    Unavailable(String),
    /// The keyring answered with an error.
    Failed(String),
}

impl fmt::Display for SecretsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretsError::Unavailable(why) => write!(
                f,
                "No system keyring is available ({why}). Install/unlock one (Windows Credential Manager, or GNOME Keyring / KWallet on Linux); OpenCircle never stores API keys in a file."
            ),
            SecretsError::Failed(why) => write!(f, "The system keyring refused the request: {why}"),
        }
    }
}

impl std::error::Error for SecretsError {}

/// The keyring entry `name` (`ai-key-<provider>`, `song-key-<service>`).
fn named(name: &str) -> Result<keyring::Entry, SecretsError> {
    keyring::Entry::new(SERVICE, name).map_err(map_err)
}

/// Keys that are not an AI provider's (song search's service), by entry name.
pub mod by_name {
    use super::*;

    pub fn get(name: &str) -> Result<Option<Secret>, SecretsError> {
        match named(name)?.get_password() {
            Ok(k) => Ok(Some(Secret::new(k))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_err(e)),
        }
    }

    /// Is a key saved? Finds the entry without reading it (no unlock prompt).
    pub fn exists(name: &str) -> Result<bool, SecretsError> {
        match named(name)?.inner.get_credential() {
            Ok(_) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(map_err(e)),
        }
    }

    pub fn set(name: &str, key: &Secret) -> Result<(), SecretsError> {
        named(name)?.set_password(key.expose()).map_err(map_err)
    }

    pub fn delete(name: &str) -> Result<(), SecretsError> {
        match named(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_err(e)),
        }
    }
}

/// Only fixed text or the error's `Display`: never `Debug`, because
/// `BadEncoding` carries the stored bytes.
fn map_err(e: keyring::Error) -> SecretsError {
    use keyring::Error::*;
    match e {
        NoDefaultStore | NoStorageAccess(_) | PlatformFailure(_) => {
            SecretsError::Unavailable(e.to_string())
        }
        BadEncoding(_) | BadDataFormat(..) => {
            SecretsError::Failed("the stored key is unreadable; set it again".into())
        }
        other => SecretsError::Failed(other.to_string()),
    }
}

/// Whose AI key: an AI provider's own entry (`ai::providers::Preset::id`):
/// one per provider, so a key is only ever sent to the one it was saved for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AiKey(pub &'static str);

impl AiKey {
    /// The one entry every OpenAI-compatible provider shared before 0.1.2
    /// (`migrate_shared` moves it once).
    pub const SHARED: AiKey = AiKey("openai_compatible");

    /// The keyring entry's name (`ai-key-anthropic`, `ai-key-openrouter`, ...).
    pub fn entry(self) -> String {
        format!("ai-key-{}", self.0)
    }
}

/// The stored key, or `None` if none was saved.
pub fn get(owner: AiKey) -> Result<Option<Secret>, SecretsError> {
    by_name::get(&owner.entry())
}

pub fn set(owner: AiKey, key: &Secret) -> Result<(), SecretsError> {
    by_name::set(&owner.entry(), key)
}

/// Deleting a key that does not exist is fine.
pub fn delete(owner: AiKey) -> Result<(), SecretsError> {
    by_name::delete(&owner.entry())
}

/// Once: the key every OpenAI-compatible provider shared moves to the
/// provider `ai` selects now, if it speaks that API (read, write, then delete
/// the old entry); otherwise nobody gets it and it is deleted. A key already
/// saved for that provider stays.
pub fn migrate_shared(store: &dyn KeyStore, ai: &AiConfig) -> Result<(), SecretsError> {
    let Some(key) = store.read(AiKey::SHARED)? else {
        return Ok(());
    };
    let to = (crate::ai::providers::Preset::of(ai))
        .filter(|p| p.api == crate::ai::providers::Api::OpenAi)
        .map(|p| AiKey(p.id));
    if let Some(to) = to
        && !store.is_set(to)?
    {
        store.set(to, &key)?;
    }
    store.delete(AiKey::SHARED)
}

// ---- the keyring behind a trait, so UI tests never touch the real one ----

/// `K` = whose key: an AI provider, or a song-search service. Blocking (the OS
/// keyring can take a while or ask to unlock): callers run it off the UI thread.
pub trait KeyStore<K = AiKey>: Send + Sync {
    fn is_set(&self, owner: K) -> Result<bool, SecretsError>;
    /// The key itself (only the migration and a request read it, never a view).
    fn read(&self, owner: K) -> Result<Option<Secret>, SecretsError>;
    fn set(&self, owner: K, key: &Secret) -> Result<(), SecretsError>;
    fn delete(&self, owner: K) -> Result<(), SecretsError>;
}

/// The real thing (Windows Credential Manager / Secret Service).
pub struct Keyring;

impl KeyStore for Keyring {
    fn is_set(&self, owner: AiKey) -> Result<bool, SecretsError> {
        get(owner).map(|k| k.is_some())
    }
    fn read(&self, owner: AiKey) -> Result<Option<Secret>, SecretsError> {
        get(owner)
    }
    fn set(&self, owner: AiKey, key: &Secret) -> Result<(), SecretsError> {
        set(owner, key)
    }
    fn delete(&self, owner: AiKey) -> Result<(), SecretsError> {
        delete(owner)
    }
}

impl KeyStore<Service> for Keyring {
    /// Finds the entry without reading the key (no unlock prompt).
    fn is_set(&self, service: Service) -> Result<bool, SecretsError> {
        by_name::exists(service.key_entry())
    }
    fn read(&self, service: Service) -> Result<Option<Secret>, SecretsError> {
        by_name::get(service.key_entry())
    }
    fn set(&self, service: Service, key: &Secret) -> Result<(), SecretsError> {
        by_name::set(service.key_entry(), key)
    }
    fn delete(&self, service: Service) -> Result<(), SecretsError> {
        by_name::delete(service.key_entry())
    }
}

/// In-memory keyring for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(std::sync::Mutex<std::collections::HashMap<&'static str, String>>);

#[cfg(test)]
impl MemoryStore {
    pub fn is_empty(&self) -> bool {
        self.0.lock().unwrap().is_empty()
    }
    pub fn get(&self, id: &str) -> Option<String> {
        self.0.lock().unwrap().get(id).cloned()
    }
    fn put(&self, id: &'static str, key: &Secret) {
        self.0.lock().unwrap().insert(id, key.expose().to_string());
    }
    fn has(&self, id: &str) -> bool {
        self.0.lock().unwrap().contains_key(id)
    }
    fn del(&self, id: &str) {
        self.0.lock().unwrap().remove(id);
    }
}

#[cfg(test)]
impl KeyStore for MemoryStore {
    fn is_set(&self, p: AiKey) -> Result<bool, SecretsError> {
        Ok(self.has(p.0))
    }
    fn read(&self, p: AiKey) -> Result<Option<Secret>, SecretsError> {
        Ok(self.get(p.0).map(Secret::new))
    }
    fn set(&self, p: AiKey, key: &Secret) -> Result<(), SecretsError> {
        self.put(p.0, key);
        Ok(())
    }
    fn delete(&self, p: AiKey) -> Result<(), SecretsError> {
        self.del(p.0);
        Ok(())
    }
}

#[cfg(test)]
impl KeyStore<Service> for MemoryStore {
    fn is_set(&self, s: Service) -> Result<bool, SecretsError> {
        Ok(self.has(s.key_entry()))
    }
    fn read(&self, s: Service) -> Result<Option<Secret>, SecretsError> {
        Ok(self.get(s.key_entry()).map(Secret::new))
    }
    fn set(&self, s: Service, key: &Secret) -> Result<(), SecretsError> {
        self.put(s.key_entry(), key);
        Ok(())
    }
    fn delete(&self, s: Service) -> Result<(), SecretsError> {
        self.del(s.key_entry());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_redacted() {
        let s = Secret::new("sk-ant-FAKE-1234567890");
        for text in [
            format!("{s:?}"),
            format!("{s:#?}"),
            format!("{:?}", Some(&s)),
            format!("{:?}", vec![s.clone()]),
        ] {
            assert!(!text.contains("FAKE") && !text.contains("sk-ant"), "{text}");
            assert!(text.contains("redacted"));
        }
        assert_eq!(s.expose(), "sk-ant-FAKE-1234567890");
    }

    fn fake(tag: &str) -> Secret {
        // built at run time: no key-shaped literal in the source
        Secret::new(format!("fake-{tag}-{:x}", std::process::id()))
    }

    fn ai(provider: crate::config::Provider, base_url: &str) -> AiConfig {
        AiConfig {
            provider,
            base_url: base_url.into(),
            model: "m".into(),
        }
    }

    /// The one shared OpenAI-compatible key moves once to the provider the
    /// config selects; nobody else gets it, and a key saved there stays.
    #[test]
    fn the_shared_key_moves_once_to_the_selected_provider_only() {
        use crate::config::Provider::{Anthropic, Browser, OpenaiCompatible};
        let old = fake("old");
        let shared = || {
            let s = MemoryStore::default();
            s.set(AiKey::SHARED, &old).unwrap();
            s
        };
        // OpenRouter selected: it gets the key, the shared entry is gone
        let s = shared();
        migrate_shared(&s, &ai(OpenaiCompatible, "https://openrouter.ai/api/v1")).unwrap();
        assert_eq!(s.get("openrouter").as_deref(), Some(old.expose()));
        assert!(!s.has("openai_compatible"));
        assert_eq!(s.0.lock().unwrap().len(), 1, "nobody else got it");
        // run again: nothing left to move
        migrate_shared(&s, &ai(OpenaiCompatible, "https://api.openai.com/v1")).unwrap();
        assert!(!s.has("openai"));
        // a custom server's URL: its own entry
        let s = shared();
        migrate_shared(&s, &ai(OpenaiCompatible, "http://10.0.0.2:8000/v1")).unwrap();
        assert_eq!(s.get("custom").as_deref(), Some(old.expose()));
        // the browser or Anthropic selected: nobody gets it
        for other in [ai(Browser, ""), ai(Anthropic, "")] {
            let s = shared();
            migrate_shared(&s, &other).unwrap();
            assert!(s.is_empty(), "{other:?}");
        }
        // a key already saved for that provider stays
        let s = shared();
        let mine = fake("mine");
        s.set(AiKey("openrouter"), &mine).unwrap();
        migrate_shared(&s, &ai(OpenaiCompatible, "https://openrouter.ai/api/v1")).unwrap();
        assert_eq!(s.get("openrouter").as_deref(), Some(mine.expose()));
        assert!(!s.has("openai_compatible"));
    }

    #[test]
    fn key_is_trimmed() {
        assert_eq!(Secret::new("  abc\n").expose(), "abc");
        assert!(Secret::new(" \n").is_empty());
    }

    #[test]
    fn keyring_errors_never_carry_the_key() {
        let e = map_err(keyring::Error::BadEncoding(b"sk-secret-bytes".to_vec()));
        assert!(!e.to_string().contains("sk-secret"));
        assert!(matches!(
            map_err(keyring::Error::NoDefaultStore),
            SecretsError::Unavailable(_)
        ));
        assert!(
            map_err(keyring::Error::NoDefaultStore)
                .to_string()
                .contains("never stores API keys in a file")
        );
    }
}
