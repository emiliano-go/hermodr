use super::*;
use whatsapp_rust::{bytes::Bytes, wacore::types::events::DecryptedPayload};

const LIMIT: u64 = whatsapp_rust::wacore::history_sync::MAX_DECOMPRESSED;
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Default)]
pub(super) struct Capture(Arc<Mutex<Option<Active>>>);

struct Active {
    own: Vec<Jid>,
    frames: tokio::sync::mpsc::Sender<Bytes>,
    failed: Arc<AtomicBool>,
}

struct Window {
    capture: Capture,
    frames: tokio::sync::mpsc::Receiver<Bytes>,
    failed: Arc<AtomicBool>,
}

impl Drop for Window {
    fn drop(&mut self) {
        self.capture.0.lock().unwrap().take();
    }
}

impl Capture {
    fn begin(&self, own: Vec<Jid>) -> Window {
        let (frames, receiver) = tokio::sync::mpsc::channel(4);
        let failed = Arc::new(AtomicBool::new(false));
        *self.0.lock().unwrap() = Some(Active {
            own,
            frames,
            failed: failed.clone(),
        });
        Window {
            capture: self.clone(),
            frames: receiver,
            failed,
        }
    }

    pub(super) fn forward(&self, raw: &DecryptedPayload) {
        let active = self.0.lock().unwrap();
        let Some(active) = active.as_ref() else {
            return;
        };
        let source = &raw.info.source;
        if !source.is_from_me
            || source.sender.device != 0
            || !active.own.contains(&source.sender.to_non_ad())
        {
            return;
        }
        if raw.payload.len() as u64 > LIMIT * 2
            || active.frames.try_send(raw.payload.clone()).is_err()
        {
            active.failed.store(true, Ordering::Release);
        }
    }
}

pub(super) struct Proof {
    version: u64,
    hash: [u8; 128],
    pub(super) chats: Vec<String>,
}

fn inflate(blob: &[u8], compressed: bool) -> Result<Vec<u8>> {
    let maximum = if compressed {
        LIMIT + LIMIT / 1000 + 64
    } else {
        LIMIT
    };
    anyhow::ensure!(
        !blob.is_empty() && blob.len() as u64 <= maximum,
        "favorite recovery has no bounded collection"
    );
    if !compressed {
        return Ok(blob.to_vec());
    }
    let mut reader = whatsapp_rust::wacore_binary::zlib_pool::InflateReader::new(blob, LIMIT);
    let mut plain = Vec::new();
    while reader.ensure(1)? {
        let chunk = reader.available();
        let length = chunk.len();
        anyhow::ensure!(
            length > 0,
            "favorite recovery decompression made no progress"
        );
        plain.extend_from_slice(chunk);
        reader.consume(length);
    }
    let (read, whole) = reader.compressed_progress();
    anyhow::ensure!(
        reader.stream_ended() && read == whole,
        "favorite recovery compressed collection is incomplete"
    );
    Ok(plain)
}

fn collection(snapshot: wa::SyncdSnapshotRecovery) -> Result<Proof> {
    anyhow::ensure!(
        snapshot.collection_name.as_deref() == Some(WAPatchName::RegularHigh.as_str()),
        "favorite recovery collection does not match"
    );
    let version = snapshot
        .version
        .as_option()
        .and_then(|v| v.version)
        .ok_or_else(|| anyhow::anyhow!("favorite recovery has no version"))?;
    let hash = snapshot
        .collection_lthash
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("favorite recovery has no hash"))?;
    let hash: [u8; 128] = hash
        .try_into()
        .map_err(|_| anyhow::anyhow!("favorite recovery hash is not 128 bytes"))?;
    let mut indices = std::collections::HashSet::new();
    let mut chats = Vec::new();
    for record in snapshot.mutation_records {
        anyhow::ensure!(
            record.key_id.as_ref().is_some_and(|key| !key.is_empty())
                && record.mac.as_ref().is_some_and(|mac| mac.len() == 32),
            "favorite recovery has an incomplete record"
        );
        let data = record
            .value
            .as_option()
            .ok_or_else(|| anyhow::anyhow!("favorite recovery record has no data"))?;
        let index: Vec<String> = serde_json::from_slice(
            data.index
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("favorite recovery record has no index"))?,
        )?;
        anyhow::ensure!(
            !index.is_empty() && !index[0].is_empty() && indices.insert(index.clone()),
            "favorite recovery index is empty or duplicated"
        );
        let value = data
            .value
            .as_option()
            .ok_or_else(|| anyhow::anyhow!("favorite recovery record has no action"))?;
        let favorites = value.favorites_action.as_option();
        if index[0] != "favorites" {
            anyhow::ensure!(
                favorites.is_none(),
                "favorite recovery action has the wrong index"
            );
            continue;
        }
        anyhow::ensure!(
            index.len() == 1,
            "favorite recovery index is not a singleton"
        );
        let favorites = favorites
            .ok_or_else(|| anyhow::anyhow!("favorite recovery index has no favorites action"))?;
        let raw: Vec<String> = favorites
            .favorites
            .iter()
            .map(|favorite| {
                favorite.id.clone().ok_or_else(|| {
                    anyhow::anyhow!("favorite recovery contains an addressless favorite")
                })
            })
            .collect::<Result<_>>()?;
        chats = normalize(&raw)?;
        anyhow::ensure!(
            chats.len() == raw.len(),
            "favorite recovery contains duplicate chats"
        );
    }
    Ok(Proof {
        version,
        hash,
        chats,
    })
}

