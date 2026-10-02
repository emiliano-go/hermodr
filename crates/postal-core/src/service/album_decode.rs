use super::*;
use crate::store::Album;

pub(super) fn metadata(message: &wa::Message, header: &MessageHeader) -> Option<Album> {
    let base = decoded_message(message).message;
    if let Some(album) = base.album_message.as_option() {
        return Some(Album { expected_images: album.expected_image_count,
            expected_videos: album.expected_video_count, ..Default::default() });
    }
    if !base.image_message.is_set() && !base.video_message.is_set() { return None; }
    let association = association(message)?;
    if association.association_type != Some(wa::message_association::AssociationType::MEDIA_ALBUM) { return None; }
    let key = association.parent_message_key.as_option()?;
    let id = key.id.as_deref().filter(|id| !id.is_empty() && id.len() <= 512 && !id.contains('\0') && *id != header.id)?;
    if key.participant.as_deref().is_some_and(|sender| !same_jid(sender, &header.sender)) { return None; }
    if let Some(chat) = key.remote_jid.as_deref() {
        let destination: Jid = chat.parse().ok()?;
        if header.chat.ends_with("@g.us") {
            if !same_jid(chat, &header.chat) { return None; }
        } else if !destination.is_pn() && !destination.is_lid() { return None; }
    }
    // Parent keys describe the sender's view; their DM destination/from_me differ on receipt.
    Some(Album { parent_id: Some(id.into()), index: association.message_index.and_then(|index| u32::try_from(index).ok()), ..Default::default() })
}

fn same_jid(left: &str, right: &str) -> bool {
    match (left.parse::<Jid>(), right.parse::<Jid>()) {
        (Ok(left), Ok(right)) => left.to_non_ad() == right.to_non_ad(),
        _ => false,
    }
}

fn association(mut message: &wa::Message) -> Option<&wa::MessageAssociation> {
    loop {
        if message.associated_child_message.is_set() {
            return message.message_context_info.as_option()?.message_association.as_option();
        }
        let base = message.get_base_message();
        if !std::ptr::eq(base, message) { message = base; continue; }
        message = message.spoiler_message.as_option()?.message.as_option()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buffa::MessageField;
    use whatsapp_rust::wacore::proto_helpers::wrap_as_album_child;

    fn header() -> MessageHeader {
        MessageHeader { chat: "1@s.whatsapp.net".into(), sender: "1@s.whatsapp.net".into(),
            id: "child".into(), timestamp: 20, from_me: false }
    }

    fn child(caption: &str) -> wa::Message {
        wrap_as_album_child(wa::Message { image_message: MessageField::some(wa::message::ImageMessage {
            caption: Some(caption.into()), ..Default::default() }), ..Default::default() },
            wa::MessageKey { id: Some("parent".into()), remote_jid: Some("2@s.whatsapp.net".into()), from_me: Some(true), ..Default::default() })
    }

    #[tokio::test]
    async fn associated_children_decode_with_caption_quote_and_private_flags() {
        let mut message = child("caption survives");
        message.associated_child_message.as_option_mut().unwrap().message.as_option_mut().unwrap().image_message.as_option_mut().unwrap().context_info = MessageField::some(wa::ContextInfo {
            stanza_id: Some("quote".into()), participant: Some("3@s.whatsapp.net".into()),
            quoted_message: MessageField::some(wa::Message::text("quoted")), ..Default::default()
        });
        let row = stored_message(&message, header(), None, None, false).await.unwrap();
        assert_eq!(row.text, "caption survives");
        assert_eq!(row.media.kind.as_deref(), Some("image"));
        assert_eq!(row.album.unwrap().parent_id.as_deref(), Some("parent"));
        assert_eq!(row.quote.id.as_deref(), Some("quote"));
        let hidden = wa::Message { spoiler_message: MessageField::some(wa::message::FutureProofMessage {
            message: MessageField::some(message), ..Default::default() }), ..Default::default() };
        let row = stored_message(&hidden, header(), None, None, false).await.unwrap();
        assert!(row.spoiler);
        assert!(row.album.is_some());
    }

    #[tokio::test]
    async fn parent_counts_decode_without_inventing_children() {
        let message = wa::Message { album_message: MessageField::some(wa::message::AlbumMessage {
            expected_image_count: Some(2), expected_video_count: Some(1), ..Default::default() }), ..Default::default() };
        let row = stored_message(&message, header(), None, None, false).await.unwrap();
        assert_eq!(row.media.kind.as_deref(), Some("album"));
        assert_eq!(row.album.unwrap(), Album { expected_images: Some(2), expected_videos: Some(1), ..Default::default() });
    }

    #[test]
    fn foreign_and_unrelated_associations_do_not_group_media() {
        let mut message = child("caption");
        message.message_context_info.as_option_mut().unwrap().message_association.as_option_mut().unwrap().association_type = Some(wa::message_association::AssociationType::BOT_PLUGIN);
        assert_eq!(metadata(&message, &header()), None);
        message.message_context_info.as_option_mut().unwrap().message_association.as_option_mut().unwrap().association_type = Some(wa::message_association::AssociationType::MEDIA_ALBUM);
        message.message_context_info.as_option_mut().unwrap().message_association.as_option_mut().unwrap().parent_message_key.as_option_mut().unwrap().participant = Some("3@s.whatsapp.net".into());
        assert_eq!(metadata(&message, &header()), None);
        let mut group_header = header();
        group_header.chat = "4@g.us".into();
        assert_eq!(metadata(&child("caption"), &group_header), None);
        assert_eq!(metadata(&wa::Message::text("plain"), &header()), None);
    }
}
