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
        if scenario == "recovered" {
            store.set_media_download_error(&row.header.chat, &row.header.id, Some(crate::message_ref::MessageFailure {
                message: MessageRef::new("error.media_reupload_unavailable"), diagnostic: None,
            })).await.unwrap();
        }
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
                    return Err(whatsapp_rust::download::MediaDownloadError::ReferenceRejected(
                        anyhow::anyhow!("Download failed with status: 403")).into());
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
        let failure = store.marks(&row.header.chat).await.unwrap().download_failures
            .and_then(|failures| failures.get(&row.header.id).cloned());
        assert_eq!(failure.is_some(), !matches!(scenario, "direct" | "recovered"));
        if scenario == "download-failed" {
            assert_eq!(failure.as_ref().unwrap().message.code, "error.media_fresh_reference_rejected");
        }
        if matches!(scenario, "direct" | "recovered") {
            let updated = result.unwrap();
            assert_eq!(updated.media.path, persisted.media.path);
            assert_eq!(std::fs::read(updated.media.path.unwrap()).unwrap(), b"synthetic attachment");
        } else {
            assert!(result.is_err(), "{scenario}");
            if scenario == "download-failed" {
                let failure = crate::message_ref::MessageFailure::from(result.unwrap_err());
                assert_eq!(failure.message.code, "error.media_fresh_reference_rejected");
                assert!(failure.diagnostic.unwrap().contains("403"));
            } else if scenario == "reupload-failed" {
                let failure = crate::message_ref::MessageFailure::from(result.unwrap_err());
                assert_eq!(failure.message.code, "error.media_reupload_unavailable");
                let diagnostic = failure.diagnostic.unwrap();
                assert!(diagnostic.contains("403") && diagnostic.contains("synthetic sender refusal"));
            }
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

#[test]
fn ptv_reupload_repoints_the_inner_video_without_dropping_spoiler_or_once_flags() {
    let mut message = wa::Message { spoiler_message: MessageField::some(wa::message::FutureProofMessage {
        message: MessageField::some(wa::Message { ptv_message: MessageField::some(wa::message::VideoMessage {
            url: Some("https://old.invalid/video".into()), media_key: Some(vec![9; 32]), view_once: Some(true), ..Default::default()
        }), ..Default::default() }), ..Default::default()
    }), ..Default::default() };
    assert_eq!(media_key(&message), Some(vec![9; 32]));
    assert!(!has_direct_path(&message));
    set_direct_path(&mut message, "/reuploaded-video");
    let decoded = decoded_message(&message);
    assert!(decoded.spoiler && decoded.view_once);
    assert!(message.spoiler_message.is_set());
    assert!(has_direct_path(&message));
    let video = decoded.message.ptv_message.as_option().unwrap();
    assert_eq!(video.direct_path.as_deref(), Some("/reuploaded-video"));
    assert!(video.url.is_none());
    assert_eq!(video.media_key, Some(vec![9; 32]));
}

#[tokio::test]
async fn round_video_retry_persists_ptv_keys_and_the_reuploaded_path() {
    let directory = std::env::temp_dir().join(format!("postal-ptv-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    let row = StoredMessage { header: MessageHeader { chat: "test@g.us".into(), id: "ptv".into(), ..Default::default() },
        media: Media { kind: Some("round_video".into()), thumb: Some("data:image/jpeg;base64,AQ==".into()), ..Default::default() },
        spoiler: true, ..Default::default() };
    let locator = wa::Message { ptv_message: MessageField::some(wa::message::VideoMessage {
        url: Some("https://old.invalid/video".into()), direct_path: Some("/old".into()), media_key: Some(vec![9; 32]), ..Default::default()
    }), ..Default::default() };
    store.insert_message(&row).await.unwrap();
    store.set_media_ref(&row.header.chat, &row.header.id, &locator.encode_to_vec()).await.unwrap();
    let attempts = AtomicUsize::new(0);
    let downloads = &attempts;
    let updated = fetch_stored_media(&store, &directory, &row.header.chat, &row.header.id, locator,
        |media, mut writer| async move {
            let attempt = downloads.fetch_add(1, Ordering::SeqCst);
            assert_eq!(media.kind, "round_video");
            assert_eq!(media.downloadable.direct_path(), Some(if attempt == 0 { "/old" } else { "/new" }));
            if attempt == 0 { anyhow::bail!("synthetic stale CDN path"); }
            writer.write_all(b"synthetic round video")?;
            Ok(writer)
        },
        |message| async move { assert_eq!(media_key(&message), Some(vec![9; 32])); Ok("/new".into()) },
    ).await.unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert!(updated.spoiler);
    assert_eq!(updated.media.kind.as_deref(), Some("round_video"));
    assert!(updated.media.path.as_deref().unwrap().ends_with("ptv.mp4"));
    let bytes = store.media_ref_for(&row.header.chat, &row.header.id).await.unwrap().unwrap();
    let persisted = wa::Message::decode(&mut bytes.as_slice()).unwrap();
    assert_eq!(persisted.ptv_message.direct_path.as_deref(), Some("/new"));
    assert_eq!(persisted.ptv_message.media_key, Some(vec![9; 32]));
    assert!(persisted.ptv_message.url.is_none() && persisted.video_message.is_unset());
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn artwork_cache_keeps_the_music_row_and_reports_unavailable_sources() {
    let mut jpeg = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 2).write_to(&mut jpeg, image::ImageFormat::Jpeg).unwrap();
    for (uri, available) in [(Some("https://artwork.invalid/image"), true), (Some("https://artwork.invalid/image"), false), (None, true)] {
        let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
        let row = StoredMessage { header: MessageHeader { chat: "test@g.us".into(), id: "music".into(), ..Default::default() },
            text: "Synthetic track — Artist".into(), spoiler: true, media: Media { kind: Some("music".into()), ..Default::default() }, ..Default::default() };
        store.insert_message(&row).await.unwrap();
        let music = wa::message::MusicMessage { artwork_uri: uri.map(str::to_owned), ..Default::default() };
        let bytes = jpeg.get_ref().clone();
        let result = fetch_music_artwork(&store, &row.header.chat, &row.header.id, &music, move |request| {
            assert_eq!(Some(request), uri, "missing URI must not call the fetcher");
            available.then_some(bytes)
        }).await;
        assert_eq!(result.is_ok(), available && uri.is_some());
        let updated = store.message(&row.header.chat, &row.header.id).await.unwrap();
        assert_eq!(updated.text, row.text);
        assert!(updated.spoiler);
        assert_eq!(updated.media.kind.as_deref(), Some("music"));
        assert!(updated.media.path.is_none());
        assert_eq!(updated.media.thumb.is_some(), available && uri.is_some());
        if let Some(thumb) = updated.media.thumb { assert!(thumb.starts_with("data:image/jpeg;base64,")); }
    }
}
