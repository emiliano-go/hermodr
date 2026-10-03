use super::*;

#[cfg(test)]
#[path = "media_receive_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "media_receive_hash_tests.rs"]
mod hash_tests;

pub(super) async fn receive_media(
    client: &Client,
    media: &MediaInfo,
    directory: &Path,
    id: &str,
) -> Result<PathBuf> {
    receive_file(directory, id, &media.extension(), |writer| {
        super::media_download::download_file(client, media, writer)
    })
    .await
}

async fn receive_file<D, F>(
    directory: &Path,
    id: &str,
    extension: &str,
    download: D,
) -> Result<PathBuf>
where
    D: FnOnce(std::fs::File) -> F,
    F: std::future::Future<Output = Result<std::fs::File>>,
{
    let destination = super::media_download::media_path(directory, id, extension)?;
    let (temporary, writer) = super::media_download::download_target(directory).await?;
    let verified = download(writer).await?;
    drop(verified);
    tokio::fs::rename(&temporary.path, &destination).await?;
    Ok(destination)
}
