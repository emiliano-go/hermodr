use super::*;
use std::io::Write;

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "postal-receive-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn names(&self) -> Vec<String> {
        std::fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[tokio::test]
async fn received_media_is_published_only_after_writer_success() {
    let directory = Directory::new();
    let destination = directory.0.join("message.mp4");
    let pending = &destination;
    let path = receive_file(&directory.0, "message", "mp4", |mut writer| async move {
        assert!(!pending.exists());
        writer.write_all(b"verified media")?;
        assert!(!pending.exists());
        Ok(writer)
    })
    .await
    .unwrap();
    assert_eq!(path, destination);
    assert_eq!(std::fs::read(path).unwrap(), b"verified media");
    assert_eq!(directory.names(), ["message.mp4"]);
}

#[tokio::test]
async fn failed_receive_removes_partial_output_and_keeps_existing_media() {
    let directory = Directory::new();
    let destination = directory.0.join("message.mp4");
    std::fs::write(&destination, b"existing media").unwrap();
    let result = receive_file(&directory.0, "message", "mp4", |mut writer| async move {
        writer.write_all(b"unverified partial media")?;
        anyhow::bail!("synthetic verification failure")
    })
    .await;
    assert!(result.is_err());
    assert_eq!(std::fs::read(destination).unwrap(), b"existing media");
    assert_eq!(directory.names(), ["message.mp4"]);
}

#[tokio::test]
async fn failed_receive_without_cache_leaves_no_files() {
    let directory = Directory::new();
    let result = receive_file(&directory.0, "message", "mp4", |mut writer| async move {
        writer.write_all(b"partial media")?;
        anyhow::bail!("synthetic transport failure")
    })
    .await;
    assert!(result.is_err());
    assert!(directory.names().is_empty());
}

#[tokio::test]
async fn failed_receive_publication_removes_verified_temporary_file() {
    let directory = Directory::new();
    std::fs::create_dir(directory.0.join("message.mp4")).unwrap();
    let result = receive_file(&directory.0, "message", "mp4", |mut writer| async move {
        writer.write_all(b"verified media")?;
        Ok(writer)
    })
    .await;
    assert!(result.is_err());
    assert_eq!(directory.names(), ["message.mp4"]);
    assert!(directory.0.join("message.mp4").is_dir());
}

#[tokio::test]
async fn receive_rejects_path_traversal_before_opening_writer() {
    let directory = Directory::new();
    for id in [
        "../escape",
        "..\\escape",
        "/escape",
        "C:\\escape",
        "bad\0id",
    ] {
        let result = receive_file(&directory.0, id, "mp4", |_| async {
            panic!("invalid identifier reached downloader")
        })
        .await;
        assert!(result.is_err(), "accepted {id:?}");
        assert!(directory.names().is_empty());
    }
}

#[tokio::test]
async fn auto_download_without_client_preserves_locator_for_later_fetch() {
    let message = wa::Message {
        video_message: MessageField::some(wa::message::VideoMessage {
            direct_path: Some("/synthetic-media".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let header = MessageHeader {
        chat: "123@s.whatsapp.net".into(),
        id: "message".into(),
        sender: "123@s.whatsapp.net".into(),
        timestamp: 1,
        from_me: false,
    };
    let row = super::super::message_decode::stored_message(&message, header, None, None, true)
        .await
        .unwrap();
    assert_eq!(row.media.kind.as_deref(), Some("video"));
    assert!(row.media.path.is_none());
    assert_eq!(row.media.locator, Some(media_locator(&message)));
}
