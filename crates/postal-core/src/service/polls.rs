//! Polls and events: creating them, votes and RSVPs.

use super::*;

pub(super) fn validate_vote_target(row: &StoredMessage) -> Result<()> {
    anyhow::ensure!(
        row.media.kind.as_deref() == Some("poll") && row.media.once_kind.is_none()
            && !row.local.deleted && !row.local.revoked && !row.spoiler
            && !row.is_unavailable() && row.system.kind.is_none(),
        "Poll is unavailable or private."
    );
    Ok(())
}

/// A poll's question, its options, and whether more than one may be chosen.
pub(super) fn poll_of(message: &wa::Message) -> Option<(String, Vec<String>, bool)> {
    let base = message.get_base_message();
    let poll = base
        .poll_creation_message
        .as_option()
        .or_else(|| base.poll_creation_message_v2.as_option())
        .or_else(|| base.poll_creation_message_v3.as_option())?;
    let options = poll.options.iter().filter_map(|o| o.option_name.clone()).collect();
    Some((
        poll.name.clone().unwrap_or_default(),
        options,
        poll.selectable_options_count.unwrap_or(0) != 1,
    ))
}

pub(super) fn event_of(message: &wa::Message) -> Option<crate::store::NewEvent> {
    let base = message.get_base_message();
    match (base.event_message.as_option(), base.event_invite_message.as_option()) {
    (Some(event), None) => Some(crate::store::NewEvent {
        name: event.name.clone().unwrap_or_default(),
        description: event.description.clone(),
        start: event.start_time,
        end: event.end_time,
        location: event
            .location
            .as_option()
            .and_then(|l| l.name.clone().or_else(|| l.address.clone())),
        link: event.join_link.clone(),
        canceled: event.is_canceled.unwrap_or(false),
        extra_guests_allowed: event.extra_guests_allowed,
        is_scheduled_call: event.is_schedule_call,
        has_reminder: event.has_reminder,
        reminder_offset_sec: event.reminder_offset_sec,
        invitation_id: None,
        invitation: false,
    }),
    (None, Some(invite)) => Some(crate::store::NewEvent {
        name: invite.event_title.clone().unwrap_or_default(),
        description: invite.caption.clone(), start: invite.start_time, end: invite.end_time,
        link: invite.call_link.clone(), canceled: invite.is_canceled.unwrap_or(false),
        invitation_id: invite.event_id.clone(), invitation: true, ..Default::default()
    }),
    _ => None,
    }
}

/// The per-message secret polls and events key their votes and RSVPs with.
pub(super) fn message_secret(message: &wa::Message) -> Option<Vec<u8>> {
    let secret = |m: &wa::Message| {
        m.message_context_info
            .as_option()
            .and_then(|c| c.message_secret.clone())
    };
    secret(message).or_else(|| secret(message.get_base_message()))
}

/// Keeps a poll's or event's definition, which its later votes and RSVPs need.
pub(super) async fn remember_structures(store: &StoreWorker, chat: &str, id: &str, creator: &str, message: &wa::Message) {
    let secret = message_secret(message);
    if let Some((name, options, multi)) = poll_of(message) {
        store.save_poll(chat, id, creator, &name, &options, multi, secret.as_deref()).await.logged();
        secret_edits::remember_options(store, chat, id, message).await.logged();
    }
    if let Some(event) = event_of(message) {
        let event_secret = if message.get_base_message().event_invite_message.is_set() { None } else { secret.as_deref() };
        store.save_event(chat, id, creator, &event, event_secret).await.logged();
    }
}

impl WhatsAppService {
    pub async fn create_poll(&self, chat: &str, question: &str, options: Vec<String>, multi: bool) -> Result<()> {
        let to = broadcast_lists::writable_target(chat)?;
        let to_self = self.is_self_jid(&to);
        let selectable = if multi { options.len() as u32 } else { 1 };
        let (result, secret) = self
            .client
            .polls()
            .create(to, question, &options, selectable)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let id = result.message_id.clone();
        self.store
            .save_poll(chat, &id, &self.own_jid(), question, &options, multi, Some(&secret)).await?;
        let stored = self.own_message(chat, &id, question.to_string(), "poll", to_self);
        let stored = self.store.insert_message_row(&stored).await?;
        let _ = self.events.send(ServiceEvent::arrival(&stored));
        Ok(())
    }

