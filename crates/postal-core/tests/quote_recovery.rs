//! Recovering the view-once a reply quotes: the locator round-trips, every
//! reply quoting the same view-once shares one file, and a file no reply names
//! any more is pruned.
use postal_core::store::{MessageHeader, MessageStore, Quote, Retention, StoredMessage};

fn store() -> MessageStore {
    MessageStore::open(std::path::Path::new(":memory:"), Retention::unlimited()).unwrap()
}

/// A reply quoting `quoted`, carrying the only copy of that view-once.
fn reply(id: &str, quoted: &str) -> StoredMessage {
    StoredMessage {
        header: MessageHeader {
            chat: "a@s".into(),
            id: id.into(),
            sender: "them".into(),
            timestamp: 1,
            from_me: false,
        },
        text: "look".into(),
        quote: Quote {
            id: Some(quoted.into()),
            text: Some("Photo".into()),
            sender: Some("@me".into()),
            view_once: true,
            recoverable: true,
            locator: Some(vec![0xde, 0xad]),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn locator_and_flags_survive_a_round_trip() {
    let s = store();
    s.insert_message(&reply("r1", "q1")).unwrap();
    let got = s.message("a@s", "r1").unwrap();
    assert!(got.quote.view_once && got.quote.recoverable);
    assert_eq!(got.quote.locator.as_deref(), Some(&[0xde, 0xad][..]));
}

#[test]
fn a_quote_from_someone_else_keeps_the_copy_and_the_operator_may_take_it() {
    let mut m = reply("r1", "q1");
    m.quote.sender = Some("them".into());
    m.quote.recoverable = true;
    let s = store();
    s.insert_message(&m).unwrap();
    let got = s.message("a@s", "r1").unwrap();
    assert!(got.quote.view_once, "the flag is shown either way");
    assert!(got.quote.recoverable, "the owner of the client may take it");
    assert!(
        got.quote.locator.is_some(),
        "the copy is kept, and the gate is what decides"
    );
}

#[test]
fn a_bare_echo_does_not_wipe_a_stored_quote() {
    let s = store();
    s.insert_message(&reply("r1", "q1")).unwrap();
    // An echo that names the quoted message but carries none of its content:
    // same row, empty quote fields.
    let mut echo = reply("r1", "q1");
    echo.quote.text = Some(String::new());
    echo.quote.kind = None;
    echo.quote.thumb = None;
    echo.quote.locator = None;
    echo.quote.view_once = false;
    echo.quote.recoverable = false;
    s.insert_message(&echo).unwrap();
    let got = s.message("a@s", "r1").unwrap();
    assert_eq!(got.quote.text.as_deref(), Some("Photo"), "the text survives");
    assert!(got.quote.view_once, "the flag survives");
    assert!(got.quote.recoverable, "the permission survives");
    assert_eq!(got.quote.locator, Some(vec![0xde, 0xad]), "the copy survives");
}

#[test]
fn a_view_once_with_no_copy_recorded_is_not_recoverable() {
    let mut m = reply("r1", "q1");
    m.quote.recoverable = false;
    m.quote.locator = None;
    let s = store();
    s.insert_message(&m).unwrap();
    let got = s.message("a@s", "r1").unwrap();
    assert!(!got.quote.recoverable, "there is nothing to take");
    assert!(got.quote.locator.is_none());
}

#[test]
fn an_ordinary_quote_is_anyones() {
    let mut m = reply("r1", "q1");
    m.quote.sender = Some("them".into());
    m.quote.view_once = false;
    m.quote.recoverable = true;
    m.quote.locator = None;
    let s = store();
    s.insert_message(&m).unwrap();
    let got = s.message("a@s", "r1").unwrap();
    assert!(got.quote.recoverable);
    assert!(
        got.quote.locator.is_none(),
        "its media is an ordinary message of its own, so nothing is copied here"
    );
}

#[test]
fn every_reply_quoting_the_same_view_once_shares_one_path() {
    let s = store();
    s.insert_message(&reply("r1", "q1")).unwrap();
    s.insert_message(&reply("r2", "q1")).unwrap();
    s.set_quote_media_path("a@s", "r1", "/media/quote-q1.jpg").unwrap();
    for id in ["r1", "r2"] {
        assert_eq!(
            s.quote_media_path("a@s", id).unwrap().as_deref(),
            Some("/media/quote-q1.jpg"),
            "{id} names the one copy"
        );
    }
}

#[test]
fn a_recovered_copy_is_listed_for_account_removal() {
    let s = store();
    s.insert_message(&reply("r1", "q1")).unwrap();
    s.set_quote_media_path("a@s", "r1", "/media/quote-q1.jpg").unwrap();
    assert!(s.media_paths().unwrap().contains(&"/media/quote-q1.jpg".to_string()));
    assert!(s.quote_media_paths().unwrap().contains("/media/quote-q1.jpg"));
}

#[test]
fn pruning_keeps_a_named_copy_and_drops_an_orphan() {
    let dir = std::env::temp_dir().join("postal-quote-recovery");
    std::fs::create_dir_all(&dir).unwrap();
    let kept = dir.join("quote-q1.jpg");
    let orphan = dir.join("quote-q2.jpg");
    std::fs::write(&kept, b"kept").unwrap();
    std::fs::write(&orphan, b"orphan").unwrap();
    std::fs::write(dir.join("q3.jpg"), b"not a copy").unwrap();

    let s = store();
    s.insert_message(&reply("r1", "q1")).unwrap();
    s.set_quote_media_path("a@s", "r1", &kept.to_string_lossy()).unwrap();
    let removed = postal_core::service::prune_quote_files(Some(&dir), &s).unwrap();
    assert_eq!(removed, 1, "only the unreferenced copy goes");
    assert!(kept.exists());
    assert!(!orphan.exists());
    assert!(dir.join("q3.jpg").exists(), "a message's own media is not touched");
}

#[test]
fn a_revoked_reply_stops_naming_its_copy() {
    let s = store();
    s.insert_message(&reply("r1", "q1")).unwrap();
    s.set_quote_media_path("a@s", "r1", "/media/quote-q1.jpg").unwrap();
    assert!(s.revoke_message("a@s", "r1").unwrap());
    assert!(s.quote_media_paths().unwrap().is_empty());
}

#[test]
fn flushing_media_forgets_recovered_copies_too() {
    let s = store();
    s.insert_message(&reply("r1", "q1")).unwrap();
    s.set_quote_media_path("a@s", "r1", "/media/quote-q1.jpg").unwrap();
    s.clear_media_paths().unwrap();
    assert!(s.media_paths().unwrap().is_empty());
}
