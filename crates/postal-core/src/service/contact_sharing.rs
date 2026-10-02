use super::*;
use whatsapp_rust::wacore_binary::{Node, NodeRef};

const MAX_CONTACTS: usize = 50;
const MAX_CARD_BYTES: usize = 16 * 1024;
const MAX_BATCH_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ContactSendResult {
    pub message_id: String,
    pub warning: Option<String>,
}

impl WhatsAppService {
    pub async fn own_contact_link(
        &self,
        current: impl Fn() -> Result<()> + Send,
    ) -> Result<String> {
        current()?;
        let query = ContactQrQuery::Own;
        let response = self.client.send_iq_node(query.build(), None).await?;
        current()?;
        query.parse_response(response.get())
    }

    pub async fn resolve_contact_link(
        &self,
        link: &str,
        current: impl Fn() -> Result<()> + Send,
    ) -> Result<String> {
        anyhow::ensure!(link.len() <= 2048, "Contact link is too long.");
        if let Some(code) = link.trim().strip_prefix("https://wa.me/qr/") {
            validate_qr_code(code)?;
            let query = ContactQrQuery::Resolve(code.to_owned());
            current()?;
            let response = self.client.send_iq_node(query.build(), None).await?;
            current()?;
            return query.parse_response(response.get());
        }
        let target = contact_link_jid(link)?;
        current()?;
        let entries = self
            .client
            .contacts()
            .is_on_whatsapp(&[target.clone()])
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        current()?;
        let found = entries.iter().find(|entry| {
            entry.jid.to_non_ad() == target
                || entry
                    .pn_jid
                    .as_ref()
                    .is_some_and(|jid| jid.to_non_ad() == target)
        });
        anyhow::ensure!(
            found.is_some_and(|entry| entry.is_registered && entry.contact_error.is_none()),
            "This phone number could not be verified on WhatsApp."
        );
        Ok(target.to_string())
    }

