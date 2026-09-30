use super::*;

fn header() -> MessageHeader {
    MessageHeader { chat: "100@g.us".into(), id: "synthetic".into(), sender: "200@lid".into(), timestamp: 1000, from_me: false }
}

fn spoiler(message: wa::Message) -> wa::Message {
    wa::Message { spoiler_message: MessageField::some(wa::message::FutureProofMessage {
        message: MessageField::some(message), ..Default::default()
    }), ..Default::default() }
}

fn round_video(view_once: bool) -> wa::Message {
    wa::Message { ptv_message: MessageField::some(wa::message::VideoMessage {
        media_key: Some(vec![7; 32]), direct_path: Some("/synthetic-video".into()),
        jpeg_thumbnail: Some(vec![1, 2, 3]), seconds: Some(12), view_once: Some(view_once),
        caption: Some("Synthetic round caption".into()),
        context_info: MessageField::some(wa::ContextInfo { stanza_id: Some("quoted".into()), ..Default::default() }),
        ..Default::default()
    }), ..Default::default() }
}

#[tokio::test]
async fn round_video_retains_the_video_download_key_and_mp4_kind() {
    let message = round_video(false);
    let media = detect_media(&message).unwrap();
    assert_eq!((media.kind, media.extension(), media.duration), ("round_video", "mp4".into(), Some(12)));
    let stored = stored_message(&message, header(), None, None, false).await.unwrap();
    assert_eq!(stored.media.kind.as_deref(), Some("round_video"));
    assert_eq!(stored.text, "Synthetic round caption");
    assert_eq!(stored.media.duration, Some(12));
    assert!(stored.media.thumb.is_some());
    let locator = <wa::Message as buffa::Message>::decode(&mut stored.media.locator.unwrap().as_slice()).unwrap();
    let video = locator.ptv_message.as_option().unwrap();
    assert_eq!(video.media_key, Some(vec![7; 32]));
    assert_eq!(video.direct_path.as_deref(), Some("/synthetic-video"));
    assert!(video.jpeg_thumbnail.is_none());
    assert!(video.context_info.is_unset());
    assert!(locator.video_message.is_unset());
}

#[tokio::test]
async fn wrapper_and_context_spoilers_preserve_the_text_without_history_admission() {
    let secret = "Synthetic spoiler https://example.invalid/private @200";
    let context_flag = wa::Message { extended_text_message: MessageField::some(wa::message::ExtendedTextMessage {
        text: Some(secret.into()), context_info: MessageField::some(wa::ContextInfo { is_spoiler: Some(true), ..Default::default() }),
        ..Default::default()
    }), ..Default::default() };
    let wrapped = wa::Message { ephemeral_message: MessageField::some(wa::message::FutureProofMessage {
        message: MessageField::some(spoiler(wa::Message::text(secret))), ..Default::default()
    }), ..Default::default() };
    for message in [wrapped, context_flag] {
        let stored = stored_message(&message, header(), None, None, false).await.unwrap();
        assert!(stored.spoiler);
        assert_eq!(stored.text, secret);
        assert!(!stored.history_shareable);
    }
}

#[tokio::test]
async fn nested_spoiler_round_video_keeps_view_once_and_redacts_quotes() {
    let message = spoiler(round_video(true));
    assert!(decoded_message(&message).view_once);
    let stored = stored_message(&message, header(), None, None, false).await.unwrap();
    assert!(stored.spoiler);
    assert_eq!(stored.media.kind.as_deref(), Some("view_once"));
    assert_eq!(stored.media.once_kind.as_deref(), Some("round_video"));
    assert!(stored.media.thumb.is_none());
    assert!(!stored.text.contains("Synthetic round caption"));
    let reply = wa::Message { extended_text_message: MessageField::some(wa::message::ExtendedTextMessage {
        text: Some("Reply".into()), context_info: MessageField::some(wa::ContextInfo {
            stanza_id: Some("quoted".into()), quoted_message: MessageField::some(message), ..Default::default()
        }), ..Default::default()
    }), ..Default::default() };
    let quote = quote_of(&reply, &header()).unwrap();
    assert_eq!(quote.text, "[Spoiler]");
    assert_eq!(quote.kind, "round_video");
    assert!(quote.thumb.is_none());
    assert!(quote.view_once);
    assert!(quote.locator.is_some());
    let stub = empty_view_once(Some("round_video"));
    assert!(decoded_message(&stub).view_once);
    assert!(decoded_message(&stub).message.ptv_message.is_set());
}

