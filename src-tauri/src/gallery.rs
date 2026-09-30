use postal_core::store::gallery::{GalleryCursor, GalleryFilter, GalleryPage};
use tauri::State;
use crate::AppState;
use std::sync::Arc;

#[tauri::command(async)]
pub(crate) async fn gallery_page(state: State<'_, AppState>, account: String, filter: GalleryFilter,
    cursor: Option<GalleryCursor>, limit: Option<u32>) -> Result<GalleryPage, String> {
    let service = state.account_service(&account)?;
    let page = service.gallery_page(filter, cursor, limit.unwrap_or(60)).await.map_err(|error| error.to_string())?;
    let current = state.account_service(&account)?;
    if !Arc::ptr_eq(&service, &current) { return Err("account changed before operation".into()); }
    Ok(page)
}
