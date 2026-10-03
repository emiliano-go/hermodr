use super::*;
use crate::message_ref::{MessageFailure, MessageRef};
use crate::store::{
    quiz_polls::{QuizCipher, QuizDefinition},
    ChatMarks, Poll, PollVote, QuizFeedback,
};
use std::collections::HashMap;
use whatsapp_rust::wacore::poll::{compute_option_hash, PollVoteCiphertext};

fn creation(message: &wa::Message) -> Option<&wa::message::PollCreationMessage> {
    let base = message.get_base_message();
    base.poll_creation_message
        .as_option()
        .or_else(|| base.poll_creation_message_v2.as_option())
        .or_else(|| base.poll_creation_message_v3.as_option())
}

pub(crate) async fn remember_quiz_definition(
    store: &StoreWorker,
    chat: &str,
    id: &str,
    creator: &str,
    message: &wa::Message,
) -> Result<bool> {
    let Some(poll) = creation(message) else {
        return Ok(false);
    };
    if poll.poll_type != Some(wa::message::PollType::QUIZ) {
        return Ok(false);
    }
    let name = poll.name.clone().unwrap_or_default();
    let options = poll
        .options
        .iter()
        .filter_map(|o| o.option_name.clone())
        .collect::<Vec<_>>();
    let (hash, valid) = correct_answer(poll, &options);
    let secret = super::polls::message_secret(message);
    let (chat, id, creator) = (chat.to_owned(), id.to_owned(), creator.to_owned());
    store
        .run(move |s| {
            s.save_quiz_definition(
                &chat,
                &id,
                &creator,
                &name,
                &options,
                hash.as_deref(),
                valid,
                secret.as_deref(),
            )
        })
        .await
}

fn correct_answer(
    poll: &wa::message::PollCreationMessage,
    options: &[String],
) -> (Option<Vec<u8>>, bool) {
    let Some(answer) = poll.correct_answer.as_option() else {
        return (None, false);
    };
    let Some(name) = answer.option_name.as_deref() else {
        return (None, false);
    };
    let Some(hex) = answer
        .option_hash
        .as_deref()
        .filter(|h| h.len() == 64 && h.is_ascii())
    else {
        return (None, false);
    };
    let hash = (0..32)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16))
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok();
    let valid = poll.selectable_options_count == Some(1)
        && (2..=12).contains(&options.len())
        && options.iter().all(|o| !o.is_empty())
        && options
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            == options.len()
        && options.iter().filter(|o| o.as_str() == name).count() == 1
        && hash.as_deref() == Some(compute_option_hash(name).as_slice());
    (hash, valid)
}

pub(crate) async fn capture_quiz_vote(
    store: &StoreWorker,
    chat: &str,
    update_id: &str,
    voter: &Jid,
    alt: Option<&Jid>,
    from_me: bool,
    update: &wa::message::PollUpdateMessage,
    source_timestamp_seconds: i64,
) -> Result<bool> {
    let Some(id) = update
        .poll_creation_message_key
        .as_option()
        .and_then(|k| k.id.as_deref())
    else {
        return Ok(false);
    };
    let Some(vote) = update.vote.as_option() else {
        return Ok(false);
    };
    let time = update.sender_timestamp_ms.filter(|t| *t > 0).or_else(|| {
        source_timestamp_seconds
            .checked_mul(1000)
            .filter(|t| *t > 0)
    });
    let payload = vote.enc_payload.as_deref().unwrap_or_default();
    let iv = vote.enc_iv.as_deref().unwrap_or_default();
    store
        .capture_quiz_cipher(
            chat,
            id,
            QuizCipher {
                update_id: update_id.into(),
                voter: voter.to_non_ad().to_string(),
                alt: alt.map(|a| a.to_non_ad().to_string()),
                from_me,
                source_time: time,
                payload: if payload.len() <= 4096 {
                    payload.to_vec()
                } else {
                    Vec::new()
                },
                iv: if iv.len() <= 64 {
                    iv.to_vec()
                } else {
                    Vec::new()
                },
            },
        )
        .await
}

