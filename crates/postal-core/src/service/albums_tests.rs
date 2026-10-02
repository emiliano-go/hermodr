use super::*;
use buffa::MessageField;
use std::cell::RefCell;

fn input(name: &str) -> AlbumMediaInput {
    AlbumMediaInput { name: name.into(), path: PathBuf::from("synthetic.part"),
        caption: Some("Caption\n送貨".into()), quality: None, progress: Some("progress-1".into()) }
}

fn result() -> AlbumSendResult {
    AlbumSendResult { account_id: "synthetic-account".into(), chat: "15550000001@s.whatsapp.net".into(),
        parent_id: "parent-id".into(), ..Default::default() }
}

#[test]
fn album_counts_bound_metadata_and_reject_non_album_media() {
    assert_eq!(album_counts(&[input("photo.JPG"), input("clip.mp4"), input("still.webp")], false).unwrap(), (2, 1));
    assert!(album_counts(&[], false).is_err());
    assert!(album_counts(&[input("one.jpg")], false).is_err());
    assert_eq!(album_counts(&[input("one.jpg")], true).unwrap(), (1, 0));
    assert!(album_counts(&(0..9).map(|_| input("photo.jpg")).collect::<Vec<_>>(), false).is_err());
    for name in ["sound.mp3", "document.pdf", "animated.gif", "unknown", ""] {
        assert!(album_counts(&[input("photo.jpg"), input(name)], false).is_err());
    }
    let mut huge = input("photo.jpg");
    huge.caption = Some("x".repeat(MAX_CAPTION_BYTES + 1));
    assert!(album_counts(&[input("clip.mp4"), huge], false).is_err());
}

#[test]
fn album_destination_reply_and_mentions_stay_bounded() {
    let reply = ("quote-id".into(), "15550000002@s.whatsapp.net".into(), "Quoted caption".into());
    assert!(album_target("15550000001@s.whatsapp.net", Some(&reply), &["15550000002@s.whatsapp.net".into()]).is_ok());
    assert!(album_target("123@g.us", None, &[]).is_ok());
    for target in ["", "status@broadcast", "123@newsletter", "15550000001:1@s.whatsapp.net"] {
        assert!(album_target(target, None, &[]).is_err());
    }
    assert!(album_target("123@g.us", None, &["123@g.us".into()]).is_err());
    assert!(album_target("123@g.us", None, &vec!["15550000001@s.whatsapp.net".into(); 257]).is_err());
    assert!(album_target("123@g.us", Some(&("".into(), reply.1, reply.2)), &[]).is_err());
}

#[test]
fn parent_counts_reply_and_child_caption_use_public_protocol_shapes() {
    let context = wa::ContextInfo { stanza_id: Some("quoted-id".into()), ..Default::default() };
    let parent = album_message(2, 1, Some(Box::new(context)));
    let album = parent.album_message.as_option().unwrap();
    assert_eq!(album.expected_image_count, Some(2));
    assert_eq!(album.expected_video_count, Some(1));
    assert_eq!(album.context_info.stanza_id.as_deref(), Some("quoted-id"));
    let key = wa::MessageKey { remote_jid: Some("15550000001@s.whatsapp.net".into()),
        id: Some("parent-id".into()), from_me: Some(true), ..Default::default() };
    let inner = wa::Message { image_message: MessageField::some(wa::message::ImageMessage {
        caption: Some("Item caption\n送貨".into()), context_info: MessageField::some(wa::ContextInfo {
            mentioned_jid: vec!["15550000002@s.whatsapp.net".into()], ..Default::default()
        }), ..Default::default()
    }), message_context_info: MessageField::some(wa::MessageContextInfo {
        message_secret: Some(vec![7; 32]), ..Default::default()
    }), ..Default::default() };
    let wrapped = wrap_as_album_child(inner, key);
    let child = wrapped.associated_child_message.message.as_option().unwrap();
    assert_eq!(child.image_message.caption.as_deref(), Some("Item caption\n送貨"));
    assert_eq!(child.image_message.context_info.mentioned_jid, vec!["15550000002@s.whatsapp.net"]);
    let association = wrapped.message_context_info.message_association.as_option().unwrap();
    assert_eq!(association.parent_message_key.id.as_deref(), Some("parent-id"));
    assert_eq!(association.message_index, None);
    assert_eq!(wrapped.message_context_info.message_secret.as_deref(), Some(&[7; 32][..]));
    assert!(failed_before_transport(&whatsapp_rust::SendError::NotLoggedIn));
    assert!(failed_before_transport(&whatsapp_rust::SendError::InvalidRequest("synthetic".into())));
    assert!(!failed_before_transport(&whatsapp_rust::SendError::Internal(anyhow::anyhow!("synthetic indeterminate failure"))));
}

