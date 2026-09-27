use super::*;

/// Marks a message context as forwarded, keeping anything already in it (a quote).
pub(super) fn forwarded_context(context: Option<Box<wa::ContextInfo>>) -> Box<wa::ContextInfo> {
    let mut context = context.unwrap_or_default();
    context.is_forwarded = Some(true);
    context.forwarding_score = Some(context.forwarding_score.unwrap_or(0) + 1);
    context
}

/// Whether a received message carries the "Forwarded" label.
pub(super) fn is_forwarded(message: &wa::Message) -> bool {
    message_context(message).is_some_and(|c| c.is_forwarded == Some(true))
}

/// The new group member tag, if this message sets one; empty clears it.
pub(super) fn member_label_change(message: &wa::Message) -> Option<String> {
    use wa::message::protocol_message::Type;
    let protocol = message.get_base_message().protocol_message.as_option()?;
    if protocol.r#type != Some(Type::GROUP_MEMBER_LABEL_CHANGE) {
        return None;
    }
    Some(protocol.member_label.as_option()?.label.clone().unwrap_or_default())
}

/// The edited message's id and its new text (or caption), if this message is an edit.
pub(super) fn edit_of(message: &wa::Message) -> Option<(String, String)> {
    use wa::message::protocol_message::Type;
    let protocol = message.get_base_message().protocol_message.as_option()?;
    if protocol.r#type != Some(Type::MESSAGE_EDIT) {
        return None;
    }
    let target = protocol.key.as_option()?.id.clone().filter(|id| !id.is_empty())?;
    let edited = protocol.edited_message.as_option()?;
    let text = edited.text_content().or_else(|| edited.get_caption())?.to_string();
    Some((target, text))
}

/// The id of the message a revoke refers to, if this message is a revoke.
pub(super) fn revoke_target(message: &wa::Message) -> Option<String> {
    use wa::message::protocol_message::Type;
    let protocol = message.get_base_message().protocol_message.as_option()?;
    if protocol.r#type != Some(Type::REVOKE) {
        return None;
    }
    let key = protocol.key.as_option()?;
    key.id
        .as_deref()
        .filter(|id| !id.is_empty())
        .map(|id| id.to_string())
}

/// What a reply quotes, as far as the message still carries it.
struct Quoted {
    id: String,
    /// Empty when the quote has no participant, which a private chat never sets.
    sender: String,
    text: String,
    kind: &'static str,
    thumb: Option<Vec<u8>>,
    chat: Option<String>,
    /// The quoted message was view-once.
    view_once: bool,
    /// Its media submessage, kept only for a view-once: any other quoted media
    /// is already stored as a message of its own, with its own locator.
    locator: Option<Vec<u8>>,
}

/// The quote a reply carries, as stored beside it; the same for replies received
/// and sent, so both render what was on the wire.
pub(super) fn stored_quote(message: &wa::Message, header: &MessageHeader, client: Option<&Client>) -> Option<Quote> {
    let q = quote_of(message, header)?;
    // Quoting our own message should read "You", not our phone number.
    let mine = client
        .map(|c| [c.pn(), c.lid()].into_iter().flatten().any(|j| j.to_non_ad().to_string() == q.sender))
        .unwrap_or(false);
    // A view-once reaches a linked device only inside a reply quoting
    // it. The copy is kept whatever the gate says, as the bot keeps it,
    // and the gate decides who may use it; dropping it here would make
    // the gate unfalsifiable and the behaviour unmeasurable.
    let locator = if q.view_once { q.locator } else { None };
    // The operator owns this client, so the owner branch of the gate is
    // the one that applies. A view-once is recoverable only when the copy in
    // the reply can actually be fetched: a reply that quoted the bare stub
    // (one sent from this app, for instance) carries no address.
    let allowed = may_take_quote(q.view_once, mine, true);
    let fetchable = locator.as_deref().is_some_and(|bytes| {
        let mut slice = bytes;
        <wa::Message as buffa::Message>::decode(&mut slice)
            .map(|m| {
                use whatsapp_rust::wacore::proto_helpers::MessageExt;
                super::media_download::has_direct_path(m.get_base_message())
            })
            .unwrap_or(false)
    });
    let recoverable = allowed && (!q.view_once || fetchable);
    Some(Quote {
        id: Some(q.id),
        text: Some(q.text),
        sender: Some(if mine { "@me".to_string() } else { q.sender }),
        chat: q.chat,
        kind: if q.kind.is_empty() { None } else { Some(q.kind.to_string()) },
        thumb: q.thumb.as_deref().map(thumb_uri),
        view_once: q.view_once,
        recoverable,
        path: None,
        locator,
    })
}