fn decode(frame: &Bytes, id: &str) -> Result<Option<Proof>> {
    anyhow::ensure!(!id.is_empty(), "favorite recovery request has no id");
    let message = whatsapp_rust::waproto::codec::message_decode(frame)?;
    let base = message.get_base_message();
    let Some(protocol) = base.protocol_message.as_option() else {
        return Ok(None);
    };
    let Some(response) = protocol
        .peer_data_operation_request_response_message
        .as_option()
    else {
        return Ok(None);
    };
    if response.stanza_id.as_deref() != Some(id) {
        return Ok(None);
    }
    anyhow::ensure!(
        response.peer_data_operation_request_type
            == Some(
                wa::message::PeerDataOperationRequestType::COMPANION_SYNCD_SNAPSHOT_FATAL_RECOVERY
            ),
        "favorite recovery response has the wrong operation"
    );
    let mut results = response
        .peer_data_operation_result
        .iter()
        .filter_map(|result| result.syncd_snapshot_fatal_recovery_response.as_option());
    let result = results
        .next()
        .ok_or_else(|| anyhow::anyhow!("favorite recovery response has no collection"))?;
    anyhow::ensure!(
        results.next().is_none(),
        "favorite recovery response contains multiple collections"
    );
    let blob = result
        .collection_snapshot
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("favorite recovery response has no collection"))?;
    let plain = inflate(blob, result.is_compressed.unwrap_or(false))?;
    Ok(Some(collection(
        whatsapp_rust::waproto::codec::syncd_snapshot_recovery_decode(&plain)?,
    )?))
}

fn matches(proof: &Proof, state: &HashState) -> bool {
    proof.version == state.version
        && proof.hash == state.hash
        && state.bootstrapped
        && !state.mac_mismatch_fatal
}

fn eligible(proof: &Proof, before: &HashState) -> bool {
    proof.version > 0 && (proof.version > before.version || matches(proof, before))
}

pub(super) async fn baseline(client: &Client) -> Result<HashState> {
    client
        .persistence_manager()
        .backend()
        .get_version(WAPatchName::RegularHigh.as_str())
        .await?
        .ok_or_else(|| anyhow::anyhow!("Favorite chats have no synchronized collection baseline."))
}

