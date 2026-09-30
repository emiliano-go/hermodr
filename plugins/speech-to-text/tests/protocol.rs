use base64::{engine::general_purpose::STANDARD, Engine as _};
use postal_plugins::{PluginHost, TranscriptionConfig, TranscriptionRequest};
use sha2::{Digest, Sha256};

#[tokio::test]
async fn native_sidecar_uses_v1_handshake_and_host_requests_then_unloads() {
    let generated = std::process::Command::new(env!("CARGO_BIN_EXE_postal-stt"))
        .arg("--manifest")
        .output()
        .unwrap();
    assert!(generated.status.success());
    let generated: serde_json::Value = serde_json::from_slice(&generated.stdout).unwrap();
    assert_eq!(
        generated["entrypoint"],
        if cfg!(windows) {
            "postal-stt.exe"
        } else {
            "postal-stt"
        }
    );
    let root = tempfile::tempdir().unwrap();
    let plugin = root.path().join("plugins/stt");
    std::fs::create_dir_all(&plugin).unwrap();
    let name = if cfg!(windows) {
        "postal-stt.exe"
    } else {
        "postal-stt"
    };
    std::fs::copy(env!("CARGO_BIN_EXE_postal-stt"), plugin.join(name)).unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_str(include_str!("../plugin.json")).unwrap();
    manifest["entrypoint"] = serde_json::json!(name);
    std::fs::write(plugin.join("plugin.json"), manifest.to_string()).unwrap();
    let host = PluginHost::discover(
        &root.path().join("plugins"),
        root.path().join("grants.json"),
    )
    .unwrap();
    let id = "org.postal.speech-to-text";
    host.set_enabled(id, true, vec!["transcribe".into()])
        .await
        .unwrap();
    assert_eq!(host.list()[0].state, "idle");
    let data = host.transcription_data_directory(id).unwrap();
    let bytes = b"synthetic model";
    std::fs::write(data.join("tiny.bin"), bytes).unwrap();
    let mut config = TranscriptionConfig::default();
    config.data_directory = Some(data);
    config.model = Some("tiny.bin".into());
    config.whisper_executable = Some(env!("CARGO_BIN_EXE_postal-stt-fixture").into());
    config.decoder_executable = config.whisper_executable.clone();
    config.model_sha256 = Some(
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    );
    config.timeout_secs = Some(10);
    let request = TranscriptionRequest {
        provider: "local-whisper".into(),
        chat: "synthetic@chat".into(),
        message_id: "synthetic".into(),
        mime: "audio/ogg".into(),
        duration_ms: 100,
        audio: STANDARD.encode(b"OggSsynthetic"),
        config,
    };
    assert_eq!(
        host.transcribe(id, request).await.unwrap().text,
        "synthetic transcript"
    );
    tokio::time::timeout(std::time::Duration::from_secs(4), async {
        while host.list()[0].state != "idle" {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    host.shutdown().await;
}