/// Whether this account may take what a reply quotes.
///
/// This is `puedeRecuperarCitado`, rule for rule: anything that is not a
/// view-once is anyone's, a view-once is its author's, and the client's owner
/// always may. The owner branch is what makes it usable here — a personal
/// client has one user, and that user is its owner, so the operator may take
/// any view-once a reply carries. The author branch is the rule as written, kept
/// because it is the rule; a client with more than one user would pass that
/// user's role instead of `true`.
pub(super) fn may_take_quote(view_once: bool, author_is_me: bool, is_owner: bool) -> bool {
    !view_once || is_owner || author_is_me
}

/// Extracts the quoted message from an incoming message.
///
/// `context_info` lives on each inner message type rather than on `Message`
/// itself, so the carriers a reply can arrive on are checked in turn. The quoted
/// message is unwrapped before it is read: a view-once quote arrives inside a
/// `viewOnceMessage`, possibly wrapped again in an ephemeral one, and without
/// that both its media and its flag stay invisible.
fn quote_of(message: &wa::Message, header: &MessageHeader) -> Option<Quoted> {
    use whatsapp_rust::wacore::proto_helpers::MessageExt;
    let context = message_context(message)?;
    let id = context.stanza_id.as_ref()?.to_string();
    // Who wrote the quoted message. A group always names the participant; a
    // private chat may leave it out, where the author is the reply's sender.
    // In a group an absent participant is left absent rather than guessed at, so
    // that a reply cannot pass itself off as the author of what it quotes.
    let private = !header.chat.parse::<Jid>().is_ok_and(|c| c.is_group());
    let sender = context
        .participant
        .as_ref()
        .map(|p| p.to_string())
        .filter(|p| !p.is_empty())
        .or_else(|| private.then(|| header.sender.clone()))
        .unwrap_or_default();
    // A reply may name its quoted message and carry none of its content: that
    // is what a client sends when it has nothing to quote, and the recipient
    // resolves it from its own history. The name is kept so the quote still
    // points somewhere, and what is left blank is left blank rather than
    // invented, which would otherwise erase a quote already stored.
    let wrapper = context.quoted_message.as_option();
    // The flag rides on the wrapper, so it is read before unwrapping.
    let view_once = wrapper.is_some_and(|w| w.is_view_once());
    let none_quoted = wa::Message::text("");
    let quoted = wrapper.map(|w| w.get_base_message()).unwrap_or(&none_quoted);
    // A quoted media message has no text, so name its type instead of saying
    // "media". The caption, when there is one, wins.
    let (kind, name) = if quoted.image_message.as_option().is_some() {
        ("image", None)
    } else if quoted.video_message.as_option().is_some() {
        ("video", None)
    } else if quoted.audio_message.as_option().is_some() {
        ("audio", None)
    } else if let Some(document) = quoted.document_message.as_option() {
        ("document", document.file_name.clone())
    } else {
        ("", None)
    };
    let text = if wrapper.is_none() {
        // Nothing was quoted but the name, so no text is claimed at all: the
        // empty string is what hides the quote rather than showing a guess.
        String::new()
    } else {
        quoted
            .text_content()
            .filter(|t| !t.is_empty())
            .map(|t| t.to_string())
            .unwrap_or_else(|| match kind {
                "image" => "Photo".to_string(),
                "video" => "Video".to_string(),
                "audio" => "Voice message".to_string(),
                "document" => name.unwrap_or_else(|| "Document".to_string()),
                _ => "[media]".to_string(),
            })
    };
    let thumb = quoted
        .image_message
        .as_option()
        .and_then(|m| m.jpeg_thumbnail.clone())
        .or_else(|| {
            quoted
                .video_message
                .as_option()
                .and_then(|m| m.jpeg_thumbnail.clone())
        })
        .or_else(|| {
            quoted
                .document_message
                .as_option()
                .and_then(|m| m.jpeg_thumbnail.clone())
        });
    // The quoted message exactly as it arrived, view-once wrapper and thumbnail
    // included, so a reply from here can quote it in the same form.
    let locator = wrapper.filter(|_| view_once).map(buffa::Message::encode_to_vec);
    Some(Quoted {
        id,
        sender,
        text,
        kind,
        thumb,
        chat: context.remote_jid.clone(),
        view_once,
        locator,
    })
}

/// Converts a protocol message into a storable one, downloading any media.
///
/// Returns `None` for messages that carry neither text nor media, so protocol
/// traffic does not fill the store with empty rows.
pub(super) async fn incoming_message(
    chat: &str,
    inbound: &InboundMessage,
    client: Option<&Client>,
    media_dir: Option<&Path>,
    auto_download: bool,
) -> Option<StoredMessage> {
    let info = &inbound.info;
    let header = MessageHeader {
        chat: chat.to_string(),
        id: info.id.to_string(),
        sender: info.source.sender.to_string(),
        timestamp: info.timestamp.timestamp(),
        from_me: info.source.is_from_me,
    };
    stored_message(&inbound.message, header, client, media_dir, auto_download).await
}

