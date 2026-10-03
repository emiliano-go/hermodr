use std::{collections::HashMap, fs::{self, OpenOptions}, io::{Seek, SeekFrom, Write}, path::{Path, PathBuf}, sync::{Mutex, atomic::{AtomicU64, Ordering}}};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use tauri::{AppHandle, Manager, State};
use crate::{AppState, account_store::active_account};
use std::sync::Arc;
use postal_core::WhatsAppService;
use crate::command_error::{CommandError, CommandResult};
use postal_core::message_ref::MessageRef;

pub(crate) const CHUNK_BYTES: usize = 256 * 1024;
pub(crate) const MAX_PENDING_ATTACHMENTS: usize = 8;
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
    fn begin(&self, root: &Path, owner: String, name: String, size: u64) -> CommandResult<String> {
        if name.is_empty() || name.len() > 1024 { return Err(CommandError::code("error.upload_name_invalid")); }
        let mut pending = self.0.lock().unwrap();
        pending.expire();
        if pending.entries.len() >= MAX_PENDING_ATTACHMENTS { return Err(CommandError::new(MessageRef::new("error.upload_pending_limit").with_param("max_items", serde_json::Number::from(MAX_PENDING_ATTACHMENTS)))); }
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

    fn append(&self, owner: &str, token: &str, offset: u64, bytes: &[u8]) -> CommandResult<()> {
        if bytes.len() > CHUNK_BYTES { return Err(CommandError::new(MessageRef::new("error.upload_chunk_limit").with_param("max_bytes", serde_json::Number::from(CHUNK_BYTES)))); }
        let mut pending = self.0.lock().unwrap();
        pending.expire();
        let file = pending.entries.get_mut(token).ok_or_else(|| CommandError::code("error.upload_unknown"))?;
        if file.owner != owner { return Err(CommandError::code("error.upload_account_mismatch")); }
        if file.written != offset || offset.checked_add(bytes.len() as u64).is_none_or(|end| end > file.size) {
            return Err(CommandError::code("error.upload_chunk_order"));
        }
        let mut output = OpenOptions::new().write(true).open(&file.path).map_err(|e| e.to_string())?;
        output.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        output.write_all(bytes).map_err(|e| e.to_string())?;
        file.written += bytes.len() as u64;
        file.touched = std::time::Instant::now();
        Ok(())
    }

    pub(crate) fn take(&self, owner: &str, token: &str) -> CommandResult<StagedUpload> {
        let mut pending = self.0.lock().unwrap();
        pending.expire();
        let file = pending.entries.get(token).ok_or_else(|| CommandError::code("error.upload_unknown"))?;
        if file.owner != owner { return Err(CommandError::code("error.upload_account_mismatch")); }
        if file.written != file.size || fs::metadata(&file.path).map_err(|e| e.to_string())?.len() != file.size {
            return Err(CommandError::code("error.upload_incomplete"));
        }
        Ok(pending.entries.remove(token).expect("checked upload exists"))
    }

    pub(crate) fn take_many(&self, owner: &str, tokens: &[String]) -> CommandResult<Vec<StagedUpload>> {
        if tokens.is_empty() || tokens.len() > MAX_PENDING_ATTACHMENTS {
            return Err(CommandError::new(MessageRef::new("error.upload_batch_limit").with_param("max_items", serde_json::Number::from(MAX_PENDING_ATTACHMENTS))));
        }
        let mut pending = self.0.lock().unwrap();
        pending.expire();
        for (index, token) in tokens.iter().enumerate() {
            if tokens[..index].contains(token) { return Err(CommandError::code("error.upload_duplicate")); }
            let file = pending.entries.get(token).ok_or_else(|| CommandError::code("error.upload_unknown"))?;
            if file.owner != owner { return Err(CommandError::code("error.upload_account_mismatch")); }
            if file.written != file.size || fs::metadata(&file.path).map_err(|e| e.to_string())?.len() != file.size {
                return Err(CommandError::code("error.upload_incomplete"));
            }
        }
        Ok(tokens.iter().map(|token| pending.entries.remove(token).expect("checked upload exists")).collect())
    }

    fn cancel_owned(&self, owner: &str, token: &str) -> CommandResult<()> {
        let mut pending = self.0.lock().unwrap();
        if pending.entries.get(token).is_some_and(|file| file.owner != owner) {
            return Err(CommandError::code("error.upload_account_mismatch"));
        }
        pending.entries.remove(token);
        Ok(())
    }

    fn complete_begin(&self, owner: &str, token: String, current: CommandResult<()>) -> CommandResult<String> {
        if let Err(error) = current {
            self.cancel_owned(owner, &token)?;
            return Err(error);
        }
        Ok(token)
    }

    #[cfg(test)]
    fn cancel(&self, token: &str) { self.0.lock().unwrap().entries.remove(token); }
}

