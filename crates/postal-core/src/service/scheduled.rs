use super::*;
use anyhow::Context;
use crate::store::scheduled::{ScheduledMessage, ScheduledOutbox};

fn validate_schedule(chat: &str, text: &str, due_at: i64, now: i64) -> Result<()> {
    let _: Jid = chat.parse()?;
    anyhow::ensure!(!text.trim().is_empty(), "scheduled message cannot be empty");
    anyhow::ensure!(due_at > now, "choose a future time");
    Ok(())
}

fn scheduled_wire(message: &ScheduledMessage, subject: Option<String>) -> wa::Message {
    if message.mentions.is_empty() {
        return wa::Message { conversation: Some(message.text.clone()), ..Default::default() };
    }
    let mut context = wa::ContextInfo {
        mentioned_jid: message.mentions.iter().filter(|jid| *jid != "@all").cloned().collect(),
        ..Default::default()
    };
    if message.mentions.iter().any(|jid| jid == "@all") {
        context.group_mentions = vec![wa::GroupMention { group_jid: Some(message.chat.clone()), group_subject: subject }];
    }
    wa::Message { extended_text_message: MessageField::some(wa::message::ExtendedTextMessage {
        text: Some(message.text.clone()), context_info: MessageField::some(context), ..Default::default()
    }), ..Default::default() }
}

impl WhatsAppService {
    pub async fn schedule_message(&self, chat: &str, text: String, mentions: Vec<String>, due_at: i64) -> Result<String> {
        validate_schedule(chat, &text, due_at, unix_now())?;
        for jid in mentions.iter().filter(|jid| *jid != "@all") { let _: Jid = jid.parse()?; }
        let id = self.client.generate_message_id();
        let (saved_id, chat) = (id.clone(), chat.to_owned());
        self.scheduled.run(move |store| store.schedule_message(&saved_id, &chat, &text, &mentions, due_at)).await?;
        Ok(id)
    }

    pub async fn scheduled_messages(&self) -> Result<Vec<ScheduledMessage>> {
        self.scheduled.run(ScheduledOutbox::scheduled_messages).await
    }

    pub async fn update_scheduled_message(&self, id: String, text: String, due_at: i64) -> Result<()> {
        anyhow::ensure!(!text.trim().is_empty(), "scheduled message cannot be empty");
        anyhow::ensure!(due_at > unix_now(), "choose a future time");
        self.scheduled.run(move |store| store.update_scheduled_message(&id, &text, due_at)).await
    }

    pub async fn cancel_scheduled_message(&self, id: String) -> Result<()> {
        self.scheduled.run(move |store| store.cancel_scheduled_message(&id)).await
    }

    pub async fn retry_scheduled_message(&self, id: String) -> Result<()> {
        self.scheduled.run(move |store| store.retry_scheduled_message(&id)).await
    }

    pub async fn send_scheduled_message(&self, id: String) -> Result<bool> {
        anyhow::ensure!(self.is_connected() && self.shutdown.lock().unwrap().is_some(), "account is not connected");
        let now = unix_now();
        let Some(scheduled) = self.scheduled.run(move |store| store.claim_scheduled_message(&id, now)).await? else { return Ok(false); };
        let result = self.dispatch_scheduled_message(&scheduled).await;
        if let Err(error) = result {
            self.note_error(&error);
            let (id, failure) = (scheduled.id.clone(), error.to_string());
            self.scheduled.run(move |store| store.fail_scheduled_message(&id, &failure, true)).await?;
            return Err(error);
        }
        Ok(true)
    }

    async fn dispatch_scheduled_message(&self, scheduled: &ScheduledMessage) -> Result<()> {
        let to: Jid = scheduled.chat.parse()?;
        let subject = self.store.name_for(&scheduled.chat).await.observed().flatten();
        let message = scheduled_wire(scheduled, subject);
        super::group_history::guard_ordinary_message(&message)?;
        self.unarchive_on_send(&scheduled.chat).await;
        anyhow::ensure!(self.is_connected() && self.shutdown.lock().unwrap().is_some(), "account stopped before scheduled send");
        let result = self.client.send_message_with_options(to.clone(), message,
            whatsapp_rust::SendOptions::default().with_message_id(&scheduled.id)).await?;
        let mut stored = self.own_message(&scheduled.chat, &scheduled.id, scheduled.text.clone(), "", self.is_self_jid(&to));
        stored.history_shareable = super::group_history::is_shareable_text(&result.message);
        self.store.insert_message(&stored).await
            .context("sent; local confirmation failed, delivery may have succeeded")?;
        let _ = self.events.send(ServiceEvent::hint(&stored, true));
        let id = scheduled.id.clone();
        self.scheduled.run(move |store| store.complete_scheduled_message(&id)).await
            .context("sent; outbox confirmation failed, delivery may have succeeded")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduled_text_validates_time_and_preserves_mentions() {
        assert!(validate_schedule("1@s.whatsapp.net", "hello", 101, 100).is_ok());
        assert!(validate_schedule("1@s.whatsapp.net", "hello", 100, 100).is_err());
        assert!(validate_schedule("1@s.whatsapp.net", "   ", 101, 100).is_err());
        let scheduled = ScheduledMessage { id: "stable".into(), chat: "1@g.us".into(), text: "@2 hello".into(),
            mentions: vec!["2@lid".into(), "@all".into()], due_at: 101, status: "pending".into(), error: None, attempted: false };
        let wire = scheduled_wire(&scheduled, Some("Group".into()));
        let extended = wire.extended_text_message.as_option().unwrap();
        assert_eq!(extended.text.as_deref(), Some("@2 hello"));
        let context = extended.context_info.as_option().unwrap();
        assert_eq!(context.mentioned_jid, ["2@lid"]);
        assert_eq!(context.group_mentions[0].group_jid.as_deref(), Some("1@g.us"));
        assert_eq!(context.group_mentions[0].group_subject.as_deref(), Some("Group"));
    }
}
