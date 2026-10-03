use super::receive_file;
use std::{fs, io::Cursor, path::PathBuf, sync::atomic::{AtomicU64, Ordering}};
use whatsapp_rust::wacore::{
    download::{DownloadUtils, MediaDecryptionError, MediaType},
    upload::{EncryptedMedia, encrypt_media_with_key},
};

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("postal-media-hash-{}-{nonce}-{}",
            std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn names(&self) -> Vec<String> {
        let mut names = fs::read_dir(&self.0).unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
        names.sort();
        names
    }
}

impl Drop for Directory {
    fn drop(&mut self) { fs::remove_dir_all(&self.0).unwrap(); }
}

#[tokio::test]
async fn media_hash_valid_stream_publishes_only_after_verification() {
    for size in [0, 1, 16, 8191, 8192, 8193, 32785] {
        let directory = Directory::new();
        let destination = directory.0.join("verified.mp4");
        let plaintext = (0..size).map(|index| (index % 251) as u8).collect::<Vec<_>>();
        let encrypted = encrypt_media_with_key(&plaintext, MediaType::Video, Some(&[0x42; 32])).unwrap();
        let path = receive_file(&directory.0, "verified", "mp4", |mut writer| async {
            assert!(!destination.exists());
            let written = DownloadUtils::decrypt_stream_to_writer_with_hashes(
                Cursor::new(&encrypted.data_to_upload), &encrypted.media_key, MediaType::Video,
                Some(&encrypted.file_enc_sha256), Some(&encrypted.file_sha256), &mut writer,
            )?;
            assert_eq!(written, plaintext.len() as u64);
            assert!(!destination.exists());
            Ok(writer)
        }).await.unwrap();
        assert_eq!(path, destination);
        assert_eq!(fs::read(path).unwrap(), plaintext);
        assert_eq!(directory.names(), ["verified.mp4"]);
    }
}

enum Corruption { EncryptedHash, PlaintextHash, Ciphertext }

async fn rejects_without_publication(corruption: Corruption) {
    for cached in [false, true] {
        let directory = Directory::new();
        let destination = directory.0.join("verified.mp4");
        if cached { fs::write(&destination, b"existing verified media").unwrap(); }
        let plaintext = (0..32785).map(|index| (index % 251) as u8).collect::<Vec<_>>();
        let mut encrypted: EncryptedMedia = encrypt_media_with_key(&plaintext, MediaType::Video, Some(&[0x42; 32])).unwrap();
        match corruption {
            Corruption::EncryptedHash => encrypted.file_enc_sha256[0] ^= 1,
            Corruption::PlaintextHash => encrypted.file_sha256[0] ^= 1,
            Corruption::Ciphertext => encrypted.data_to_upload[0] ^= 1,
        }
        let error = receive_file(&directory.0, "verified", "mp4", |mut writer| async {
            let verified = DownloadUtils::decrypt_stream_to_writer_with_hashes(
                Cursor::new(&encrypted.data_to_upload), &encrypted.media_key, MediaType::Video,
                Some(&encrypted.file_enc_sha256), Some(&encrypted.file_sha256), &mut writer,
            );
            assert!(writer.metadata()?.len() > 0, "fixture must reach streaming writes before verification rejects it");
            if cached { assert_eq!(fs::read(&destination)?, b"existing verified media"); }
            else { assert!(!destination.exists()); }
            verified?;
            Ok(writer)
        }).await.expect_err("corrupt media must not be published");
        match corruption {
            Corruption::EncryptedHash => assert!(matches!(error.downcast_ref::<MediaDecryptionError>(),
                Some(MediaDecryptionError::EncryptedSha256Mismatch))),
            Corruption::PlaintextHash => assert!(matches!(error.downcast_ref::<MediaDecryptionError>(),
                Some(MediaDecryptionError::PlaintextSha256Mismatch))),
            Corruption::Ciphertext => assert_eq!(error.to_string(), "MAC mismatch"),
        }
        if cached {
            assert_eq!(fs::read(destination).unwrap(), b"existing verified media");
            assert_eq!(directory.names(), ["verified.mp4"]);
        } else {
            assert!(!destination.exists());
            assert!(directory.names().is_empty());
        }
    }
}

#[tokio::test]
async fn media_hash_encrypted_sha256_mismatch_discards_unverified_output() {
    rejects_without_publication(Corruption::EncryptedHash).await;
}

#[tokio::test]
async fn media_hash_plaintext_sha256_mismatch_discards_unverified_output() {
    rejects_without_publication(Corruption::PlaintextHash).await;
}

#[tokio::test]
async fn media_hash_corrupt_ciphertext_discards_unauthenticated_output() {
    rejects_without_publication(Corruption::Ciphertext).await;
}