impl WhatsAppService {
    pub async fn create_quiz(
        &self,
        chat: &str,
        question: &str,
        options: Vec<String>,
        correct_index: usize,
    ) -> Result<()> {
        let target = broadcast_lists::writable_target(chat)?;
        validate_create(question, &options, correct_index)?;
        anyhow::ensure!(
            self.is_connected() && (target.is_pn() || target.is_lid() || target.is_group()),
            MessageRef::new("error.quiz_destination")
        );
        let to_self = self.is_self_jid(&target);
        let (sent, secret) = self
            .client
            .polls()
            .create_quiz(target, question, &options, correct_index)
            .await?;
        self.store
            .save_poll(
                chat,
                &sent.message_id,
                &self.own_jid(),
                question,
                &options,
                false,
                Some(&secret),
            )
            .await?;
        let row = self.own_message(chat, &sent.message_id, question.into(), "poll", to_self);
        let row = self.store.insert_message_row(&row).await?;
        remember_quiz_definition(
            &self.store,
            chat,
            &sent.message_id,
            &self.own_jid(),
            &sent.message,
        )
        .await?;
        let _ = self.events.send(ServiceEvent::arrival(&row));
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.into() });
        Ok(())
    }

    pub(super) async fn vote_quiz(&self, chat: &str, id: &str, choices: Vec<String>) -> Result<()> {
        let target = broadcast_lists::writable_target(chat)?;
        anyhow::ensure!(
            choices.len() <= 1,
            MessageRef::new("error.quiz_answer_count")
        );
        let def = self
            .store
            .quiz_definition(chat, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!(MessageRef::new("error.quiz_private")))?;
        let secret = def
            .secret
            .as_deref()
            .filter(|s| s.len() == 32)
            .ok_or_else(|| anyhow::anyhow!(MessageRef::new("error.quiz_key_missing")))?;
        let current = self
            .store
            .poll_secret(chat, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!(MessageRef::new("error.quiz_unavailable")))?;
        validate_quiz_choices(&def, &current.options, &choices)?;
        let creator: Jid = def.creator.parse()?;
        let own = self
            .client
            .pn()
            .ok_or_else(|| anyhow::anyhow!(MessageRef::new("error.not_connected")))?
            .to_non_ad();
        let voter = if creator.is_lid() {
            self.client.lid().unwrap_or_else(|| own.clone()).to_non_ad()
        } else {
            own.clone()
        };
        let hashes = choices
            .iter()
            .map(|c| compute_option_hash(c).to_vec())
            .collect::<Vec<_>>();
        let (payload, iv) = whatsapp_rust::wacore::poll::encrypt_poll_vote_with_secret(
            &hashes,
            secret,
            id,
            &creator.to_string(),
            &voter.to_string(),
        )?;
        let update = wa::message::PollUpdateMessage {
            poll_creation_message_key: buffa::MessageField::some(quiz_vote_key(
                &target,
                id,
                &creator,
                &own,
                self.client.lid().as_ref(),
            )),
            vote: buffa::MessageField::some(wa::message::PollEncValue {
                enc_payload: Some(payload),
                enc_iv: Some(iv.to_vec()),
            }),
            sender_timestamp_ms: Some(whatsapp_rust::wacore::time::now_millis()),
            ..Default::default()
        };
        let message = wa::Message {
            poll_update_message: buffa::MessageField::some(update.clone()),
            ..Default::default()
        };
        let sent = self.client.send_message(target, message).await?;
        capture_quiz_vote(
            &self.store,
            chat,
            &sent.message_id,
            &voter,
            None,
            true,
            &update,
            0,
        )
        .await?;
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.into() });
        Ok(())
    }

    pub async fn enrich_quiz_marks(&self, chat: &str, marks: &mut ChatMarks) -> Result<()> {
        for poll in &mut marks.polls {
            if !self.store.is_quiz(chat, &poll.id).await? {
                continue;
            }
            let legacy = !poll.votes.is_empty();
            poll.votes.clear();
            let mut feedback = QuizFeedback {
                correct_option: None,
                my_correct: None,
                results_complete: false,
                error: None,
                error_ref: None,
                diagnostic: None,
                can_vote: false,
            };
            if let Some(def) = self.store.quiz_definition(chat, &poll.id).await? {
                match self.project_quiz(chat, poll, &def, legacy).await {
                    Ok(value) => feedback = value,
                    Err(error) => {
                        feedback.error = Some(error.to_string());
                        let failure = MessageFailure::from(error);
                        feedback.error_ref = Some(failure.message);
                        feedback.diagnostic = failure.diagnostic;
                    }
                }
            } else {
                feedback.error = Some("Quiz is unavailable or private.".into());
                feedback.error_ref = Some(MessageRef::new("error.quiz_private"));
            }
            if self.store.quiz_definition(chat, &poll.id).await?.is_none() {
                poll.votes.clear();
                feedback.correct_option = None;
                feedback.my_correct = None;
                feedback.can_vote = false;
            }
            poll.quiz = Some(feedback);
        }
        Ok(())
    }

    async fn project_quiz(
        &self,
        chat: &str,
        poll: &mut Poll,
        def: &QuizDefinition,
        legacy: bool,
    ) -> Result<QuizFeedback> {
        let secret = def
            .secret
            .as_deref()
            .filter(|s| s.len() == 32)
            .ok_or_else(|| anyhow::anyhow!(MessageRef::new("error.quiz_key_missing")))?;
        let creator: Jid = def.creator.parse()?;
        let creators = quiz_creator_forms(&self.client, &self.store, &creator).await;
        let snapshot = self.store.quiz_cipher_snapshot(chat, &poll.id).await?;
        let mut rows = snapshot.records;
        resolve_cipher_aliases(&self.client, &self.store, &mut rows).await?;
        let own = [self.client.pn(), self.client.lid()]
            .into_iter()
            .flatten()
            .map(|j| j.to_non_ad().to_string())
            .collect::<Vec<_>>();
        let latest = latest_ciphers(&rows, &own);
        let mut complete = !snapshot.suppressed
            && !(legacy && rows.is_empty())
            && rows.iter().all(|r| r.source_time.is_some());
        let mut own_invalid = false;
        let mut valid = Vec::new();
        for index in latest {
            let row = &rows[index];
            let me = row.from_me
                || own
                    .iter()
                    .any(|j| *j == row.voter || row.alt.as_ref() == Some(j));
            match open_quiz_cipher(&self.client, row, &creators, secret, &poll.id, &def.options)
                .await
            {
                Ok((voter, creator)) => valid.push((voter, creator, index)),
                Err(_) => {
                    complete = false;
                    own_invalid |= me;
                }
            }
        }
        let tally =
            aggregate_quiz_votes(&self.client, &rows, &valid, &def.options, secret, &poll.id)
                .await?;
        let mut votes: HashMap<String, Vec<String>> = HashMap::new();
        for (name, voter) in tally {
            let is_me = voter
                .parse::<Jid>()
                .ok()
                .is_some_and(|v| self.is_self_jid(&v))
                || valid
                    .iter()
                    .any(|(v, _, i)| v.to_string() == voter && rows[*i].from_me);
            votes
                .entry(if is_me { "@me".into() } else { voter })
                .or_default()
                .push(name);
        }
        poll.votes = votes
            .into_iter()
            .map(|(voter, options)| PollVote { voter, options })
            .collect();
        poll.votes.sort_by(|a, b| a.voter.cmp(&b.voter));
        Ok(quiz_feedback(
            def,
            poll,
            complete,
            own_invalid,
            creators.iter().any(|c| self.is_self_jid(c)),
        ))
    }
}

