use postal_plugins::{PluginHost, TranscriptionConfig, TranscriptionRequest};
use serde_json::json;
use std::{path::PathBuf, time::Duration};

struct Fixture(PathBuf);
impl Fixture {
    fn new(mode: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "postal-stt-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let dir = root.join("plugins/test");
        std::fs::create_dir_all(&dir).unwrap();
        let executable = if cfg!(windows) {
            "fixture.exe"
        } else {
            "fixture"
        };
        std::fs::copy(
            env!("CARGO_BIN_EXE_postal-fake-plugin"),
            dir.join(executable),
        )
        .unwrap();
        std::fs::write(dir.join("mode"), mode).unwrap();
        let manifest = json!({"id":"org.postal.stt", "name":"STT", "version":"1", "api_version":1,
            "entrypoint":executable, "activation":"lazy", "idle_timeout_secs":null, "capabilities":["transcribe"],
            "contributes":{"transcription":{"id":"stt", "providers":[
                {"id":"local", "name":"Local", "kind":"local", "transmits_audio":false},
                {"id":"cloud", "name":"Cloud", "kind":"cloud", "transmits_audio":true,"requires_key":true}]}}});
        std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
        Self(root)
    }
    fn host(&self) -> PluginHost {
        PluginHost::discover(&self.0.join("plugins"), self.0.join("grants.json")).unwrap()
    }
    fn trace(&self) -> String {
        std::fs::read_to_string(self.0.join("plugins/test/trace")).unwrap_or_default()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn request(provider: &str) -> TranscriptionRequest {
    TranscriptionRequest {
        provider: provider.into(),
        chat: "synthetic@chat".into(),
        message_id: "id".into(),
        mime: "audio/ogg".into(),
        duration_ms: 100,
        audio: "T2dnUw==".into(),
        config: TranscriptionConfig::default(),
    }
}
async fn idle(host: &PluginHost) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while host.list()[0].state != "idle" {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
async fn enable(host: &PluginHost) {
    host.set_enabled("org.postal.stt", true, vec!["transcribe".into()])
        .await
        .unwrap();
}

#[tokio::test]
async fn transcribe_requires_capability_provider_and_cloud_consent_before_spawn() {
    let f = Fixture::new("");
    let h = f.host();
    assert!(h
        .transcribe("org.postal.stt", request("local"))
        .await
        .is_err());
    assert!(h
        .set_enabled("org.postal.stt", true, vec!["events:read".into()])
        .await
        .is_err());
    enable(&h).await;
    assert!(h
        .transcribe("org.postal.stt", request("unknown"))
        .await
        .is_err());
    assert!(h
        .transcribe("org.postal.stt", request("cloud"))
        .await
        .is_err());
    let mut cloud = request("cloud");
    cloud.config.cloud_consent = true;
    assert!(h.transcribe("org.postal.stt", cloud).await.is_err());
    h.publish(json!({"kind":"connected"})).unwrap();
    assert!(h.has_enabled());
    assert!(!h.has_event_readers());
    h.publish(json!({"synthetic":"x".repeat(1024*1024+1)}))
        .unwrap();
    assert!(h.send_event("org.postal.stt", json!({})).await.is_err());
    assert!(f.trace().is_empty());
    let mut cloud = request("cloud");
    cloud.config.cloud_consent = true;
    cloud.config.api_key = Some("synthetic-key".into());
    assert_eq!(
        h.transcribe("org.postal.stt", cloud)
            .await
            .unwrap()
            .provider,
        "cloud"
    );
    h.shutdown().await;
}

#[tokio::test]
async fn matching_transcript_queues_unloads_and_respawns() {
    let f = Fixture::new("slow");
    let h = f.host();
    enable(&h).await;
    let (a, b) = tokio::join!(
        h.transcribe("org.postal.stt", request("local")),
        h.transcribe("org.postal.stt", request("local"))
    );
    assert_eq!(a.unwrap().text, "synthetic transcript");
    assert_eq!(b.unwrap().language.as_deref(), Some("en"));
    idle(&h).await;
    assert_eq!(f.trace().matches("start").count(), 1);
    h.transcribe("org.postal.stt", request("local"))
        .await
        .unwrap();
    idle(&h).await;
    assert_eq!(f.trace().matches("start").count(), 2);
    h.shutdown().await;
}

#[tokio::test]
async fn error_wrong_provider_timeout_and_cancellation_stop_work() {
    for mode in ["transcribe-error", "wrong-provider", "no-transcript"] {
        let f = Fixture::new(mode);
        let h = f.host();
        enable(&h).await;
        let mut r = request("local");
        r.config.timeout_secs = Some(1);
        assert!(h.transcribe("org.postal.stt", r).await.is_err());
        h.shutdown().await;
        assert!(f.trace().contains("shutdown"));
    }
    let f = Fixture::new("no-transcript");
    let h = std::sync::Arc::new(f.host());
    enable(&h).await;
    let copy = h.clone();
    let task =
        tokio::spawn(async move { copy.transcribe("org.postal.stt", request("local")).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !f.trace().contains("transcribe:") {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    let _ = task.await;
    idle(&h).await;
    assert!(f.trace().contains("cancel"));
    assert!(f.trace().contains("shutdown"));
    h.shutdown().await;
}

#[tokio::test]
async fn invalid_payload_and_manifest_never_spawn() {
    let f = Fixture::new("");
    let h = f.host();
    enable(&h).await;
    let mut r = request("local");
    r.audio = "a".repeat(1024 * 1024);
    assert!(h.transcribe("org.postal.stt", r).await.is_err());
    assert!(f.trace().is_empty());
    h.shutdown().await;
    let file = f.0.join("plugins/test/plugin.json");
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    for mutation in ["eager", "privacy", "duplicate", "unknown"] {
        let mut m = original.clone();
        match mutation {
            "eager" => m["activation"] = json!("eager"),
            "privacy" => {
                m["contributes"]["transcription"]["providers"][0]["transmits_audio"] = json!(true)
            }
            "duplicate" => {
                let p = m["contributes"]["transcription"]["providers"][0].clone();
                m["contributes"]["transcription"]["providers"][1] = p;
            }
            _ => m["capabilities"] = json!(["media:read"]),
        }
        std::fs::write(&file, m.to_string()).unwrap();
        assert!(f.host().list().is_empty());
    }
}

#[tokio::test]
async fn model_installation_is_explicit_granted_correlated_and_unloaded() {
    for mode in ["", "wrong-model"] {
        let f = Fixture::new(mode);
        let h = f.host();
        assert!(h
            .install_model(
                "org.postal.stt",
                "https://synthetic.invalid/model".into(),
                "0".repeat(64),
                "tiny.bin".into()
            )
            .await
            .is_err());
        assert!(f.trace().is_empty());
        enable(&h).await;
        assert!(h
            .install_model(
                "org.postal.stt",
                "http://synthetic.invalid/model".into(),
                "0".repeat(64),
                "tiny.bin".into()
            )
            .await
            .is_err());
        let result = h
            .install_model(
                "org.postal.stt",
                "https://synthetic.invalid/model".into(),
                "0".repeat(64),
                "tiny.bin".into(),
            )
            .await;
        assert_eq!(result.is_ok(), mode.is_empty());
        if result.is_ok() {
            idle(&h).await;
        }
        h.shutdown().await;
        assert!(f.trace().contains("model"));
        assert!(f.trace().contains("shutdown"));
    }
}