    pub async fn send_contacts(
        &self,
        chat: &str,
        contacts: &[(String, String)],
        current: impl Fn() -> Result<()> + Send,
    ) -> Result<ContactSendResult> {
        let target = super::broadcast_lists::writable_target(chat)?;
        anyhow::ensure!(
            (target.is_group() || target.is_pn() || target.is_lid())
                && !target.user.is_empty()
                && target.device == 0
                && target.agent == 0
                && target.integrator == 0,
            "Invalid contact-sharing destination."
        );
        let message = contact_message(contacts)?;
        group_history::guard_ordinary_message(&message)?;
        let payload = contact_payload(&message)?;
        let text = contacts
            .iter()
            .map(|(name, phone)| {
                format!(
                    "{}\n+{}",
                    name.trim(),
                    phone.strip_prefix('+').unwrap_or(phone)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        current()?;
        let authored_at = unix_now();
        let sent = self.client.send_message(target.clone(), message).await?;
        anyhow::ensure!(
            !sent.message_id.is_empty(),
            "Contact send was not acknowledged."
        );
        let mut row = self.own_message(
            chat,
            &sent.message_id,
            text,
            "contact",
            self.is_self_jid(&target),
        );
        row.media.locator = payload;
        let (result, saved) = save_acknowledged_contact(&self.store, authored_at, row).await;
        if let Some(row) = saved {
            self.unarchive_on_send(chat).await;
            let _ = self.events.send(ServiceEvent::arrival(&row));
        }
        Ok(result)
    }

    pub async fn message_contacts(
        &self,
        chat: &str,
        id: &str,
        reveal_spoiler: bool,
    ) -> Result<Vec<(String, String)>> {
        let row = self.store.message(chat, id).await?;
        anyhow::ensure!(
            contacts_readable(&row, reveal_spoiler),
            "This contact message is no longer available."
        );
        let mut payload = row
            .media
            .locator
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("This contact message has no original vCard."))?;
        anyhow::ensure!(
            payload.len() <= MAX_BATCH_BYTES + 4096,
            "Contact payload is too large."
        );
        let decoded = <wa::Message as buffa::Message>::decode(&mut payload)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        contact_cards(&decoded)
    }
}

async fn save_acknowledged_contact(
    store: &StoreWorker,
    authored_at: i64,
    mut row: StoredMessage,
) -> (ContactSendResult, Option<StoredMessage>) {
    row.header.timestamp = authored_at;
    let message_id = row.header.id.clone();
    match store.insert_message_row(&row).await {
        Ok(row) => (
            ContactSendResult {
                message_id,
                warning: None,
            },
            Some(row),
        ),
        Err(error) => {
            let warning = format!("Contact sent, but local copy could not be saved: {error}");
            log::warn!("{warning}");
            (
                ContactSendResult {
                    message_id,
                    warning: Some(warning),
                },
                None,
            )
        }
    }
}

fn contacts_readable(row: &StoredMessage, reveal_spoiler: bool) -> bool {
    !row.is_unavailable()
        && !row.local.revoked
        && !row.local.deleted
        && row.media.kind.as_deref() == Some("contact")
        && row.media.once_kind.is_none()
        && (!row.spoiler || reveal_spoiler)
}

fn validate_qr_code(code: &str) -> Result<()> {
    anyhow::ensure!(
        !code.is_empty()
            && code.len() <= 256
            && code
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'),
        "Invalid contact QR token."
    );
    Ok(())
}

enum ContactQrQuery {
    Own,
    Resolve(String),
}

impl ContactQrQuery {
    fn build(&self) -> Node {
        let child = match self {
            Self::Own => NodeBuilder::new("qr")
                .attr("type", "contact")
                .attr("action", "get")
                .build(),
            Self::Resolve(code) => NodeBuilder::new("qr").attr("code", code.clone()).build(),
        };
        NodeBuilder::new("iq")
            .attr("xmlns", "w:qr")
            .attr(
                "type",
                if matches!(self, Self::Own) {
                    "set"
                } else {
                    "get"
                },
            )
            .children([child])
            .build()
    }

    fn parse_response(&self, response: &NodeRef<'_>) -> Result<String> {
        anyhow::ensure!(
            response
                .attrs()
                .optional_string("xmlns")
                .is_none_or(|namespace| namespace == "w:qr"),
            "Invalid contact QR response namespace."
        );
        anyhow::ensure!(
            response.tag == "iq"
                && response.attrs().optional_string("type").as_deref() == Some("result"),
            "Invalid contact QR response."
        );
        let qr = response
            .get_optional_child("qr")
            .ok_or_else(|| anyhow::anyhow!("Contact QR response has no QR result."))?;
        anyhow::ensure!(
            qr.get_optional_child("error").is_none(),
            "Contact QR lookup failed."
        );
        let mut attrs = qr.attrs();
        let result = match self {
            Self::Own => {
                let code = attrs
                    .optional_string("code")
                    .ok_or_else(|| anyhow::anyhow!("Contact QR response has no code."))?;
                let code = code
                    .strip_prefix("https://wa.me/qr/")
                    .or_else(|| code.strip_prefix("https://api.whatsapp.com/qr/"))
                    .unwrap_or(&code);
                validate_qr_code(code)?;
                format!("https://wa.me/qr/{code}")
            }
            Self::Resolve(_) => {
                let jid = attrs.optional_jid("jid").ok_or_else(|| {
                    anyhow::anyhow!("Contact QR response has no contact address.")
                })?;
                anyhow::ensure!(
                    (jid.is_lid() || (jid.is_pn() && contact_number(&jid.user).is_ok()))
                        && !jid.user.is_empty()
                        && jid.user.len() <= 32
                        && jid.user.bytes().all(|byte| byte.is_ascii_digit())
                        && jid.integrator == 0,
                    "Contact QR returned an invalid contact address."
                );
                jid.to_non_ad().to_string()
            }
        };
        attrs.finish()?;
        Ok(result)
    }
}

fn contact_number(value: &str) -> Result<&str> {
    let number = value.strip_prefix('+').unwrap_or(value);
    anyhow::ensure!(
        (7..=15).contains(&number.len())
            && !number.starts_with('0')
            && number.bytes().all(|byte| byte.is_ascii_digit()),
        "Use an international phone number with its country code."
    );
    Ok(number)
}

fn contact_link_jid(value: &str) -> Result<Jid> {
    anyhow::ensure!(value.len() <= 2048, "Contact link is too long.");
    let link = value.trim();
    let phone = link
        .strip_prefix("https://wa.me/")
        .ok_or_else(|| anyhow::anyhow!("Paste an https://wa.me phone link."))?;
    let phone = phone.strip_suffix('/').unwrap_or(phone);
    anyhow::ensure!(
        phone.bytes().all(|byte| byte.is_ascii_digit()),
        "Paste an https://wa.me phone link."
    );
    Ok(format!("{}@s.whatsapp.net", contact_number(phone)?).parse()?)
}

fn escape_card(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
}

fn fold_card(line: &str) -> String {
    let mut out = String::new();
    let mut bytes = 0;
    for character in line.chars() {
        if bytes + character.len_utf8() > 75 {
            out.push_str("\r\n ");
            bytes = 1;
        }
        out.push(character);
        bytes += character.len_utf8();
    }
    out
}

fn make_vcard(name: &str, phone: &str) -> Result<String> {
    let name = name.trim();
    anyhow::ensure!(
        !name.is_empty() && name.chars().count() <= 160 && !name.chars().any(char::is_control),
        "Contact name is empty, too long, or contains control characters."
    );
    let phone = contact_number(phone)?;
    let name = escape_card(name);
    Ok(format!("BEGIN:VCARD\r\nVERSION:3.0\r\n{}\r\n{}\r\nTEL;TYPE=CELL;waid={phone}:+{phone}\r\nEND:VCARD\r\n",
        fold_card(&format!("FN:{name}")), fold_card(&format!("N:;{name};;;"))))
}

fn contact_message(contacts: &[(String, String)]) -> Result<wa::Message> {
    anyhow::ensure!(
        !contacts.is_empty() && contacts.len() <= MAX_CONTACTS,
        "Choose between 1 and 50 contacts."
    );
    let mut seen = std::collections::HashSet::new();
    let mut cards = Vec::with_capacity(contacts.len());
    for (name, phone) in contacts {
        anyhow::ensure!(
            seen.insert(contact_number(phone)?),
            "The same phone number was selected twice."
        );
        cards.push(wa::message::ContactMessage {
            display_name: Some(name.trim().into()),
            vcard: Some(make_vcard(name, phone)?),
            ..Default::default()
        });
    }
    Ok(if cards.len() == 1 {
        wa::Message {
            contact_message: MessageField::some(cards.remove(0)),
            ..Default::default()
        }
    } else {
        wa::Message {
            contacts_array_message: MessageField::some(wa::message::ContactsArrayMessage {
                display_name: Some(format!("{} contacts", cards.len())),
                contacts: cards,
                ..Default::default()
            }),
            ..Default::default()
        }
    })
}

fn contact_cards(message: &wa::Message) -> Result<Vec<(String, String)>> {
    anyhow::ensure!(
        !(message.contact_message.as_option().is_some()
            && message.contacts_array_message.as_option().is_some()),
        "Mixed contact payload is not supported."
    );
    let cards: Vec<_> = if let Some(card) = message.contact_message.as_option() {
        vec![card]
    } else if let Some(array) = message.contacts_array_message.as_option() {
        array.contacts.iter().collect()
    } else {
        anyhow::bail!("This payload has no contacts.");
    };
    anyhow::ensure!(
        !cards.is_empty() && cards.len() <= MAX_CONTACTS,
        "Contact payload has too many contacts."
    );
    let mut total = 0;
    cards
        .into_iter()
        .map(|card| {
            let name = card.display_name.as_deref().unwrap_or_default();
            let vcard = card
                .vcard
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("The contact has no vCard."))?;
            total += name.len() + vcard.len();
            anyhow::ensure!(
                name.len() <= 1024 && vcard.len() <= MAX_CARD_BYTES && total <= MAX_BATCH_BYTES,
                "Contact payload is too large."
            );
            Ok((name.to_owned(), vcard.to_owned()))
        })
        .collect()
}

pub(super) fn contact_payload(message: &wa::Message) -> Result<Option<Vec<u8>>> {
    let decoded = super::message_decode::decoded_message(message);
    if decoded.view_once {
        return Ok(None);
    }
    let message = decoded.message;
    if message.contact_message.as_option().is_none()
        && message.contacts_array_message.as_option().is_none()
    {
        return Ok(None);
    }
    let cards = contact_cards(message)?
        .into_iter()
        .map(|(name, vcard)| wa::message::ContactMessage {
            display_name: Some(name),
            vcard: Some(vcard),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    let clean = if message.contact_message.as_option().is_some() {
        wa::Message {
            contact_message: MessageField::some(cards.into_iter().next().unwrap()),
            ..Default::default()
        }
    } else {
        wa::Message {
            contacts_array_message: MessageField::some(wa::message::ContactsArrayMessage {
                contacts: cards,
                ..Default::default()
            }),
            ..Default::default()
        }
    };
    Ok(Some(buffa::Message::encode_to_vec(&clean)))
}

#[cfg(test)]
#[path = "contact_sharing_tests.rs"]
mod tests;
