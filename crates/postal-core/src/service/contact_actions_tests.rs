use super::*;

#[test]
fn contact_edit_validates_bare_addresses_and_exact_phone_mapping() {
    let pn = editable_contact("59891954564@s.whatsapp.net").unwrap();
    let lid = editable_contact("77@lid").unwrap();
    assert_eq!(mapped_contact_phone(&pn, None).unwrap(), pn);
    assert_eq!(
        mapped_contact_phone(&lid, Some(("77".into(), "59891954564".into()))).unwrap(),
        pn
    );
    for address in [
        "59891954564:3@s.whatsapp.net",
        "77:1@lid",
        "@lid",
        "name@s.whatsapp.net",
        "1@g.us",
        "status@broadcast",
        "1@newsletter",
        "59891954564",
    ] {
        assert!(editable_contact(address).is_err(), "{address}");
    }
    for mapping in [
        None,
        Some(("88".into(), "59891954564".into())),
        Some(("77".into(), "name".into())),
        Some(("77".into(), "59891954564:3".into())),
    ] {
        assert!(mapped_contact_phone(&lid, mapping).is_err());
    }
}

#[tokio::test]
async fn contact_edit_waits_for_acceptance_and_keeps_push_names_and_aliases_on_remove() {
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    let target = editable_contact("59891954564@s.whatsapp.net").unwrap();
    store.set_lid_pn("77", "59891954564").await.unwrap();
    store.set_push_name("77@lid", "Push").await.unwrap();
    let aliases = crate::aliases::AliasStore::open(Path::new(":memory:")).unwrap();
    aliases.add(&["77@lid".into()], "local_alias").unwrap();
    let sent = AtomicBool::new(false);
    let accepted = || async {
        sent.store(true, Ordering::Release);
        Ok(())
    };
    assert!(
        commit_contact_change(&store, &target, Some("Saved"), 10, || false, accepted())
            .await
            .is_err()
    );
    assert!(!sent.load(Ordering::Acquire));
    assert!(
        commit_contact_change(&store, &target, Some("Saved"), 10, || true, async {
            anyhow::bail!("provider rejected")
        })
        .await
        .is_err()
    );
    let before = store
        .run(|store| store.contact_identity("77@lid"))
        .await
        .unwrap();
    assert_eq!(before.contact_saved, None);
    assert_eq!(before.push_name.as_deref(), Some("Push"));
    assert!(
        commit_contact_change(&store, &target, Some("Saved"), 10, || true, accepted())
            .await
            .unwrap()
    );
    let saved = store
        .run(|store| store.contact_identity("77@lid"))
        .await
        .unwrap();
    assert_eq!(saved.saved_name.as_deref(), Some("Saved"));
    store.set_push_name("77@lid", "New push").await.unwrap();
    assert!(
        commit_contact_change(&store, &target, Some("Renamed"), 11, || true, async {
            Ok(())
        })
        .await
        .unwrap()
    );
    assert_eq!(
        store
            .run(|store| store.contact_identity("77@lid"))
            .await
            .unwrap()
            .saved_name
            .as_deref(),
        Some("Renamed")
    );
    assert!(
        commit_contact_change(&store, &target, None, 12, || true, async { Ok(()) })
            .await
            .unwrap()
    );
    for jid in ["77@lid", "59891954564@s.whatsapp.net"] {
        let key = jid.to_owned();
        let identity = store
            .run(move |store| store.contact_identity(&key))
            .await
            .unwrap();
        assert_eq!(identity.contact_saved, Some(false));
        assert!(identity.saved_name.is_none() && identity.legacy_name.is_none());
        assert_eq!(identity.push_name.as_deref(), Some("New push"));
    }
    assert_eq!(
        aliases.all().unwrap(),
        [("77@lid".into(), "local_alias".into())]
    );
    store
        .set_contact_state(&target.to_string(), Some("New remote name"), true, 20)
        .await
        .unwrap();
    assert!(
        !commit_contact_change(&store, &target, Some("Older edit"), 15, || true, async {
            Ok(())
        })
        .await
        .unwrap()
    );
    assert_eq!(
        store
            .run(|store| store.contact_identity("77@lid"))
            .await
            .unwrap()
            .saved_name
            .as_deref(),
        Some("New remote name")
    );
}
