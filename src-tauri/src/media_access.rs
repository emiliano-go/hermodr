use std::{collections::BTreeMap, path::{Path, PathBuf}};
use tauri::{AppHandle, Manager, State};
use crate::{AppState, account_store::media_cache_dir, media::READABLE_EXTENSIONS};
use crate::command_error::{CommandError, CommandResult};
use postal_core::message_ref::MessageRef;

fn resolved_boundary(path: &Path) -> CommandResult<PathBuf> {
    if !path.is_absolute() { return Err(CommandError::code("error.media_directory_absolute")); }
    let mut parent = path;
    let mut suffix = Vec::new();
    while !parent.exists() {
        suffix.push(parent.file_name().ok_or_else(|| CommandError::code("error.media_directory_invalid"))?.to_owned());
        parent = parent.parent().ok_or_else(|| CommandError::code("error.media_directory_invalid"))?;
    }
    let mut resolved = dunce::canonicalize(parent).map_err(|error| error.to_string())?;
    for component in suffix.into_iter().rev() { resolved.push(component); }
    Ok(resolved)
}

fn checked_root(directory: &Path, private: &[PathBuf], fallback: &Path) -> CommandResult<PathBuf> {
    let root = resolved_boundary(directory)?;
    for boundary in private {
        let fallback_allowed = Some(boundary.as_path()) == fallback.parent() && root.starts_with(fallback);
        if (root.starts_with(boundary) || boundary.starts_with(&root)) && !fallback_allowed {
            return Err(CommandError::code("error.media_directory_private"));
        }
    }
    Ok(root)
}

pub(crate) fn validate_directory(app: &AppHandle, directory: &Path) -> CommandResult<PathBuf> {
    let data = resolved_boundary(&app.path().app_data_dir().map_err(|error| error.to_string())?)?;
    let config = resolved_boundary(&app.path().app_config_dir().map_err(|error| error.to_string())?)?;
    let uploads = resolved_boundary(&app.path().app_cache_dir().map_err(|error| error.to_string())?.join("uploads"))?;
    checked_root(directory, &[data.clone(), config, uploads], &data.join("media"))
}

pub(crate) fn media_root(app: &AppHandle, state: &AppState) -> CommandResult<PathBuf> {
    let running = state.service.lock().unwrap().as_ref().and_then(|service| service.media_dir());
    let configured = running.unwrap_or_else(|| state.settings.lock().unwrap().media_dir.as_deref()
        .filter(|directory| !directory.trim().is_empty()).map(PathBuf::from).unwrap_or_else(|| media_cache_dir(app)));
    validate_directory(app, &configured)
}

fn authorized_file(root: &Path, path: &str) -> CommandResult<PathBuf> {
    let path = Path::new(path);
    if !path.is_absolute() { return Err(CommandError::code("error.media_file_absolute")); }
    let target = dunce::canonicalize(path).map_err(|error| error.to_string())?;
    if !target.starts_with(root) || !target.is_file() { return Err(CommandError::code("error.media_path_denied")); }
    let extension = target.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
    if !READABLE_EXTENSIONS.contains(&extension.as_str()) { return Err(CommandError::code("error.media_type_unsupported")); }
    Ok(target)
}

pub(crate) fn readable_file(app: &AppHandle, state: &AppState, path: &str) -> CommandResult<PathBuf> {
    authorized_file(&media_root(app, state)?, path)
}

fn grant_paths(root: &Path, paths: &[String], mut grant: impl FnMut(&Path) -> Result<(), String>) -> BTreeMap<String, Option<String>> {
    paths.iter().map(|path| {
        let target = authorized_file(root, path).and_then(|target| {
            grant(&target)?;
            Ok(target.to_string_lossy().into_owned())
        }).ok();
        (path.clone(), target)
    }).collect()
}

#[tauri::command(async)]
pub(crate) fn authorize_media_assets(app: AppHandle, state: State<'_, AppState>, paths: Vec<String>) -> CommandResult<BTreeMap<String, Option<String>>> {
    if paths.len() > 512 { return Err(CommandError::new(MessageRef::new("error.media_paths_limit").with_param("max_items", serde_json::Number::from(512)))); }
    let root = media_root(&app, &state)?;
    Ok(grant_paths(&root, &paths, |path| app.asset_protocol_scope().allow_file(path).map_err(|error| error.to_string())))
}

