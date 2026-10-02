use super::*;
use buffa::MessageField;

fn hex(hash: &[u8]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}
fn definition(correct_name: &str, correct_hash: &str) -> wa::message::PollCreationMessage {
    wa::message::PollCreationMessage {
        poll_type: Some(wa::message::PollType::QUIZ),
        selectable_options_count: Some(1),
        options: vec![
            wa::message::poll_creation_message::Option {
                option_name: Some("A".into()),
                ..Default::default()
            },
            wa::message::poll_creation_message::Option {
                option_name: Some("B".into()),
                ..Default::default()
            },
        ],
        correct_answer: MessageField::some(wa::message::poll_creation_message::Option {
            option_name: Some(correct_name.into()),
            option_hash: Some(correct_hash.into()),
        }),
        ..Default::default()
    }
}
fn cipher(id: &str, voter: &str, alt: Option<&str>, time: Option<i64>, me: bool) -> QuizCipher {
    QuizCipher {
        update_id: id.into(),
        voter: voter.into(),
        alt: alt.map(str::to_owned),
        from_me: me,
        source_time: time,
        payload: vec![],
        iv: vec![],
    }
}

#[test]
fn quiz_correct_answer_requires_matching_wire_name_hash_and_single_selection() {
    let hash = hex(&compute_option_hash("B"));
    let poll = definition("B", &hash);
    assert_eq!(
        correct_answer(&poll, &["A".into(), "B".into()]),
        (Some(compute_option_hash("B").to_vec()), true)
    );
    assert!(!correct_answer(&definition("A", &hash), &["A".into(), "B".into()]).1);
    assert!(!correct_answer(&definition("B", "bad"), &["A".into(), "B".into()]).1);
    let mut multi = poll;
    multi.selectable_options_count = Some(2);
    assert!(!correct_answer(&multi, &["A".into(), "B".into()]).1);
    assert!(validate_create("Question", &["A".into(), "B".into()], 1).is_ok());
    for (question, options, index) in [
        ("", vec!["A".into(), "B".into()], 0),
        ("Q", vec!["A".into(), "A".into()], 0),
        ("Q", vec!["A".into(), "B".into()], 2),
    ] {
        assert!(validate_create(question, &options, index).is_err());
    }
}

#[test]
fn aliases_collapse_to_latest_identity_and_unknown_time_never_replaces_dated_vote() {
    let rows = vec![
        cipher(
            "old",
            "15550000002@s.whatsapp.net",
            Some("777@lid"),
            Some(1000),
            false,
        ),
        cipher(
            "new",
            "777@lid",
            Some("15550000002@s.whatsapp.net"),
            Some(2000),
            false,
        ),
        cipher("sparse", "777@lid", None, None, false),
        cipher(
            "own-pn",
            "15550000001@s.whatsapp.net",
            None,
            Some(3000),
            true,
        ),
        cipher("own-lid", "999@lid", None, Some(4000), true),
    ];
    assert_eq!(
        latest_ciphers(
            &rows,
            &["15550000001@s.whatsapp.net".into(), "999@lid".into()]
        ),
        vec![1, 4]
    );
}

#[test]
fn own_lid_creator_is_from_me_with_phone_primary_identity() {
    let own_pn = Jid::pn("15550000001");
    let own_lid = Jid::lid("999");
    let group: Jid = "123@g.us".parse().unwrap();
    let key = quiz_vote_key(&group, "quiz", &own_lid, &own_pn, Some(&own_lid));
    assert_eq!(key.from_me, Some(true));
    assert_eq!(key.participant.as_deref(), Some("999@lid"));
    assert_eq!(
        quiz_vote_key(&group, "quiz", &Jid::lid("777"), &own_pn, Some(&own_lid)).from_me,
        Some(false)
    );
}