    /// Casts or changes our vote; no options withdraws it.
    pub async fn vote_poll(&self, chat: &str, id: &str, options: Vec<String>) -> Result<()> {
        broadcast_lists::writable_target(chat)?;
        validate_vote_target(&self.store.message(chat, id).await?)?;
        if self.store.is_quiz(chat,id).await? { return self.vote_quiz(chat,id,options).await; }
        let def = self
            .store
            .poll_secret(chat, id).await?
            .ok_or_else(|| anyhow::anyhow!("this poll arrived without its key, so it cannot be voted on here"))?;
        secret_edits::vote(&self.store, &self.client, chat, id, &def, &options).await?;
        self.store.set_poll_vote(chat, id, "@me", &options).await?;
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    pub async fn create_event(&self, chat: &str, event: crate::store::NewEvent) -> Result<()> {
        use whatsapp_rust::EventCreationParams;
        event_rsvps::validate_create(&event)?;
        let to = broadcast_lists::writable_target(chat)?;
        let to_self = self.is_self_jid(&to);
        let params = EventCreationParams {
            name: event.name.clone(),
            description: event.description.clone(),
            start_time: event.start,
            end_time: event.end,
            join_link: event.link.clone(),
            location: event.location.clone().map(|name| wa::message::LocationMessage {
                name: Some(name),
                ..Default::default()
            }),
            extra_guests_allowed: event.extra_guests_allowed,
            is_scheduled_call: event.is_scheduled_call,
            ..Default::default()
        };
        let (result, secret) = self
            .client
            .events()
            .create(to, params)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let id = result.message_id.clone();
        let stored = self.own_message(chat, &id, event.name.clone(), "event", to_self);
        let stored = self.store.insert_message_row(&stored).await?;
        self.store.save_event(chat, &id, &self.own_jid(), &event, Some(&secret)).await?;
        let _ = self.events.send(ServiceEvent::arrival(&stored));
        Ok(())
    }

    /// Edits or cancels one of our events. The edit is encrypted with the
    /// event's secret, which is how WhatsApp sends event edits.
    pub async fn edit_event(&self, chat: &str, id: &str, mut event: crate::store::NewEvent) -> Result<()> {
        let to = broadcast_lists::writable_target(chat)?;
        let current = event_rsvps::event_for_action(&self.store, chat, id).await?;
        event_rsvps::preserve_omitted_metadata(&current,&mut event);
        event_rsvps::validate_edit(&current, &event)?;
        let context = self
            .store
            .event_rsvp_context(chat, id, "@me").await?
            .ok_or_else(|| anyhow::anyhow!("this event's key never reached this device"))?;
        anyhow::ensure!(!context.invitation && context.invitation_id.is_none(), "Event invitations are read-only.");
        anyhow::ensure!(event.has_reminder == context.event.has_reminder && event.reminder_offset_sec == context.event.reminder_offset_sec,
            "Event reminder changed while preparing the edit.");
        let def = context.secret;
        let own: Vec<String> = [self.client.pn(), self.client.lid()]
            .into_iter()
            .flatten()
            .map(|j| j.to_non_ad().to_string())
            .collect();
        let creators = event_rsvps::namespace_forms(&self.store, Some(&self.client), &def.creator.parse::<Jid>()?).await?;
        if !creators.iter().any(|creator|own.contains(&creator.to_string())) {
            anyhow::bail!("only the event's creator can change it");
        }
        let content = event_rsvps::event_content(&event);
        let timestamp_ms = unix_now() * 1000;
        let result = self.client
            .edit_message_encrypted(to, id, &def.secret, content)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let revision = crate::store::EditRevision { timestamp_ms, message_id: result.message_id };
        anyhow::ensure!(self.store.replace_event_content(chat, id, &event, &revision).await?, "event changed or was removed while sending");
        let updated = self.store.message(chat, id).await?;
        let _ = self.events.send(ServiceEvent::hint(&updated, false));
        let notice = self.store.message(chat, &revision.message_id).await?;
        let _ = self.events.send(ServiceEvent::hint(&notice, false));
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    /// Answers an event after validating its current public state.
    pub async fn respond_event(&self, chat: &str, id: &str, response: &str, extra_guest_count: Option<i32>) -> Result<()> {
        let jid = broadcast_lists::writable_target(chat)?;
        let current = event_rsvps::event_for_action(&self.store, chat, id).await?;
        let answer = event_rsvps::validate_response(&current, response, extra_guest_count)?;
        let context = self.store.event_rsvp_context(chat, id, "@me").await?
            .ok_or_else(|| anyhow::anyhow!("This event arrived without its key, so it cannot be answered here."))?;
        anyhow::ensure!(context.secret.secret.len()==32,"Event vote key must be 32 bytes.");
        anyhow::ensure!(!context.canceled && !context.invitation && context.invitation_id.is_none(),"This event cannot be answered on this device.");
        anyhow::ensure!(extra_guest_count.is_none_or(|count|count<=0) || context.extra_guests_allowed,"This event no longer permits extra guests.");
        let creator: Jid = context.secret.creator.parse()?;
        let request_started_ms=whatsapp_rust::wacore::time::now_millis();
        let sent = self.client
            .events()
            .respond(jid, id, &creator, &context.secret.secret, answer, extra_guest_count)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        if self.store.commit_event_rsvp_ack_with_bound(chat, id, "@me", &context.prior, response, extra_guest_count, &sent.message_id, Some(request_started_ms)).await? {
            let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        }
        Ok(())
    }
}
