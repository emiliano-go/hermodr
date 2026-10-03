use postal_core::store::gallery::{GalleryCursor, GalleryFilter, GalleryPage};
use tauri::State;
use crate::{AppState, command_error::{CommandError, CommandResult}};
use std::sync::Arc;

#[tauri::command(async)]
pub(crate) async fn gallery_page(state: State<'_, AppState>, account: String, filter: GalleryFilter,
    cursor: Option<GalleryCursor>, limit: Option<u32>) -> CommandResult<GalleryPage> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let page = service.gallery_page(filter, cursor, limit.unwrap_or(60)).await.map_err(CommandError::from)?;
    let current = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if !Arc::ptr_eq(&service, &current) { return Err(CommandError::code("error.account_changed")); }
    Ok(page)
}
