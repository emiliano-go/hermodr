use super::*;

fn png(image: DynamicImage) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn bytes(input: &MediaInput) -> Vec<u8> {
    match input {
        MediaInput::Bytes(bytes) => bytes.clone(),
        MediaInput::File(path) => std::fs::read(path).unwrap(),
    }
}

#[test]
fn standard_photo_bounds_dimensions_without_upscaling_and_preserves_alpha() {
    let large = png(DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        3200,
        1600,
        image::Rgb([25, 70, 210]),
    )));
    let (converted, extension) = standard_photo(&large).unwrap();
    assert_eq!(extension, "jpg");
    assert_eq!(image::guess_format(&converted).unwrap(), ImageFormat::Jpeg);
    let image = image::load_from_memory(&converted).unwrap();
    assert_eq!((image.width(), image.height()), (1600, 800));
    let small = png(DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        64,
        32,
        image::Rgba([20, 150, 200, 77]),
    )));
    let (converted, extension) = standard_photo(&small).unwrap();
    assert_eq!(extension, "png");
    let image = image::load_from_memory(&converted).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (64, 32));
    assert_eq!(image.get_pixel(10, 10).0, [20, 150, 200, 77]);
}

#[test]
fn standard_photo_applies_exif_orientation_before_encoding() {
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
        .encode_image(&image::RgbImage::from_pixel(
            30,
            20,
            image::Rgb([30, 120, 200]),
        ))
        .unwrap();
    let exif = [
        b'E', b'x', b'i', b'f', 0, 0, b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0,
        0, 6, 0, 0, 0, 0, 0, 0, 0,
    ];
    let mut segment = vec![0xff, 0xe1];
    segment.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
    segment.extend_from_slice(&exif);
    jpeg.splice(2..2, segment);
    let (converted, _) = standard_photo(&jpeg).unwrap();
    let image = image::load_from_memory(&converted).unwrap();
    assert_eq!((image.width(), image.height()), (20, 30));
}

#[tokio::test]
async fn hd_and_unspecified_quality_preserve_exact_bytes_and_paths() {
    let original = png(DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        40,
        20,
        image::Rgb([120, 210, 20]),
    )));
    for quality in [None, Some(MediaQuality::Hd)] {
        let prepared = prepare(
            MediaInput::Bytes(original.clone()),
            "photo.png".into(),
            "image",
            quality,
            false,
        )
        .await
        .unwrap();
        assert_eq!(prepared.file_name, "photo.png");
        assert_eq!(bytes(&prepared.input), original);
    }
    let (owned, mut file) = TemporaryFile::download(&std::env::temp_dir()).unwrap();
    file.write_all(&original).unwrap();
    drop(file);
    let prepared = prepare(
        MediaInput::File(owned.path.clone()),
        "photo.png".into(),
        "image",
        Some(MediaQuality::Hd),
        false,
    )
    .await
    .unwrap();
    assert!(matches!(&prepared.input, MediaInput::File(path) if path == &owned.path));
    assert_eq!(bytes(&prepared.input), original);
    assert!(prepared._temporary.is_empty());
}

#[tokio::test]
async fn unsupported_quality_and_invalid_images_fail_instead_of_passing_through() {
    let mut gif = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut gif);
        encoder
            .encode_frame(image::Frame::new(image::RgbaImage::from_pixel(
                2,
                2,
                image::Rgba([250, 0, 0, 255]),
            )))
            .unwrap();
        encoder
            .encode_frame(image::Frame::new(image::RgbaImage::from_pixel(
                2,
                2,
                image::Rgba([0, 0, 250, 255]),
            )))
            .unwrap();
    }
    assert!(standard_photo(&gif).is_err());
    for (name, kind, looping) in [
        ("movie.mp4", "video", true),
        ("image.gif", "image", false),
        ("note.ogg", "audio", false),
    ] {
        assert!(
            prepare(
                MediaInput::Bytes(vec![1]),
                name.into(),
                kind,
                Some(MediaQuality::Standard),
                looping
            )
            .await
            .is_err()
        );
    }
    assert!(standard_photo(b"not an image").is_err());
    let huge_header = png(DynamicImage::ImageRgba8(image::RgbaImage::new(8193, 1)));
    assert!(standard_photo(&huge_header).is_err());
}

fn video_backend_available(require_probe: bool) -> bool {
    let programs: &[&str] = if require_probe {
        &["ffmpeg", "ffprobe"]
    } else {
        &["ffmpeg"]
    };
    for program in programs {
        match hidden_command(program).arg("-version").status() {
            Ok(status) => assert!(
                status.success(),
                "{program} must report its version successfully"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!(
                    "Skipping real video quality checks: optional {program} is absent from PATH"
                );
                return false;
            }
            Err(error) => panic!("could not check {program}: {error}"),
        }
    }
    true
}