fn check_upload_scope(owner: &str, active: Option<&str>, same_service: bool) -> CommandResult<()> {
    if active == Some(owner) && same_service { Ok(()) }
    else { Err(CommandError::code("error.upload_account_changed")) }
}

fn upload_current(state: &AppState, owner: &str, expected: &Arc<WhatsAppService>) -> CommandResult<()> {
    let active = active_account(state);
    let service = state.account_service(owner).map_err(|error| CommandError::code("error.upload_account_changed").with_diagnostic(error))?;
    check_upload_scope(owner, active.as_deref(), Arc::ptr_eq(expected, &service))
}

fn upload_owner(state: &AppState, account: Option<String>) -> CommandResult<(String, Arc<WhatsAppService>)> {
    let active = active_account(state).ok_or_else(|| CommandError::code("error.no_active_account"))?;
    let owner = account.unwrap_or_else(|| active.clone());
    check_upload_scope(&owner, Some(&active), true)?;
    let service = state.account_service(&owner).map_err(|error| CommandError::code("error.upload_account_changed").with_diagnostic(error))?;
    upload_current(state, &owner, &service)?;
    Ok((owner, service))
}

#[tauri::command]
pub(crate) async fn begin_upload(app: AppHandle, state: State<'_, AppState>, name: String, size: u64, account_id: Option<String>) -> CommandResult<String> {
    let (owner, service) = upload_owner(&state, account_id)?;
    let root = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("uploads");
    let uploads = state.uploads.clone();
    let captured = owner.clone();
    let token = tauri::async_runtime::spawn_blocking(move || uploads.begin(&root, captured, name, size)).await.map_err(|e| e.to_string())??;
    state.uploads.complete_begin(&owner, token, upload_current(&state, &owner, &service))
}

#[tauri::command]
pub(crate) async fn append_upload(state: State<'_, AppState>, token: String, offset: u64, data: String, account_id: Option<String>) -> CommandResult<()> {
    if token.is_empty() || token.len() > 256 { return Err(CommandError::code("error.upload_token_invalid")); }
    if data.len() > CHUNK_BYTES.div_ceil(3) * 4 { return Err(CommandError::new(MessageRef::new("error.upload_chunk_limit").with_param("max_bytes", serde_json::Number::from(CHUNK_BYTES)))); }
    let (owner, service) = upload_owner(&state, account_id)?;
    let uploads = state.uploads.clone();
    let captured = owner.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = BASE64.decode(data).map_err(|error| CommandError::code("error.media_base64_invalid").with_diagnostic(error))?;
        uploads.append(&captured, &token, offset, &bytes)
    }).await.map_err(|e| e.to_string())??;
    upload_current(&state, &owner, &service)
}

#[tauri::command]
pub(crate) async fn cancel_upload(state: State<'_, AppState>, token: String, account_id: Option<String>) -> CommandResult<()> {
    let owner = account_id.or_else(|| active_account(&state)).ok_or_else(|| CommandError::code("error.no_active_account"))?;
    if owner.is_empty() || owner.len() > 256 || token.is_empty() || token.len() > 256 { return Err(CommandError::code("error.upload_scope_invalid")); }
    let uploads = state.uploads.clone();
    tauri::async_runtime::spawn_blocking(move || uploads.cancel_owned(&owner, &token)).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
#[path = "albums_upload_tests.rs"]
mod album_tests;

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
        assert_eq!(uploads.take("one", &token).err().unwrap().message.code, "error.upload_incomplete");
        assert_eq!(uploads.append("two", &token, 0, &bytes[..10]).unwrap_err().message.code, "error.upload_account_mismatch");
        assert_eq!(uploads.append("one", &token, 1, &bytes[..10]).unwrap_err().message.code, "error.upload_chunk_order");
        let large = uploads.append("one", &token, 0, &bytes).unwrap_err();
        assert_eq!(large.message.code, "error.upload_chunk_limit");
        assert_eq!(serde_json::to_value(large).unwrap()["params"]["max_bytes"], CHUNK_BYTES);
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
