use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use postal_plugins::{Transcript, TranscriptionConfig, TranscriptionRequest};
use reqwest::{multipart, Client};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt},
    process::Command,
};

const MAX_JSON: usize = 1024 * 1024;
const MAX_PCM: u64 = 20 * 1024 * 1024;
const MAX_MODEL: u64 = 256 * 1024 * 1024;

pub async fn line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> std::io::Result<Option<zeroize::Zeroizing<Vec<u8>>>> {
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    let mut oversized = false;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok((!bytes.is_empty() || oversized).then_some(bytes));
        }
        let newline = chunk.iter().position(|b| *b == b'\n');
        let count = newline.map_or(chunk.len(), |i| i + 1);
        if !oversized && bytes.len() + count <= MAX_JSON {
            bytes.extend_from_slice(&chunk[..count]);
        } else {
            oversized = true;
            bytes.clear();
        }
        reader.consume(count);
        if newline.is_some() {
            return Ok(Some(bytes));
        }
    }
}

fn client(timeout: Duration) -> Result<Client> {
    Ok(Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(timeout)
        .build()?)
}

fn data_directory(config: &TranscriptionConfig) -> Result<PathBuf> {
    let path = config
        .data_directory
        .as_ref()
        .context("plugin data directory required")?;
    ensure!(
        path.is_absolute() && path.is_dir(),
        "plugin data directory must exist and be absolute"
    );
    Ok(path.canonicalize()?)
}

fn executable(path: &Option<PathBuf>, label: &str) -> Result<PathBuf> {
    let path = path
        .as_ref()
        .with_context(|| format!("{label} executable required"))?;
    ensure!(
        path.is_absolute() && path.is_file(),
        "{label} executable must be an absolute file"
    );
    Ok(path.canonicalize()?)
}

fn checksum(path: &Path, expected: &str) -> Result<()> {
    ensure!(
        expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
        "model SHA-256 required"
    );
    let mut file = std::fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() <= MAX_MODEL,
        "model exceeds size limit"
    );
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let actual: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    ensure!(
        actual.eq_ignore_ascii_case(expected),
        "model checksum mismatch"
    );
    Ok(())
}

async fn run(
    mut command: Command,
    timeout: Duration,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<()> {
    ensure!(!*cancel.borrow(), "transcription cancelled");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .env_clear();
    #[cfg(windows)]
    {
        command.creation_flags(0x08000000);
        for key in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
    }
    let mut child = command
        .spawn()
        .context("cannot start configured audio engine")?;
    let status = tokio::select! {
        _ = cancel.changed() => None,
        status = tokio::time::timeout(timeout, child.wait()) => Some(status),
    };
    match status {
        Some(Ok(status)) => ensure!(status?.success(), "audio engine failed"),
        _ => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            bail!("audio engine cancelled or timed out");
        }
    }
    Ok(())
}

fn bounded_file(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "engine output exceeds size limit"
    );
    Ok(bytes)
}

async fn decode(
    request: &TranscriptionRequest,
    audio: &[u8],
    directory: &Path,
    timeout: Duration,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<PathBuf> {
    let input = directory.join("audio.ogg");
    let output = directory.join("audio.wav");
    std::fs::write(&input, audio)?;
    let mut command = Command::new(executable(&request.config.decoder_executable, "decoder")?);
    command
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(input)
        .args([
            "-map",
            "0:a:0",
            "-vn",
            "-t",
            "601",
            "-ar",
            "16000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            "-fs",
        ])
        .arg((MAX_PCM + 1).to_string())
        .arg(&output);
    run(command, timeout, cancel).await?;
    let bytes = bounded_file(&output, MAX_PCM)?;
    ensure!(
        bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"),
        "decoder did not return WAV"
    );
    ensure!(
        wav_duration(&bytes)? <= 600_000,
        "decoded audio exceeds ten minutes"
    );
    Ok(output)
}

pub fn wav_duration(bytes: &[u8]) -> Result<u64> {
    ensure!(
        bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"),
        "invalid WAV"
    );
    let mut offset = 12usize;
    let mut rate = None;
    let mut length = None;
    while offset.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into()?) as usize;
        let start = offset + 8;
        let end = start.checked_add(size).context("invalid WAV chunk")?;
        ensure!(end <= bytes.len(), "truncated WAV");
        match &bytes[offset..offset + 4] {
            b"fmt " => {
                ensure!(size >= 16, "invalid WAV format");
                ensure!(
                    u16::from_le_bytes(bytes[start..start + 2].try_into()?) == 1
                        && u16::from_le_bytes(bytes[start + 2..start + 4].try_into()?) == 1
                        && u32::from_le_bytes(bytes[start + 4..start + 8].try_into()?) == 16000
                        && u16::from_le_bytes(bytes[start + 14..start + 16].try_into()?) == 16,
                    "WAV must be mono 16 kHz PCM16"
                );
                let byte_rate = u32::from_le_bytes(bytes[start + 8..start + 12].try_into()?);
                ensure!(byte_rate == 32000, "invalid WAV byte rate");
                rate = Some(byte_rate);
            }
            b"data" => {
                ensure!(
                    length.is_none() && size > 0 && size % 2 == 0,
                    "invalid WAV data"
                );
                length = Some(size);
            }
            _ => {}
        }
        offset = end.checked_add(size % 2).context("invalid WAV padding")?;
    }
    Ok((length.context("WAV data missing")? as u64 * 1000)
        .div_ceil(rate.context("WAV format missing")? as u64))
}