/// Builds the stored form of a message that arrived live or through history sync.
pub(super) async fn stored_message(
    message: &wa::Message,
    header: MessageHeader,
    client: Option<&Client>,
    media_dir: Option<&Path>,
    auto_download: bool,
) -> Option<StoredMessage> {
    let id = header.id.as_str();
    // Wrappers (disappearing, view-once, captioned document, edit) are flags on
    // the outer message; everything read below lives in the innermost one.
    let outer = message;
    let message = outer.get_base_message();
    let mut text = message.text_content().unwrap_or_default().to_string();
    if text.is_empty() && !outer.is_view_once() {
        text = message.get_caption().unwrap_or_default().to_string();
    }

    let mut media_kind = None;
    let mut media_once_kind = None;
    let mut media_path = None;
    let mut media_thumb = None;
    let mut media_ref = None;
    let mut media_duration = None;

    if let Some(media) = detect_media(message) {
        media_kind = Some(media.kind.to_string());
        media_duration = media.duration;

        // The thumbnail rides in the message, so it is kept even when the file
        // itself is not downloaded. A view-once thumbnail would show it unopened.
        media_thumb = media.thumb.as_deref().filter(|_| !outer.is_view_once()).map(thumb_uri);

        // A view-once has no CDN address, so fetching it here could only fail;
        // the locator is kept instead, so opening the message can ask the
        // sender's phone to upload it again.
        if auto_download && !outer.is_view_once() {
            // Download when a destination and a client are available. A failure
            // still records the message, so the text and metadata are not lost.
            if let (Some(client), Some(dir)) = (client, media_dir) {
                let started = std::time::Instant::now();
                match client.download(media.downloadable.as_ref()).await {
                    Ok(bytes) => {
                        log::debug!("downloaded {id} {} ({} KB) in {:?}", media.kind, bytes.len() / 1024, started.elapsed());
                        if std::fs::create_dir_all(dir).observed().is_some() {
                            let path = dir.join(format!("{}.{}", id, media.extension()));
                            if std::fs::write(&path, &bytes).observed().is_some() {
                                media_path = Some(path.to_string_lossy().to_string());
                            }
                        }
                    }
                    Err(e) => log::warn!("failed to download {id} media: {e}"),
                }
            }
        } else {
            media_ref = Some(media_locator(message));
        }

        if text.is_empty() {
            text = format!("[{}]", media.kind);
        }
    }

        // A view-once has no CDN address of its own, so it cannot be downloaded;
        // the media key is kept because asking the sender's phone to upload it
        // again needs the key and nothing else. The kind is rewritten so the row
        // renders as one-time media rather than as an ordinary photo, and the
        // kind it had is kept so a recovered copy is shown by the right player.
        if outer.is_view_once() && media_kind.is_some() {
            media_once_kind = media_kind.clone();
            media_kind = Some("view_once".to_string());
        }

    // Polls and events render as cards; the row carries their title.
    if media_kind.is_none() {
        if let Some((question, _, _)) = poll_of(message) {
            text = question;
            media_kind = Some("poll".to_string());
        } else if let Some(event) = event_of(message) {
            text = event.name;
            media_kind = Some("event".to_string());
        }
    }

    // Kinds without a dedicated view still arrive as readable cards.
    if text.is_empty() && media_kind.is_none() {
        if let Some((kind, card, thumb)) = card_of(message) {
            text = card;
            media_kind = Some(kind.to_string());
            media_thumb = thumb.as_deref().map(thumb_uri);
        }
    }

    if text.is_empty() && media_kind.is_none() {
        return None;
    }

    // A reply carries the quote in the message context. We do not keep the
    // original protobuf, so the text is copied out for display.
    let quote = stored_quote(message, &header, client).unwrap_or_default();

    // Names are resolved separately and joined by the store on read; a new
    // message stays unread until its chat is opened.
    Some(StoredMessage {
        header,
        text,
        media: Media {
            kind: media_kind,
            path: media_path,
            thumb: media_thumb,
            duration: media_duration,
            locator: media_ref,
            once_kind: media_once_kind,
        },
        quote,
        link: link_preview(message),
        ..Default::default()
    })
}