pub(crate) fn prepare_message(app: &AppHandle, message: &mut postal_core::store::StoredMessage) {
    if ![&message.media.path, &message.media.thumb, &message.quote.path, &message.quote.thumb]
        .into_iter().any(|field| field.as_deref().is_some_and(|path| Path::new(path).is_absolute())) { return; }
    let state = app.state::<AppState>();
    let root = media_root(app, &state);
    for field in [&mut message.media.path, &mut message.media.thumb, &mut message.quote.path, &mut message.quote.thumb] {
        let Some(path) = field.as_deref().filter(|path| Path::new(path).is_absolute()) else { continue };
        *field = root.as_ref().ok().and_then(|root| grant_paths(root, &[path.to_owned()], |target|
            app.asset_protocol_scope().allow_file(target).map_err(|error| error.to_string())).remove(path).flatten());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("postal-media-authority-{}-{}-{}", std::process::id(), crate::account_store::now_millis(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::create_dir_all(&path).unwrap();
            Self(dunce::canonicalize(path).unwrap())
        }
        fn file(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"synthetic").unwrap();
            path
        }
    }
    impl Drop for Fixture { fn drop(&mut self) {
        assert!(self.0.starts_with(dunce::canonicalize(std::env::temp_dir()).unwrap()));
        std::fs::remove_dir_all(&self.0).unwrap();
    } }

    #[test]
    fn media_authority_grants_only_canonical_media_files() {
        let fixture = Fixture::new();
        let allowed = fixture.file("media/写真 café.JPG");
        let outside = fixture.file("media-other/outside.jpg");
        let session = fixture.file("private/session.jpg");
        let upload = fixture.file("uploads/stage.jpg");
        let unsupported = fixture.file("media/session.db");
        let root = allowed.parent().unwrap();
        let paths: Vec<_> = [&allowed, &outside, &session, &upload, &unsupported].into_iter()
            .map(|path| path.to_string_lossy().into_owned()).collect();
        let mut granted = Vec::new();
        let result = grant_paths(root, &paths, |path| { granted.push(path.to_owned()); Ok(()) });
        assert_eq!(granted, [dunce::canonicalize(&allowed).unwrap()]);
        assert_eq!(result[&paths[0]].as_deref(), Some(dunce::canonicalize(&allowed).unwrap().to_string_lossy().as_ref()));
        for path in &paths[1..] { assert_eq!(result[path], None); }
        assert!(authorized_file(root, &fixture.0.join("media/../private/session.jpg").to_string_lossy()).is_err());
        assert!(authorized_file(root, "relative.jpg").is_err());
        assert!(authorized_file(root, &root.to_string_lossy()).is_err());
        assert_eq!(grant_paths(root, &[paths[0].clone()], |_| Err("denied".into()))[&paths[0]], None);
    }

    #[test]
    fn media_authority_rejects_private_roots_but_keeps_data_media_fallback() {
        let fixture = Fixture::new();
        let data = fixture.0.join("data");
        let config = fixture.0.join("config");
        let uploads = fixture.0.join("cache/uploads");
        let private = [data.clone(), config.clone(), uploads.clone()];
        let fallback = data.join("media");
        for path in [&fixture.0, &data, &config, &uploads, &data.join("accounts/one")] {
            assert!(checked_root(path, &private, &fallback).is_err(), "{path:?}");
        }
        for path in [fallback.clone(), fallback.join("avatars"), fixture.0.join("custom/new-folder"), fixture.0.join("cache/media")] {
            assert_eq!(checked_root(&path, &private, &fallback).unwrap(), path);
        }
        assert!(checked_root(Path::new("relative"), &private, &fallback).is_err());
        assert!(checked_root(&fallback, &[fallback.join("private-config")], &fallback).is_err());
    }

    #[test]
    fn media_authority_static_scope_contains_no_directory_grants() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["app"]["security"]["assetProtocol"]["scope"], serde_json::json!([]));
    }

    #[cfg(windows)]
    #[test]
    fn media_authority_denies_directory_junction_escapes() {
        use std::os::windows::process::CommandExt;
        let fixture = Fixture::new();
        let inside = fixture.file("media/inside.jpg");
        let outside = fixture.file("private/session.jpg");
        let root = inside.parent().unwrap();
        let link = root.join("escaped");
        let command = format!("New-Item -ItemType Junction -Path '{}' -Target '{}' -ErrorAction Stop | Out-Null",
            link.to_string_lossy().replace('\'', "''"), outside.parent().unwrap().to_string_lossy().replace('\'', "''"));
        let output = std::process::Command::new("powershell.exe").creation_flags(0x08000000)
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", &command]).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(authorized_file(root, &link.join("session.jpg").to_string_lossy()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn media_authority_denies_symlink_escapes() {
        let fixture = Fixture::new();
        let inside = fixture.file("media/inside.jpg");
        let outside = fixture.file("private/session.jpg");
        let root = inside.parent().unwrap();
        let link = root.join("escape.jpg");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(authorized_file(root, &link.to_string_lossy()).is_err());
        std::os::unix::fs::symlink(outside.parent().unwrap(), root.join("escaped")).unwrap();
        assert!(authorized_file(root, &root.join("escaped/session.jpg").to_string_lossy()).is_err());
        std::os::unix::fs::symlink(&inside, root.join("safe.jpg")).unwrap();
        assert_eq!(authorized_file(root, &root.join("safe.jpg").to_string_lossy()).unwrap(), inside);
    }
}
