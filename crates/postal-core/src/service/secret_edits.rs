use super::*;
use crate::store::{EditRevision, EventEdit, PollEdit, PollOption, SecretEdit};
use buffa::Message as _;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Weak, time::Instant};
use whatsapp_rust::wacore::types::{
    events::{EventHandler, EventInterest, EventKind, InboundMessage},
    message::MessageInfo,
};
use whatsapp_rust::{
    features::message_edit::{self, SecretEncKind},
    DecryptedPayloadLease,
};

const CAPACITY: usize = 1024;
const MAX_AGE: Duration = Duration::from_secs(300);

#[derive(Clone)]
enum Candidate {
    Plain,
    Envelope { kind: SecretEncKind, target: wa::MessageKey },
    Rejected,
}

struct Capture {
    info: Weak<MessageInfo>,
    digest: [u8; 32],
    candidate: Candidate,
    received: Instant,
}

#[derive(Default)]
struct Captures {
    entries: HashMap<usize, Capture>,
    forwarding: Option<DecryptedPayloadLease>,
    degraded: bool,
}

impl Captures {
    fn prune(&mut self, now: Instant) {
        // ponytail: scans at most 1024 captures; use an expiry queue if profiling requires it.
        self.entries.retain(|_, capture| {
            if capture.info.strong_count() == 0 {
                return false;
            }
            if now.duration_since(capture.received) >= MAX_AGE {
                self.degraded = true;
                return false;
            }
            true
        });
    }

    fn record(&mut self, info: &Arc<MessageInfo>, payload: &[u8], now: Instant) {
        self.prune(now);
        let key = Arc::as_ptr(info) as usize;
        let digest = Sha256::digest(payload).into();
        if let Some(capture) = self.entries.get_mut(&key) {
            if capture.digest != digest {
                capture.candidate = Candidate::Rejected;
            }
            return;
        }
        if self.entries.len() == CAPACITY {
            if let Some(oldest) = self.entries.iter().min_by_key(|(_, c)| c.received).map(|(key, _)| *key) {
                self.entries.remove(&oldest);
                self.degraded = true;
            }
        }
        let candidate = match wa::Message::decode_from_slice(payload) {
            Ok(message) => {
                let base = message.get_base_message();
                if message_edit::carries_secret_encrypted(base) {
                    match message_edit::extract_secret_encrypted(base) {
                        Some(envelope) if envelope.target_id().is_some_and(|id| !id.is_empty()) => Candidate::Envelope {
                            kind: envelope.kind,
                            target: envelope.target_message_key.clone(),
                        },
                        _ => Candidate::Rejected,
                    }
                } else {
                    Candidate::Plain
                }
            }
            Err(_) => Candidate::Rejected,
        };
        self.entries.insert(
            key,
            Capture {
                info: Arc::downgrade(info),
                digest,
                candidate,
                received: now,
            },
        );
    }

    fn candidate(&mut self, info: &Arc<MessageInfo>, now: Instant) -> Option<Candidate> {
        self.prune(now);
        // Raw payload and batch share this allocation; message IDs alone can collide.
        self.entries
            .get(&(Arc::as_ptr(info) as usize))
            .filter(|capture| capture.info.upgrade().is_some_and(|raw| Arc::ptr_eq(&raw, info)))
            .map(|capture| capture.candidate.clone())
    }
}

#[derive(Clone, Default)]
pub(super) struct SecretEdits(Arc<Mutex<Captures>>);

pub(super) enum Outcome {
    Continue,
    Applied { id: String },
    Drop,
}

impl SecretEdits {
    pub(super) fn activate(&self, client: &Arc<Client>) {
        self.0.lock().unwrap().forwarding = Some(client.acquire_decrypted_payload_forwarding());
    }

    pub(super) fn handler(&self) -> impl EventHandler {
        RawEdits(self.clone())
    }

