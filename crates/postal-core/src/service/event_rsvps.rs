use super::*;
use crate::message_ref::MessageRef;
use crate::store::{event_rsvp_pending::PendingEventRsvp, event_rsvps::EventRsvpUpdate};
use buffa::Message as _;

pub(super) fn validate_public_target(row: &StoredMessage) -> Result<()> {
    anyhow::ensure!(
        row.media.kind.as_deref() == Some("event")
            && row.media.once_kind.is_none()
            && !row.local.deleted
            && !row.local.revoked
            && !row.spoiler
            && !row.is_unavailable()
            && row.system.kind.is_none(),
        MessageRef::new("error.event_unavailable")
    );
    Ok(())
}

pub(super) async fn event_for_action(
    store: &StoreWorker,
    chat: &str,
    id: &str,
) -> Result<crate::store::Event> {
    validate_public_target(&store.message(chat, id).await?)?;
    let marks = store.marks_for(chat, Some(&[id.to_owned()])).await?;
    marks
        .events
        .into_iter()
        .find(|event| event.id == id)
        .ok_or_else(|| anyhow::anyhow!(MessageRef::new("error.event_details")))
}

fn validate_event(event: &crate::store::NewEvent) -> Result<()> {
    anyhow::ensure!(!event.name.trim().is_empty(), MessageRef::new("error.event_name"));
    anyhow::ensure!(
        !event.invitation && event.invitation_id.is_none(),
        MessageRef::new("error.event_invitation_read_only")
    );
    if let Some((start, end)) = event.start.zip(event.end) {
        anyhow::ensure!(end >= start, MessageRef::new("error.event_time_order"));
    }
    Ok(())
}

pub(super) fn validate_create(event: &crate::store::NewEvent) -> Result<()> {
    validate_event(event)?;
    anyhow::ensure!(!event.canceled, MessageRef::new("error.event_create_canceled"));
    anyhow::ensure!(
        event.has_reminder.is_none() && event.reminder_offset_sec.is_none(),
        MessageRef::new("error.event_reminder_unsupported")
    );
    Ok(())
}

pub(super) fn validate_edit(
    current: &crate::store::Event,
    event: &crate::store::NewEvent,
) -> Result<()> {
    validate_event(event)?;
    anyhow::ensure!(
        !current.invitation && current.invitation_id.is_none(),
        MessageRef::new("error.event_invitation_read_only")
    );
    anyhow::ensure!(
        current.has_reminder == event.has_reminder
            && current.reminder_offset_sec == event.reminder_offset_sec,
        MessageRef::new("error.event_reminder_unsupported")
    );
    Ok(())
}

pub(super) fn preserve_omitted_metadata(
    current: &crate::store::Event,
    event: &mut crate::store::NewEvent,
) {
    event.extra_guests_allowed = event.extra_guests_allowed.or(current.extra_guests_allowed);
    event.is_scheduled_call = event.is_scheduled_call.or(current.is_scheduled_call);
    event.has_reminder = event.has_reminder.or(current.has_reminder);
    event.reminder_offset_sec = event.reminder_offset_sec.or(current.reminder_offset_sec);
}