#[test]
fn shared_vote_boundary_accepts_public_polls_and_rejects_private_targets() {
    let mut public = StoredMessage::default();
    public.media.kind = Some("poll".into());
    assert!(super::super::polls::validate_vote_target(&public).is_ok());
    for kind in 0..7 {
        let mut row = public.clone();
        match kind {
            0 => row.local.deleted = true,
            1 => row.local.revoked = true,
            2 => row.spoiler = true,
            3 => row.media.kind = Some("view_once".into()),
            4 => row.media.once_kind = Some("poll".into()),
            5 => row.system.kind = Some("UNAVAILABLE_MESSAGE".into()),
            _ => row.system.kind = Some("OTHER_CONTROL".into()),
        }
        assert!(super::super::polls::validate_vote_target(&row).is_err());
    }
}

#[tokio::test]
async fn public_aggregate_handles_alias_revotes_withdrawal_and_invalid_latest() {
    use whatsapp_rust::{
        lid_pn_cache::LearningSource,
        prelude::{Bot, SqliteStore},
    };
    let (database, file) =
        super::super::media_files::TemporaryFile::download(&std::env::temp_dir()).unwrap();
    drop(file);
    let bot = Bot::builder()
        .with_backend(
            SqliteStore::new(database.path.to_str().unwrap())
                .await
                .unwrap(),
        )
        .build()
        .await
        .unwrap();
    let client = bot.client();
    client
        .add_lid_pn_mapping("111", "15550000002", LearningSource::Usync)
        .await
        .unwrap();
    client
        .add_lid_pn_mapping("777", "15550000003", LearningSource::Usync)
        .await
        .unwrap();
    let creator = Jid::lid("111");
    let pn = Jid::pn("15550000003");
    let lid = Jid::lid("777");
    let secret = [7; 32];
    let choices = vec!["A".into(), "B".into()];
    let encrypted = |name: Option<&str>, voter: &Jid| {
        let hashes = name
            .map(|n| vec![compute_option_hash(n).to_vec()])
            .unwrap_or_default();
        whatsapp_rust::wacore::poll::encrypt_poll_vote_with_secret(
            &hashes,
            &secret,
            "quiz",
            &Jid::pn("15550000002").to_string(),
            &voter.to_string(),
        )
        .unwrap()
    };
    let (a, a_iv) = encrypted(Some("A"), &pn);
    let (b, b_iv) = encrypted(Some("B"), &lid);
    let (withdraw, w_iv) = encrypted(None, &pn);
    let votes = vec![
        (
            &pn,
            PollVoteCiphertext {
                enc_payload: &a,
                enc_iv: &a_iv,
            },
        ),
        (
            &lid,
            PollVoteCiphertext {
                enc_payload: &b,
                enc_iv: &b_iv,
            },
        ),
    ];
    let tally = client
        .polls()
        .aggregate_votes(&choices, &votes, &secret, "quiz", &creator)
        .await
        .unwrap();
    assert!(tally[0].voters.is_empty());
    assert_eq!(tally[1].voters.len(), 1);
    let mut votes = votes;
    votes.push((
        &pn,
        PollVoteCiphertext {
            enc_payload: &withdraw,
            enc_iv: &w_iv,
        },
    ));
    assert!(client
        .polls()
        .aggregate_votes(&choices, &votes, &secret, "quiz", &creator)
        .await
        .unwrap()
        .iter()
        .all(|o| o.voters.is_empty()));
    let store = StoreWorker::new(MessageStore::open(Path::new(":memory:")).unwrap());
    let mut rows = vec![
        QuizCipher {
            update_id: "old".into(),
            voter: lid.to_string(),
            alt: None,
            from_me: false,
            source_time: Some(1000),
            payload: a,
            iv: a_iv.to_vec(),
        },
        QuizCipher {
            update_id: "bad".into(),
            voter: pn.to_string(),
            alt: None,
            from_me: false,
            source_time: Some(2000),
            payload: vec![0; 16],
            iv: vec![0; 12],
        },
    ];
    resolve_cipher_aliases(&client, &store, &mut rows)
        .await
        .unwrap();
    assert_eq!(latest_ciphers(&rows, &[]), vec![1]);
    let input = [(
        &pn,
        PollVoteCiphertext {
            enc_payload: &rows[1].payload,
            enc_iv: &rows[1].iv,
        },
    )];
    let tally = client
        .polls()
        .aggregate_votes(&choices, &input, &secret, "quiz", &creator)
        .await
        .unwrap();
    assert!(tally.iter().all(|o| o.voters.is_empty()));
    assert!(client
        .polls()
        .decrypt_vote(input[0].1, &secret, "quiz", &creator, &pn)
        .await
        .is_err());
}