    pub(super) async fn apply(&self, store: &StoreWorker, inbound: &InboundMessage, chat: &str, own: &[String]) -> Result<Outcome> {
        let (candidate, safe_missing) = {
            let mut captures = self.0.lock().unwrap();
            let candidate = captures.candidate(&inbound.info, Instant::now());
            (candidate, captures.forwarding.is_some() && !captures.degraded)
        };
        let base = inbound.message.get_base_message();
        if message_edit::carries_secret_encrypted(base) || matches!(candidate, Some(Candidate::Rejected)) {
            return Ok(Outcome::Drop);
        }
        if base.protocol_message.is_set() && (has_structure(base) || base.poll_add_option_message.is_set()) {
            return Ok(Outcome::Drop);
        }
        let mut author_is_editor = false;
        let edit = match candidate {
            Some(Candidate::Envelope { kind, target }) => match kind {
                SecretEncKind::PollEdit | SecretEncKind::EventEdit | SecretEncKind::PollAddOption => {
                    let Some(edit) = edit_body(base, kind)? else {
                        return Ok(Outcome::Drop);
                    };
                    Some((target, edit))
                }
                SecretEncKind::MessageEdit => {
                    author_is_editor = true;
                    let Some(protocol) = legacy_edit(base) else {
                        return Ok(Outcome::Drop);
                    };
                    if protocol.key.as_option().and_then(|key| key.id.as_deref()) != target.id.as_deref() {
                        return Ok(Outcome::Drop);
                    }
                    structured_edit(protocol.edited_message.as_option().unwrap())?.map(|edit| (target, edit))
                }
                SecretEncKind::EncReaction | SecretEncKind::EncComment => None,
            },
            _ => {
                if let Some(protocol) = legacy_edit(base) {
                    author_is_editor = true;
                    let edit = structured_edit(protocol.edited_message.as_option().unwrap())?;
                    if edit.is_some() && candidate.is_none() && !safe_missing {
                        return Ok(Outcome::Drop);
                    }
                    edit.map(|edit| (protocol.key.as_option().unwrap().clone(), edit))
                } else if let Some(add) = base.poll_add_option_message.as_option() {
                    if candidate.is_none() && !safe_missing {
                        return Ok(Outcome::Drop);
                    }
                    let target = add.poll_creation_message_key.as_option().cloned();
                    let edit = edit_body(base, SecretEncKind::PollAddOption)?;
                    match target.zip(edit) {
                        Some((target, edit)) => Some((target, edit)),
                        None => return Ok(Outcome::Drop),
                    }
                } else {
                    if has_structure(base)
                        && candidate.is_none()
                        && !safe_missing
                        && !inbound.info.source.chat.is_newsletter()
                        && inbound.info.unavailable_request_id.is_none()
                    {
                        return Ok(Outcome::Drop);
                    }
                    None
                }
            }
        };
        let Some((target, edit)) = edit else { return Ok(Outcome::Continue) };
        apply_selected(store, inbound, chat, own, &target, &edit, author_is_editor).await
    }
}

async fn apply_selected(
    store: &StoreWorker,
    inbound: &InboundMessage,
    chat: &str,
    own: &[String],
    target: &wa::MessageKey,
    edit: &SecretEdit,
    author_is_editor: bool,
) -> Result<Outcome> {
    let base = inbound.message.get_base_message();
    let Some(id) = target.id.as_deref().filter(|id| !id.is_empty()) else {
        return Ok(Outcome::Drop);
    };
    if !same_chat(store, chat, target.remote_jid.as_deref()).await? {
        return Ok(Outcome::Drop);
    }
    if let SecretEdit::AddOption(_) = edit {
        if let Some(add) = base.poll_add_option_message.as_option() {
            let Some(inner_target) = add.poll_creation_message_key.as_option() else {
                return Ok(Outcome::Drop);
            };
            if inner_target.id.as_deref() != Some(id) || !same_chat(store, chat, inner_target.remote_jid.as_deref()).await? {
                return Ok(Outcome::Drop);
            }
            if inner_target.participant.is_some() && inner_target.participant != target.participant {
                return Ok(Outcome::Drop);
            }
            if inner_target.from_me.is_some() && target.from_me.is_some() && inner_target.from_me != target.from_me {
                return Ok(Outcome::Drop);
            }
        }
    }
    let editor = if inbound.info.source.is_from_me {
        own.to_vec()
    } else {
        vec![inbound.info.source.sender.to_non_ad().to_string()]
    };
    let author = if author_is_editor {
        match target.participant.as_deref() {
            Some(participant) => vec![participant.parse::<Jid>()?.to_non_ad().to_string()],
            None => editor.clone(),
        }
    } else {
        target_author(&target, own)?
    };
    let revision = EditRevision {
        timestamp_ms: inbound.info.timestamp.timestamp_millis(),
        message_id: inbound.info.id.to_string(),
    };
    if store.apply_secret_edit(chat, id, &editor, &author, edit, &revision).await? {
        Ok(Outcome::Applied { id: id.to_owned() })
    } else {
        Ok(Outcome::Drop)
    }
}

