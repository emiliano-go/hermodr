use super::*;
use buffa::Message as _;
use std::sync::atomic::AtomicUsize;
use std::io::Write;

#[tokio::test]
async fn retries_replace_and_persist_locators_and_stop_after_one_reupload() {
    let root = std::env::temp_dir().join(format!("postal-retry-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    for scenario in ["direct", "recovered", "download-failed", "reupload-failed", "write-failed"] {
        let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
        let directory = root.join(scenario);
        let locator = wa::Message { image_message: MessageField::some(wa::message::ImageMessage {
            url: Some("https://old.invalid/file".into()), direct_path: Some("/old".into()),
            media_key: Some(vec![7; 32]), mimetype: Some("image/jpeg".into()), ..Default::default()
        }), ..Default::default() };
        let row = StoredMessage { header: MessageHeader {
            chat: "test@s.whatsapp.net".into(), id: "fixture".into(), ..Default::default()
        }, text: "caption survives".into(), media: Media { kind: Some("image".into()), ..Default::default() },
            ..Default::default() };
        store.insert_message(&row).await.unwrap();
        store.set_media_ref(&row.header.chat, &row.header.id, &locator.encode_to_vec()).await.unwrap();
        if scenario == "write-failed" {
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(&directory, b"not a directory").unwrap();
        }
        let attempts = AtomicUsize::new(0);
        let requests = AtomicUsize::new(0);
        let (downloads, reuploads) = (&attempts, &requests);
        let result = fetch_stored_media(&store, &directory, &row.header.chat, &row.header.id, locator,
            |media, mut writer| async move {
                let attempt = downloads.fetch_add(1, Ordering::SeqCst);
                assert_eq!(media.downloadable.direct_path(), Some(if attempt == 0 { "/old" } else { "/new" }));
                if scenario != "direct" && attempt == 0 || scenario == "download-failed" {
                    writer.write_all(b"unverified partial data that must be discarded")?;
                    anyhow::bail!("synthetic CDN failure");
                }
                writer.write_all(b"synthetic attachment")?;
                Ok(writer)
            },
            |message| async move {
                reuploads.fetch_add(1, Ordering::SeqCst);
                assert_eq!(message.image_message.url.as_deref(), Some("https://old.invalid/file"));
                if scenario == "reupload-failed" { anyhow::bail!("synthetic sender refusal"); }
                Ok("/new".into())
            },
        ).await;
        assert_eq!(attempts.load(Ordering::SeqCst), match scenario { "write-failed" => 0, "direct" | "reupload-failed" => 1, _ => 2 });
        assert_eq!(requests.load(Ordering::SeqCst), usize::from(!matches!(scenario, "direct" | "write-failed")));
        let persisted = store.message(&row.header.chat, &row.header.id).await.unwrap();
        assert_eq!(persisted.text, "caption survives");
        if matches!(scenario, "direct" | "recovered") {
            let updated = result.unwrap();
            assert_eq!(updated.media.path, persisted.media.path);
            assert_eq!(std::fs::read(updated.media.path.unwrap()).unwrap(), b"synthetic attachment");
        } else {
            assert!(result.is_err(), "{scenario}");
            assert!(persisted.media.path.is_none(), "failed download must not advertise a file");
        }
        let bytes = store.media_ref_for(&row.header.chat, &row.header.id).await.unwrap().unwrap();
        let stored = wa::Message::decode(&mut bytes.as_slice()).unwrap();
        if !matches!(scenario, "direct" | "reupload-failed" | "write-failed") {
            assert_eq!(stored.image_message.direct_path.as_deref(), Some("/new"));
            assert!(stored.image_message.url.is_none());
        } else {
            assert_eq!(stored.image_message.direct_path.as_deref(), Some("/old"));
        }
        if directory.is_dir() {
            assert!(std::fs::read_dir(&directory).unwrap().all(|entry| entry.unwrap().path().extension().is_none_or(|extension| extension != "part")));
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn media_identifiers_cannot_escape_the_download_folder() {
    assert!(media_path(Path::new("media"), "fixture", "jpg").is_ok());
    for id in ["../outside", "/absolute", "nested/file", "nested\\file", "file:stream"] {
        assert!(media_path(Path::new("media"), id, "jpg").is_err());
    }
}
