use super::*;

#[test]
fn group_settings_permissions_follow_fresh_membership_role_and_info_lock() {
    let changes = [GroupSettingChange::Subject { text: "Name".into() },
        GroupSettingChange::Description { text: Some("Description".into()), previous_id: None },
        GroupSettingChange::Announce { enabled: true }, GroupSettingChange::Locked { enabled: true },
        GroupSettingChange::Approval { enabled: true }];
    for member in [false, true] {
        for admin in [false, true] {
            for locked in [false, true] {
                let settings = GroupSettings { member, admin, locked, ..Default::default() };
                for (index, change) in changes.iter().enumerate() {
                    let allowed = member && if index < 2 { admin || !locked } else { admin };
                    assert_eq!(validate_change(&settings, change).is_ok(), allowed,
                        "member={member} admin={admin} locked={locked} change={change:?}");
                }
            }
        }
    }
}

#[test]
fn group_settings_description_requires_the_held_token_and_supports_explicit_removal() {
    let mut settings = GroupSettings { member: true, description_id: Some("fresh".into()), ..Default::default() };
    for previous_id in [None, Some("old".into())] {
        assert!(validate_change(&settings, &GroupSettingChange::Description { text: Some("New".into()), previous_id }).is_err());
    }
    assert!(validate_change(&settings, &GroupSettingChange::Description { text: None, previous_id: Some("fresh".into()) }).is_ok());
    assert!(description_value(None).unwrap().is_none());
    assert!(description_value(Some(String::new())).unwrap().is_none());
    settings.description_id = None;
    assert!(validate_change(&settings, &GroupSettingChange::Description { text: Some("First".into()), previous_id: None }).is_ok());
}

#[test]
fn group_settings_subject_and_description_use_scalar_limits_and_keep_text() {
    assert!(subject_value(" \n").is_err());
    let subject = "🎉".repeat(100);
    assert_eq!(subject_value(&subject).unwrap().as_str(), subject);
    assert!(subject_value(&"🎉".repeat(101)).is_err());
    let description = "🎉".repeat(2048);
    assert_eq!(description_value(Some(description.clone())).unwrap().unwrap().as_str(), description);
    assert!(description_value(Some("🎉".repeat(2049))).is_err());
}

#[test]
fn group_settings_role_matching_uses_phone_lid_aliases_without_device_suffixes() {
    let phone: Jid = "100:2@s.whatsapp.net".parse().unwrap();
    let lid: Jid = "200:3@lid".parse().unwrap();
    let other: Jid = "300@s.whatsapp.net".parse().unwrap();
    let listed_lid: Jid = "200@lid".parse().unwrap();
    let listed_phone: Jid = "100@s.whatsapp.net".parse().unwrap();
    let own = [phone, lid];
    assert!(matches_own(&own, &listed_lid, None, None));
    assert!(matches_own(&own, &other, Some(&listed_phone), None));
    assert!(matches_own(&own, &other, None, Some(&listed_lid)));
    assert!(!matches_own(&own, &other, None, None));
    assert!(!matches_own(&[], &listed_phone, None, None));
    assert!(settings_group("123@g.us").is_ok());
    assert!(settings_group("123@s.whatsapp.net").is_err());
}

#[test]
fn group_settings_picture_uses_a_square_jpeg_and_empty_bytes_mean_removal() {
    assert!(picture_value(&[]).unwrap().is_none());
    assert!(picture_value(b"not an image").is_err());
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(900, 700, image::Rgb([60, 120, 180])));
    let mut source = std::io::Cursor::new(Vec::new());
    image.write_to(&mut source, image::ImageFormat::Png).unwrap();
    let picture = picture_value(source.get_ref()).unwrap().unwrap();
    assert_eq!(image::guess_format(&picture).unwrap(), image::ImageFormat::Jpeg);
    let decoded = image::load_from_memory(&picture).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (640, 640));
}

#[test]
fn group_settings_changes_keep_wire_tags_and_reject_unknown_actions() {
    let change: GroupSettingChange = serde_json::from_value(serde_json::json!({
        "kind": "description", "text": null, "previous_id": "held-token"
    })).unwrap();
    assert_eq!(serde_json::to_value(change).unwrap(), serde_json::json!({
        "kind": "description", "text": null, "previous_id": "held-token"
    }));
    assert!(serde_json::from_value::<GroupSettingChange>(serde_json::json!({ "kind": "custom_role" })).is_err());
}