struct RawEdits(SecretEdits);

impl EventHandler for RawEdits {
    fn handle_event(&self, event: Arc<Event>) {
        let Event::DecryptedPayload(raw) = event.as_ref() else { return };
        self.0 .0.lock().unwrap().record(&raw.info, &raw.payload, Instant::now());
    }
    fn interest(&self) -> EventInterest {
        EventInterest::of(&[EventKind::DecryptedPayload])
    }
}

async fn same_chat(store: &StoreWorker, chat: &str, remote: Option<&str>) -> Result<bool> {
    let Some(remote) = remote else { return Ok(true) };
    let remote: Jid = remote.parse()?;
    let remote = remote.to_non_ad().to_string();
    let canonical = store.run(move |store| Ok(store.canonical_chat(&remote)?.into_owned())).await?;
    Ok(canonical == chat)
}

fn target_author(target: &wa::MessageKey, own: &[String]) -> Result<Vec<String>> {
    if let Some(participant) = target.participant.as_deref() {
        let jid: Jid = participant.parse()?;
        let jid = jid.to_non_ad().to_string();
        return Ok(if own.contains(&jid) { own.to_vec() } else { vec![jid] });
    }
    if target.from_me == Some(true) {
        return Ok(own.to_vec());
    }
    let jid: Jid = target
        .remote_jid
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("secret edit has no target author"))?
        .parse()?;
    Ok(vec![jid.to_non_ad().to_string()])
}

fn legacy_edit(message: &wa::Message) -> Option<&wa::message::ProtocolMessage> {
    let protocol = message.protocol_message.as_option()?;
    (protocol.r#type == Some(wa::message::protocol_message::Type::MESSAGE_EDIT)
        && protocol.edited_message.is_set()
        && protocol.key.is_set())
    .then_some(protocol)
}

fn poll(message: &wa::Message) -> Option<&wa::message::PollCreationMessage> {
    message
        .poll_creation_message
        .as_option()
        .or_else(|| message.poll_creation_message_v2.as_option())
        .or_else(|| message.poll_creation_message_v3.as_option())
}

fn has_structure(message: &wa::Message) -> bool {
    poll(message).is_some() || message.event_message.is_set()
}

fn poll_option(option: &wa::message::poll_creation_message::Option) -> Result<PollOption> {
    let name = option
        .option_name
        .clone()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| anyhow::anyhow!("poll option has no name"))?;
    let hash = match option.option_hash.as_deref() {
        None => whatsapp_rust::wacore::poll::compute_option_hash(&name),
        Some(raw) => {
            anyhow::ensure!(raw.len() == 64 && raw.is_ascii(), "invalid poll option hash");
            let mut hash = [0; 32];
            for (index, digits) in raw.as_bytes().chunks_exact(2).enumerate() {
                let hi = (digits[0] as char)
                    .to_digit(16)
                    .ok_or_else(|| anyhow::anyhow!("invalid poll option hash"))?;
                let lo = (digits[1] as char)
                    .to_digit(16)
                    .ok_or_else(|| anyhow::anyhow!("invalid poll option hash"))?;
                hash[index] = (hi * 16 + lo) as u8;
            }
            hash
        }
    };
    Ok(PollOption { name, hash })
}

