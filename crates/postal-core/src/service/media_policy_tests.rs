use super::*;
use buffa::MessageField;

async fn handler() -> Inbound {
    Inbound {
        store: StoreWorker::open(Path::new(":memory:")).await.unwrap(),
        disk_retention: Arc::new(DiskRetentionManager::new(DiskRetention::unlimited())),
        events: broadcast::channel(8).0,
        connected: Arc::default(),
        client_for_events: Arc::default(),
        media_dir: None,
        group_cache: Arc::default(),
        groups_cache: Arc::default(),
        older_waits: Arc::default(),
        message_capping_check: Arc::default(),
        downloads: Arc::new(tokio::sync::Semaphore::new(1)),
        sync_progress: Arc::default(),
        media_auto_download: Arc::default(),
        keep_archived: Arc::default(),
        keep_view_once: Arc::default(),
        one_time_only: false,
        tally: Arc::default(),
        secret_edits: Default::default(),
    }
}

#[tokio::test]
async fn inbound_download_gate_applies_live_global_and_independent_nullable_overrides() {
    let handler = handler().await;
    let chat = "100@g.us";
    for kind in ["image", "audio", "sticker", "future_kind"] {
        assert!(
            !handler.auto_download_for(&handler.store, chat, kind).await,
            "{kind}"
        );
    }
    *handler.media_auto_download.write().unwrap() = MediaAutoDownload {
        audio: true,
        ..Default::default()
    };
    assert!(
        handler
            .auto_download_for(&handler.store, chat, "audio")
            .await
    );
    assert!(
        !handler
            .auto_download_for(&handler.store, chat, "sticker")
            .await
    );
    handler
        .store
        .set_chat_media_auto_download(
            chat,
            MediaAutoDownloadOverrides {
                audio: Some(false),
                sticker: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(
        !handler
            .auto_download_for(&handler.store, chat, "audio")
            .await
    );
    assert!(
        handler
            .auto_download_for(&handler.store, chat, "sticker")
            .await
    );
    handler
        .store
        .set_chat_media_auto_download(chat, MediaAutoDownloadOverrides::default())
        .await
        .unwrap();
    assert!(
        handler
            .auto_download_for(&handler.store, chat, "audio")
            .await
    );
    assert!(
        !handler
            .auto_download_for(&handler.store, chat, "sticker")
            .await
    );
    *handler.media_auto_download.write().unwrap() = MediaAutoDownload::all(true);
    for kind in [
        "image",
        "video",
        "round_video",
        "audio",
        "document",
        "sticker",
        "gif",
    ] {
        assert!(
            handler.auto_download_for(&handler.store, chat, kind).await,
            "{kind}"
        );
    }
    assert!(
        !handler
            .auto_download_for(&handler.store, chat, "future_kind")
            .await
    );
    *handler.media_auto_download.write().unwrap() = MediaAutoDownload::default();
    assert!(
        !handler
            .auto_download_for(&handler.store, chat, "audio")
            .await
    );
}

#[tokio::test]
async fn canonical_media_decode_maps_all_downloadable_kinds_to_policy() {
    use wa::message::*;
    let cases = [
        (
            wa::Message {
                image_message: MessageField::some(ImageMessage::default()),
                ..Default::default()
            },
            "image",
        ),
        (
            wa::Message {
                video_message: MessageField::some(VideoMessage::default()),
                ..Default::default()
            },
            "video",
        ),
        (
            wa::Message {
                ptv_message: MessageField::some(VideoMessage::default()),
                ..Default::default()
            },
            "round_video",
        ),
        (
            wa::Message {
                video_message: MessageField::some(VideoMessage {
                    gif_playback: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            },
            "gif",
        ),
        (
            wa::Message {
                audio_message: MessageField::some(AudioMessage {
                    ptt: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            },
            "audio",
        ),
        (
            wa::Message {
                audio_message: MessageField::some(AudioMessage {
                    ptt: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            },
            "audio",
        ),
        (
            wa::Message {
                document_message: MessageField::some(DocumentMessage::default()),
                ..Default::default()
            },
            "document",
        ),
        (
            wa::Message {
                sticker_message: MessageField::some(StickerMessage::default()),
                ..Default::default()
            },
            "sticker",
        ),
    ];
    for (message, kind) in cases {
        let stored = stored_message(&message, MessageHeader::default(), None, None, false)
            .await
            .unwrap();
        let actual = stored.media.kind.as_deref().unwrap();
        assert_eq!(actual, kind);
        assert!(MediaAutoDownload::all(true).enabled(actual));
        assert!(!MediaAutoDownload::default().enabled(actual));
        let audio_only = MediaAutoDownload {
            audio: true,
            ..Default::default()
        };
        assert_eq!(audio_only.enabled(actual), kind == "audio");
    }
}
