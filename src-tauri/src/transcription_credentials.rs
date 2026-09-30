use zeroize::Zeroizing;

fn entry(plugin: &str, provider: &str) -> Result<keyring::Entry, String> {
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
        return Err("invalid credential owner".into());
    }
    keyring::Entry::new("org.postal.transcription", &format!("{plugin}/{provider}"))
        .map_err(|_| "credential store unavailable".into())
}

pub(crate) fn get(plugin: &str, provider: &str) -> Result<Option<Zeroizing<String>>, String> {
    match entry(plugin, provider)?.get_password() {
        Ok(value) => Ok(Some(Zeroizing::new(value))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("credential store unavailable".into()),
    }
}

#[allow(dead_code)]
pub(crate) fn set(plugin: &str, provider: &str, key: Zeroizing<String>) -> Result<(), String> {
    if key.trim().is_empty() || key.len() > 4096 || key.contains(['\r', '\n']) {
        return Err("invalid API key".into());
    }
    entry(plugin, provider)?
        .set_password(&key)
        .map_err(|_| "cannot save credential".into())
}

pub(crate) fn delete(plugin: &str, provider: &str) -> Result<(), String> {
    match entry(plugin, provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("cannot remove credential".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
