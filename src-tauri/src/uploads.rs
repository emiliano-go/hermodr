use std::{collections::HashMap, fs::{self, OpenOptions}, io::{Seek, SeekFrom, Write}, path::{Path, PathBuf}, sync::{Mutex, atomic::{AtomicU64, Ordering}}};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use tauri::{AppHandle, Manager, State};
use crate::{AppState, account_store::active_account};

pub(crate) const CHUNK_BYTES: usize = 256 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);

pub(crate) struct StagedUpload {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    owner: String,
    size: u64,
    written: u64,
    touched: std::time::Instant,
}

impl Drop for StagedUpload {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound { log::warn!("could not remove staged upload: {error}"); }
        }
    }
}

#[derive(Default)]
struct Pending {
    entries: HashMap<String, StagedUpload>,
    cleaned: bool,
}

#[derive(Default)]
pub(crate) struct Uploads(Mutex<Pending>);

impl Pending {
    fn expire(&mut self) {
        self.entries.retain(|_, file| file.touched.elapsed() < std::time::Duration::from_secs(3600));
    }
}

impl Uploads {
    fn begin(&self, root: &Path, owner: String, name: String, size: u64) -> Result<String, String> {
        if name.is_empty() || name.len() > 1024 { return Err("invalid attachment name".into()); }
        let mut pending = self.0.lock().unwrap();
        pending.expire();
        if pending.entries.len() >= 8 { return Err("too many pending attachments".into()); }
        fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        if !pending.cleaned {
            for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                let stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
                let suffix = path.extension().and_then(|n| n.to_str()).unwrap_or("");
                let owned = stem.strip_prefix("stage-").is_some_and(|s| s.split('-').count() == 3 && s.split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())));
                let old = entry.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age >= std::time::Duration::from_secs(24 * 3600));
                if owned && old && matches!(suffix, "part" | "encrypted") && entry.file_type().is_ok_and(|t| t.is_file()) {
                    if let Err(error) = fs::remove_file(path) { log::warn!("could not remove stale upload: {error}"); }
                }
            }
            pending.cleaned = true;
        }
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
        let token = format!("stage-{}-{nonce}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
        let path = root.join(format!("{token}.part"));
        OpenOptions::new().create_new(true).write(true).open(&path).map_err(|e| e.to_string())?;
        pending.entries.insert(token.clone(), StagedUpload { path, name, owner, size, written: 0, touched: std::time::Instant::now() });
        Ok(token)
    }

    fn append(&self, owner: &str, token: &str, offset: u64, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > CHUNK_BYTES { return Err("attachment chunk is too large".into()); }
        let mut pending = self.0.lock().unwrap();
        pending.expire();
        let file = pending.entries.get_mut(token).ok_or("unknown attachment upload")?;
        if file.owner != owner { return Err("attachment belongs to another account".into()); }
        if file.written != offset || offset.checked_add(bytes.len() as u64).is_none_or(|end| end > file.size) {
            return Err("attachment chunk is out of order or exceeds its size".into());
        }
        let mut output = OpenOptions::new().write(true).open(&file.path).map_err(|e| e.to_string())?;
        output.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        output.write_all(bytes).map_err(|e| e.to_string())?;
        file.written += bytes.len() as u64;
        file.touched = std::time::Instant::now();
        Ok(())
    }

    pub(crate) fn take(&self, owner: &str, token: &str) -> Result<StagedUpload, String> {
        let mut pending = self.0.lock().unwrap();
        pending.expire();
        let file = pending.entries.get(token).ok_or("unknown attachment upload")?;
        if file.owner != owner { return Err("attachment belongs to another account".into()); }
        if file.written != file.size || fs::metadata(&file.path).map_err(|e| e.to_string())?.len() != file.size {
            return Err("attachment upload is incomplete".into());
        }
        Ok(pending.entries.remove(token).expect("checked upload exists"))
    }

    fn cancel(&self, token: &str) { self.0.lock().unwrap().entries.remove(token); }
}

