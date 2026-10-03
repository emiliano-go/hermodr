use super::*;

#[tokio::test]
async fn acknowledged_contact_keeps_authorship_and_id_when_local_save_fails() {
    let mut row = StoredMessage::default();
    row.header.chat = "12025550101@s.whatsapp.net".into();
    row.header.sender = "12025550102@s.whatsapp.net".into();
    row.header.from_me = true;
    row.header.id = "synthetic-ack".into();
    row.header.timestamp = 999;
    row.text = "Synthetic contact".into();
    row.media.kind = Some("contact".into());
    let readonly = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    readonly.query_only_for_test().await.unwrap();
    let (result, saved) = save_acknowledged_contact(&readonly, 42, row.clone()).await;
    assert_eq!(result.message_id, "synthetic-ack");
    assert!(saved.is_none());
    assert_eq!(result.warning_ref.as_ref().unwrap().code, "warning.contact_local_save");
    assert!(result.diagnostic.as_deref().unwrap().to_ascii_lowercase().contains("readonly"));
    let warning = result.warning.unwrap();
    assert!(warning.starts_with("Contact sent, but local copy could not be saved:"));
    assert!(warning.to_ascii_lowercase().contains("readonly"));
    let writable = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    let (result, saved) = save_acknowledged_contact(&writable, 42, row).await;
    assert_eq!(result.message_id, "synthetic-ack");
    assert!(result.warning.is_none());
    assert!(result.warning_ref.is_none() && result.diagnostic.is_none());
    assert_eq!(saved.unwrap().header.timestamp, 42);
    assert_eq!(
        writable
            .message("12025550101@s.whatsapp.net", "synthetic-ack")
            .await
            .unwrap()
            .header
            .timestamp,
        42
    );
}

#[test]
fn contact_link_rejects_non_contact_uris() {
    assert_eq!(
        contact_link_jid(" https://wa.me/59812345678/ ")
            .unwrap()
            .to_string(),
        "59812345678@s.whatsapp.net"
    );
    for link in [
        "http://wa.me/59812345678",
        "https://wa.me.evil/59812345678",
        "https://wa.me@evil/59812345678",
        "https://wa.me/qr/CODE",
        "https://wa.me/+59812345678",
        "https://wa.me/59812345678?text=x",
        "https://wa.me/59812345678#x",
        "https://wa.me/59812345678/../1234567",
        "https://wa.me/0000000",
        "https://wa.me/59812345678%0a",
    ] {
        assert!(contact_link_jid(link).is_err(), "{link}");
    }
}

#[test]
fn contact_proto_is_single_or_multiple_and_keeps_only_shared_fields() {
    let first = ("Name, Semi; Slash\\".into(), "59812345678".into());
    let single = contact_message(&[first.clone()]).unwrap();
    let card = single.contact_message.as_option().unwrap();
    assert!(card
        .vcard
        .as_deref()
        .unwrap()
        .contains("FN:Name\\, Semi\\; Slash\\\\\r\n"));
    assert!(card.context_info.as_option().is_none());
    assert!(single.contacts_array_message.as_option().is_none());
    let multiple = contact_message(&[first, ("Other".into(), "59812345679".into())]).unwrap();
    assert_eq!(
        multiple
            .contacts_array_message
            .as_option()
            .unwrap()
            .contacts
            .len(),
        2
    );
    assert!(multiple.contact_message.as_option().is_none());
    let bytes = contact_payload(&multiple).unwrap().unwrap();
    let decoded = <wa::Message as buffa::Message>::decode(&mut bytes.as_slice()).unwrap();
    assert_eq!(contact_cards(&decoded).unwrap().len(), 2);
    for text in ["EMAIL:", "ADR:", "PHOTO:", "NOTE:", "ORG:"] {
        assert!(!card.vcard.as_deref().unwrap().contains(text));
    }
    assert!(contact_message(&[]).is_err());
    assert!(contact_message(&[("Bad\r\nPHOTO:secret".into(), "59812345678".into())]).is_err());
    assert!(contact_message(&[("Bad".into(), "123;PHOTO:secret".into())]).is_err());
    assert!(contact_message(&[
        ("A".into(), "59812345678".into()),
        ("B".into(), "+59812345678".into())
    ])
    .is_err());
}

