use super::*;

/// The media carried by a message, if any.
pub(super) struct MediaInfo {
    /// `image`, `video`, `gif`, `audio` or `document`. A video sent with
    /// `gifPlayback` is a GIF, which WhatsApp keeps as a muted looping clip.
    pub(super) kind: &'static str,
    media_type: MediaType,
    /// The four media protos are distinct types that each implement
    /// `Downloadable`.
    pub(super) downloadable: Box<dyn Downloadable + Send + Sync>,
    /// The small JPEG the message carries, available without downloading.
    pub(super) thumb: Option<Vec<u8>>,
    /// Audio length in seconds, when the message carries it.
    pub(super) duration: Option<u32>,
    /// A document's own extension, so the file opens as what it is.
    ext: Option<String>,
}

impl MediaInfo {
    pub(super) fn extension(&self) -> String {
        self.ext.clone().unwrap_or_else(|| extension_for(self.kind, self.media_type).to_string())
    }
}

/// The file extension an audio message's mimetype implies. WhatsApp sends
/// voice notes as Ogg Opus and forwarded audio as AAC/MP4, and saving one
/// under the other's name makes a forward re-send the wrong container.
fn audio_extension(mimetype: Option<&str>) -> Option<String> {
    let base = mimetype?.split(';').next()?.trim().to_ascii_lowercase();
    let extension = match base.as_str() {
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => "m4a",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/aac" | "audio/aacp" | "audio/x-aac" => "aac",
        "audio/wav" | "audio/wave" | "audio/x-wav" => "wav",
        "audio/ogg" | "audio/opus" => "ogg",
        _ => return None,
    };
    Some(extension.to_string())
}

/// A safe file extension from a document's name, or its MIME type for an SVG.
fn document_extension(document: &wa::message::DocumentMessage) -> Option<String> {
    let from_name = document
        .file_name
        .as_deref()
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| (1..=8).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()));
    from_name.or_else(|| (document.mimetype.as_deref() == Some("image/svg+xml")).then(|| "svg".to_string()))
}

pub(super) fn detect_media(message: &wa::Message) -> Option<MediaInfo> {
    // A Lottie sticker arrives in a future-proof envelope whose inner media is
    // what downloads. It is kept as a sticker; the UI draws a placeholder when
    // the file is not a raster image it can show.
    if let Some(wrapper) = message.lottie_sticker_message.as_option() {
        if let Some(inner) = wrapper.message.as_option() {
            if let Some(media) = detect_media(inner) {
                return Some(media);
            }
        }
    }
    if let Some(image) = message.image_message.as_option() {
        return Some(MediaInfo {
            kind: "image",
            media_type: MediaType::Image,
            downloadable: Box::new(image.clone()),
            thumb: image.jpeg_thumbnail.clone(),
            duration: None,
            ext: None,
        });
    }
    if let Some(video) = message.video_message.as_option().or(message.ptv_message.as_option()) {
        let kind = if message.ptv_message.is_set() && message.video_message.is_unset() {
            "round_video"
        } else if video.gif_playback.unwrap_or(false) {
            "gif"
        } else {
            "video"
        };
        return Some(MediaInfo {
            kind,
            media_type: MediaType::Video,
            downloadable: Box::new(video.clone()),
            thumb: video.jpeg_thumbnail.clone(),
            duration: (kind == "round_video").then_some(video.seconds).flatten(),
            ext: None,
        });
    }
    if let Some(audio) = message.audio_message.as_option() {
        return Some(MediaInfo {
            kind: "audio",
            media_type: MediaType::Audio,
            downloadable: Box::new(audio.clone()),
            thumb: None,
            duration: audio.seconds,
            ext: audio_extension(audio.mimetype.as_deref()),
        });
    }
    if let Some(document) = message.document_message.as_option() {
        return Some(MediaInfo {
            kind: "document",
            media_type: MediaType::Document,
            downloadable: Box::new(document.clone()),
            thumb: document.jpeg_thumbnail.clone(),
            duration: None,
            ext: document_extension(document),
        });
    }
    if let Some(sticker) = message.sticker_message.as_option() {
        return Some(MediaInfo {
            kind: "sticker",
            media_type: MediaType::Sticker,
            downloadable: Box::new(sticker.clone()),
            thumb: sticker.png_thumbnail.clone(),
            duration: None,
            ext: None,
        });
    }
    None
}

