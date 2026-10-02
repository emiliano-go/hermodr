use super::*;

#[cfg(test)]
#[path = "message_decode_special_tests.rs"]
mod special_message_tests;

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

/// The target, replacement text and spoiler flag of a text edit.
pub(super) fn edit_of(message: &wa::Message) -> Option<(String, String, bool)> {
    use wa::message::protocol_message::Type;
    let protocol = decoded_message(message).message.protocol_message.as_option()?;
    if protocol.r#type != Some(Type::MESSAGE_EDIT) {
        return None;
    }
    let target = protocol.key.as_option()?.id.clone().filter(|id| !id.is_empty())?;
    let edited = decoded_message(protocol.edited_message.as_option()?);
    let text = edited.message.text_content().or_else(|| edited.message.get_caption())
        .or_else(|| edited.message.ptv_message.as_option().and_then(|video| video.caption.as_deref()))?.to_string();
    Some((target, text, edited.spoiler))
}

/// A live location update, as carried by an edit of the original message.
pub(super) enum LiveLocationUpdate {
    /// A new position and its fresh map snapshot.
    Moved { live: LiveLocation, thumb: Option<Vec<u8>> },
    /// The share was stopped; the last position stays.
    Ended,
}

/// The target and update of a message that edits a live location, if it is
/// one. An edit with no coordinates means the share stopped.
pub(super) fn live_location_edit_of(message: &wa::Message, timestamp: i64) -> Option<(String, LiveLocationUpdate)> {
    use wa::message::protocol_message::Type;
    let protocol = message.get_base_message().protocol_message.as_option()?;
    if protocol.r#type != Some(Type::MESSAGE_EDIT) {
        return None;
    }
    let target = protocol.key.as_option()?.id.clone().filter(|id| !id.is_empty())?;
    let edited = protocol.edited_message.as_option()?;
    let at = edited.get_base_message().live_location_message.as_option()?;
    // Frequent while moving; the shape (sequence, offset, whether a position
    // came) is what tells a stop apart from an update if the card ever stalls.
    log::debug!(
        "live location edit: sequence={:?} offset={:?} has_position={} expiration={:?}",
        at.sequence_number,
        at.time_offset,
        at.degrees_latitude.is_some() && at.degrees_longitude.is_some(),
        at.context_info.as_option().and_then(|c| c.expiration),
    );
    let Some(live) = live_from_proto(at, timestamp, live_expiry(at, timestamp)) else {
        return Some((target, LiveLocationUpdate::Ended));
    };
    Some((target, LiveLocationUpdate::Moved { live, thumb: at.jpeg_thumbnail.clone() }))
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
    let context = message_context(message)?;
    let id = context.stanza_id.as_ref()?.to_string();
    let sender = quote_author(context, header);
    // The wrapper is read before unwrapping: the flag and the locator ride on
    // it, while the text and media live in the message it wraps.
    let wrapper = context.quoted_message.as_option();
    let none_quoted = wa::Message::text("");
    let decoded = decoded_message(wrapper.unwrap_or(&none_quoted));
    let view_once = decoded.view_once;
    let quoted = decoded.message;
    let (kind, name) = quoted_kind_and_name(quoted);
    let text = if decoded.spoiler { "[Spoiler]".into() } else { quoted_text(quoted, wrapper.is_some(), kind, name.as_deref()) };
    let thumb = if decoded.spoiler { None } else { quoted_thumb(quoted) };
    Some(Quoted {
        id,
        sender,
        text,
        kind,
        thumb,
        chat: context.remote_jid.clone(),
        view_once,
        // The quoted message exactly as it arrived, view-once wrapper included,
        // so a reply from here can quote it in the same form.
        locator: wrapper.filter(|_| view_once).map(buffa::Message::encode_to_vec),
    })
}

/// Who wrote the quoted message. A group always names the participant; a
/// private chat may leave it out, where the author is the reply's sender.
/// In a group an absent participant is left absent rather than guessed at, so
/// that a reply cannot pass itself off as the author of what it quotes.
fn quote_author(context: &wa::ContextInfo, header: &MessageHeader) -> String {
    let private = !header.chat.parse::<Jid>().is_ok_and(|c| c.is_group());
    context
        .participant
        .as_ref()
        .map(|p| p.to_string())
        .filter(|p| !p.is_empty())
        .or_else(|| private.then(|| header.sender.clone()))
        .unwrap_or_default()
}