fn quiz_feedback(
    def: &QuizDefinition,
    poll: &Poll,
    complete: bool,
    own_invalid: bool,
    creator_is_me: bool,
) -> QuizFeedback {
    let unchanged = def.options == poll.options && def.name == poll.name;
    let correct = def
        .correct_hash
        .as_deref()
        .and_then(|h| {
            def.options
                .iter()
                .find(|o| compute_option_hash(o).as_slice() == h)
        })
        .cloned();
    let mine = poll
        .votes
        .iter()
        .find(|v| v.voter == "@me")
        .map(|v| v.options.as_slice())
        .unwrap_or_default();
    let valid_answer = def.answer_valid && unchanged && correct.is_some();
    let visible = valid_answer && (creator_is_me || (!own_invalid && mine.len() == 1));
    QuizFeedback {
        correct_option: visible.then(|| correct.clone()).flatten(),
        my_correct: (valid_answer && !own_invalid && mine.len() == 1)
            .then(|| Some(mine[0] == *correct.as_ref().unwrap()))
            .flatten(),
        results_complete: complete,
        can_vote: valid_answer,
        diagnostic: None,
        error_ref: if !unchanged {
            Some(MessageRef::new("error.quiz_answers_edited"))
        } else if !def.answer_valid {
            Some(MessageRef::new("error.quiz_correct_unavailable"))
        } else if !complete {
            Some(MessageRef::new("warning.quiz_incomplete"))
        } else {
            None
        },
        error: if !unchanged {
            Some("Edited quiz answers are unavailable.".into())
        } else if !def.answer_valid {
            Some("Quiz correct answer is unavailable.".into())
        } else if !complete {
            Some("Some quiz votes are unavailable; results are incomplete.".into())
        } else {
            None
        },
    }
}