async fn confirmed(client: &Arc<Client>, proof: &Proof, before: &HashState) -> Result<HashState> {
    anyhow::ensure!(
        eligible(proof, before),
        "Favorite recovery has no safely completed collection baseline."
    );
    // Positive equal versions are rejected by the SDK before any recovery write.
    if proof.version > before.version {
        loop {
            let state = baseline(client).await?;
            anyhow::ensure!(
                state.version <= proof.version,
                "Favorite collection advanced during recovery; try again."
            );
            if matches(proof, &state) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    let report = client
        .resync_app_state([WAPatchName::RegularHigh], AppStateResyncMode::Incremental)
        .await?;
    anyhow::ensure!(
        requested_synced(&report),
        "Favorite recovery is still synchronizing; try again."
    );
    let state = baseline(client).await?;
    anyhow::ensure!(
        matches(proof, &state),
        "Favorite collection changed during recovery; try again."
    );
    Ok(state)
}

pub(super) async fn recover(
    client: &Arc<Client>,
    capture: &Capture,
    before: &HashState,
) -> Result<(Proof, HashState)> {
    let own: Vec<Jid> = [client.pn(), client.lid()]
        .into_iter()
        .flatten()
        .map(|jid| jid.to_non_ad())
        .collect();
    anyhow::ensure!(
        !own.is_empty(),
        "Favorite recovery requires a paired account."
    );
    let mut window = capture.begin(own);
    let _lease = client.acquire_decrypted_payload_forwarding();
    tokio::time::timeout(TIMEOUT, async {
        let id = client
            .request_syncd_snapshot_recovery(WAPatchName::RegularHigh.as_str())
            .await?;
        anyhow::ensure!(
            !id.is_empty(),
            "Favorite recovery is already pending; try again."
        );
        let proof = loop {
            let frame = window
                .frames
                .recv()
                .await
                .ok_or_else(|| anyhow::anyhow!("Favorite recovery capture ended."))?;
            let request = id.clone();
            if let Some(proof) =
                tokio::task::spawn_blocking(move || decode(&frame, &request)).await??
            {
                break proof;
            }
        };
        let state = confirmed(client, &proof, before).await?;
        anyhow::ensure!(
            !window.failed.load(Ordering::Acquire),
            "Favorite recovery exceeded its capture limit; try again."
        );
        Ok((proof, state))
    })
    .await
    .map_err(|_| {
        anyhow::anyhow!("Favorite recovery did not complete; existing favorites were kept.")
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use buffa::Message as _;
    use std::io::Write;
    use whatsapp_rust::wacore::types::message::MessageInfo;

    fn snapshot(records: Vec<wa::SyncdPlainTextRecord>) -> wa::SyncdSnapshotRecovery {
        wa::SyncdSnapshotRecovery {
            version: MessageField::some(wa::SyncdVersion {
                version: Some(7),
                ..Default::default()
            }),
            collection_name: Some("regular_high".into()),
            collection_lthash: Some(vec![9; 128]),
            mutation_records: records,
            ..Default::default()
        }
    }

    fn record(index: &[&str], chats: Option<&[&str]>) -> wa::SyncdPlainTextRecord {
        wa::SyncdPlainTextRecord {
            key_id: Some(vec![1]),
            mac: Some(vec![2; 32]),
            value: MessageField::some(wa::SyncActionData {
                index: Some(serde_json::to_vec(index).unwrap()),
                value: MessageField::some(chats.map_or_else(
                    wa::SyncActionValue::default,
                    |chats| {
                        action(
                            &chats.iter().map(|chat| (*chat).into()).collect::<Vec<_>>(),
                            1,
                        )
                    },
                )),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn frame(id: &str, blob: Vec<u8>, compressed: bool, duplicate: bool) -> Bytes {
        use wa::message::peer_data_operation_request_response_message::PeerDataOperationResult;
        use wa::message::peer_data_operation_request_response_message::peer_data_operation_result::SyncDSnapshotFatalRecoveryResponse;
        let result = PeerDataOperationResult {
            syncd_snapshot_fatal_recovery_response: MessageField::some(
                SyncDSnapshotFatalRecoveryResponse {
                    collection_snapshot: Some(blob),
                    is_compressed: Some(compressed),
                    ..Default::default()
                },
            ),
            ..Default::default()
        };
        wa::Message {
            protocol_message: MessageField::some(wa::message::ProtocolMessage {
                peer_data_operation_request_response_message: MessageField::some(wa::message::PeerDataOperationRequestResponseMessage {
                    stanza_id: Some(id.into()),
                    peer_data_operation_request_type: Some(wa::message::PeerDataOperationRequestType::COMPANION_SYNCD_SNAPSHOT_FATAL_RECOVERY),
                    peer_data_operation_result: if duplicate { vec![result.clone(), result] } else { vec![result] },
                    ..Default::default()
                }), ..Default::default()
            }), ..Default::default()
        }.encode_to_vec().into()
    }

    #[test]
    fn favorite_recovery_proves_complete_indices_and_rejects_malformed_collections() {
        let absent = snapshot(vec![record(&["mute", "200@s.whatsapp.net"], None)]);
        assert!(collection(absent.clone()).unwrap().chats.is_empty());
        assert!(
            collection(snapshot(vec![record(&["favorites"], Some(&[]))]))
                .unwrap()
                .chats
                .is_empty()
        );
        let ordered = snapshot(vec![record(
            &["favorites"],
            Some(&["200@s.whatsapp.net", "100@g.us"]),
        )]);
        assert_eq!(
            collection(ordered.clone()).unwrap().chats,
            ["200@s.whatsapp.net", "100@g.us"]
        );
        let mut bad = absent.clone();
        bad.collection_name = Some("regular_low".into());
        assert!(collection(bad).is_err());
        let mut bad = absent.clone();
        bad.version.take();
        assert!(collection(bad).is_err());
        let mut bad = absent.clone();
        bad.collection_lthash = Some(vec![9; 127]);
        assert!(collection(bad).is_err());
        let mut bad = absent.clone();
        bad.mutation_records[0].mac = None;
        assert!(collection(bad).is_err());
        let mut bad = absent.clone();
        bad.mutation_records[0].value.as_option_mut().unwrap().index = Some(b"[".to_vec());
        assert!(collection(bad).is_err());
        let mut bad = absent.clone();
        bad.mutation_records[0]
            .value
            .as_option_mut()
            .unwrap()
            .value
            .take();
        assert!(collection(bad).is_err());
        for records in [
            vec![record(&[], None)],
            vec![record(&["favorites"], None)],
            vec![record(&["favorites", "extra"], Some(&[]))],
            vec![record(&["mute"], Some(&[]))],
            vec![record(&["favorites"], Some(&["@s.whatsapp.net"]))],
            vec![record(
                &["favorites"],
                Some(&["200@s.whatsapp.net", "200:1@s.whatsapp.net"]),
            )],
        ] {
            assert!(collection(snapshot(records)).is_err());
        }
        let mut bad = ordered.clone();
        bad.mutation_records.extend(ordered.mutation_records);
        assert!(collection(bad).is_err());
        let mut bad = snapshot(vec![record(&["favorites"], Some(&["200@s.whatsapp.net"]))]);
        bad.mutation_records[0]
            .value
            .as_option_mut()
            .unwrap()
            .value
            .as_option_mut()
            .unwrap()
            .favorites_action
            .as_option_mut()
            .unwrap()
            .favorites[0]
            .id = None;
        assert!(collection(bad).is_err());
        let bytes = absent.encode_to_vec();
        assert!(
            decode(&frame("request", bytes.clone(), false, false), "request")
                .unwrap()
                .is_some()
        );
        assert!(
            decode(&frame("other", bytes.clone(), false, false), "request")
                .unwrap()
                .is_none()
        );
        assert!(decode(&frame("request", bytes.clone(), false, true), "request").is_err());
        assert!(decode(&frame("request", Vec::new(), false, false), "request").is_err());
        assert!(decode(&frame("", bytes.clone(), false, false), "").is_err());
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&bytes).unwrap();
        let compressed = encoder.finish().unwrap();
        assert!(
            decode(
                &frame("request", compressed.clone(), true, false),
                "request"
            )
            .unwrap()
            .is_some()
        );
        let mut truncated = compressed.clone();
        truncated.pop();
        assert!(decode(&frame("request", truncated, true, false), "request").is_err());
        let mut trailing = compressed;
        trailing.push(0);
        assert!(decode(&frame("request", trailing, true, false), "request").is_err());
    }

    #[test]
    fn favorite_recovery_baseline_requires_positive_exact_completed_state() {
        let proof = collection(snapshot(Vec::new())).unwrap();
        let exact = HashState {
            version: 7,
            hash: [9; 128],
            bootstrapped: true,
            ..Default::default()
        };
        assert!(eligible(&proof, &exact) && matches(&proof, &exact));
        assert!(eligible(
            &proof,
            &HashState {
                version: 6,
                ..exact.clone()
            }
        ));
        assert!(!eligible(
            &proof,
            &HashState {
                version: 8,
                ..exact.clone()
            }
        ));
        for bad in [
            HashState {
                hash: [0; 128],
                ..exact.clone()
            },
            HashState {
                bootstrapped: false,
                ..exact.clone()
            },
            HashState {
                mac_mismatch_fatal: true,
                ..exact.clone()
            },
        ] {
            assert!(!eligible(&proof, &bad) && !matches(&proof, &bad));
        }
        let zero = Proof {
            version: 0,
            hash: [0; 128],
            chats: Vec::new(),
        };
        assert!(!eligible(
            &zero,
            &HashState {
                bootstrapped: true,
                ..Default::default()
            }
        ));
        assert!(!matches(
            &proof,
            &HashState {
                version: 8,
                ..exact
            }
        ));
    }

    #[tokio::test]
    async fn favorite_recovery_capture_is_primary_scoped_bounded_and_accepts_early_reply() {
        let own: Jid = "200@s.whatsapp.net".parse().unwrap();
        let capture = Capture::default();
        let mut window = capture.begin(vec![own.clone()]);
        let mut info = MessageInfo::default();
        info.source.is_from_me = true;
        info.source.sender = own;
        let raw = |info: MessageInfo| {
            DecryptedPayload::builder()
                .info(Arc::new(info))
                .enc_index(0)
                .enc_type("msg")
                .payload(frame(
                    "early",
                    snapshot(Vec::new()).encode_to_vec(),
                    false,
                    false,
                ))
                .build()
        };
        let mut wrong = info.clone();
        wrong.source.is_from_me = false;
        capture.forward(&raw(wrong));
        let mut wrong = info.clone();
        wrong.source.sender.device = 1;
        capture.forward(&raw(wrong));
        let mut wrong = info.clone();
        wrong.source.sender = "300@s.whatsapp.net".parse().unwrap();
        capture.forward(&raw(wrong));
        assert!(window.frames.try_recv().is_err());
        capture.forward(&raw(info.clone()));
        let early = window.frames.recv().await.unwrap();
        assert!(decode(&early, "early").unwrap().is_some());
        for _ in 0..5 {
            capture.forward(&raw(info.clone()));
        }
        assert!(window.failed.load(Ordering::Acquire));
        drop(window);
        capture.forward(&raw(info));
        assert!(capture.0.lock().unwrap().is_none());
    }
}
