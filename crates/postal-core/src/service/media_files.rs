use super::*;
use std::{fs::File, io::{Seek, SeekFrom}};
use whatsapp_rust::wacore::upload::{encrypt_media_streaming, EncryptedMediaInfo, UploadSource};

pub(super) struct FileSource {
    file: TemporaryFile,
    length: u64,
}

impl UploadSource for FileSource {
    fn len(&self) -> u64 { self.length }

    fn reader_from(&self, offset: u64) -> std::io::Result<Box<dyn std::io::Read + Send>> {
        let mut file = File::open(&self.file.path)?;
        file.seek(SeekFrom::Start(offset.min(self.length)))?;
        Ok(Box::new(file))
    }
}

pub(super) struct TemporaryFile {
    pub(super) path: PathBuf,
}

impl TemporaryFile {
    fn create(path: PathBuf) -> Result<(Self, File)> {
        let file = File::create_new(&path)?;
        Ok((Self { path }, file))
    }

    pub(super) fn download(directory: &Path) -> Result<(Self, File)> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
        Self::create(directory.join(format!("download-{}-{nonce}-{}.part", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))))
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound { log::warn!("could not remove transfer temporary file: {error}"); }
        }
    }
}

pub(super) fn encrypt_file(path: &Path, media_type: MediaType) -> Result<(FileSource, EncryptedMediaInfo)> {
    let mut input = File::open(path)?;
    let destination = path.with_extension("encrypted");
    let (file, mut output) = TemporaryFile::create(destination)?;
    let mut source = FileSource { file, length: 0 };
    let info = encrypt_media_streaming(&mut input, &mut output, media_type)?;
    source.length = output.metadata()?.len();
    Ok((source, info))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn file_previews_decode_staged_video_without_loading_its_bytes() {
        let root = std::env::temp_dir().join(format!("postal-preview café 📨-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        let file = root.join("staged.part");
        let named = root.join("named.mp4");
        for (bytes, dimensions) in [
            (include_bytes!("../../tests/fixtures/tiny-video.mp4").as_slice(), (256, 192)),
            (include_bytes!("../../tests/fixtures/portrait-video.mp4").as_slice(), (192, 256)),
        ] {
            std::fs::write(&file, bytes).unwrap();
            std::fs::write(&named, bytes).unwrap();
            for preview in [
                super::super::media_codec::media_thumbnail("video", bytes),
                super::super::media_codec::media_thumbnail_file("video", &named),
                super::super::media_codec::media_thumbnail_file("video", &file),
                super::super::media_codec::media_thumbnail_file("video", &file.canonicalize().unwrap()),
            ] {
                let preview = preview.expect("synthetic video must decode; Linux/macOS tests require ffmpeg on PATH");
                let image = image::load_from_memory(&preview).unwrap();
                assert_eq!((image.width(), image.height()), dimensions);
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn file_upload_matches_buffered_crypto_and_reopens_at_retry_offsets() {
        let root = std::env::temp_dir().join(format!("postal-stream-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        for size in [0, 1, 16, 8193, 4 * 1024 * 1024 + 17] {
            let bytes = vec![37; size];
            let path = root.join("synthetic.part");
            std::fs::write(&path, &bytes).unwrap();
            let (source, info) = encrypt_file(&path, MediaType::Video).unwrap();
            let expected = whatsapp_rust::wacore::upload::encrypt_media_with_key(&bytes, MediaType::Video, Some(&info.media_key)).unwrap();
            assert_eq!(info.file_sha256, expected.file_sha256);
            assert_eq!(info.file_enc_sha256, expected.file_enc_sha256);
            assert_eq!(info.streaming_sidecar, expected.streaming_sidecar);
            assert_eq!(source.len(), expected.data_to_upload.len() as u64);
            for offset in [0, 3, source.len() / 2, source.len() + 1] {
                let mut actual = Vec::new();
                source.reader_from(offset).unwrap().read_to_end(&mut actual).unwrap();
                assert_eq!(actual, expected.data_to_upload[offset.min(source.len()) as usize..]);
            }
            let encrypted = source.file.path.clone();
            drop(source);
            assert!(!encrypted.exists());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        let (temporary, file) = TemporaryFile::download(&root).unwrap();
        let path = temporary.path.clone();
        drop(file);
        drop(temporary);
        assert!(!path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
