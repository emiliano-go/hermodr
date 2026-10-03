use super::*;
use std::collections::BTreeMap;

#[derive(Default)]
struct Fake {
    secrets: BTreeMap<(String, String), Zeroizing<Vec<u8>>>,
    reads: usize,
    writes: usize,
    failure: Option<DatabaseKeyError>,
    write_failure: bool,
    lose_write: bool,
    appear: Option<Vec<u8>>,
    identities: Vec<(String, String)>,
}

impl Fake {
    fn seed(&mut self, account: &str, bytes: Vec<u8>) {
        self.secrets.insert((SERVICE.into(), owner(account).unwrap()), Zeroizing::new(bytes));
    }
}

impl KeyProvider for Fake {
    fn get(&mut self, service: &str, owner: &str) -> Result<Option<Zeroizing<Vec<u8>>>, DatabaseKeyError> {
        self.reads += 1;
        let identity = (service.to_owned(), owner.to_owned());
        self.identities.push(identity.clone());
        if let Some(failure) = self.failure { return Err(failure); }
        if self.reads == 2 {
            if let Some(secret) = self.appear.take() { self.secrets.insert(identity.clone(), Zeroizing::new(secret)); }
        }
        Ok(self.secrets.get(&identity).map(|secret| Zeroizing::new(secret.to_vec())))
    }

    fn set(&mut self, service: &str, owner: &str, secret: &[u8]) -> Result<(), DatabaseKeyError> {
        self.writes += 1;
        if self.write_failure { return Err(DatabaseKeyError::SaveFailed); }
        let identity = (service.to_owned(), owner.to_owned());
        assert!(!self.secrets.contains_key(&identity));
        if !self.lose_write { self.secrets.insert(identity, Zeroizing::new(secret.to_vec())); }
        Ok(())
    }
}

fn random(bytes: &mut [u8; 32]) -> Result<(), DatabaseKeyError> {
    bytes.fill(0x41);
    Ok(())
}

#[test]
fn existing_key_is_reused_without_randomness_or_write_even_when_creation_disabled() {
    let mut fake = Fake::default();
    fake.seed("default", vec![0x17; 32]);
    let key = load_with(&mut fake, "default", false, |_| panic!("must reuse existing key")).unwrap();
    assert_eq!(format!("{key:?}"), "DatabaseKey([REDACTED])");
    assert_eq!(fake.reads, 1);
    assert_eq!(fake.writes, 0);
    assert_eq!(fake.secrets.values().next().unwrap().as_slice(), &[0x17; 32]);
}

#[test]
fn missing_existing_key_fails_closed_without_creation() {
    let mut fake = Fake::default();
    assert_eq!(load_with(&mut fake, "default", false, |_| panic!("must not generate key")).unwrap_err(), DatabaseKeyError::Missing);
    assert_eq!(fake.writes, 0);
    assert_eq!(fake.reads, 1);
}

#[test]
fn malformed_existing_keys_are_never_replaced() {
    for length in [0, 31, 33] {
        let mut fake = Fake::default();
        fake.seed("default", vec![0x17; length]);
        assert_eq!(load_with(&mut fake, "default", true, |_| panic!("must not replace corrupt key")).unwrap_err(), DatabaseKeyError::Corrupt);
        assert_eq!(fake.writes, 0);
        assert_eq!(fake.secrets.values().next().unwrap().len(), length);
    }
}

#[test]
fn unavailable_provider_never_turns_into_missing_or_creation() {
    for may_create in [false, true] {
        let mut fake = Fake { failure: Some(DatabaseKeyError::Unavailable), ..Default::default() };
        assert_eq!(load_with(&mut fake, "default", may_create, |_| panic!("must not generate key")).unwrap_err(), DatabaseKeyError::Unavailable);
        assert_eq!(fake.writes, 0);
    }
}

#[test]
fn creation_persists_exactly_32_bytes_and_reuses_them() {
    let mut fake = Fake::default();
    load_with(&mut fake, "acct-123", true, random).unwrap();
    assert_eq!(fake.writes, 1);
    assert_eq!(fake.secrets.values().next().unwrap().as_slice(), &[0x41; 32]);
    load_with(&mut fake, "acct-123", true, |_| panic!("must reuse key")).unwrap();
    assert_eq!(fake.writes, 1);
}

#[test]
fn key_appearing_during_creation_is_reused_without_overwrite() {
    let mut fake = Fake { appear: Some(vec![0x17; 32]), ..Default::default() };
    load_with(&mut fake, "default", true, random).unwrap();
    assert_eq!(fake.writes, 0);
    assert_eq!(fake.secrets.values().next().unwrap().as_slice(), &[0x17; 32]);
    let mut fake = Fake { appear: Some(vec![0x17; 31]), ..Default::default() };
    assert_eq!(load_with(&mut fake, "default", true, random).unwrap_err(), DatabaseKeyError::Corrupt);
    assert_eq!(fake.writes, 0);
}