fn synthetic_video(size: &str) -> TemporaryFile {
    let (owned, file) = TemporaryFile::download(&std::env::temp_dir()).unwrap();
    drop(file);
    let mut command = hidden_command("ffmpeg");
    command
        .args(["-y", "-nostdin", "-loglevel", "error", "-f", "lavfi", "-i"])
        .arg(format!("color=c=red:s={size}:r=10"))
        .args([
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:sample_rate=44100",
            "-t",
            "0.3",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-threads",
            "2",
            "-c:a",
            "aac",
            "-f",
            "mp4",
        ])
        .arg(&owned.path);
    transcode(
        &mut command,
        &owned.path,
        Duration::from_secs(30),
        MAX_VIDEO_BYTES,
    )
    .expect("synthetic quality tests require ffmpeg with libx264 and AAC on PATH");
    owned
}

fn probe(path: &Path, stream: &str, fields: &str) -> String {
    let output = hidden_command("ffprobe")
        .args(["-v", "error", "-select_streams", stream, "-show_entries"])
        .arg(format!("stream={fields}"))
        .args(["-of", "csv=p=0"])
        .arg(path)
        .stdout(Stdio::piped())
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[tokio::test]
async fn standard_video_reencodes_real_dimensions_audio_and_owned_files() {
    if !video_backend_available(true) {
        return;
    }
    for (size, (source_width, source_height)) in
        [("1280x720", (1280, 720)), ("320x240", (320, 240))]
    {
        let source = synthetic_video(size);
        let original = std::fs::read(&source.path).unwrap();
        let hd = prepare(
            MediaInput::Bytes(original.clone()),
            "video.mp4".into(),
            "video",
            Some(MediaQuality::Hd),
            false,
        )
        .await
        .unwrap();
        assert_eq!(bytes(&hd.input), original);
        let prepared = prepare(
            MediaInput::File(source.path.clone()),
            "video.mov".into(),
            "video",
            Some(MediaQuality::Standard),
            false,
        )
        .await
        .unwrap();
        assert_eq!(prepared.file_name, "video.mp4");
        let MediaInput::File(output) = &prepared.input else {
            panic!("video conversion must remain file backed")
        };
        let output = output.clone();
        assert_ne!(output, source.path);
        let stream = probe(
            &output,
            "v:0",
            "codec_name,width,height,display_aspect_ratio",
        );
        let fields: Vec<_> = stream.split(',').collect();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0], "h264");
        let width: u32 = fields[1].parse().unwrap();
        let height: u32 = fields[2].parse().unwrap();
        assert_eq!(height, source_height.min(480));
        assert_eq!(width % 2, 0);
        assert_eq!(height % 2, 0);
        assert!(width <= source_width && height <= source_height);
        let proportional_width = source_width as f64 * height as f64 / source_height as f64;
        assert!((width as f64 - proportional_width).abs() <= 2.0);
        let aspect: Vec<f64> = fields[3]
            .split(':')
            .map(|value| value.parse().unwrap())
            .collect();
        assert_eq!(aspect.len(), 2);
        assert!(aspect[1] > 0.0);
        assert!(
            (aspect[0] / aspect[1] - source_width as f64 / source_height as f64).abs() < 0.000001
        );
        assert_eq!(probe(&output, "a:0", "codec_name"), "aac");
        assert_eq!(std::fs::read(&source.path).unwrap(), original);
        assert_ne!(bytes(&prepared.input), original);
        drop(prepared);
        assert!(!output.exists());
        assert!(source.path.exists());
    }
}

#[test]
fn video_failures_and_output_bounds_remain_errors() {
    let (owned, file) = TemporaryFile::download(&std::env::temp_dir()).unwrap();
    drop(file);
    assert!(
        transcode(
            &mut hidden_command("postal-nonexistent-transcoder"),
            &owned.path,
            Duration::ZERO,
            1
        )
        .is_err()
    );
    if !video_backend_available(false) {
        return;
    }
    let mut invalid = hidden_command("ffmpeg");
    invalid
        .args(["-nostdin", "-loglevel", "error", "-i"])
        .arg(&owned.path)
        .arg(&owned.path);
    assert!(
        transcode(
            &mut invalid,
            &owned.path,
            Duration::from_secs(10),
            MAX_VIDEO_BYTES
        )
        .is_err()
    );
    std::fs::write(&owned.path, [0, 1]).unwrap();
    let mut running = hidden_command("ffmpeg");
    running.args(["-nostdin", "-f", "lavfi", "-i", "color", "-f", "null", "-"]);
    assert!(transcode(&mut running, &owned.path, Duration::from_secs(5), 1).is_err());
    let mut timeout = hidden_command("ffmpeg");
    timeout.args(["-nostdin", "-f", "lavfi", "-i", "color", "-f", "null", "-"]);
    assert!(transcode(&mut timeout, &owned.path, Duration::ZERO, MAX_VIDEO_BYTES).is_err());
}
