use super::*;

fn hash() -> String {
    filehash_of_hash(&[255; 32])
}

fn action() -> wa::sync_action_value::StickerAction {
    wa::sync_action_value::StickerAction {
        direct_path: Some("/synthetic/sticker.enc".into()),
        media_key: Some(vec![1; 32]),
        file_enc_sha256: Some(vec![2; 32]),
        is_favorite: Some(true),
        ..Default::default()
    }
}

#[test]
fn wire_hash_with_slashes_becomes_single_safe_filename() {
    let hash = hash();
    assert!(hash.contains('/'));
    let name = sticker_filename(&hash).unwrap();
    assert_eq!(name, format!("{}.webp", "ff".repeat(32)));
    assert!(!name.contains('/') && !name.contains('\\'));
    for invalid in ["../outside", "", "ZmFrZQ=="] {
        assert!(sticker_filename(invalid).is_err());
    }
    assert_eq!(filehash_of_hash(&sticker_hash(&hash).unwrap()), hash);
}

#[test]
fn sparse_favorite_has_no_replacement_locator_and_complete_action_has_plain_hash() {
    let sparse = wa::sync_action_value::StickerAction {
        is_favorite: Some(false),
        ..Default::default()
    };
    assert!(locator_from_sticker_action(&hash(), &sparse).is_none());
    let locator = locator_from_sticker_action(&hash(), &action()).unwrap();
    let message = <wa::Message as buffa::Message>::decode(&mut locator.as_slice()).unwrap();
    assert_eq!(
        message
            .sticker_message
            .as_option()
            .unwrap()
            .file_sha256
            .as_deref(),
        Some([255; 32].as_slice())
    );
    let mut malformed = action();
    malformed.media_key = Some(vec![1; 31]);
    assert!(locator_from_sticker_action(&hash(), &malformed).is_none());
}

#[test]
fn sync_values_keep_milliseconds_and_optional_recent_threshold() {
    let at = 1_700_000_000_123;
    let favorite = favorite_value(action(), at);
    assert_eq!(favorite.timestamp, Some(at));
    let removed = recent_removal_value(at, Some(at - 321));
    assert_eq!(removed.timestamp, Some(at));
    assert_eq!(
        removed
            .remove_recent_sticker_action
            .as_option()
            .unwrap()
            .last_sticker_sent_ts,
        Some(at - 321)
    );
    assert!(recent_removal_value(at, None)
        .remove_recent_sticker_action
        .as_option()
        .unwrap()
        .last_sticker_sent_ts
        .is_none());
}

#[test]
fn failed_push_returns_error_with_local_change_and_unknown_remote_outcome() {
    assert!(pushed_result(Ok(()), "Favorite").is_ok());
    let error = pushed_result(Err(anyhow::anyhow!("synthetic")), "Favorite")
        .unwrap_err()
        .to_string();
    assert!(error.contains("saved locally") && error.contains("Remote outcome is unknown"));
}

#[test]
fn sticker_recency_uses_payload_timestamp_and_suppresses_private_context() {
    let mut message = wa::Message {
        sticker_message: buffa::MessageField::some(wa::message::StickerMessage {
            file_sha256: Some(vec![255; 32]),
            sticker_sent_ts: Some(12345),
            is_lottie: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    };
    let info = sticker_id_from_locator(&media_locator(&message)).unwrap();
    assert_eq!((info.2, info.4), (true, Some(12345)));
    message.sticker_message = buffa::MessageField::some(wa::message::StickerMessage {
        file_sha256: Some(vec![255; 32]),
        ..Default::default()
    });
    assert!(sticker_id_from_locator(&media_locator(&message))
        .unwrap()
        .4
        .is_none());
    message.sticker_message = buffa::MessageField::some(wa::message::StickerMessage {
        file_sha256: Some(vec![255; 32]),
        context_info: buffa::MessageField::some(wa::ContextInfo {
            is_spoiler: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    });
    assert!(sticker_id_from_locator(&buffa::Message::encode_to_vec(&message)).is_none());
}

#[test]
fn resync_report_keeps_catalog_and_mirror_evidence_separate() {
    let report = StickerResyncReport {
        app_state_synced: true,
        packs: 1,
        stickers: 2,
        ..Default::default()
    };
    let value = serde_json::to_value(report).unwrap();
    assert_eq!(value["app_state_synced"], true);
    assert_eq!(value["mirror_verified"], false);
    assert_eq!(value["catalog_complete"], false);
}

#[test]
fn received_pack_header_rejects_private_wrapper() {
    let public = wa::Message {
        sticker_pack_message: buffa::MessageField::some(wa::message::StickerPackMessage {
            sticker_pack_id: Some("synthetic-pack".into()),
            name: Some("Pack".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(
        pack_from_message(&public).unwrap().pack_id,
        "synthetic-pack"
    );
    let private = wa::Message {
        view_once_message_v2: buffa::MessageField::some(wa::message::FutureProofMessage {
            message: buffa::MessageField::some(public),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(pack_from_message(&private).is_none());
}

#[tokio::test]
async fn accepted_sticker_uses_leased_worker_and_genuine_send_timestamp() {
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    let lease = store.batch().await;
    let message = wa::Message {
        sticker_message: buffa::MessageField::some(wa::message::StickerMessage {
            file_sha256: Some(vec![255; 32]),
            sticker_sent_ts: Some(12345),
            ..Default::default()
        }),
        ..Default::default()
    };
    let row = StoredMessage {
        header: MessageHeader {
            from_me: true,
            ..Default::default()
        },
        media: Media {
            kind: Some("sticker".into()),
            locator: Some(media_locator(&message)),
            ..Default::default()
        },
        ..Default::default()
    };
    let dirty = tokio::time::timeout(Duration::from_secs(2), record_sticker(&lease, &row))
        .await
        .unwrap()
        .unwrap();
    assert!(dirty);
    lease.finish().await.unwrap();
    assert_eq!(
        store.sticker_recent_sent_ms(hash()).await.unwrap(),
        Some(12345)
    );
    let lease = store.batch().await;
    let mut private = row;
    private.spoiler = true;
    assert!(!record_sticker(&lease, &private).await.unwrap());
    lease.finish().await.unwrap();
}