pub fn parse_transcript(provider: &str, value: Value) -> Result<Transcript> {
    let (text, language) = match provider {
        "local-whisper" => {
            let segments = value["transcription"]
                .as_array()
                .context("Whisper transcript missing")?;
            let mut text = String::new();
            for segment in segments {
                text.push_str(
                    segment["text"]
                        .as_str()
                        .context("Whisper segment missing")?,
                );
            }
            (
                text,
                value["result"]["language"].as_str().map(str::to_owned),
            )
        }
        "openai" => (
            value["text"]
                .as_str()
                .context("OpenAI transcript missing")?
                .to_owned(),
            value["language"].as_str().map(str::to_owned),
        ),
        "deepgram" => {
            let channel = &value["results"]["channels"][0];
            (
                channel["alternatives"][0]["transcript"]
                    .as_str()
                    .context("Deepgram transcript missing")?
                    .to_owned(),
                channel["detected_language"].as_str().map(str::to_owned),
            )
        }
        _ => bail!("unsupported provider"),
    };
    ensure!(
        text.len() <= postal_plugins::transcription::MAX_TRANSCRIPT_BYTES,
        "transcript exceeds size limit"
    );
    ensure!(
        language.as_ref().is_none_or(|s| s.len() <= 64),
        "invalid transcript language"
    );
    Ok(Transcript {
        provider: provider.into(),
        text: text.trim().into(),
        language,
    })
}

async fn response(mut response: reqwest::Response) -> Result<Value> {
    ensure!(
        response.status().is_success(),
        "cloud provider HTTP {}",
        response.status().as_u16()
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= MAX_JSON,
            "cloud response exceeds size limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).context("invalid cloud response")
}

pub async fn cloud(
    client: &Client,
    url: &str,
    provider: &str,
    key: &str,
    audio: Vec<u8>,
    mime: &str,
    language: Option<&str>,
) -> Result<Transcript> {
    let mut request = client.post(url);
    if provider == "openai" {
        let part = multipart::Part::bytes(audio)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;
        let mut form = multipart::Form::new()
            .part("file", part)
            .text("model", "whisper-1")
            .text("response_format", "verbose_json");
        if let Some(language) = language {
            form = form.text("language", language.to_owned());
        }
        request = request.bearer_auth(key).multipart(form);
    } else {
        request = request
            .header("Authorization", format!("Token {key}"))
            .header("Content-Type", mime)
            .query(&[("model", "nova-3"), ("smart_format", "true")])
            .body(audio);
        request = if let Some(language) = language {
            request.query(&[("language", language)])
        } else {
            request.query(&[("detect_language", "true")])
        };
    }
    let reply = request
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("cloud request failed or timed out"))?;
    parse_transcript(provider, response(reply).await?)
}

pub async fn transcribe(request: TranscriptionRequest) -> Result<Transcript> {
    let (_keep_alive, cancel) = tokio::sync::watch::channel(false);
    transcribe_with_cancel(request, cancel).await
}

