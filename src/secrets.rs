//! API keys in the OS keyring (Windows Credential Manager, Linux Secret
//! Service). One entry per AI provider, one per song-search service (`by_name`). Never a plain file: with no keyring the
//! caller gets `SecretsError::Unavailable` and must tell the user.

use std::fmt;

use crate::config::Provider;

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

fn ai_entry(provider: Provider) -> String {
    format!("ai-key-{}", provider.id())
}

/// The stored key, or `None` if none was saved.
pub fn get(provider: Provider) -> Result<Option<Secret>, SecretsError> {
    by_name::get(&ai_entry(provider))
}

pub fn set(provider: Provider, key: &Secret) -> Result<(), SecretsError> {
    by_name::set(&ai_entry(provider), key)
}

/// Deleting a key that does not exist is fine.
pub fn delete(provider: Provider) -> Result<(), SecretsError> {
    by_name::delete(&ai_entry(provider))
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