/// A quoted media message has no text, so its type and file name are read
/// instead of saying "media".
fn quoted_kind_and_name(quoted: &wa::Message) -> (&'static str, Option<String>) {
    if quoted.image_message.as_option().is_some() {
        ("image", None)
    } else if quoted.video_message.as_option().is_some() {
        ("video", None)
    } else if quoted.ptv_message.is_set() {
        ("round_video", None)
    } else if quoted.audio_message.as_option().is_some() {
        ("audio", None)
    } else if let Some(document) = quoted.document_message.as_option() {
        ("document", document.file_name.clone())
    } else {
        ("", None)
    }
}

/// The text shown for a quote: the quoted message's own text, or its media name
/// when it has none. A reply that named its quoted message but carried none of
/// its content keeps an empty text rather than claiming a guess.
fn quoted_text(quoted: &wa::Message, wrapper: bool, kind: &str, name: Option<&str>) -> String {
    if !wrapper {
        return String::new();
    }
    quoted
        .text_content()
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .unwrap_or_else(|| match kind {
            "image" => "Photo".to_string(),
            "video" => "Video".to_string(),
            "round_video" => "Round video".to_string(),
            "audio" => "Voice message".to_string(),
            "document" => name.unwrap_or("Document").to_string(),
            _ => "[media]".to_string(),
        })
}

/// The quoted message's thumbnail, when it carries one.
fn quoted_thumb(quoted: &wa::Message) -> Option<Vec<u8>> {
    quoted
        .image_message
        .as_option()
        .and_then(|m| m.jpeg_thumbnail.clone())
        .or_else(|| quoted.video_message.as_option().and_then(|m| m.jpeg_thumbnail.clone()))
        .or_else(|| quoted.ptv_message.as_option().and_then(|m| m.jpeg_thumbnail.clone()))
        .or_else(|| quoted.document_message.as_option().and_then(|m| m.jpeg_thumbnail.clone()))
}

#[cfg(test)]
mod quote_tests {
    use super::*;
    use buffa::MessageField;