#[tauri::command]
pub(crate) async fn begin_upload(app: AppHandle, state: State<'_, AppState>, name: String, size: u64) -> Result<String, String> {
    let owner = active_account(&state).ok_or("no active account")?;
    let root = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("uploads");
    let uploads = state.uploads.clone();
    tauri::async_runtime::spawn_blocking(move || uploads.begin(&root, owner, name, size)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn append_upload(state: State<'_, AppState>, token: String, offset: u64, data: String) -> Result<(), String> {
    if data.len() > CHUNK_BYTES.div_ceil(3) * 4 { return Err("attachment chunk is too large".into()); }
    let owner = active_account(&state).ok_or("no active account")?;
    let uploads = state.uploads.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = BASE64.decode(data).map_err(|e| e.to_string())?;
        uploads.append(&owner, &token, offset, &bytes)
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn cancel_upload(state: State<'_, AppState>, token: String) -> Result<(), String> {
    let uploads = state.uploads.clone();
    tauri::async_runtime::spawn_blocking(move || uploads.cancel(&token)).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_names_never_become_local_paths() {
        let root = std::env::temp_dir().join(format!("postal-staging café 📨-{}-{}", std::process::id(), crate::account_store::now_millis()));
        let uploads = Uploads::default();
        for name in ["../outside.png", "..\\outside.png", "C:\\temp\\photo.png", "\\\\server\\share\\photo.png", "CON", "photo:stream", "写真 e\u{301}.png"] {
            let token = uploads.begin(&root, "one".into(), name.into(), 3).unwrap();
            uploads.append("one", &token, 0, b"abc").unwrap();
            let file = uploads.take("one", &token).unwrap();
            assert_eq!(file.name, name);
            assert_eq!(file.path.parent(), Some(root.canonicalize().unwrap().as_path()));
            assert_eq!(fs::read(&file.path).unwrap(), b"abc");
            let path = file.path.clone();
            drop(file);
            assert!(!path.exists());
        }
        drop(uploads);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn chunks_are_ordered_owned_bounded_and_cleaned() {
        let root = std::env::temp_dir().join(format!("postal-staging-{}-{}", std::process::id(), crate::account_store::now_millis()));
        let uploads = Uploads::default();
        let bytes = vec![23; CHUNK_BYTES + 17];
        let token = uploads.begin(&root, "one".into(), "synthetic.mp4".into(), bytes.len() as u64).unwrap();
        assert!(uploads.take("one", &token).is_err());
        assert!(uploads.append("two", &token, 0, &bytes[..10]).is_err());
        assert!(uploads.append("one", &token, 1, &bytes[..10]).is_err());
        assert!(uploads.append("one", &token, 0, &bytes).is_err());
        uploads.append("one", &token, 0, &bytes[..CHUNK_BYTES]).unwrap();
        assert!(uploads.append("one", &token, 0, &bytes[..10]).is_err());
        assert!(uploads.append("one", &token, CHUNK_BYTES as u64, &[1; 18]).is_err());
        uploads.append("one", &token, CHUNK_BYTES as u64, &bytes[CHUNK_BYTES..]).unwrap();
        assert!(uploads.take("two", &token).is_err());
        let file = uploads.take("one", &token).unwrap();
        assert_eq!(fs::read(&file.path).unwrap(), bytes);
        assert!(uploads.take("one", &token).is_err());
        let path = file.path.clone();
        drop(file);
        assert!(!path.exists());
        let token = uploads.begin(&root, "one".into(), "cancel.bin".into(), 4).unwrap();
        uploads.append("one", &token, 0, &[1, 2]).unwrap();
        uploads.cancel(&token);
        uploads.cancel(&token);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        for _ in 0..8 { uploads.begin(&root, "one".into(), "pending.bin".into(), 1).unwrap(); }
        assert!(uploads.begin(&root, "one".into(), "overflow.bin".into(), 1).is_err());
        for file in uploads.0.lock().unwrap().entries.values_mut() { file.touched -= std::time::Duration::from_secs(3601); }
        let empty = uploads.begin(&root, "one".into(), "empty.bin".into(), 0).unwrap();
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        drop(uploads.take("one", &empty).unwrap());
        drop(uploads);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir(root).unwrap();
    }
}
