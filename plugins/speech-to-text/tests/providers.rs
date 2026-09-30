use base64::{engine::general_purpose::STANDARD, Engine as _};
use postal_plugins::{TranscriptionConfig, TranscriptionRequest};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn request(dir: &std::path::Path, audio: &[u8]) -> TranscriptionRequest {
    let model = b"synthetic model";
    std::fs::write(dir.join("tiny.bin"), model).unwrap();
    let mut config = TranscriptionConfig::default();
    config.data_directory = Some(dir.to_owned());
    config.model = Some("tiny.bin".into());
    config.model_sha256 = Some(
        Sha256::digest(model)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    );
    config.whisper_executable = Some(env!("CARGO_BIN_EXE_postal-stt-fixture").into());
    config.decoder_executable = config.whisper_executable.clone();
    TranscriptionRequest {
        provider: "local-whisper".into(),
        chat: "synthetic@chat".into(),
        message_id: "synthetic".into(),
        mime: "audio/ogg".into(),
        duration_ms: 100,
        audio: STANDARD.encode(audio),
        config,
    }
}

#[tokio::test]
async fn local_engine_decodes_verifies_model_returns_language_and_cleans_audio() {
    let dir = tempfile::tempdir().unwrap();
    let r = postal_stt::transcribe(request(dir.path(), b"OggSsynthetic"))
        .await
        .unwrap();
    assert_eq!(
        (r.text.as_str(), r.language.as_deref()),
        ("synthetic transcript", Some("en"))
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    let mut bad = request(dir.path(), b"OggSsynthetic");
    bad.config.model_sha256 = Some("0".repeat(64));
    assert!(postal_stt::transcribe(bad)
        .await
        .unwrap_err()
        .to_string()
        .contains("checksum mismatch"));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn provider_privacy_validation_timeout_and_cancel() {
    let dir = tempfile::tempdir().unwrap();
    for provider in ["openai", "deepgram", "unknown"] {
        let mut r = request(dir.path(), b"OggSsynthetic");
        r.provider = provider.into();
        assert!(postal_stt::transcribe(r).await.is_err());
    }
    let mut r = request(dir.path(), b"OggSsynthetic");
    r.audio = "!".into();
    assert!(postal_stt::transcribe(r).await.is_err());
    let mut r = request(dir.path(), b"RIFFsynthetic");
    r.config.timeout_secs = Some(1);
    assert!(postal_stt::transcribe(r).await.is_err());
    let mut r = request(dir.path(), b"OggSslow");
    r.config.timeout_secs = Some(1);
    assert!(postal_stt::transcribe(r)
        .await
        .unwrap_err()
        .to_string()
        .contains("timed out"));
    let r = request(dir.path(), b"OggSslow");
    let (cancel, rx) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(postal_stt::transcribe_with_cancel(r, rx));
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.send(true).unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

async fn server(status: &str, body: String) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/transcribe", listener.local_addr().unwrap());
    let status = status.to_owned();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let n = stream.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                let length = headers
                    .lines()
                    .find_map(|s| s.strip_prefix("content-length: "))
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                if bytes.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let response=format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
        stream.write_all(response.as_bytes()).await.unwrap();
        bytes
    });
    (url, task)
}

#[tokio::test]
async fn cloud_adapters_use_documented_wire_formats_against_loopback_only() {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    for (provider, body) in [
        (
            "openai",
            r#"{"text":"synthetic cloud","language":"english"}"#,
        ),
        (
            "deepgram",
            r#"{"results":{"channels":[{"detected_language":"en","alternatives":[{"transcript":"synthetic cloud"}]}]}}"#,
        ),
    ] {
        let (url, server) = server("200 OK", body.into()).await;
        let r = postal_stt::cloud(
            &client,
            &url,
            provider,
            "synthetic-key",
            b"OggSsynthetic".to_vec(),
            "audio/ogg",
            Some("en"),
        )
        .await
        .unwrap();
        assert_eq!(r.text, "synthetic cloud");
        let bytes = server.await.unwrap();
        let text = String::from_utf8_lossy(&bytes).to_lowercase();
        assert!(text.contains(if provider == "openai" {
            "authorization: bearer synthetic-key"
        } else {
            "authorization: token synthetic-key"
        }));
        if provider == "openai" {
            assert!(text.contains("multipart/form-data"));
            assert!(text.contains("whisper-1"));
            assert!(text.contains("verbose_json"));
        } else {
            assert!(text.contains("model=nova-3"));
            assert!(text.contains("content-type: audio/ogg"));
        }
        assert!(bytes
            .windows(b"OggSsynthetic".len())
            .any(|s| s == b"OggSsynthetic"));
    }
    let (url, server) = server("403 Forbidden", "synthetic-key MUST NOT BE ECHOED".into()).await;
    let error = postal_stt::cloud(
        &client,
        &url,
        "deepgram",
        "synthetic-key",
        b"OggS".to_vec(),
        "audio/ogg",
        None,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("403"));
    assert!(!error.contains("synthetic-key"));
    server.await.unwrap();
}

#[test]
fn empty_speech_is_valid_but_missing_provider_data_is_error() {
    assert!(postal_stt::parse_transcript("openai", serde_json::json!({})).is_err());
    assert_eq!(
        postal_stt::parse_transcript("openai", serde_json::json!({"text":""}))
            .unwrap()
            .text,
        ""
    );
    assert!(postal_stt::parse_transcript(
        "local-whisper",
        serde_json::json!({"transcription":[{}]})
    )
    .is_err());
    assert!(postal_stt::parse_transcript("deepgram", serde_json::json!({"results":{}})).is_err());
}
