use super::*;

#[test]
fn space_metadata_decoder_rejects_unknown_fields_versions_and_trailing_data() {
    let valid = r#"{"version":1,"snapshot":{"spaces":[],"items":[]}}"#;
    assert!(decode_archive(valid).is_ok());
    for invalid in [valid.replace("\"version\":1", "\"version\":2"),
        valid.replace("\"spaces\":[]", "\"spaces\":[],\"messages\":[]"), format!("{valid} true"),
        r#"{"version":1,"snapshot":{"spaces":[],"items":[],"credentials":"x"}}"#.into()] {
        assert!(decode_archive(&invalid).is_err());
    }
}

#[test]
fn space_metadata_failures_keep_typed_reason_version_and_parser_context() {
    let error = decode_archive(r#"{"version":2,"snapshot":{"spaces":[],"items":[]}}"#).unwrap_err();
    let message = error.downcast_ref::<MessageRef>().unwrap();
    assert_eq!(message.code, "error.space_metadata_version");
    assert_eq!(serde_json::to_value(&message.params).unwrap(), serde_json::json!({ "version": 2, "supported": 1 }));
    let error = decode_archive("{").unwrap_err();
    assert_eq!(error.downcast_ref::<MessageRef>().unwrap().code, "error.space_metadata_json_invalid");
    assert!(error.chain().count() > 1);
}

#[test]
fn space_targets_and_actions_keep_wire_discriminants_and_nested_filter_keys() {
    let targets = [
        r#"{"kind":"chat","jid":"1@s.whatsapp.net"}"#,
        r#"{"kind":"group","jid":"1@g.us"}"#,
        r#"{"kind":"community","jid":"1@g.us"}"#,
        r#"{"kind":"channel","jid":"1@newsletter"}"#,
        r#"{"kind":"contact","jid":"1@lid"}"#,
        r#"{"kind":"favorite_contact","jid":"1@s.whatsapp.net"}"#,
        r#"{"kind":"label","label_id":"a"}"#,
        r#"{"kind":"saved_message","chat":"1@g.us","message_id":"m"}"#,
        r#"{"kind":"saved_search","query":"needle","chat":null}"#,
        r#"{"kind":"inbox_view","filters":{"unread":false,"mentions":false,"labelled":false,"muted":false,"archived":false,"label":"","query":""}}"#,
    ];
    for json in targets {
        let target: SpaceTarget = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_value(&target).unwrap(), serde_json::from_str::<serde_json::Value>(json).unwrap());
        let action = SpaceAction::AddItem { id: "item".into(), space_id: "space".into(), target };
        let value = serde_json::to_value(&action).unwrap();
        assert_eq!(value["kind"], "add_item");
        assert!(serde_json::from_value::<SpaceAction>(value).is_ok());
    }
    assert!(serde_json::from_str::<SpaceTarget>(r#"{"kind":"saved_message","chat":"1@g.us","message_id":"m","text":"copied"}"#).is_err());
}