fn structured_edit(message: &wa::Message) -> Result<Option<SecretEdit>> {
    let message = message.get_base_message();
    if poll(message).is_some() {
        edit_body(message, SecretEncKind::PollEdit)
    } else if message.event_message.is_set() {
        edit_body(message, SecretEncKind::EventEdit)
    } else {
        Ok(None)
    }
}

fn edit_body(message: &wa::Message, kind: SecretEncKind) -> Result<Option<SecretEdit>> {
    let poll_count = [
        message.poll_creation_message.is_set(),
        message.poll_creation_message_v2.is_set(),
        message.poll_creation_message_v3.is_set(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    let event = message.event_message.as_option();
    let add = message.poll_add_option_message.as_option();
    if message_edit::carries_secret_encrypted(message) || message.protocol_message.is_set() {
        return Ok(None);
    }
    Ok(match kind {
        SecretEncKind::PollEdit if poll_count == 1 && event.is_none() && add.is_none() => {
            let poll = poll(message).unwrap();
            if poll.name.is_none() && poll.options.is_empty() && poll.selectable_options_count.is_none() {
                return Ok(None);
            }
            Some(SecretEdit::Poll(PollEdit {
                name: poll.name.clone(),
                options: poll.options.iter().map(poll_option).collect::<Result<_>>()?,
                selectable: poll.selectable_options_count,
            }))
        }
        SecretEncKind::EventEdit if poll_count == 0 && add.is_none() => event.map(|event| {
            SecretEdit::Event(EventEdit {
                name: event.name.clone(),
                description: event.description.clone(),
                start: event.start_time,
                end: event.end_time,
                location: event
                    .location
                    .as_option()
                    .and_then(|location| location.name.clone().or_else(|| location.address.clone())),
                link: event.join_link.clone(),
                canceled: event.is_canceled,
            })
        }),
        SecretEncKind::PollAddOption if poll_count == 0 && event.is_none() => add
            .and_then(|add| add.add_option.as_option())
            .map(poll_option)
            .transpose()?
            .map(SecretEdit::AddOption),
        _ => None,
    })
}

pub(super) async fn remember_options(store: &StoreWorker, chat: &str, id: &str, message: &wa::Message) -> Result<()> {
    let Some(poll) = poll(message.get_base_message()) else {
        return Ok(());
    };
    let options = poll.options.iter().map(poll_option).collect::<Result<Vec<_>>>()?;
    let allow = poll.allow_add_option.unwrap_or(false);
    store.remember_poll_options(chat, id, &options, allow).await
}

pub(super) async fn vote(
    store: &StoreWorker,
    client: &Client,
    chat: &str,
    id: &str,
    definition: &crate::store::Secretive,
    options: &[String],
) -> Result<()> {
    let to = super::broadcast_lists::writable_target(chat)?;
    let hashes = store.poll_option_hashes(chat, id, options).await?;
    let creator: Jid = definition.creator.parse()?;
    let own = client.pn().ok_or_else(|| anyhow::anyhow!("not logged in"))?.to_non_ad();
    let voter = if creator.is_lid() {
        client.lid().unwrap_or_else(|| own.clone()).to_non_ad()
    } else {
        own.clone()
    };
    let (payload, iv) = whatsapp_rust::wacore::poll::encrypt_poll_vote_with_secret(
        &hashes,
        &definition.secret,
        id,
        &creator.to_non_ad().to_string(),
        &voter.to_string(),
    )?;
    let message = wa::Message {
        poll_update_message: MessageField::some(wa::message::PollUpdateMessage {
            poll_creation_message_key: MessageField::some(wa::MessageKey {
                remote_jid: Some(to.to_string()),
                from_me: Some(own.is_same_user_as(&creator)),
                id: Some(id.to_owned()),
                participant: to.is_group().then(|| creator.to_string()),
                ..Default::default()
            }),
            vote: MessageField::some(wa::message::PollEncValue {
                enc_payload: Some(payload),
                enc_iv: Some(iv.to_vec()),
            }),
            sender_timestamp_ms: Some(whatsapp_rust::wacore::time::now_millis()),
            ..Default::default()
        }),
        ..Default::default()
    };
    client
        .send_message(to, message)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
#[path = "secret_edits_tests.rs"]
mod tests;