async fn open_quiz_cipher(
    client: &Client,
    row: &QuizCipher,
    creators: &[Jid],
    secret: &[u8],
    id: &str,
    options: &[String],
) -> Result<(Jid, Jid)> {
    let mut voters = vec![row.voter.clone()];
    if let Some(alt) = &row.alt {
        voters.push(alt.clone())
    }
    if row.from_me {
        voters.extend(
            [client.pn(), client.lid()]
                .into_iter()
                .flatten()
                .map(|j| j.to_non_ad().to_string()),
        );
    }
    for creator in creators {
        for voter in voters.iter().filter_map(|v| v.parse::<Jid>().ok()) {
            let cipher = PollVoteCiphertext {
                enc_payload: &row.payload,
                enc_iv: &row.iv,
            };
            if let Ok(hashes) = client
                .polls()
                .decrypt_vote(cipher, secret, id, creator, &voter)
                .await
            {
                anyhow::ensure!(
                    hashes.len() <= 1
                        && hashes.iter().all(|h| options
                            .iter()
                            .any(|o| compute_option_hash(o).as_slice() == h)),
                    MessageRef::new("error.quiz_selection")
                );
                return Ok((voter, creator.clone()));
            }
        }
    }
    anyhow::bail!(MessageRef::new("error.quiz_decrypt"))
}

async fn quiz_creator_forms(client: &Client, store: &StoreWorker, creator: &Jid) -> Vec<Jid> {
    let mut forms = vec![creator.clone()];
    if let Some((lid, pn)) = super::contacts::other_form(client, store, creator).await {
        let twin = if creator.is_lid() {
            Jid::pn(pn)
        } else {
            Jid::lid(lid)
        };
        if !forms.contains(&twin) {
            forms.push(twin);
        }
    }
    forms
}