pub(super) fn event_content(event: &crate::store::NewEvent) -> wa::Message {
    wa::Message {
        event_message: MessageField::some(wa::message::EventMessage {
            name: Some(event.name.clone()),
            description: event.description.clone(),
            start_time: event.start,
            end_time: event.end,
            join_link: event.link.clone(),
            is_canceled: Some(event.canceled),
            location: event
                .location
                .clone()
                .map(|name| wa::message::LocationMessage {
                    name: Some(name),
                    ..Default::default()
                })
                .into(),
            extra_guests_allowed: event.extra_guests_allowed,
            is_schedule_call: event.is_scheduled_call,
            has_reminder: event.has_reminder,
            reminder_offset_sec: event.reminder_offset_sec,
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub(super) fn validate_response(
    event: &crate::store::Event,
    response: &str,
    guests: Option<i32>,
) -> Result<wa::message::event_response_message::EventResponseType> {
    use wa::message::event_response_message::EventResponseType;
    anyhow::ensure!(
        event.can_respond && !event.canceled && !event.invitation && event.invitation_id.is_none(),
        MessageRef::new("error.event_reply_unavailable")
    );
    let answer = match response {
        "going" => EventResponseType::GOING,
        "maybe" => EventResponseType::MAYBE,
        "not_going" => EventResponseType::NOT_GOING,
        "" | "clear" => {
            anyhow::bail!(MessageRef::new("error.event_clear_unsupported"))
        }
        _ => anyhow::bail!(MessageRef::new("error.event_response")),
    };
    anyhow::ensure!(
        guests.is_none_or(|count| count >= 0),
        MessageRef::new("error.event_guests_negative")
    );
    if guests.is_some_and(|count| count > 0) {
        anyhow::ensure!(
            event.extra_guests_allowed == Some(true),
            MessageRef::new("error.event_guests_disallowed")
        );
    }
    Ok(answer)
}

#[derive(Debug, Clone)]
pub(crate) struct RsvpSnapshot {
    pub source_id: String,
    pub response: String,
    pub timestamp_ms: Option<i64>,
    pub extra_guest_count: Option<i32>,
}

fn response_snapshot(
    source_id: &str,
    response: wa::message::EventResponseMessage,
) -> Result<RsvpSnapshot> {
    use wa::message::event_response_message::EventResponseType;
    anyhow::ensure!(
        !source_id.is_empty() && source_id.len() <= 256,
        MessageRef::new("error.event_source_id")
    );
    anyhow::ensure!(
        response.extra_guest_count.is_none_or(|count| count >= 0),
        MessageRef::new("error.event_guests_invalid")
    );
    anyhow::ensure!(
        response.timestamp_ms.is_none_or(|time| time > 0),
        MessageRef::new("error.event_timestamp")
    );
    let state = match response.response {
        Some(EventResponseType::GOING) => "going",
        Some(EventResponseType::MAYBE) => "maybe",
        Some(EventResponseType::NOT_GOING) => "not_going",
        Some(EventResponseType::UNKNOWN) | None => "",
    };
    Ok(RsvpSnapshot {
        source_id: source_id.to_owned(),
        response: state.into(),
        timestamp_ms: response.timestamp_ms,
        extra_guest_count: response.extra_guest_count,
    })
}

pub(crate) async fn namespace_forms(
    store: &StoreWorker,
    client: Option<&Client>,
    jid: &Jid,
) -> Result<Vec<Jid>> {
    let bare = jid.to_non_ad();
    anyhow::ensure!(
        (bare.is_pn() || bare.is_lid()) && !bare.user.is_empty(),
        MessageRef::new("error.event_participant")
    );
    let pair = if let Some(client) = client {
        if let Some(entry) = client.get_lid_pn_entry(&bare).await? {
            let pair = (user_part(&entry.lid), user_part(&entry.phone_number));
            store.set_lid_pn(&pair.0, &pair.1).await?;
            Some(pair)
        } else {
            store.lid_pn(&bare.user).await?
        }
    } else {
        store.lid_pn(&bare.user).await?
    };
    let mut forms = vec![bare.clone()];
    if let Some((lid, pn)) = pair {
        let alternate = if bare.is_lid() {
            Jid::pn(pn)
        } else {
            Jid::lid(lid)
        };
        if !forms.contains(&alternate) {
            forms.push(alternate);
        }
    }
    Ok(forms)
}

#[cfg(test)]
pub(crate) fn decrypt_snapshot(
    source_id: &str,
    event_id: &str,
    secret: &[u8],
    creators: &[Jid],
    responders: &[Jid],
    cipher: &wa::message::EncEventResponseMessage,
) -> Result<RsvpSnapshot> {
    anyhow::ensure!(secret.len() == 32, MessageRef::new("error.event_key_length").with_param("expected_bytes", serde_json::Number::from(32)));
    anyhow::ensure!(
        !event_id.is_empty()
            && creators
                .iter()
                .chain(responders)
                .all(|jid| (jid.is_pn() || jid.is_lid()) && !jid.user.is_empty()),
        MessageRef::new("error.event_identity")
    );
    anyhow::ensure!(
        cipher
            .event_creation_message_key
            .as_option()
            .and_then(|key| key.id.as_deref())
            == Some(event_id),
        MessageRef::new("error.event_reference")
    );
    let payload = authenticated_payload(event_id, secret, creators, responders, cipher)
        .ok_or_else(|| anyhow::anyhow!(MessageRef::new("error.event_authentication")))?;
    response_snapshot(
        source_id,
        wa::message::EventResponseMessage::decode_from_slice(&payload)?,
    )
}

fn authenticated_payload(
    event_id: &str,
    secret: &[u8],
    creators: &[Jid],
    responders: &[Jid],
    cipher: &wa::message::EncEventResponseMessage,
) -> Option<Vec<u8>> {
    for creator in creators {
        for responder in responders {
            if let Ok(payload) =
                whatsapp_rust::wacore::event::decrypt_event_response_payload_with_secret(
                    cipher.enc_payload.as_deref().unwrap_or_default(),
                    cipher.enc_iv.as_deref().unwrap_or_default(),
                    secret,
                    event_id,
                    &creator.to_non_ad().to_string(),
                    &responder.to_non_ad().to_string(),
                )
            {
                return Some(payload);
            }
        }
    }
    None
}

pub(crate) async fn capture_event_response(
    store: &StoreWorker,
    client: Option<&Client>,
    chat: &str,
    source_id: &str,
    responder: &Jid,
    responder_alt: Option<&Jid>,
    from_me: bool,
    own: &[String],
    response: &wa::message::EncEventResponseMessage,
) -> Result<bool> {
    let Some(key) = response.event_creation_message_key.as_option() else {
        return Ok(false);
    };
    let Some(event_id) = key.id.clone() else {
        return Ok(false);
    };
    let record = PendingEventRsvp {
        event_id: event_id.clone(),
        source_id: source_id.to_owned(),
        responder: responder.to_non_ad().to_string(),
        responder_alt: responder_alt.map(|jid| jid.to_non_ad().to_string()),
        from_me,
        key_chat: key.remote_jid.clone(),
        creator_hint: key.participant.clone(),
        key_from_me: key.from_me,
        payload: response.enc_payload.clone().unwrap_or_default(),
        iv: response.enc_iv.clone().unwrap_or_default(),
        received_at: whatsapp_rust::wacore::time::now_millis(),
    };
    store.queue_event_rsvp(chat, record).await?;
    flush_pending(store, client, chat, Some(&event_id), own).await
}

pub(crate) async fn flush_pending(
    store: &StoreWorker,
    client: Option<&Client>,
    chat: &str,
    event: Option<&str>,
    own: &[String],
) -> Result<bool> {
    let mut changed = false;
    for record in store.pending_event_rsvps(chat, event).await? {
        let sender: Jid = record.responder.parse()?;
        let responders = namespace_forms(store, client, &sender).await?;
        let me = is_own_response(client, record.from_me, &responders, own);
        let who = if me {
            "@me".to_owned()
        } else {
            record.responder.clone()
        };
        let Some(context) = store
            .event_rsvp_context(chat, &record.event_id, &who)
            .await?
        else {
            continue;
        };
        if context.secret.secret.len() != 32 {
            continue;
        }
        let mut creators =
            namespace_forms(store, client, &context.secret.creator.parse::<Jid>()?).await?;
        if !approved_key(store, client, chat, &record, &mut creators, own).await? {
            continue;
        }
        let cipher = pending_cipher(&record);
        let Some(payload) = authenticated_payload(
            &record.event_id,
            &context.secret.secret,
            &creators,
            &responders,
            &cipher,
        ) else {
            continue;
        };
        if let Ok(update) = wa::message::EventResponseMessage::decode_from_slice(&payload)
            .map_err(anyhow::Error::from)
            .and_then(|response| response_snapshot(&record.source_id, response))
        {
            let update = EventRsvpUpdate {
                response: update.response,
                timestamp_ms: update.timestamp_ms,
                extra_guest_count: update.extra_guest_count,
                source_id: update.source_id,
            };
            changed |= store
                .apply_event_rsvp(chat, &record.event_id, &who, &update)
                .await?;
        }
        store
            .remove_pending_event_rsvp(chat, &record.event_id, &record.source_id)
            .await?;
    }
    Ok(changed)
}

async fn approved_key(
    store: &StoreWorker,
    client: Option<&Client>,
    chat: &str,
    record: &PendingEventRsvp,
    creators: &mut Vec<Jid>,
    own: &[String],
) -> Result<bool> {
    if let Some(key_chat) = &record.key_chat {
        let transport: Jid = chat.parse()?;
        let key: Jid = key_chat.parse()?;
        if transport.is_group() {
            if transport.to_non_ad() != key.to_non_ad() {
                return Ok(false);
            }
        } else {
            let key_forms = namespace_forms(store, client, &key).await?;
            let mut allowed = namespace_forms(store, client, &transport).await?;
            for address in own
                .iter()
                .filter_map(|address| address.parse::<Jid>().ok())
                .chain(
                    client
                        .into_iter()
                        .flat_map(|client| [client.pn(), client.lid()].into_iter().flatten()),
                )
            {
                for form in namespace_forms(store, client, &address).await? {
                    if !allowed.contains(&form) {
                        allowed.push(form);
                    }
                }
            }
            if !key_forms.iter().any(|form| allowed.contains(form)) {
                return Ok(false);
            }
        }
    }
    if let Some(hint) = &record.creator_hint {
        let hints = namespace_forms(store, client, &hint.parse::<Jid>()?).await?;
        if !hints.iter().any(|hint| creators.contains(hint)) {
            return Ok(false);
        }
        for hint in hints {
            if !creators.contains(&hint) {
                creators.push(hint);
            }
        }
    }
    Ok(true)
}

fn is_own_response(
    client: Option<&Client>,
    from_me: bool,
    responders: &[Jid],
    own: &[String],
) -> bool {
    from_me
        || responders.iter().any(|jid| {
            own.iter().any(|own| {
                own.parse::<Jid>()
                    .is_ok_and(|own| own.to_non_ad() == jid.to_non_ad())
            }) || client.is_some_and(|client| {
                [client.pn(), client.lid()]
                    .into_iter()
                    .flatten()
                    .any(|own| own.to_non_ad() == jid.to_non_ad())
            })
        })
}

fn pending_cipher(record: &PendingEventRsvp) -> wa::message::EncEventResponseMessage {
    wa::message::EncEventResponseMessage {
        event_creation_message_key: MessageField::some(wa::MessageKey {
            id: Some(record.event_id.clone()),
            remote_jid: record.key_chat.clone(),
            participant: record.creator_hint.clone(),
            from_me: record.key_from_me,
            ..Default::default()
        }),
        enc_payload: Some(record.payload.clone()),
        enc_iv: Some(record.iv.clone()),
    }
}

#[cfg(test)]
#[path = "event_rsvps_tests.rs"]
mod tests;