pub async fn transcribe_with_cancel(
    request: TranscriptionRequest,
    mut cancel: tokio::sync::watch::Receiver<bool>,
) -> Result<Transcript> {
    request.validate()?;
    ensure!(
        matches!(
            request.provider.as_str(),
            "local-whisper" | "openai" | "deepgram"
        ),
        "unsupported provider"
    );
    let cloud_provider = request.provider != "local-whisper";
    ensure!(
        !cloud_provider || request.config.cloud_consent,
        "explicit cloud provider consent required"
    );
    let key = request.config.api_key.as_deref().unwrap_or("");
    ensure!(
        !cloud_provider
            || (!key.trim().is_empty() && key.len() <= 4096 && !key.contains(['\r', '\n'])),
        "cloud provider key required"
    );
    let language = request.config.language.as_deref().filter(|s| !s.is_empty());
    ensure!(
        language.is_none_or(
            |s| s.len() <= 32 && s.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
        ),
        "invalid language"
    );
    let audio = STANDARD.decode(&request.audio)?;
    ensure!(
        if request.mime.contains("wav") {
            audio.starts_with(b"RIFF")
        } else {
            audio.starts_with(b"OggS")
        },
        "audio MIME does not match OGG/Opus or WAV"
    );
    let timeout = Duration::from_secs(request.config.timeout_secs.unwrap_or(300));
    if request.provider == "deepgram" {
        let http = client(timeout)?;
        return tokio::select! {
            _ = cancel.changed() => Err(anyhow::anyhow!("transcription cancelled")),
            result = cloud(&http,"https://api.deepgram.com/v1/listen","deepgram",key,audio,&request.mime,language) => result,
        };
    }
    let root = data_directory(&request.config)?;
    let directory = tempfile::Builder::new()
        .prefix("transcribe-")
        .tempdir_in(&root)?;
    let wav = decode(&request, &audio, directory.path(), timeout, &mut cancel).await?;
    if request.provider == "openai" {
        let http = client(timeout)?;
        return tokio::select! {
            _ = cancel.changed() => Err(anyhow::anyhow!("transcription cancelled")),
            result = cloud(&http,"https://api.openai.com/v1/audio/transcriptions","openai",key,bounded_file(&wav,MAX_PCM)?,"audio/wav",language) => result,
        };
    }
    let model = request
        .config
        .model
        .as_ref()
        .context("Whisper model required")?;
    let model = root
        .join(model)
        .canonicalize()
        .context("Whisper model missing")?;
    ensure!(
        model.starts_with(&root) && model.is_file(),
        "model escapes plugin data directory"
    );
    checksum(
        &model,
        request
            .config
            .model_sha256
            .as_deref()
            .context("model SHA-256 required")?,
    )?;
    let output = directory.path().join("transcript");
    let mut command = Command::new(executable(&request.config.whisper_executable, "Whisper")?);
    command
        .arg("-m")
        .arg(model)
        .arg("-f")
        .arg(wav)
        .args(["-l", language.unwrap_or("auto"), "-ng", "-oj", "-of"])
        .arg(&output);
    run(command, timeout, &mut cancel).await?;
    parse_transcript(
        "local-whisper",
        serde_json::from_slice(&bounded_file(
            &output.with_extension("json"),
            MAX_JSON as u64,
        )?)?,
    )
}

pub async fn install_model(
    url: &str,
    expected: &str,
    directory: &Path,
    filename: &str,
) -> Result<PathBuf> {
    ensure!(url.starts_with("https://"), "model download requires HTTPS");
    ensure!(
        Path::new(filename).components().count() == 1
            && matches!(
                Path::new(filename).components().next(),
                Some(Component::Normal(_))
            ),
        "invalid model filename"
    );
    ensure!(
        expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
        "model SHA-256 required"
    );
    std::fs::create_dir_all(directory)?;
    let directory = directory.canonicalize()?;
    let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
    let mut response = client(Duration::from_secs(600))?.get(url).send().await?;
    ensure!(response.status().is_success(), "model download failed");
    let mut length = 0u64;
    while let Some(chunk) = response.chunk().await? {
        length += chunk.len() as u64;
        ensure!(length <= MAX_MODEL, "model exceeds size limit");
        temporary.write_all(&chunk)?;
    }
    publish_model(temporary, expected, filename)
}

fn publish_model(
    mut temporary: tempfile::NamedTempFile,
    expected: &str,
    filename: &str,
) -> Result<PathBuf> {
    ensure!(
        Path::new(filename).components().count() == 1
            && matches!(
                Path::new(filename).components().next(),
                Some(Component::Normal(_))
            ),
        "invalid model filename"
    );
    temporary.flush()?;
    checksum(temporary.path(), expected)?;
    temporary.as_file().sync_all()?;
    let path = temporary
        .path()
        .parent()
        .context("model directory missing")?
        .join(filename);
    temporary.persist_noclobber(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_checksum_and_atomic_no_overwrite_publication() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = b"synthetic model";
        let hash: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let make = || {
            let mut f = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
            f.write_all(bytes).unwrap();
            f
        };
        assert!(publish_model(make(), &"0".repeat(64), "tiny.bin").is_err());
        assert!(!dir.path().join("tiny.bin").exists());
        let path = publish_model(make(), &hash, "tiny.bin").unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert!(publish_model(make(), &hash, "tiny.bin").is_err());
        assert!(publish_model(make(), &hash, "../outside.bin").is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
