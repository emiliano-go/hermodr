use super::*;
use crate::store::gallery::{GalleryCursor, GalleryFilter, GalleryPage};

impl WhatsAppService {
    pub async fn gallery_page(&self, filter: GalleryFilter, cursor: Option<GalleryCursor>, limit: u32) -> Result<GalleryPage> {
        self.store.run(move |store| store.gallery_page(&filter, cursor.as_ref(), limit)).await
    }
}
