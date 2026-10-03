use postal_core::database_crypto::DatabaseKey;
use std::{fs::{File, OpenOptions}, path::Path};
use zeroize::Zeroizing;

const SERVICE: &str = "com.postal.database";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DatabaseKeyError {
    InvalidAccount,
    Missing,
    Corrupt,
    Unavailable,
    SaveFailed,
    RandomUnavailable,
}

impl DatabaseKeyError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::InvalidAccount => "error.database_key_invalid_account",
            Self::Missing => "error.database_key_missing",
            Self::Corrupt => "error.database_key_corrupt",
            Self::Unavailable => "error.database_key_unavailable",
            Self::SaveFailed => "error.database_key_save_failed",
            Self::RandomUnavailable => "error.database_key_random_unavailable",
        }
    }
}

impl std::fmt::Display for DatabaseKeyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for DatabaseKeyError {}

trait KeyProvider {
    fn get(&mut self, service: &str, owner: &str) -> Result<Option<Zeroizing<Vec<u8>>>, DatabaseKeyError>;
    fn set(&mut self, service: &str, owner: &str, secret: &[u8]) -> Result<(), DatabaseKeyError>;
}

struct OsKeyProvider;

impl KeyProvider for OsKeyProvider {
    fn get(&mut self, service: &str, owner: &str) -> Result<Option<Zeroizing<Vec<u8>>>, DatabaseKeyError> {
        let entry = keyring::Entry::new(service, owner).map_err(|_| DatabaseKeyError::Unavailable)?;
        match entry.get_secret() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(DatabaseKeyError::Unavailable),
        }
    }

    fn set(&mut self, service: &str, owner: &str, secret: &[u8]) -> Result<(), DatabaseKeyError> {
        keyring::Entry::new(service, owner)
            .map_err(|_| DatabaseKeyError::Unavailable)?
            .set_secret(secret)
            .map_err(|_| DatabaseKeyError::SaveFailed)
    }
}

fn owner(account_id: &str) -> Result<String, DatabaseKeyError> {
    if account_id.is_empty() || account_id.len() > 128
        || !account_id.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')) {
        return Err(DatabaseKeyError::InvalidAccount);
    }
    Ok(format!("account/{account_id}"))
}

fn decode(secret: &Zeroizing<Vec<u8>>) -> Result<DatabaseKey, DatabaseKeyError> {
    if secret.len() != 32 { return Err(DatabaseKeyError::Corrupt); }
    let mut bytes = Zeroizing::new([0; 32]);
    bytes.copy_from_slice(secret);
    Ok(DatabaseKey::from_bytes(*bytes))
}

fn load_with(
    provider: &mut impl KeyProvider,
    account_id: &str,
    may_create: bool,
    random: impl FnOnce(&mut [u8; 32]) -> Result<(), DatabaseKeyError>,
) -> Result<DatabaseKey, DatabaseKeyError> {
    let owner = owner(account_id)?;
    if let Some(secret) = provider.get(SERVICE, &owner)? { return decode(&secret); }
    if !may_create { return Err(DatabaseKeyError::Missing); }
    let mut bytes = Zeroizing::new([0; 32]);
    random(&mut bytes)?;
    if let Some(secret) = provider.get(SERVICE, &owner)? { return decode(&secret); }
    provider.set(SERVICE, &owner, &bytes[..])?;
    let stored = provider.get(SERVICE, &owner)?.ok_or(DatabaseKeyError::SaveFailed)?;
    if stored.as_slice() != &bytes[..] { return Err(DatabaseKeyError::SaveFailed); }
    decode(&stored)
}

fn load_locked(provider: &mut impl KeyProvider, account_id: &str, may_create: bool, directory: &Path,
    random: impl FnOnce(&mut [u8; 32]) -> Result<(), DatabaseKeyError>) -> Result<DatabaseKey, DatabaseKeyError> {
    owner(account_id)?;
    let _lock = account_lock(directory).map_err(|_| DatabaseKeyError::Unavailable)?;
    load_with(provider, account_id, may_create, random)
}

fn account_lock(directory: &Path) -> std::io::Result<File> {
    std::fs::create_dir_all(directory)?;
    let path = directory.join("database-key.lock");
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let file = match options.create_new(true).open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if !std::fs::symlink_metadata(&path)?.file_type().is_file() {
                return Err(std::io::Error::other("account key lock is not a regular file"));
            }
            options.create_new(false).open(&path)?
        }
        Err(error) => return Err(error),
    };
    file.lock()?;
    Ok(file)
}

pub(crate) fn load_or_create(account_id: &str, may_create: bool, directory: &Path) -> Result<DatabaseKey, String> {
    load_locked(&mut OsKeyProvider, account_id, may_create, directory, |bytes| {
        getrandom::fill(bytes).map_err(|_| DatabaseKeyError::RandomUnavailable)
    }).map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "database_keys_tests.rs"]
mod tests;