#[tokio::test]
async fn partial_sequence_separates_before_send_failure_from_uncertain_item() {
    for uncertain in [false, true] {
        let calls = RefCell::new(Vec::new());
        let mut result = result();
        send_album_sequence(4, |index| {
            calls.borrow_mut().push(index);
            async move {
                if index == 2 {
                    return Err(if uncertain { AlbumSendFailure::Uncertain("item-2".into(), "transport interrupted".into()) }
                        else { AlbumSendFailure::Before("local save failed before transport".into()) });
                }
                Ok(AlbumSendAttempt { id: format!("item-{index}"), warnings: vec![format!("warning-{index}")], stop: None })
            }
        }, &mut result).await;
        assert_eq!(calls.into_inner(), vec![0, 1, 2]);
        assert_eq!(result.sent_ids, vec!["item-0", "item-1"]);
        assert_eq!(result.next_index, if uncertain { 3 } else { 2 });
        assert_eq!(result.uncertain_index, uncertain.then_some(2));
        assert_eq!(result.uncertain_id.as_deref(), uncertain.then_some("item-2"));
        assert_eq!(result.warnings, vec!["warning-0", "warning-1"]);
        assert_eq!(result.account_id, "synthetic-account");
        assert_eq!(result.chat, "15550000001@s.whatsapp.net");
        assert!(result.error.is_some());
    }
}

#[tokio::test]
async fn account_fence_after_written_item_preserves_sent_prefix_without_retry() {
    let calls = RefCell::new(Vec::new());
    let mut result = result();
    send_album_sequence(4, |index| {
        calls.borrow_mut().push(index);
        async move { Ok(AlbumSendAttempt { id: format!("item-{index}"), warnings: Vec::new(),
            stop: (index == 1).then(|| "account changed after transport".into()) }) }
    }, &mut result).await;
    assert_eq!(calls.into_inner(), vec![0, 1]);
    assert_eq!(result.sent_ids, vec!["item-0", "item-1"]);
    assert_eq!(result.next_index, 2);
    assert_eq!(result.uncertain_index, None);
    assert_eq!(result.account_id, "synthetic-account");
}

#[tokio::test]
async fn successful_sequence_keeps_order_and_serializes_explicit_outcome_fields() {
    let mut result = result();
    send_album_sequence(3, |index| async move {
        Ok(AlbumSendAttempt { id: format!("item-{index}"), warnings: Vec::new(), stop: None })
    }, &mut result).await;
    assert_eq!(result.sent_ids, vec!["item-0", "item-1", "item-2"]);
    assert_eq!(result.next_index, 3);
    assert_eq!(result.error, None);
    let json = serde_json::to_value(result).unwrap();
    assert_eq!(json["parent_id"], "parent-id");
    assert!(json["uncertain_index"].is_null());
    assert_eq!(json["parent_uncertain"], false);
}

#[test]
fn continuation_key_requires_own_public_root_and_preflight_outcome_keeps_scope() {
    let mut parent = StoredMessage::default();
    parent.header.chat = "123@g.us".into();
    parent.header.id = "parent-id".into();
    parent.header.sender = "15550000001@s.whatsapp.net".into();
    parent.header.from_me = true;
    parent.media.kind = Some("album".into());
    parent.local.status = Some("sent".into());
    parent.album = Some(Album { expected_images: Some(2), expected_videos: Some(1), ..Default::default() });
    let key = continuation_key(&parent, "123@g.us").unwrap();
    assert_eq!(key.id.as_deref(), Some("parent-id"));
    assert_eq!(key.remote_jid.as_deref(), Some("123@g.us"));
    assert_eq!(key.from_me, Some(true));
    assert_eq!(key.participant, None);
    assert!(continuation_key(&parent, "456@g.us").is_err());
    for flag in 0..5 {
        let mut invalid = parent.clone();
        match flag {
            0 => invalid.header.from_me = false,
            1 => invalid.local.deleted = true,
            2 => invalid.local.revoked = true,
            3 => invalid.spoiler = true,
            _ => invalid.album.as_mut().unwrap().parent_id = Some("another-parent".into()),
        }
        assert!(continuation_key(&invalid, "123@g.us").is_err());
    }
    let failure = AlbumSendResult::preflight_failure("captured-account", "123@g.us", Some("parent-id"), "parent was removed");
    assert!(failure.preflight_failed);
    assert!(!failure.parent_uncertain);
    assert!(failure.sent_ids.is_empty());
    assert_eq!(failure.next_index, 0);
    assert_eq!(failure.account_id, "captured-account");
    assert_eq!(failure.parent_id, "parent-id");
    assert_eq!(parent.album.unwrap().expected_images, Some(2));
}