#[test]
fn card_folding_preserves_utf8_and_payload_bounds() {
    let name = "é界".repeat(60);
    let card = make_vcard(&name, "59812345678").unwrap();
    assert!(card.lines().all(|line| line.len() <= 75));
    assert!(card
        .replace("\r\n ", "")
        .contains(&format!("FN:{name}\r\n")));
    let over = wa::Message {
        contact_message: MessageField::some(wa::message::ContactMessage {
            vcard: Some("x".repeat(MAX_CARD_BYTES + 1)),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(contact_payload(&over).is_err());
}

#[test]
fn contact_qr_nodes_match_get_and_resolve_and_parse_server_identity() {
    let own = ContactQrQuery::Own;
    let node = own.build();
    let request = node.as_node_ref();
    assert_eq!(
        request.attrs().optional_string("xmlns").as_deref(),
        Some("w:qr")
    );
    assert_eq!(
        request.attrs().optional_string("type").as_deref(),
        Some("set")
    );
    assert!(request.attrs().optional_string("to").is_none());
    assert!(request.attrs().optional_string("id").is_none());
    assert_eq!(request.children().unwrap().len(), 1);
    let child = request.get_optional_child("qr").unwrap();
    assert_eq!(
        child.attrs().optional_string("type").as_deref(),
        Some("contact")
    );
    assert_eq!(
        child.attrs().optional_string("action").as_deref(),
        Some("get")
    );
    assert!(child.attrs().optional_string("revoke").is_none());
    assert!(child.children().is_none());
    for code in ["OPAQUE_77", "https://wa.me/qr/OPAQUE_77"] {
        let response = NodeBuilder::new("iq")
            .attr("type", "result")
            .children([NodeBuilder::new("qr").attr("code", code).build()])
            .build();
        assert_eq!(
            own.parse_response(&response.as_node_ref()).unwrap(),
            "https://wa.me/qr/OPAQUE_77"
        );
    }
    let resolve = ContactQrQuery::Resolve("SCAN_77".into());
    let node = resolve.build();
    let request = node.as_node_ref();
    assert_eq!(
        request.attrs().optional_string("xmlns").as_deref(),
        Some("w:qr")
    );
    assert!(request.attrs().optional_string("id").is_none());
    assert_eq!(request.children().unwrap().len(), 1);
    assert_eq!(
        request.attrs().optional_string("type").as_deref(),
        Some("get")
    );
    assert_eq!(
        request
            .get_optional_child("qr")
            .unwrap()
            .attrs()
            .optional_string("code")
            .as_deref(),
        Some("SCAN_77")
    );
    for address in ["12025550101@s.whatsapp.net", "9912345678901234@lid"] {
        let response = NodeBuilder::new("iq")
            .attr("type", "result")
            .children([NodeBuilder::new("qr")
                .attr("jid", address)
                .attr("notify", "Synthetic name")
                .attr("type", "contact")
                .build()])
            .build();
        assert_eq!(
            resolve.parse_response(&response.as_node_ref()).unwrap(),
            address
        );
    }
    for address in ["12025550101@g.us", "person@evil.invalid", "not-a-jid"] {
        let response = NodeBuilder::new("iq")
            .attr("type", "result")
            .children([NodeBuilder::new("qr").attr("jid", address).build()])
            .build();
        assert!(resolve.parse_response(&response.as_node_ref()).is_err());
    }
    let missing = NodeBuilder::new("iq").attr("type", "result").build();
    assert!(own.parse_response(&missing.as_node_ref()).is_err());
    for namespace in ["w:qr", "wrong:namespace"] {
        let response = NodeBuilder::new("iq")
            .attr("type", "result")
            .attr("xmlns", namespace)
            .children([NodeBuilder::new("qr").attr("code", "OPAQUE_77").build()])
            .build();
        assert_eq!(
            own.parse_response(&response.as_node_ref()).is_ok(),
            namespace == "w:qr"
        );
    }
    let malformed = NodeBuilder::new("iq")
        .attr("type", "error")
        .children([NodeBuilder::new("qr").attr("code", "OPAQUE_77").build()])
        .build();
    assert!(own.parse_response(&malformed.as_node_ref()).is_err());
    let no_address = NodeBuilder::new("iq")
        .attr("type", "result")
        .children([NodeBuilder::new("qr").build()])
        .build();
    assert!(resolve.parse_response(&no_address.as_node_ref()).is_err());
    let lookup_error = NodeBuilder::new("iq")
        .attr("type", "result")
        .children([NodeBuilder::new("qr")
            .attr("jid", "12025550101@s.whatsapp.net")
            .children([NodeBuilder::new("error").attr("code", "404").build()])
            .build()])
        .build();
    assert!(resolve.parse_response(&lookup_error.as_node_ref()).is_err());
    assert!(validate_qr_code(&"A".repeat(256)).is_ok());
    assert!(validate_qr_code(&"A".repeat(257)).is_err());
    let too_long = NodeBuilder::new("iq")
        .attr("type", "result")
        .children([NodeBuilder::new("qr").attr("code", "A".repeat(257)).build()])
        .build();
    assert!(own.parse_response(&too_long.as_node_ref()).is_err());
    for code in [
        "",
        "https://evil.invalid/token",
        "CODE?text=private",
        "CODE/extra",
    ] {
        assert!(validate_qr_code(code).is_err());
    }
}

#[test]
fn private_contacts_require_spoiler_reveal_and_never_export_view_once() {
    let mut row = StoredMessage {
        media: Media {
            kind: Some("contact".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(contacts_readable(&row, false));
    row.spoiler = true;
    assert!(!contacts_readable(&row, false));
    assert!(contacts_readable(&row, true));
    row.media.once_kind = Some("contact".into());
    assert!(!contacts_readable(&row, true));
    row.media.once_kind = None;
    row.local.revoked = true;
    assert!(!contacts_readable(&row, true));
    row.local.revoked = false;
    row.local.deleted = true;
    assert!(!contacts_readable(&row, true));
    let message = contact_message(&[("Synthetic".into(), "12025550101".into())]).unwrap();
    let wrapped = wa::Message {
        view_once_message: MessageField::some(wa::message::FutureProofMessage {
            message: MessageField::some(message),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(contact_payload(&wrapped).unwrap().is_none());
    let nested = wa::Message {
        ephemeral_message: MessageField::some(wa::message::FutureProofMessage {
            message: MessageField::some(wrapped),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(contact_payload(&nested).unwrap().is_none());
}
