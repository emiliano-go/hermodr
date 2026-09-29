//! Playback preparation: turns stored audio into PCM WAV.
//!
//! Every web view plays PCM WAV without optional codecs — Chromium, WebKitGTK
//! (whose GStreamer stack ships without AAC on a plain install) and
//! AVFoundation alike — so decoding happens here instead. The decoders are
//! pure Rust, which keeps Windows, Linux and macOS identical and free of
//! system dependencies.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};

use super::*;

/// The subdirectory of the media folder holding converted playback files.
pub(super) const PLAYABLE_DIR: &str = "playable";

/// Interleaved samples and their shape, ready for the WAV writer.
struct Pcm {
    rate: u32,
    channels: usize,
    samples: Vec<f32>,
}

/// Where a source file's WAV conversion lives, under the media folder.
pub(super) fn playable_path(media_dir: &Path, source: &Path) -> PathBuf {
    let stem = source.file_stem().unwrap_or_default().to_string_lossy();
    media_dir.join(PLAYABLE_DIR).join(format!("{stem}.wav"))
}

/// Whether a path is inside the playable conversion folder.
pub(super) fn is_playable(root: &Path, path: &Path) -> bool {
    path.parent() == Some(root.join(PLAYABLE_DIR).as_path())
}

/// Whether the file already is PCM WAV, which no web view needs help with.
fn is_wav(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 12];
    let Ok(mut file) = std::fs::File::open(path) else { return false };
    file.read_exact(&mut head).is_ok() && &head[..4] == b"RIFF" && &head[8..] == b"WAVE"
}

/// Reads a WAV file's shape and interleaved samples, for the tests.
#[cfg(test)]
fn decode(path: &Path) -> Result<Pcm> {
    let bytes = std::fs::read(path).with_context(|| format!("{} is not readable", path.display()))?;
    decode_bytes(&bytes, path.extension().and_then(|e| e.to_str()))
}

/// Decodes one audio file by its container; the extension is a probe hint only.
fn decode_bytes(bytes: &[u8], extension: Option<&str>) -> Result<Pcm> {
    if bytes.starts_with(b"OggS") && opus_head(bytes) {
        return decode_ogg_opus(bytes);
    }
    decode_symphonia(bytes, extension)
}

/// Whether the first Ogg page announces an Opus stream.
fn opus_head(bytes: &[u8]) -> bool {
    bytes.get(..128.min(bytes.len())).is_some_and(|head| head.windows(8).any(|w| w == b"OpusHead"))
}

/// Ogg Opus, the shape WhatsApp voice notes take. The reader applies the
/// stream's pre-skip and end-trim, which the raw packets do not carry.
fn decode_ogg_opus(bytes: &[u8]) -> Result<Pcm> {
    use opus_pure::{OggOpusReader, Trim, MAX_PACKET_SAMPLES};
    let rate = 48_000i32;
    let mut reader = OggOpusReader::new(std::io::Cursor::new(bytes))
        .map_err(|e| anyhow::anyhow!("not an Ogg Opus stream: {e}"))?;
    let head = reader.head().clone();
    let channels = head.channel_count as usize;
    anyhow::ensure!(channels > 0, "an Opus stream without channels");
    let mut decoder = head
        .decoder(rate)
        .map_err(|e| anyhow::anyhow!("cannot open the Opus stream: {e}"))?;
    let mut trim = Trim::new(&head, rate, channels)
        .map_err(|e| anyhow::anyhow!("cannot trim the Opus stream: {e}"))?;
    let mut samples = Vec::new();
    let mut block = vec![0.0f32; MAX_PACKET_SAMPLES * channels];
    for packet in reader.packets() {
        let packet = packet.map_err(|e| anyhow::anyhow!("broken Ogg page: {e}"))?;
        let decoded = decoder
            .decode(&packet.data, MAX_PACKET_SAMPLES, &mut block)
            .map_err(|e| anyhow::anyhow!("cannot decode an Opus packet: {e}"))?;
        samples.extend_from_slice(trim.keep(&packet, &block[..decoded * channels]));
    }
    Ok(Pcm { rate: rate as u32, channels, samples })
}

