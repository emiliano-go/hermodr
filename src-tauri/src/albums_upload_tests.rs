use super::*;

#[test]
fn album_staging_validates_all_tokens_before_consuming_any() {
    let root = std::env::temp_dir().join(format!(".postal-album-upload-{}-{}",
        std::process::id(), crate::account_store::now_millis()));
    let uploads = Uploads::default();
    let first = uploads.begin(&root, "synthetic-one".into(), "first.jpg".into(), 3).unwrap();
    let second = uploads.begin(&root, "synthetic-one".into(), "second.mp4".into(), 3).unwrap();
    let foreign = uploads.begin(&root, "synthetic-two".into(), "foreign.jpg".into(), 3).unwrap();
    uploads.append("synthetic-one", &first, 0, b"abc").unwrap();
    uploads.append("synthetic-one", &second, 0, b"de").unwrap();
    uploads.append("synthetic-two", &foreign, 0, b"xyz").unwrap();
    for tokens in [vec![first.clone(), first.clone()], vec![first.clone(), second.clone()],
        vec![first.clone(), foreign.clone()], vec![first.clone(), "missing".into()]] {
        assert!(uploads.take_many("synthetic-one", &tokens).is_err());
        assert_eq!(uploads.0.lock().unwrap().entries.len(), 3);
    }
    uploads.append("synthetic-one", &second, 2, b"f").unwrap();
    assert!(uploads.take_many("synthetic-two", &[first.clone(), second.clone()]).is_err());
    let staged = uploads.take_many("synthetic-one", &[second, first]).unwrap();
    assert_eq!(staged.iter().map(|file| file.name.as_str()).collect::<Vec<_>>(), vec!["second.mp4", "first.jpg"]);
    assert_eq!(fs::read(&staged[0].path).unwrap(), b"def");
    assert_eq!(fs::read(&staged[1].path).unwrap(), b"abc");
    assert_eq!(uploads.0.lock().unwrap().entries.len(), 1);
    let paths = staged.iter().map(|file| file.path.clone()).collect::<Vec<_>>();
    drop(staged);
    assert!(paths.iter().all(|path| !path.exists()));
    uploads.cancel(&foreign);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    fs::remove_dir(root).unwrap();
}

#[test]
fn staged_upload_scope_requires_captured_active_account_and_service() {
    assert!(check_upload_scope("captured", Some("captured"), true).is_ok());
    assert!(check_upload_scope("captured", Some("changed"), true).is_err());
    assert!(check_upload_scope("captured", None, true).is_err());
    assert!(check_upload_scope("captured", Some("captured"), false).is_err());
}

#[test]
fn stale_begin_cleanup_removes_only_its_owned_token() {
    let root = std::env::temp_dir().join(format!(".postal-album-stale-begin-{}-{}",
        std::process::id(), crate::account_store::now_millis()));
    let uploads = Uploads::default();
    let old = uploads.begin(&root, "captured".into(), "old.jpg".into(), 0).unwrap();
    let foreign = uploads.begin(&root, "changed".into(), "foreign.jpg".into(), 0).unwrap();
    let fresh = uploads.begin(&root, "captured".into(), "fresh.jpg".into(), 0).unwrap();
    assert!(uploads.complete_begin("captured", fresh.clone(), Err("account changed".into())).is_err());
    let pending = uploads.0.lock().unwrap();
    assert!(!pending.entries.contains_key(&fresh));
    assert!(pending.entries.contains_key(&old));
    assert!(pending.entries.contains_key(&foreign));
    drop(pending);
    assert!(uploads.cancel_owned("changed", &old).is_err());
    assert_eq!(uploads.0.lock().unwrap().entries.len(), 2);
    uploads.cancel_owned("captured", &old).unwrap();
    uploads.cancel_owned("captured", &old).unwrap();
    assert!(uploads.0.lock().unwrap().entries.contains_key(&foreign));
    uploads.cancel_owned("changed", &foreign).unwrap();
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    fs::remove_dir(root).unwrap();
}