#[test]
fn pending_own_album_preserves_local_read_without_increasing_unread() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let mut row = StoredMessage::default();
    row.header.chat = "15550000001@s.whatsapp.net".into();
    row.header.id = "synthetic-album-root".into();
    row.header.sender = "15550000002@s.whatsapp.net".into();
    row.header.from_me = true;
    row.media.kind = Some("album".into());
    row.album = Some(Album { expected_images: Some(2), ..Default::default() });
    row.local.status = Some("delivered".into());
    pending_album_row(&mut row);
    store.insert_message(&row).unwrap();
    let stored = store.message(&row.header.chat, &row.header.id).unwrap();
    assert!(!stored.local.read);
    assert_eq!(stored.local.status.as_deref(), Some("pending"));
    assert!(store.unread_ids(&row.header.chat).unwrap().is_empty());
    assert_eq!(store.chats().unwrap()[0].unread_count, 0);
}

#[tokio::test]
async fn written_album_provenance_keeps_pending_and_atomic_child_insert_rejects_cleared_root() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let mut root = StoredMessage::default();
    root.header.chat = "15550000001@s.whatsapp.net".into();
    root.header.id = "synthetic-root".into();
    root.header.sender = "15550000002@s.whatsapp.net".into();
    root.header.from_me = true;
    root.media.kind = Some("album".into());
    root.album = Some(Album { expected_images: Some(2), ..Default::default() });
    pending_album_row(&mut root);
    store.insert_message(&root).unwrap();
    let worker = StoreWorker::new(store);
    worker.mark_album_request_written(&root.header.chat, &root.header.id).await.unwrap();
    assert_eq!(worker.message(&root.header.chat, &root.header.id).await.unwrap().local.status.as_deref(), Some("pending"));
    let mut child = root.clone();
    child.header.id = "synthetic-child".into();
    child.media.kind = Some("image".into());
    child.album = Some(Album { parent_id: Some(root.header.id.clone()), index: Some(0), ..Default::default() });
    pending_album_row(&mut child);
    let inserted = worker.insert_album_child(&root.header.id, &child).await.unwrap();
    assert!(!inserted.local.read);
    assert_eq!(inserted.local.status.as_deref(), Some("pending"));
    restore_self_album_delivery(&worker, &root, false).await.unwrap();
    assert_eq!(worker.message(&root.header.chat, &root.header.id).await.unwrap().local.status.as_deref(), Some("pending"));
    restore_self_album_delivery(&worker, &root, true).await.unwrap();
    assert_eq!(worker.message(&root.header.chat, &root.header.id).await.unwrap().local.status.as_deref(), Some("delivered"));
    worker.set_delivery_state(&root.header.chat, &root.header.id, "read").await.unwrap();
    restore_self_album_delivery(&worker, &root, true).await.unwrap();
    assert_eq!(worker.message(&root.header.chat, &root.header.id).await.unwrap().local.status.as_deref(), Some("read"));
    let chat = root.header.chat.clone();
    worker.run(move |store| store.clear_chat(&chat)).await.unwrap();
    child.header.id = "must-not-revive".into();
    assert!(worker.insert_album_child(&root.header.id, &child).await.is_err());
    assert!(worker.message(&root.header.chat, &child.header.id).await.is_err());
}

#[tokio::test]
async fn unsent_rollback_removes_generated_pending_row_and_preserves_written_and_unrelated_rows() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let mut generated = StoredMessage::default();
    generated.header.chat = "15550000001@s.whatsapp.net".into();
    generated.header.id = "synthetic-unsent-root".into();
    generated.header.sender = "15550000002@s.whatsapp.net".into();
    generated.header.from_me = true;
    generated.media.kind = Some("album".into());
    generated.album = Some(Album { expected_images: Some(2), ..Default::default() });
    pending_album_row(&mut generated);
    let mut written = generated.clone();
    written.header.id = "synthetic-written-root".into();
    let mut unrelated = generated.clone();
    unrelated.header.id = "unrelated-message".into();
    unrelated.media.kind = None;
    unrelated.album = None;
    unrelated.text = "Keep this unrelated message".into();
    for row in [&generated, &written, &unrelated] { store.insert_message(row).unwrap(); }
    let worker = StoreWorker::new(store);
    worker.mark_album_request_written(&written.header.chat, &written.header.id).await.unwrap();
    worker.discard_unsent_album_attempt(&generated).await.unwrap();
    assert!(worker.message(&generated.header.chat, &generated.header.id).await.is_err());
    let _ = worker.discard_unsent_album_attempt(&written).await;
    let _ = worker.discard_unsent_album_attempt(&unrelated).await;
    assert!(worker.message(&written.header.chat, &written.header.id).await.is_ok());
    assert_eq!(worker.message(&unrelated.header.chat, &unrelated.header.id).await.unwrap().text, unrelated.text);
}