    #[test]
    fn a_quote_names_its_kind_and_falls_back_to_a_readable_text() {
        let photo = wa::Message {
            image_message: MessageField::some(wa::message::ImageMessage::default()),
            ..Default::default()
        };
        assert_eq!(quoted_kind_and_name(&photo), ("image", None));
        assert_eq!(quoted_text(&photo, true, "image", None), "Photo");
        assert_eq!(quoted_text(&photo, false, "image", None), "");

        let doc = wa::Message {
            document_message: MessageField::some(wa::message::DocumentMessage {
                file_name: Some("notes.pdf".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(quoted_kind_and_name(&doc), ("document", Some("notes.pdf".into())));
        assert_eq!(quoted_text(&doc, true, "document", Some("notes.pdf")), "notes.pdf");

        let text = wa::Message::text("see this");
        assert_eq!(quoted_kind_and_name(&text), ("", None));
        assert_eq!(quoted_text(&text, true, "", None), "see this");
    }
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
    let mut stored = stored_message(&inbound.message, header, client, media_dir, auto_download).await?;
    stored.history_shareable &= inbound.ephemeral_expiration.is_none() && inbound.comment_target.is_none();
    Some(stored)
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
    let decoded = decoded_message(outer);
    let message = decoded.message;
    if message.protocol_message.is_set() || message.reaction_message.is_set()
        || message.enc_reaction_message.is_set() || message.poll_update_message.is_set()
        || message.enc_event_response_message.is_set() || message.pin_in_chat_message.is_set()
        || message.keep_in_chat_message.is_set() || message.sender_key_distribution_message.is_set() {
        return None;
    }
    if let Some(system) = super::structured_notices::scheduled_call_notice(outer) {
        let text = if system.kind.as_deref() == Some("SCHEDULED_CALL_CREATED") {
            system.params.first().cloned().unwrap_or_default()
        } else { String::new() };
        return Some(StoredMessage { header, text, system, local: LocalState { read: true, ..Default::default() }, ..Default::default() });
    }
    let mut text = message.text_content().unwrap_or_default().to_string();
    if text.is_empty() && !decoded.view_once {
        text = message.get_caption().or_else(|| message.ptv_message.as_option().and_then(|video| video.caption.as_deref()))
            .unwrap_or_default().to_string();
    }

    let mut media_kind = None;
    let mut media_once_kind = None;
    let mut media_path = None;
    let mut media_thumb = None;
    let mut media_ref = None;
    let mut media_duration = None;
    let mut live_location = None;

    if let Some(media) = detect_media(message) {
        media_kind = Some(media.kind.to_string());
        media_duration = media.duration;

        // The thumbnail rides in the message, so it is kept even when the file
        // itself is not downloaded. A view-once thumbnail would show it unopened.
        media_thumb = media.thumb.as_deref().filter(|_| !decoded.view_once).map(thumb_uri);

        // A view-once has no CDN address, so fetching it here could only fail;
        // the locator is kept instead, so opening the message can ask the
        // sender's phone to upload it again.
        if auto_download && !decoded.view_once {
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
        if decoded.view_once && media_kind.is_some() {
            media_once_kind = media_kind.clone();
            media_kind = Some("view_once".to_string());
        }

    // Polls and events render as cards; the row carries their title.
    if media_kind.is_none() {
        if message.album_message.is_set() {
            text = if decoded.view_once { "View once message" } else { "[Album]" }.into();
            media_kind = Some(if decoded.view_once { "view_once" } else { "album" }.into());
            media_once_kind = decoded.view_once.then(|| "album".into());
        } else if let Some((question, _, _)) = poll_of(message) {
            text = question;
            media_kind = Some("poll".to_string());
        } else if let Some(event) = event_of(message) {
            text = event.name;
            media_kind = Some("event".to_string());
        } else if let Some(music) = message.music_message.as_option() {
            let track = music.embedded_music.as_option();
            let title = track.and_then(|m| m.title.clone()).unwrap_or_default();
            let author = track.and_then(|m| m.author.clone()).unwrap_or_default();
            text = if title.is_empty() { author } else if author.is_empty() { title } else { format!("{title} — {author}") };
            if text.is_empty() { text = "[Music]".into(); }
            media_kind = Some("music".to_string());
            if music.artwork_uri.as_deref().is_some_and(|uri| uri.starts_with("https://") || uri.starts_with("http://")) {
                media_ref = Some(media_locator(message));
            }
        }
    }

    if let Some(invite) = message.group_invite_message.as_option() {
        text = invite.caption.clone().filter(|text| !text.trim().is_empty())
            .or_else(|| invite.group_name.clone()).unwrap_or_else(|| "Group invitation".into());
        media_kind = Some("group_invite".into());
        media_thumb = invite.jpeg_thumbnail.as_deref().map(thumb_uri);
        let mut payload = invite.clone();
        payload.jpeg_thumbnail = None;
        payload.context_info = Default::default();
        media_ref = Some(buffa::Message::encode_to_vec(&wa::Message {
            group_invite_message: MessageField::some(payload), ..Default::default()
        }));
    }

    // Kinds without a dedicated view still arrive as readable cards.
    if text.is_empty() && media_kind.is_none() {
        if let Some(card) = card_of(message, header.timestamp) {
            text = card.text;
            media_kind = Some(card.kind.to_string());
            media_thumb = card.thumb.as_deref().map(thumb_uri);
            live_location = card.live;
            if card.kind == "contact" {
                if decoded.view_once {
                    text = "View once message".into();
                    media_kind = Some("view_once".into());
                    media_once_kind = Some("contact".into());
                } else {
                    media_ref = contact_sharing::contact_payload(message).observed().flatten();
                }
            }
        }
    }

    if text.is_empty() && media_kind.is_none() {
        if buffa::Message::encode_to_vec(message).is_empty() { return None; }
        text = "[Unsupported message]".into();
        media_kind = Some("unknown".into());
    }

    // A reply carries the quote in the message context. We do not keep the
    // original protobuf, so the text is copied out for display.
    let quote = stored_quote(message, &header, client).unwrap_or_default();

    // Names are resolved separately and joined by the store on read; a new
    // message stays unread until its chat is opened.
    let album = super::album_decode::metadata(outer, &header);
    Some(StoredMessage {
        spoiler: decoded.spoiler,
        history_shareable: header.chat.ends_with("@g.us") && group_history::is_shareable_text(outer),
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
        live_location,
        album,
        ..Default::default()
    })
}

/// A kind without a view of its own, as a card: a label, readable text, the
/// map thumbnail when one was sent, and live-location state when it is one.
struct Card {
    kind: &'static str,
    text: String,
    thumb: Option<Vec<u8>>,
    live: Option<LiveLocation>,
}

/// The live-location state a proto carries. `None` when it holds no position,
/// which is how a stopped share can arrive.
fn live_from_proto(
    at: &wa::message::LiveLocationMessage,
    timestamp: i64,
    expires_at: Option<i64>,
) -> Option<LiveLocation> {
    Some(LiveLocation {
        lat: at.degrees_latitude?,
        lng: at.degrees_longitude?,
        accuracy: at.accuracy_in_meters,
        speed: at.speed_in_mps,
        heading: at.degrees_clockwise_from_magnetic_north,
        sequence: at.sequence_number,
        started_at: timestamp,
        updated_at: timestamp,
        expires_at,
        ended: false,
    })
}

/// The share's own expiry, when the message carries one.
fn live_expiry(at: &wa::message::LiveLocationMessage, timestamp: i64) -> Option<i64> {
    at.context_info
        .as_option()
        .and_then(|c| c.expiration)
        .filter(|seconds| *seconds > 0)
        .map(|seconds| timestamp + i64::from(seconds))
}

/// Location and contact messages as a kind, readable text and the map
/// thumbnail when one was sent; `None` for anything else.
fn card_of(message: &wa::Message, timestamp: i64) -> Option<Card> {
    use whatsapp_rust::wacore::proto_helpers::MessageExt;
    let base = message.get_base_message();
    let lines = |parts: Vec<Option<String>>| {
        parts.into_iter().flatten().filter(|s| !s.trim().is_empty()).collect::<Vec<_>>().join("\n")
    };
    let map = |lat: Option<f64>, lng: Option<f64>| Some(format!("https://maps.google.com/?q={},{}", lat?, lng?));
    if let Some(at) = base.location_message.as_option() {
        let text = lines(vec![at.name.clone(), at.address.clone(), map(at.degrees_latitude, at.degrees_longitude)]);
        return Some(Card { kind: "location", text, thumb: at.jpeg_thumbnail.clone(), live: None });
    }
    if let Some(at) = base.live_location_message.as_option() {
        // The position lives in the structured state; the text is only the
        // sender's caption.
        let live = live_from_proto(at, timestamp, live_expiry(at, timestamp));
        return Some(Card { kind: "live_location", text: at.caption.clone().unwrap_or_default(), thumb: at.jpeg_thumbnail.clone(), live });
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
        return Some(Card { kind: "contact", text, thumb: None, live: None });
    }
    if let Some(list) = base.contacts_array_message.as_option() {
        let people = list
            .contacts
            .iter()
            .map(|c| lines(vec![c.display_name.clone(), Some(phones(&c.vcard))]).replace('\n', " · "))
            .collect::<Vec<_>>();
        return Some(Card { kind: "contact", text: lines(vec![list.display_name.clone(), Some(people.join("\n"))]), thumb: None, live: None });
    }
    None
}

/// A received thumbnail as a `data:` URI, stored in the row rather than as one
/// file per message. Sticker previews are PNG, everything else JPEG.
pub(super) fn thumb_uri(bytes: &[u8]) -> String {
    use base64::Engine as _;
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png" } else { "image/jpeg" };
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// The message cut down to what `download_media` needs: the media entry
/// without its thumbnail (kept in the row) or the context it quotes.
pub(super) fn media_locator(message: &wa::Message) -> Vec<u8> {
    let mut slim = wa::Message {
        image_message: message.image_message.clone(),
        video_message: message.video_message.clone(),
        ptv_message: message.ptv_message.clone(),
        audio_message: message.audio_message.clone(),
        document_message: message.document_message.clone(),
        sticker_message: message.sticker_message.clone(),
        music_message: message.music_message.clone(),
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
    if let Some(m) = slim.ptv_message.as_option_mut() {
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
    if let Some(m) = slim.music_message.as_option_mut() {
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
pub(super) fn message_context(message: &wa::Message) -> Option<&wa::ContextInfo> {
    context_of(decoded_message(message).message)
}

pub(super) struct DecodedMessage<'a> {
    pub message: &'a wa::Message,
    pub spoiler: bool,
    pub view_once: bool,
}

pub(super) fn decoded_message(mut message: &wa::Message) -> DecodedMessage<'_> {
    let mut spoiler = false;
    let mut view_once = false;
    loop {
        view_once |= message.is_view_once();
        let base = message.get_base_message();
        if !std::ptr::eq(base, message) { message = base; continue; }
        if let Some(inner) = message.associated_child_message.as_option().and_then(|child| child.message.as_option()) {
            message = inner; continue;
        }
        if let Some(wrapper) = message.spoiler_message.as_option() {
            spoiler = true;
            if let Some(inner) = wrapper.message.as_option() { message = inner; continue; }
        }
        spoiler |= context_of(message).is_some_and(|context| context.is_spoiler == Some(true));
        view_once |= message.ptv_message.as_option().is_some_and(|video| video.view_once == Some(true));
        return DecodedMessage { message, spoiler, view_once };
    }
}

fn context_of(base: &wa::Message) -> Option<&wa::ContextInfo> {
    [
        base.extended_text_message.as_option().and_then(|m| m.context_info.as_option()),
        base.image_message.as_option().and_then(|m| m.context_info.as_option()),
        base.video_message.as_option().and_then(|m| m.context_info.as_option()),
        base.ptv_message.as_option().and_then(|m| m.context_info.as_option()),
        base.music_message.as_option().and_then(|m| m.context_info.as_option()),
        base.audio_message.as_option().and_then(|m| m.context_info.as_option()),
        base.document_message.as_option().and_then(|m| m.context_info.as_option()),
        base.sticker_message.as_option().and_then(|m| m.context_info.as_option()),
        base.contact_message.as_option().and_then(|m| m.context_info.as_option()),
        base.contacts_array_message.as_option().and_then(|m| m.context_info.as_option()),
        base.location_message.as_option().and_then(|m| m.context_info.as_option()),
        base.live_location_message.as_option().and_then(|m| m.context_info.as_option()),
        base.poll_creation_message.as_option().and_then(|m| m.context_info.as_option()),
        base.poll_creation_message_v2.as_option().and_then(|m| m.context_info.as_option()),
        base.poll_creation_message_v3.as_option().and_then(|m| m.context_info.as_option()),
        base.event_message.as_option().and_then(|m| m.context_info.as_option()),
        base.album_message.as_option().and_then(|m| m.context_info.as_option()),
        base.group_invite_message.as_option().and_then(|m| m.context_info.as_option()),
    ].into_iter().flatten().next()
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

#[cfg(test)]
mod live_location_tests {
    use super::*;
    use buffa::MessageField;

    fn live_message(
        lat: Option<f64>,
        lng: Option<f64>,
        sequence: i64,
    ) -> wa::message::LiveLocationMessage {
        wa::message::LiveLocationMessage {
            degrees_latitude: lat,
            degrees_longitude: lng,
            accuracy_in_meters: Some(12),
            speed_in_mps: Some(3.5),
            degrees_clockwise_from_magnetic_north: Some(90),
            sequence_number: Some(sequence),
            time_offset: Some(60),
            jpeg_thumbnail: Some(vec![1, 2, 3]),
            ..Default::default()
        }
    }

    fn edit_of(target: &str, live: wa::message::LiveLocationMessage) -> wa::Message {
        wa::Message {
            protocol_message: MessageField::some(wa::message::ProtocolMessage {
                r#type: Some(wa::message::protocol_message::Type::MESSAGE_EDIT),
                key: MessageField::some(wa::MessageKey { id: Some(target.into()), ..Default::default() }),
                edited_message: MessageField::some(wa::Message {
                    live_location_message: MessageField::some(live),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_live_location_decodes_into_structured_state() {
        let message = wa::Message {
            live_location_message: MessageField::some(live_message(Some(1.5), Some(-2.5), 4)),
            ..Default::default()
        };
        let header = MessageHeader {
            chat: "a@s".into(),
            id: "1".into(),
            sender: "them@s".into(),
            timestamp: 1000,
            from_me: false,
        };
        let stored = stored_message(&message, header, None, None, false).await.unwrap();
        assert_eq!(stored.media.kind.as_deref(), Some("live_location"));
        assert_eq!(stored.text, "", "the caption alone is text; no maps URL");
        let live = stored.live_location.unwrap();
        assert_eq!((live.lat, live.lng), (1.5, -2.5));
        assert_eq!(live.accuracy, Some(12));
        assert_eq!(live.speed, Some(3.5));
        assert_eq!(live.heading, Some(90));
        assert_eq!(live.sequence, Some(4));
        assert_eq!(live.started_at, 1000);
        assert_eq!(live.updated_at, 1000);
        assert!(!live.ended);
        assert!(stored.media.thumb.is_some(), "the map snapshot is kept");
    }

    #[test]
    fn edits_carry_moves_and_stops() {
        let moved = edit_of("1", live_message(Some(2.5), Some(3.5), 5));
        let Some((target, LiveLocationUpdate::Moved { live, thumb })) =
            live_location_edit_of(&moved, 2000)
        else {
            panic!("expected a moved update");
        };
        assert_eq!(target, "1");
        assert_eq!((live.lat, live.lng, live.sequence), (2.5, 3.5, Some(5)));
        assert_eq!(live.updated_at, 2000);
        assert_eq!(thumb, Some(vec![1, 2, 3]));

        // No position in an edit means the share stopped.
        let stopped = edit_of("1", live_message(None, None, 6));
        assert!(matches!(
            live_location_edit_of(&stopped, 3000),
            Some((_, LiveLocationUpdate::Ended))
        ));

        // A normal text edit is not a live location update.
        let text_edit = wa::Message {
            protocol_message: MessageField::some(wa::message::ProtocolMessage {
                r#type: Some(wa::message::protocol_message::Type::MESSAGE_EDIT),
                key: MessageField::some(wa::MessageKey { id: Some("1".into()), ..Default::default() }),
                edited_message: MessageField::some(wa::Message::text("fixed")),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(live_location_edit_of(&text_edit, 3000).is_none());
    }
}

#[cfg(test)]
mod incoming_history_tests {
    use super::*;
    use whatsapp_rust::wacore::types::message::{MessageInfo, MessageSource};

    #[tokio::test]
    async fn live_history_selection_rejects_ephemeral_and_threaded_plaintext() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let mut now = 0_i64;
        for (id, expiration, threaded) in [
            ("plain", None, false), ("ephemeral", Some(86400), false),
            ("zero-expiration", Some(0), false), ("threaded", None, true),
            ("ephemeral-thread", Some(86400), true),
        ] {
            let info = MessageInfo { id: id.into(), source: MessageSource {
                chat: "1@g.us".parse().unwrap(), sender: "100@s.whatsapp.net".parse().unwrap(),
                is_group: true, ..Default::default()
            }, ..Default::default() };
            let inbound = InboundMessage::builder()
                .message(Arc::new(wa::Message::text(id))).info(Arc::new(info))
                .maybe_ephemeral_expiration(expiration)
                .maybe_comment_target(threaded.then(|| Box::new(wa::MessageKey {
                    remote_jid: Some("2@g.us".into()), id: Some("parent".into()), ..Default::default()
                }))).build();
            let stored = incoming_message("1@g.us", &inbound, None, None, false).await.unwrap();
            assert_eq!(stored.history_shareable, id == "plain", "{id}");
            now = now.max(stored.header.timestamp);
            store.insert_message(&stored).unwrap();
        }
        assert_eq!(store.count().unwrap(), 5, "privacy metadata must not hide messages from their original chat");
        let selected = store.group_history_text("1@g.us", now, 10, 100).unwrap();
        assert_eq!(selected.iter().map(|row| row.header.id.as_str()).collect::<Vec<_>>(), ["plain"]);
    }
}