/// Nests a media message in the V2 view-once container.
///
/// The protocol library sends a caller-built message unchanged, so the container
/// is ours to build; its classification unwraps it for the stanza type and the
/// `<meta view_once="true"/>` hint, and recipients render what is inside as
/// one-time media.
pub(super) fn wrap_view_once(message: wa::Message) -> wa::Message {
    wa::Message {
        view_once_message_v2: MessageField::some(wa::message::FutureProofMessage {
            message: MessageField::some(message),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// A view-once of `kind` with no media in it, for quoting one this device never saw.
pub(super) fn empty_view_once(kind: Option<&str>) -> wa::Message {
    let inner = match kind {
        Some("round_video") => wa::Message {
            ptv_message: MessageField::some(wa::message::VideoMessage { view_once: Some(true), ..Default::default() }),
            ..Default::default()
        },
        Some("video") | Some("gif") => wa::Message {
            video_message: MessageField::some(wa::message::VideoMessage { view_once: Some(true), ..Default::default() }),
            ..Default::default()
        },
        Some("audio") => wa::Message {
            audio_message: MessageField::some(wa::message::AudioMessage {
                view_once: Some(true),
                ptt: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        },
        _ => wa::Message {
            image_message: MessageField::some(wa::message::ImageMessage { view_once: Some(true), ..Default::default() }),
            ..Default::default()
        },
    };
    wrap_view_once(inner)
}

/// The stored media kind for the type a view-once stub announces.
pub(super) fn once_kind_of(media: &whatsapp_rust::wacore::types::wire_enums::EncMediaType) -> String {
    use whatsapp_rust::wacore::types::wire_enums::EncMediaType as T;
    match media {
        T::Video | T::Ptv => "video",
        T::Gif => "gif",
        T::Audio | T::Ptt => "audio",
        _ => "image",
    }
    .to_string()
}

/// Where a chat's cached profile picture lives.
pub(super) fn avatar_path(media_dir: &Path, jid: &str) -> PathBuf {
    let name: String = jid
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    media_dir.join("avatars").join(format!("{name}.jpg"))
}

/// Where the full-size picture is cached, beside the preview.
pub(super) fn avatar_full_path(media_dir: &Path, jid: &str) -> PathBuf {
    let preview = avatar_path(media_dir, jid);
    let stem = preview.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    preview.with_file_name(format!("{stem}-full.jpg"))
}

/// File extension for a downloaded media item.
fn extension_for(kind: &str, media_type: MediaType) -> &'static str {
    match kind {
        "image" => "jpg",
        "video" | "round_video" | "gif" => "mp4",
        "audio" => "ogg",
        "sticker" => "webp",
        "document" => "bin",
        _ => match media_type {
            MediaType::Image => "jpg",
            _ => "bin",
        },
    }
}

/// MIME type for an outgoing attachment, from its file extension.
pub(super) fn mime_for(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" | "mov" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "ogg" | "opus" => "audio/ogg; codecs=opus",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "wav" => "audio/wav",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(mimetype: Option<&str>) -> wa::Message {
        wa::Message {
            audio_message: MessageField::some(wa::message::AudioMessage {
                mimetype: mimetype.map(str::to_string),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn audio_extensions_follow_the_mimetype() {
        assert_eq!(audio_extension(Some("audio/mp4")).as_deref(), Some("m4a"));
        assert_eq!(audio_extension(Some("audio/ogg; codecs=opus")).as_deref(), Some("ogg"));
        assert_eq!(audio_extension(Some("audio/mpeg")).as_deref(), Some("mp3"));
        assert_eq!(audio_extension(Some("audio/aac")).as_deref(), Some("aac"));
        assert_eq!(audio_extension(Some("audio/wav")).as_deref(), Some("wav"));
        assert_eq!(audio_extension(Some("audio/amr")), None);
        assert_eq!(audio_extension(None), None);
    }

    #[test]
    fn an_audio_message_keeps_its_container_as_extension() {
        assert_eq!(detect_media(&audio(Some("audio/mp4"))).unwrap().extension(), "m4a");
        assert_eq!(detect_media(&audio(Some("audio/ogg; codecs=opus"))).unwrap().extension(), "ogg");
        // Older PTT without a mimetype keeps the Ogg Opus default.
        assert_eq!(detect_media(&audio(None)).unwrap().extension(), "ogg");
    }
}