/// Location and contact messages as a kind, readable text and the map
/// thumbnail when one was sent; `None` for anything else.
fn card_of(message: &wa::Message) -> Option<(&'static str, String, Option<Vec<u8>>)> {
    use whatsapp_rust::wacore::proto_helpers::MessageExt;
    let base = message.get_base_message();
    let lines = |parts: Vec<Option<String>>| {
        parts.into_iter().flatten().filter(|s| !s.trim().is_empty()).collect::<Vec<_>>().join("\n")
    };
    let map = |lat: Option<f64>, lng: Option<f64>| Some(format!("https://maps.google.com/?q={},{}", lat?, lng?));
    if let Some(at) = base.location_message.as_option() {
        let text = lines(vec![at.name.clone(), at.address.clone(), map(at.degrees_latitude, at.degrees_longitude)]);
        return Some(("location", text, at.jpeg_thumbnail.clone()));
    }
    if let Some(at) = base.live_location_message.as_option() {
        let text = lines(vec![at.caption.clone(), map(at.degrees_latitude, at.degrees_longitude)]);
        return Some(("live_location", text, at.jpeg_thumbnail.clone()));
    }
    // A vCard's TEL lines carry the numbers.
    let phones = |vcard: &Option<String>| {
        vcard
            .iter()
            .flat_map(|v| v.lines())
            .filter(|l| l.to_ascii_uppercase().starts_with("TEL"))
            .filter_map(|l| l.split_once(':').map(|(_, n)| n.trim().to_string()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if let Some(contact) = base.contact_message.as_option() {
        let text = lines(vec![contact.display_name.clone(), Some(phones(&contact.vcard))]);
        return Some(("contact", text, None));
    }
    if let Some(list) = base.contacts_array_message.as_option() {
        let people = list
            .contacts
            .iter()
            .map(|c| lines(vec![c.display_name.clone(), Some(phones(&c.vcard))]).replace('\n', " · "))
            .collect::<Vec<_>>();
        return Some(("contact", lines(vec![list.display_name.clone(), Some(people.join("\n"))]), None));
    }
    None
}

/// A received thumbnail as a `data:` URI, stored in the row rather than as one
/// file per message.
pub(super) fn thumb_uri(jpeg: &[u8]) -> String {
    use base64::Engine as _;
    format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(jpeg))
}

/// The message cut down to what `download_media` needs: the media entry
/// without its thumbnail (kept in the row) or the context it quotes.
fn media_locator(message: &wa::Message) -> Vec<u8> {
    let mut slim = wa::Message {
        image_message: message.image_message.clone(),
        video_message: message.video_message.clone(),
        audio_message: message.audio_message.clone(),
        document_message: message.document_message.clone(),
        sticker_message: message.sticker_message.clone(),
        ..Default::default()
    };
    if let Some(m) = slim.image_message.as_option_mut() {
        m.jpeg_thumbnail = None;
        m.context_info = Default::default();
    }
    if let Some(m) = slim.video_message.as_option_mut() {
        m.jpeg_thumbnail = None;
        m.context_info = Default::default();
    }
    if let Some(m) = slim.audio_message.as_option_mut() {
        m.context_info = Default::default();
    }
    if let Some(m) = slim.document_message.as_option_mut() {
        m.jpeg_thumbnail = None;
        m.context_info = Default::default();
    }
    if let Some(m) = slim.sticker_message.as_option_mut() {
        m.context_info = Default::default();
    }
    buffa::Message::encode_to_vec(&slim)
}

/// The link preview a message carries.
fn link_preview(message: &wa::Message) -> LinkCard {
    use whatsapp_rust::wacore::proto_helpers::MessageExt;
    let Some(text) = message
        .get_base_message()
        .extended_text_message
        .as_option()
    else {
        return LinkCard::default();
    };
    // `matched_text` is the URL as it appeared in the message.
    let Some(url) = text.matched_text.clone() else {
        return LinkCard::default();
    };
    LinkCard {
        url: Some(url),
        title: text.title.clone(),
        desc: text.description.clone(),
        thumb: text.jpeg_thumbnail.as_deref().map(thumb_uri),
        ..Default::default()
    }
}

/// The JIDs a message mentions, from whichever message type carries them.
fn message_context(message: &wa::Message) -> Option<&wa::ContextInfo> {
    use whatsapp_rust::wacore::proto_helpers::MessageExt;
    let base = message.get_base_message();
    base.extended_text_message
        .as_option()
        .and_then(|m| m.context_info.as_option())
        .or_else(|| {
            base.image_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            base.video_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            base.audio_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            base.document_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
}

/// Whether a message mentions us: directly, or everyone through @all.
pub(super) fn mentions_me(message: &wa::Message, own: &[String]) -> bool {
    let Some(context) = message_context(message) else {
        return false;
    };
    if !context.group_mentions.is_empty() {
        return true;
    }
    context.mentioned_jid.iter().any(|mention| {
        let bare = mention.split(':').next().unwrap_or(mention);
        own.iter().any(|me| me == mention || me == bare)
    })
}