/// Everything else, by container and codec (AAC/MP4, MP3, Vorbis, FLAC, ...).
fn decode_symphonia(bytes: &[u8], extension: Option<&str>) -> Result<Pcm> {
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::errors::Error as SymphoniaError;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;

    let stream = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(bytes.to_vec())),
        Default::default(),
    );
    let mut hint = Hint::new();
    if let Some(extension) = extension {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| anyhow::anyhow!("unrecognized audio format: {e}"))?;
    let track = format
        .default_track(TrackType::Audio)
        .context("the file has no audio track")?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .context("the file has no audio codec parameters")?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| anyhow::anyhow!("no decoder for this audio: {e}"))?;
    let mut shape: Option<(u32, usize)> = None;
    let mut samples: Vec<f32> = Vec::new();
    while let Some(packet) = format
        .next_packet()
        .map_err(|e| anyhow::anyhow!("cannot read the audio stream: {e}"))?
    {
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = decoded.spec();
                shape.get_or_insert((spec.rate(), spec.channels().count()));
                let start = samples.len();
                samples.resize(start + decoded.samples_interleaved(), 0.0);
                decoded.copy_to_slice_interleaved(&mut samples[start..]);
            }
            // A damaged frame is skipped; the rest of the file still plays.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(anyhow::anyhow!("cannot decode the audio: {e}")),
        }
    }
    let (rate, channels) = shape.context("the file holds no decodable audio")?;
    Ok(Pcm { rate, channels, samples })
}

/// Writes 16-bit PCM WAV; more than two channels are folded down to stereo.
fn write_wav(pcm: &Pcm, path: &Path) -> Result<()> {
    let channels = pcm.channels.min(2).max(1);
    let mut samples = Vec::with_capacity(pcm.samples.len());
    for frame in pcm.samples.chunks(pcm.channels.max(1)) {
        for &sample in frame.iter().take(channels) {
            samples.push((sample.clamp(-1.0, 1.0) * 32767.0) as i16);
        }
    }
    let data_len = (samples.len() * 2) as u32;
    let block_align = (channels * 2) as u16;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(channels as u16).to_le_bytes());
    out.extend_from_slice(&pcm.rate.to_le_bytes());
    out.extend_from_slice(&(pcm.rate * block_align as u32).to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, out).with_context(|| format!("cannot write {}", path.display()))
}

