use zeroize::Zeroizing;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CredentialError {
    OwnerInvalid,
    KeyInvalid,
    StoreUnavailable,
    SaveFailed,
    RemoveFailed,
}

impl std::fmt::Display for CredentialError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::OwnerInvalid => "invalid credential owner",
            Self::KeyInvalid => "invalid API key",
            Self::StoreUnavailable => "credential store unavailable",
            Self::SaveFailed => "cannot save credential",
            Self::RemoveFailed => "cannot remove credential",
        })
    }
}

impl std::error::Error for CredentialError {}

fn checked_owner(plugin: &str, provider: &str) -> Result<(), CredentialError> {
    if plugin.is_empty()
        || plugin.len() > 200
        || provider.is_empty()
        || provider.len() > 100
        || !plugin
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        || !provider
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(CredentialError::OwnerInvalid);
    }
    Ok(())
}

fn checked_key(key: &str) -> Result<(), CredentialError> {
    if key.trim().is_empty() || key.len() > 4096 || key.contains(['\r', '\n']) {
        return Err(CredentialError::KeyInvalid);
    }
    Ok(())
}

fn entry(plugin: &str, provider: &str) -> Result<keyring::Entry, CredentialError> {
    checked_owner(plugin, provider)?;
    keyring::Entry::new("org.postal.transcription", &format!("{plugin}/{provider}"))
        .map_err(|_| CredentialError::StoreUnavailable)
}

pub(crate) fn get_typed(plugin: &str, provider: &str) -> Result<Option<Zeroizing<String>>, CredentialError> {
    match entry(plugin, provider)?.get_password() {
        Ok(value) => Ok(Some(Zeroizing::new(value))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(CredentialError::StoreUnavailable),
    }
}

#[allow(dead_code)]
pub(crate) fn set_typed(plugin: &str, provider: &str, key: Zeroizing<String>) -> Result<(), CredentialError> {
    checked_key(&key)?;
    entry(plugin, provider)?
        .set_password(&key)
        .map_err(|_| CredentialError::SaveFailed)
}

pub(crate) fn delete_typed(plugin: &str, provider: &str) -> Result<(), CredentialError> {
    match entry(plugin, provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(CredentialError::RemoveFailed),
    }
}

fn compatibility(error: CredentialError) -> String { error.to_string() }

#[allow(dead_code)]
pub(crate) fn get(plugin: &str, provider: &str) -> Result<Option<Zeroizing<String>>, String> {
    get_typed(plugin, provider).map_err(compatibility)
}

#[allow(dead_code)]
pub(crate) fn set(plugin: &str, provider: &str, key: Zeroizing<String>) -> Result<(), String> {
    set_typed(plugin, provider, key).map_err(compatibility)
}

#[allow(dead_code)]
pub(crate) fn delete(plugin: &str, provider: &str) -> Result<(), String> {
    delete_typed(plugin, provider).map_err(compatibility)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credential_validation_keeps_standalone_error_contract() {
        assert_eq!(checked_owner("org.postal.test/invalid", "cloud"), Err(CredentialError::OwnerInvalid));
        assert_eq!(checked_owner("org.postal.test", "cloud"), Ok(()));
        for key in ["".to_owned(), "\r\n".to_owned(), "x".repeat(4097)] {
            assert_eq!(checked_key(&key), Err(CredentialError::KeyInvalid));
        }
        assert_eq!(checked_key("synthetic-input"), Ok(()));
        assert_eq!(compatibility(CredentialError::OwnerInvalid), "invalid credential owner");
        assert_eq!(compatibility(CredentialError::KeyInvalid), "invalid API key");
        assert_eq!(compatibility(CredentialError::StoreUnavailable), "credential store unavailable");
        assert_eq!(compatibility(CredentialError::SaveFailed), "cannot save credential");
        assert_eq!(compatibility(CredentialError::RemoveFailed), "cannot remove credential");
    }

    #[test]
    fn synthetic_credentials_are_scoped_and_never_returned_to_ui() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        let entry = entry("org.postal.test", "cloud").unwrap();
        entry.set_password("synthetic-test-key").unwrap();
        assert_eq!(entry.get_password().unwrap(), "synthetic-test-key");
        entry.delete_credential().unwrap();
        assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
        assert!(super::entry("org.postal.test/evil", "cloud").is_err());
        assert!(set("org.postal.test", "cloud", Zeroizing::new("\r\n".into())).is_err());
    }
}
