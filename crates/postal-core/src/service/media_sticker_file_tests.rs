use super::*;

fn image_bytes(format: image::ImageFormat) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(37, 29)
        .write_to(&mut bytes, format)
        .unwrap();
    bytes.into_inner()
}

#[test]
fn prepared_webp_preserves_bytes_metadata_and_cleans_up() {
    let bytes = image_bytes(image::ImageFormat::WebP);
    let thumbnail = sticker_png_thumbnail(&bytes);
    assert!(thumbnail.is_some());
    let prepared = prepare(bytes.clone()).unwrap();
    assert_eq!(prepared.dimensions, (37, 29));
    assert!(!prepared.animated);
    assert_eq!(prepared.thumbnail, thumbnail);
    let path = prepared.file.path.clone();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    drop(prepared);
    assert!(!path.exists());
}

#[test]
fn converted_sticker_keeps_existing_dimensions_and_preview() {
    let prepared = prepare(image_bytes(image::ImageFormat::Png)).unwrap();
    let bytes = std::fs::read(&prepared.file.path).unwrap();
    assert_eq!(
        image::guess_format(&bytes).unwrap(),
        image::ImageFormat::WebP
    );
    assert_eq!(prepared.dimensions, (512, 512));
    assert!(!prepared.animated);
    let preview = image::load_from_memory(prepared.thumbnail.as_deref().unwrap()).unwrap();
    assert_eq!((preview.width(), preview.height()), (256, 256));
}

#[test]
fn animated_webp_metadata_survives_file_staging_without_reencoding() {
    let mut bytes = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
    bytes.extend_from_slice(&10u32.to_le_bytes());
    bytes.extend_from_slice(&[2, 0, 0, 0, 0x40, 1, 0, 0xef, 0, 0]);
    bytes.extend_from_slice(b"ANIM");
    bytes.extend_from_slice(&6u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    let length = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&length.to_le_bytes());
    let prepared = prepare(bytes.clone()).unwrap();
    assert_eq!(prepared.dimensions, (321, 240));
    assert!(prepared.animated);
    assert_eq!(prepared.thumbnail, sticker_png_thumbnail(&bytes));
    assert_eq!(std::fs::read(&prepared.file.path).unwrap(), bytes);
}

#[test]
fn invalid_sticker_input_keeps_existing_conversion_error() {
    let error = prepare(b"not an image".to_vec()).err().unwrap();
    assert_eq!(
        error.to_string(),
        "that file is not an image we can turn into a sticker"
    );
}

#[test]
fn sticker_file_read_checks_size_before_allocating_contents() {
    let (temporary, mut file) =
        super::super::media_files::TemporaryFile::download(&std::env::temp_dir()).unwrap();
    file.write_all(b"small sticker input").unwrap();
    assert_eq!(
        read_sticker_file(&temporary.path).unwrap(),
        b"small sticker input"
    );
    file.set_len(MAX_STICKER_BYTES + 1).unwrap();
    assert_eq!(
        read_sticker_file(&temporary.path).unwrap_err().to_string(),
        "Sticker exceeds the 64 MiB conversion limit"
    );
    drop(file);
}

fn large_pixel_png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = image_bytes(image::ImageFormat::Png);
    bytes[16..20].copy_from_slice(&width.to_be_bytes());
    bytes[20..24].copy_from_slice(&height.to_be_bytes());
    let mut crc = u32::MAX;
    for byte in &bytes[12..29] {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    bytes[29..33].copy_from_slice(&(!crc).to_be_bytes());
    bytes
}

#[test]
fn sticker_conversion_and_preview_reject_large_pixel_headers() {
    for (width, height) in [(8193, 1), (1, 8193), (8192, 8192)] {
        let bytes = large_pixel_png(width, height);
        let error = decode_image(&bytes).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<image::ImageError>(),
            Some(image::ImageError::Limits(_))
        ));
        assert!(sticker_webp(&bytes).is_none());
        assert!(sticker_png_thumbnail(&bytes).is_none());
        assert!(prepare(bytes).is_err());
    }
}