impl WhatsAppService {
    /// A path the web view can play: the source itself when it already is PCM
    /// WAV, otherwise a cached WAV converted from it. When conversion fails,
    /// the original comes back so a platform that can still play it gets a
    /// chance.
    pub async fn playable_audio(&self, path: &str) -> Result<String> {
        let media_dir = self.media_dir.clone().context("no media folder is configured")?;
        let root = media_dir.canonicalize().with_context(|| format!("media folder {} is missing", media_dir.display()))?;
        let source = Path::new(path).canonicalize().with_context(|| format!("{path} is not readable"))?;
        anyhow::ensure!(source.starts_with(&root) && source.is_file(), "only downloaded media can be played");
        let source_string = source.to_string_lossy().into_owned();
        if is_wav(&source) {
            return Ok(source_string);
        }
        let destination = playable_path(&root, &source);
        if destination.is_file() {
            return Ok(destination.to_string_lossy().into_owned());
        }
        // Concurrent first plays each decode; the rename picks one winner.
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        let temp = destination.with_extension(format!("part{}", NEXT_TEMP.fetch_add(1, Ordering::Relaxed)));
        let conversion = tokio::task::spawn_blocking({
            let source = source.clone();
            let destination = destination.clone();
            let temp = temp.clone();
            move || -> Result<()> {
                let bytes = std::fs::read(&source)?;
                let pcm = decode_bytes(&bytes, source.extension().and_then(|e| e.to_str()))?;
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                write_wav(&pcm, &temp)?;
                match std::fs::rename(&temp, &destination) {
                    Ok(()) => Ok(()),
                    // Another play finished first; its file is just as good.
                    Err(_) if destination.is_file() => {
                        let _ = std::fs::remove_file(&temp);
                        Ok(())
                    }
                    Err(e) => Err(e.into()),
                }
            }
        })
        .await;
        match conversion {
            Ok(Ok(())) => Ok(destination.to_string_lossy().into_owned()),
            Ok(Err(e)) => {
                log::warn!("could not prepare {source_string} for playback: {e:#}");
                let _ = std::fs::remove_file(&temp);
                Ok(source_string)
            }
            Err(e) => Err(anyhow::anyhow!("the audio conversion task failed: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One second of a 440 Hz tone as an Ogg Opus file, encoded in memory.
    fn ogg_opus_tone() -> Vec<u8> {
        use opus_pure::{Application, OggOpusWriter, OpusEncoder, OpusHead, MAX_PACKET_BYTES};
        const RATE: i32 = 48_000;
        const FRAME: usize = 960;
        let mut encoder = OpusEncoder::new(RATE, 1, Application::Audio).unwrap();
        let head = OpusHead::for_encoder(&encoder, RATE as u32);
        let mut writer = OggOpusWriter::new(Vec::new(), head).unwrap();
        let mut packet = vec![0u8; MAX_PACKET_BYTES];
        for i in 0..50 {
            let block: Vec<f32> = (0..FRAME)
                .map(|n| {
                    let t = (i * FRAME + n) as f32 / RATE as f32;
                    (t * 440.0 * std::f32::consts::TAU).sin() * 0.5
                })
                .collect();
            let size = encoder.encode(&block, FRAME, &mut packet).unwrap();
            writer.write_packet(&packet[..size]).unwrap();
        }
        writer.finish().unwrap()
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |top, s| top.max(s.abs()))
    }

    #[test]
    fn an_ogg_opus_tone_decodes_to_audible_pcm() {
        let pcm = decode_bytes(&ogg_opus_tone(), Some("ogg")).unwrap();
        assert_eq!(pcm.rate, 48_000);
        assert_eq!(pcm.channels, 1);
        // One second, less the codec's algorithmic delay.
        let seconds = pcm.samples.len() as f32 / pcm.rate as f32;
        assert!((0.8..=1.05).contains(&seconds), "{seconds}s of samples");
        assert!(peak(&pcm.samples) > 0.1, "the tone came out silent");
    }

    #[test]
    fn an_aac_file_decodes_to_audible_pcm() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tone.m4a");
        let pcm = decode(&fixture).unwrap();
        assert_eq!(pcm.rate, 44_100);
        assert_eq!(pcm.channels, 1);
        let seconds = pcm.samples.len() as f32 / pcm.rate as f32;
        assert!((0.9..=1.1).contains(&seconds), "{seconds}s of samples");
        assert!(peak(&pcm.samples) > 0.1, "the tone came out silent");
        // Files saved before the mimetype fix carry an `.ogg` name over MP4
        // bytes; the hint must not throw the prober off.
        let bytes = std::fs::read(&fixture).unwrap();
        let misnamed = decode_bytes(&bytes, Some("ogg")).unwrap();
        assert_eq!(misnamed.rate, 44_100);
        assert!(peak(&misnamed.samples) > 0.1);
    }

    #[test]
    fn a_wav_round_trips_through_the_writer() {
        let pcm = Pcm { rate: 8_000, channels: 1, samples: vec![0.0, 0.5, -0.5, 0.0] };
        let dir = std::env::temp_dir().join(format!("postal-wav-{}-{}", std::process::id(), NEXT_ID.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        write_wav(&pcm, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 8_000);
        assert_eq!(u16::from_le_bytes(bytes[34..36].try_into().unwrap()), 16);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 8);
        let read = decode(&path).unwrap();
        assert_eq!(read.rate, 8_000);
        assert_eq!(read.samples.len(), 4);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn something_that_is_not_audio_is_refused() {
        assert!(decode_bytes(b"not an audio file at all", None).is_err());
    }

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
}
