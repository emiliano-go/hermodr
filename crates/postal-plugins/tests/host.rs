use postal_plugins::PluginHost;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const ID: &str = "com.example.fixture";

struct Fixture {
    root: PathBuf,
    plugin: PathBuf,
}
impl Fixture {
    fn new(activation: &str, idle: Option<u64>, mode: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "postal-plugin-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let plugin = root.join("plugins").join(ID);
        std::fs::create_dir_all(&plugin).unwrap();
        let executable = if cfg!(windows) {
            "fixture.exe"
        } else {
            "fixture"
        };
        std::fs::copy(
            env!("CARGO_BIN_EXE_postal-fake-plugin"),
            plugin.join(executable),
        )
        .unwrap();
        let manifest = json!({"id":ID,"name":"Fixture","version":"1","api_version":1,"entrypoint":executable,
            "activation":activation,"idle_timeout_secs":idle,"capabilities":["events:read"],"contributes":{"commands":[]}});
        std::fs::write(plugin.join("plugin.json"), manifest.to_string()).unwrap();
        std::fs::write(plugin.join("mode"), mode).unwrap();
        Self { root, plugin }
    }
    fn host(&self) -> PluginHost {
        PluginHost::discover(&self.root.join("plugins"), self.root.join("grants.json")).unwrap()
    }
    fn trace(&self) -> String {
        std::fs::read_to_string(self.plugin.join("trace")).unwrap_or_default()
    }
    fn manifest(&self, mutate: impl FnOnce(&mut Value)) {
        let path = self.plugin.join("plugin.json");
        let mut value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        mutate(&mut value);
        std::fs::write(path, value.to_string()).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
async fn wait(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("condition not met");
}
async fn enable(host: &PluginHost) {
    host.set_enabled(ID, true, vec!["events:read".into()])
        .await
        .unwrap();
}

#[tokio::test]
async fn eager_handshake_consent_malformed_lines_and_persisted_grants() {
    let fixture = Fixture::new("eager", None, "malformed");
    let host = fixture.host();
    host.start_enabled().await.unwrap();
    assert!(fixture.trace().is_empty());
    assert!(host.set_enabled(ID, true, vec![]).await.is_err());
    assert!(host.send_event(ID, json!({})).await.is_err());
    enable(&host).await;
    host.send_event(ID, json!({"marker":"one"})).await.unwrap();
    wait(|| fixture.trace().contains("denied")).await;
    host.shutdown().await;
    let trace = fixture.trace();
    assert!(trace.find("hello").unwrap() < trace.find("event:").unwrap());
    assert!(trace.contains("shutdown"));
    let restored = fixture.host();
    assert!(restored.list()[0].enabled);
    restored.start_enabled().await.unwrap();
    restored
        .send_event(ID, json!({"marker":"two"}))
        .await
        .unwrap();
    restored.set_enabled(ID, false, vec![]).await.unwrap();
    assert!(!fixture.host().list()[0].enabled);
    restored.shutdown().await;
}

#[tokio::test]
async fn lazy_queues_handshake_then_unloads_after_ack_and_respawns() {
    let fixture = Fixture::new("lazy", None, "slow");
    let host = fixture.host();
    enable(&host).await;
    assert!(fixture.trace().is_empty());
    let (one, two) = tokio::join!(
        host.send_event(ID, json!({"marker":"one"})),
        host.send_event(ID, json!({"marker":"two"}))
    );
    one.unwrap();
    two.unwrap();
    wait(|| fixture.trace().contains("shutdown")).await;
    assert_eq!(fixture.trace().matches("start\n").count(), 1);
    host.send_event(ID, json!({"marker":"three"}))
        .await
        .unwrap();
    host.shutdown().await;
    assert_eq!(fixture.trace().matches("start\n").count(), 2);
}

#[tokio::test]
async fn lazy_waits_for_matching_ack_then_lingers() {
    let fixture = Fixture::new("lazy", Some(1), "slow-ack");
    let host = fixture.host();
    enable(&host).await;
    let started = Instant::now();
    host.send_event(ID, json!({"marker":"ack"})).await.unwrap();
    assert!(started.elapsed() >= Duration::from_millis(240));
    assert!(!fixture.trace().contains("shutdown"));
    tokio::time::sleep(Duration::from_millis(300)).await;
    host.send_event(ID, json!({"marker":"linger"}))
        .await
        .unwrap();
    assert_eq!(fixture.trace().matches("start\n").count(), 1);
    wait(|| fixture.trace().contains("shutdown")).await;
    host.shutdown().await;
}

#[tokio::test]
async fn repeated_eager_crashes_back_off_and_persist_disable() {
    let fixture = Fixture::new("eager", None, "crash");
    let host = fixture.host();
    let started = Instant::now();
    enable(&host).await;
    wait(|| !host.list()[0].enabled).await;
    host.shutdown().await;
    assert!(started.elapsed() >= Duration::from_millis(700));
    assert_eq!(fixture.trace().matches("start\n").count(), 3);
    assert!(host.list()[0].error.is_some());
    assert!(!fixture.host().list()[0].enabled);
}

#[tokio::test]
async fn lazy_crash_retries_only_on_next_event() {
    let fixture = Fixture::new("lazy", None, "event-crash");
    let host = fixture.host();
    enable(&host).await;
    assert!(host.send_event(ID, json!({"marker":"one"})).await.is_err());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fixture.trace().matches("start\n").count(), 1);
    assert!(host.list()[0].enabled);
    assert!(host.send_event(ID, json!({"marker":"two"})).await.is_err());
    host.shutdown().await;
    assert_eq!(fixture.trace().matches("start\n").count(), 2);
}

#[tokio::test]
async fn bounded_queue_and_disable_interrupt_unacknowledged_work() {
    let fixture = Fixture::new("lazy", None, "no-ack");
    let host = fixture.host();
    enable(&host).await;
    for _ in 0..100 {
        host.publish(json!({"marker":"queue"})).unwrap();
    }
    assert!(host.list()[0].error.as_ref().unwrap().contains("queue"));
    wait(|| fixture.trace().contains("event:")).await;
    let started = Instant::now();
    host.set_enabled(ID, false, vec![]).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(fixture.trace().contains("shutdown"));
    host.shutdown().await;
}

#[tokio::test]
async fn shutdown_kills_uncooperative_process() {
    let fixture = Fixture::new("eager", None, "ignore-shutdown");
    let host = fixture.host();
    enable(&host).await;
    host.send_event(ID, json!({})).await.unwrap();
    let started = Instant::now();
    host.shutdown().await;
    assert!(started.elapsed() >= Duration::from_secs(2));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn manifests_reject_unsupported_authority_and_paths() {
    for (key, value) in [
        ("id", json!("bad")),
        ("api_version", json!(2)),
        ("capabilities", json!(["message:send"])),
        ("entrypoint", json!("../fixture")),
        ("entrypoint", json!("/fixture")),
        ("activation", json!("unknown")),
        ("idle_timeout_secs", json!(0)),
        ("contributes", json!({"commands":["send"]})),
    ] {
        let fixture = Fixture::new("eager", None, "");
        fixture.manifest(|manifest| manifest[key] = value);
        let host = fixture.host();
        assert!(host.list().is_empty(), "accepted {key}");
        assert_eq!(host.discovery_errors().len(), 1);
    }
    let fixture = Fixture::new("eager", None, "");
    let second = fixture.root.join("plugins/duplicate");
    std::fs::create_dir(&second).unwrap();
    std::fs::copy(
        fixture.plugin.join("plugin.json"),
        second.join("plugin.json"),
    )
    .unwrap();
    let executable = if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    };
    std::fs::copy(fixture.plugin.join(executable), second.join(executable)).unwrap();
    assert!(PluginHost::discover(
        &fixture.root.join("plugins"),
        fixture.root.join("grants.json")
    )
    .is_err());
}

#[tokio::test]
async fn sanitized_environment_and_manifest_changes_fail_closed() {
    let fixture = Fixture::new("lazy", None, "");
    let host = fixture.host();
    std::env::set_var("POSTAL_PLUGIN_TEST_SECRET", "synthetic");
    enable(&host).await;
    host.send_event(ID, json!({})).await.unwrap();
    wait(|| fixture.trace().contains("shutdown")).await;
    assert!(!fixture.trace().contains("environment leak"));
    fixture.manifest(|manifest| manifest["version"] = json!("2"));
    assert!(host.send_event(ID, json!({})).await.is_err());
    assert_eq!(fixture.trace().matches("start\n").count(), 1);
    host.shutdown().await;
}

#[cfg(unix)]
#[test]
fn symlink_entrypoint_cannot_escape_plugin_directory() {
    let fixture = Fixture::new("lazy", None, "");
    std::os::unix::fs::symlink(
        env!("CARGO_BIN_EXE_postal-fake-plugin"),
        fixture.plugin.join("escape"),
    )
    .unwrap();
    fixture.manifest(|manifest| manifest["entrypoint"] = json!("escape"));
    assert_eq!(fixture.host().discovery_errors().len(), 1);
}

#[tokio::test]
async fn failed_consent_write_never_starts_plugin() {
    let fixture = Fixture::new("eager", None, "");
    let host = fixture.host();
    std::fs::create_dir(fixture.root.join("grants.json")).unwrap();
    assert!(host
        .set_enabled(ID, true, vec!["events:read".into()])
        .await
        .is_err());
    assert!(!host.list()[0].enabled);
    assert!(fixture.trace().is_empty());
    host.shutdown().await;
}

#[test]
fn metadata_reads_are_bounded() {
    let fixture = Fixture::new("lazy", None, "");
    std::fs::write(fixture.plugin.join("plugin.json"), vec![b' '; 65537]).unwrap();
    assert_eq!(fixture.host().discovery_errors().len(), 1);
    std::fs::write(
        fixture.root.join("grants.json"),
        vec![b' '; 1024 * 1024 + 1],
    )
    .unwrap();
    assert!(PluginHost::discover(
        &fixture.root.join("plugins"),
        fixture.root.join("grants.json")
    )
    .is_err());
}