async fn aggregate_quiz_votes(
    client: &Client,
    rows: &[QuizCipher],
    valid: &[(Jid, Jid, usize)],
    options: &[String],
    secret: &[u8],
    id: &str,
) -> Result<Vec<(String, String)>> {
    let mut groups = HashMap::<Jid, Vec<(&Jid, PollVoteCiphertext<'_>)>>::new();
    for (voter, creator, index) in valid {
        let row = &rows[*index];
        groups.entry(creator.clone()).or_default().push((
            voter,
            PollVoteCiphertext {
                enc_payload: &row.payload,
                enc_iv: &row.iv,
            },
        ));
    }
    let mut votes = Vec::new();
    for (creator, inputs) in groups {
        for option in client
            .polls()
            .aggregate_votes(options, &inputs, secret, id, &creator)
            .await?
        {
            votes.extend(option.voters.into_iter().map(|v| (option.name.clone(), v)));
        }
    }
    Ok(votes)
}

fn quiz_vote_key(
    target: &Jid,
    id: &str,
    creator: &Jid,
    own_pn: &Jid,
    own_lid: Option<&Jid>,
) -> wa::MessageKey {
    wa::MessageKey {
        remote_jid: Some(target.to_string()),
        id: Some(id.into()),
        from_me: Some(creator.matches_user_or_lid(own_pn, own_lid)),
        participant: target.is_group().then(|| creator.to_string()),
        ..Default::default()
    }
}

async fn resolve_cipher_aliases(
    client: &Client,
    store: &StoreWorker,
    rows: &mut [QuizCipher],
) -> Result<()> {
    for row in rows {
        if row.alt.is_some() {
            continue;
        }
        let voter: Jid = row.voter.parse()?;
        if let Some((lid, pn)) = super::contacts::other_form(client, store, &voter).await {
            row.alt = Some(if voter.is_lid() {
                format!("{pn}@s.whatsapp.net")
            } else {
                format!("{lid}@lid")
            });
        }
    }
    Ok(())
}

fn latest_ciphers(rows: &[QuizCipher], own: &[String]) -> Vec<usize> {
    let mut aliases: HashMap<String, String> = HashMap::new();
    for row in rows {
        let members = [Some(row.voter.clone()), row.alt.clone()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let previous = members
            .iter()
            .filter_map(|m| aliases.get(m).cloned())
            .collect::<Vec<_>>();
        let me = row.from_me
            || members.iter().any(|m| own.contains(m))
            || previous.iter().any(|p| p == "@me");
        let key = if me {
            "@me".into()
        } else {
            members
                .iter()
                .chain(previous.iter())
                .find(|m| m.ends_with("@lid"))
                .cloned()
                .unwrap_or_else(|| row.voter.clone())
        };
        for value in aliases.values_mut() {
            if previous.contains(value) {
                *value = key.clone()
            }
        }
        for member in members {
            aliases.insert(member, key.clone());
        }
    }
    let mut latest: HashMap<String, usize> = HashMap::new();
    for (index, row) in rows.iter().enumerate() {
        let key = aliases
            .get(&row.voter)
            .cloned()
            .unwrap_or_else(|| row.voter.clone());
        if latest.get(&key).is_none_or(|old| {
            (row.source_time.unwrap_or(0), &row.update_id)
                > (rows[*old].source_time.unwrap_or(0), &rows[*old].update_id)
        }) {
            latest.insert(key, index);
        }
    }
    let mut result = latest.into_values().collect::<Vec<_>>();
    result.sort_by_key(|i| {
        (
            rows[*i].source_time.unwrap_or(0),
            rows[*i].update_id.clone(),
        )
    });
    result
}

fn validate_create(question: &str, options: &[String], correct: usize) -> Result<()> {
    anyhow::ensure!(
        !question.trim().is_empty()
            && question.len() <= 4096
            && (2..=12).contains(&options.len())
            && correct < options.len()
            && options
                .iter()
                .all(|o| !o.trim().is_empty() && o.len() <= 1024)
            && options
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                == options.len(),
        MessageRef::new("error.quiz_form")
    );
    Ok(())
}

fn validate_quiz_choices(
    def: &QuizDefinition,
    current_options: &[String],
    choices: &[String],
) -> Result<()> {
    anyhow::ensure!(
        def.answer_valid && current_options == def.options,
        MessageRef::new("error.quiz_edited")
    );
    anyhow::ensure!(
        choices.iter().all(|c| def.options.contains(c)),
        MessageRef::new("error.quiz_answer")
    );
    Ok(())
}

#[cfg(test)]
#[path = "quiz_polls_tests.rs"]
mod tests;