#[test]
fn randomness_and_persistence_failures_never_return_a_usable_key() {
    let mut fake = Fake::default();
    assert_eq!(load_with(&mut fake, "default", true, |_| Err(DatabaseKeyError::RandomUnavailable)).unwrap_err(), DatabaseKeyError::RandomUnavailable);
    assert_eq!(fake.writes, 0);
    for lose_write in [false, true] {
        let mut fake = Fake { write_failure: !lose_write, lose_write, ..Default::default() };
        assert_eq!(load_with(&mut fake, "default", true, random).unwrap_err(), DatabaseKeyError::SaveFailed);
    }
}

#[test]
fn namespace_is_stable_separate_and_case_collision_inputs_are_rejected_before_access() {
    let mut fake = Fake::default();
    for account in ["default", "acct-123", "acct_123", "123"] {
        load_with(&mut fake, account, true, random).unwrap();
    }
    assert_eq!(fake.secrets.len(), 4);
    assert!(fake.identities.iter().all(|(service, owner)| service == "com.postal.database" && owner.starts_with("account/")));
    let reads = fake.reads;
    for invalid in ["", ".", "..", "Default", "a/b", "a\\b", "a.b", "a:b", " a", "a\0b", "é"] {
        assert_eq!(load_with(&mut fake, invalid, true, |_| panic!("must reject before random")).unwrap_err(), DatabaseKeyError::InvalidAccount);
    }
    assert_eq!(load_with(&mut fake, &"a".repeat(129), true, random).unwrap_err(), DatabaseKeyError::InvalidAccount);
    assert_eq!(fake.reads, reads);
    assert_eq!(fake.writes, 4);
}

struct Directory(std::path::PathBuf);

impl Directory {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("postal-key-lock-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

#[test]
fn native_lock_covers_creation_readback_and_does_not_create_restoration_target() {
    let directory = Directory::new();
    let restored = directory.0.join("restored-account");
    let mut fake = Fake::default();
    load_locked(&mut fake, "acct-restored", true, &directory.0, |bytes| {
        let competing = OpenOptions::new().read(true).write(true).open(directory.0.join("database-key.lock")).unwrap();
        assert!(competing.try_lock().is_err());
        random(bytes)
    }).unwrap();
    assert!(!restored.exists());
    let competing = OpenOptions::new().read(true).write(true).open(directory.0.join("database-key.lock")).unwrap();
    competing.try_lock().unwrap();
    drop(competing);
    load_locked(&mut fake, "acct-restored", true, &directory.0, |_| panic!("must reuse first creator's key")).unwrap();
    assert_eq!(fake.writes, 1);
}

#[test]
fn simultaneous_key_creators_use_one_native_lock_and_never_overwrite_verified_key() {
    use std::sync::{Arc, Barrier, Mutex};
    struct Shared(Arc<Mutex<Fake>>);
    impl KeyProvider for Shared {
        fn get(&mut self, service: &str, owner: &str) -> Result<Option<Zeroizing<Vec<u8>>>, DatabaseKeyError> {
            self.0.lock().unwrap().get(service, owner)
        }
        fn set(&mut self, service: &str, owner: &str, secret: &[u8]) -> Result<(), DatabaseKeyError> {
            self.0.lock().unwrap().set(service, owner, secret)
        }
    }
    let directory = Directory::new();
    let fake = Arc::new(Mutex::new(Fake::default()));
    let barrier = Arc::new(Barrier::new(8));
    let workers: Vec<_> = (0..8).map(|index| {
        let fake = fake.clone(); let barrier = barrier.clone(); let path = directory.0.clone();
        std::thread::spawn(move || {
            barrier.wait();
            let key = load_locked(&mut Shared(fake), "default", true, &path, |bytes| {
                bytes.fill(index + 1); std::thread::yield_now(); Ok(())
            }).unwrap();
            assert_eq!(format!("{key:?}"), "DatabaseKey([REDACTED])");
        })
    }).collect();
    for worker in workers { worker.join().unwrap(); }
    assert_eq!(fake.lock().unwrap().writes, 1);
}

#[test]
fn invalid_lock_file_fails_before_provider_access() {
    let directory = Directory::new();
    std::fs::create_dir(directory.0.join("database-key.lock")).unwrap();
    let mut fake = Fake::default();
    assert_eq!(load_locked(&mut fake, "default", true, &directory.0, random).unwrap_err(), DatabaseKeyError::Unavailable);
    assert_eq!(fake.reads, 0);
}