#[tokio::test]
async fn postal_only_aliases_open_genuine_votes_and_partial_peer_results_keep_own_grade() {
    use whatsapp_rust::prelude::{Bot, SqliteStore};
    let (database, file) =
        super::super::media_files::TemporaryFile::download(&std::env::temp_dir()).unwrap();
    drop(file);
    let bot = Bot::builder()
        .with_backend(
            SqliteStore::new(database.path.to_str().unwrap())
                .await
                .unwrap(),
        )
        .build()
        .await
        .unwrap();
    let client = bot.client();
    let store = StoreWorker::new(MessageStore::open(Path::new(":memory:")).unwrap());
    store.set_lid_pn("111", "15550000002").await.unwrap();
    store.set_lid_pn("777", "15550000003").await.unwrap();
    let creator = Jid::lid("111");
    let voter = Jid::lid("777");
    assert!(client.get_lid_pn_entry(&creator).await.unwrap().is_none());
    assert!(client.get_lid_pn_entry(&voter).await.unwrap().is_none());
    let secret = [7; 32];
    let options = vec!["A".into(), "B".into()];
    let (payload, iv) = whatsapp_rust::wacore::poll::encrypt_poll_vote_with_secret(
        &[compute_option_hash("B").to_vec()],
        &secret,
        "quiz",
        &Jid::pn("15550000002").to_string(),
        &Jid::pn("15550000003").to_string(),
    )
    .unwrap();
    let mut rows = vec![QuizCipher {
        payload,
        iv: iv.to_vec(),
        ..cipher("vote", &voter.to_string(), None, Some(1000), true)
    }];
    assert!(client
        .polls()
        .decrypt_vote(
            PollVoteCiphertext {
                enc_payload: &rows[0].payload,
                enc_iv: &rows[0].iv
            },
            &secret,
            "quiz",
            &creator,
            &voter
        )
        .await
        .is_err());
    resolve_cipher_aliases(&client, &store, &mut rows)
        .await
        .unwrap();
    let creators = quiz_creator_forms(&client, &store, &creator).await;
    let (opened_voter, opened_creator) =
        open_quiz_cipher(&client, &rows[0], &creators, &secret, "quiz", &options)
            .await
            .unwrap();
    assert_eq!(opened_creator, Jid::pn("15550000002"));
    assert_eq!(opened_voter, Jid::pn("15550000003"));
    let valid = vec![(opened_voter, opened_creator, 0)];
    let tally = aggregate_quiz_votes(&client, &rows, &valid, &options, &secret, "quiz")
        .await
        .unwrap();
    assert_eq!(tally.len(), 1);
    assert_eq!(tally[0].0, "B");
    let def = QuizDefinition {
        creator: creator.to_string(),
        secret: Some(secret.to_vec()),
        name: "Q".into(),
        options: options.clone(),
        correct_hash: Some(compute_option_hash("B").to_vec()),
        answer_valid: true,
    };
    let poll = Poll {
        id: "quiz".into(),
        name: "Q".into(),
        options,
        multi: false,
        votes: vec![PollVote {
            voter: "@me".into(),
            options: vec!["B".into()],
        }],
        quiz: None,
    };
    let partial = quiz_feedback(&def, &poll, false, false, false);
    assert_eq!(partial.my_correct, Some(true));
    assert_eq!(partial.correct_option.as_deref(), Some("B"));
    assert!(!partial.results_complete);
    let creator_feedback = quiz_feedback(&def, &poll, false, true, true);
    assert_eq!(creator_feedback.correct_option.as_deref(), Some("B"));
    assert_eq!(creator_feedback.my_correct, None);
    assert_eq!(
        quiz_feedback(&def, &poll, false, true, false).my_correct,
        None
    );
    assert!(client.get_lid_pn_entry(&creator).await.unwrap().is_none());
}
