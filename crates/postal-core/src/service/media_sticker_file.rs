use super::*;
use std::io::{Read, Write};

#[cfg(test)]
#[path = "media_sticker_file_tests.rs"]
mod tests;

pub(super) const MAX_STICKER_BYTES: u64 = 64 * 1024 * 1024;

pub(super) fn read_sticker_file(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    check_size(file.metadata()?.len())?;
    let mut bytes = Vec::new();
    file.take(MAX_STICKER_BYTES + 1).read_to_end(&mut bytes)?;
    check_size(bytes.len() as u64)?;
    Ok(bytes)
}

fn check_size(length: u64) -> Result<()> {
    anyhow::ensure!(
        length <= MAX_STICKER_BYTES,
        "Sticker exceeds the 64 MiB conversion limit"
    );
    Ok(())
}

pub(super) fn decode_image(bytes: &[u8]) -> Result<image::DynamicImage> {
    check_size(bytes.len() as u64)?;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader.decode()?)
}

pub(super) struct PreparedSticker {
    pub(super) file: super::media_files::TemporaryFile,
    pub(super) dimensions: (u32, u32),
    pub(super) animated: bool,
    pub(super) thumbnail: Option<Vec<u8>>,
}

pub(super) fn prepare(bytes: Vec<u8>) -> Result<PreparedSticker> {
    check_size(bytes.len() as u64)?;
    let webp = sticker_webp(&bytes)
        .ok_or_else(|| anyhow::anyhow!("that file is not an image we can turn into a sticker"))?;
    drop(bytes);
    let dimensions = webp_dimensions(&webp).unwrap_or((512, 512));
    let animated = webp_is_animated(&webp);
    let thumbnail = sticker_png_thumbnail(&webp);
    let (file, mut writer) = super::media_files::TemporaryFile::download(&std::env::temp_dir())?;
    writer.write_all(&webp)?;
    drop(writer);
    drop(webp);
    Ok(PreparedSticker {
        file,
        dimensions,
        animated,
        thumbnail,
    })
}