#[tokio::test]
async fn music_artwork_uri_is_retained_for_the_existing_download_action() {
    let message = wa::Message { music_message: MessageField::some(wa::message::MusicMessage {
        artwork_uri: Some("https://example.invalid/artwork.jpg".into()),
        embedded_music: MessageField::some(wa::EmbeddedMusic { title: Some("Synthetic track".into()), author: Some("Artist".into()), ..Default::default() }),
        context_info: MessageField::some(wa::ContextInfo { stanza_id: Some("private-quote".into()), ..Default::default() }),
        ..Default::default()
    }), ..Default::default() };
    let stored = stored_message(&message, header(), None, None, false).await.unwrap();
    assert_eq!(stored.media.kind.as_deref(), Some("music"));
    assert_eq!(stored.text, "Synthetic track — Artist");
    let locator = <wa::Message as buffa::Message>::decode(&mut stored.media.locator.unwrap().as_slice()).unwrap();
    let music = locator.music_message.as_option().unwrap();
    assert_eq!(music.artwork_uri.as_deref(), Some("https://example.invalid/artwork.jpg"));
    assert!(music.context_info.is_unset());
}

#[tokio::test]
async fn unsupported_content_has_a_readable_row_while_controls_and_empty_payloads_do_not() {
    let message = wa::Message { placeholder_message: MessageField::some(wa::message::PlaceholderMessage::default()), ..Default::default() };
    let stored = stored_message(&message, header(), None, None, false).await.unwrap();
    assert_eq!(stored.media.kind.as_deref(), Some("unknown"));
    assert_eq!(stored.text, "[Unsupported message]");
    assert!(stored_message(&wa::Message::default(), header(), None, None, false).await.is_none());
    let reaction = wa::Message { reaction_message: MessageField::some(wa::message::ReactionMessage {
        text: Some("👍".into()), ..Default::default()
    }), ..Default::default() };
    assert!(stored_message(&reaction, header(), None, None, false).await.is_none());
}

#[tokio::test]
async fn edited_spoiler_flags_reach_the_stored_row_before_a_content_free_hint() {
    let secret = "Synthetic edit secret https://example.invalid/spoiler";
    let payloads = [spoiler(wa::Message::text(secret)), wa::Message {
        extended_text_message: MessageField::some(wa::message::ExtendedTextMessage {
            text: Some(secret.into()), context_info: MessageField::some(wa::ContextInfo { is_spoiler: Some(true), ..Default::default() }),
            ..Default::default()
        }), ..Default::default()
    }];
    for payload in payloads {
        let wire = wa::Message { ephemeral_message: MessageField::some(wa::message::FutureProofMessage {
            message: MessageField::some(wa::Message { protocol_message: MessageField::some(wa::message::ProtocolMessage {
                r#type: Some(wa::message::protocol_message::Type::MESSAGE_EDIT),
                key: MessageField::some(wa::MessageKey { id: Some("synthetic".into()), ..Default::default() }),
                edited_message: MessageField::some(payload), ..Default::default()
            }), ..Default::default() }), ..Default::default()
        }), ..Default::default() };
        let (target, text, spoiler) = super::edit_of(&wire).unwrap();
        assert_eq!((target.as_str(), text.as_str(), spoiler), ("synthetic", secret, true));
        let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
        let original = StoredMessage { header: header(), text: "Originally ordinary".into(), history_shareable: true, ..Default::default() };
        store.insert_message(&original).await.unwrap();
        assert!(store.update_message_spoiler(&original.header.chat, &target, &text, spoiler).await.unwrap());
        let changed = store.message(&original.header.chat, &target).await.unwrap();
        assert!(changed.spoiler);
        assert_eq!(changed.text, secret);
        assert!(!changed.history_shareable);
        let hint = serde_json::to_string(&ServiceEvent::hint(&changed, false)).unwrap();
        assert!(!hint.contains("Synthetic edit secret") && !hint.contains("example.invalid"));
    }
}
