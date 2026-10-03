use super::*;
use crate::message_ref::MessageRef;
use anyhow::Context;
pub use crate::store::spaces::{CachedSpaceGroup, ResolvedSpaceItem, Space, SpaceAction, SpaceArchive,
    SpaceInboxFilters, SpaceItem, SpaceResolution, SpaceSelection, SpaceSnapshot, SpaceTarget};

const MAX_METADATA_BYTES: usize = 64 * 1024 * 1024;

fn decode_archive(json: &str) -> Result<SpaceArchive> {
    anyhow::ensure!(json.len() <= MAX_METADATA_BYTES, MessageRef::new("error.space_metadata_size")
        .with_param("max_bytes", serde_json::Number::from(MAX_METADATA_BYTES as u64)).with_param("actual_bytes", serde_json::Number::from(json.len() as u64)));
    let archive: SpaceArchive = serde_json::from_str(json).with_context(|| MessageRef::new("error.space_metadata_json_invalid"))?;
    anyhow::ensure!(archive.version == 1, MessageRef::new("error.space_metadata_version")
        .with_param("version", serde_json::Number::from(archive.version)).with_param("supported", serde_json::Number::from(1)));
    Ok(archive)
}

impl WhatsAppService {
    pub async fn spaces_snapshot(&self) -> Result<SpaceSnapshot> {
        self.store.run(MessageStore::spaces_snapshot).await
    }

    pub async fn spaces_action(&self, action: SpaceAction) -> Result<SpaceSnapshot> {
        self.store.run(move |store| store.spaces_action(action, unix_now())).await
    }

    pub async fn resolve_spaces(&self, selection: SpaceSelection) -> Result<SpaceResolution> {
        self.resolve_spaces_with_keywords(selection, std::collections::HashMap::new()).await
    }

    pub async fn resolve_spaces_with_keywords(&self, selection: SpaceSelection,
        keyword_counts: std::collections::HashMap<String, u32>) -> Result<SpaceResolution> {
        let aliases = self.aliases.all().await?;
        self.store.run(move |store| store.resolve_spaces_with_keywords(&selection, unix_now(), &aliases, &keyword_counts)).await
    }

    pub async fn space_group_catalog(&self) -> Result<Vec<CachedSpaceGroup>> {
        self.store.run(MessageStore::cached_space_groups).await
    }

    pub async fn export_space_metadata(&self) -> Result<String> {
        let archive = self.store.run(MessageStore::export_spaces).await?;
        let json = serde_json::to_string(&archive)?;
        anyhow::ensure!(json.len() <= MAX_METADATA_BYTES, MessageRef::new("error.space_metadata_size")
            .with_param("max_bytes", serde_json::Number::from(MAX_METADATA_BYTES as u64)).with_param("actual_bytes", serde_json::Number::from(json.len() as u64)));
        Ok(json)
    }

    pub async fn import_space_metadata(&self, json: &str) -> Result<SpaceSnapshot> {
        let archive = decode_archive(json)?;
        self.store.run(move |store| store.import_spaces(archive)).await
    }
}

#[cfg(test)]
#[path = "spaces_tests.rs"]
mod tests;
