use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

pub(crate) fn read_bounded(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "plugin metadata exceeds size limit",
        ));
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum Activation {
    #[default]
    Eager,
    Lazy,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
    pub entrypoint: PathBuf,
    #[serde(default)]
    pub activation: Activation,
    pub idle_timeout_secs: Option<u64>,
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub contributes: Contributions,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct Contributions {
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcription: Option<crate::TranscriptionContribution>,
}

#[derive(Clone, Debug)]
pub(crate) struct Plugin {
    pub manifest: Manifest,
    pub directory: PathBuf,
    pub executable: PathBuf,
}

impl Manifest {
    fn validate_capabilities(&self) -> Result<()> {
        ensure!(
            self.capabilities == ["events:read"] || self.capabilities == ["transcribe"],
            "requires exactly events:read or transcribe"
        );
        ensure!(
            self.contributes.commands.is_empty(),
            "v1 does not support commands"
        );
        if self.capabilities == ["transcribe"] {
            ensure!(
                self.activation == Activation::Lazy,
                "transcription must activate lazily"
            );
            self.contributes
                .transcription
                .as_ref()
                .context("transcribe requires provider contributions")?
                .validate()?;
        } else {
            ensure!(
                self.contributes.transcription.is_none(),
                "transcription requires transcribe capability"
            );
        }
        Ok(())
    }
}

impl Plugin {
    pub fn load(directory: &Path) -> Result<Self> {
        let directory = directory.canonicalize()?;
        let path = directory.join("plugin.json").canonicalize()?;
        ensure!(
            path.starts_with(&directory),
            "manifest escapes plugin directory"
        );
        let manifest: Manifest = serde_json::from_slice(&read_bounded(&path, 65536)?)?;
        let parts: Vec<_> = manifest.id.split('.').collect();
        ensure!(
            parts.len() >= 3
                && manifest.id.len() <= 200
                && parts.iter().all(|p| {
                    !p.is_empty()
                        && p.as_bytes()[0].is_ascii_alphabetic()
                        && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                        && !p.ends_with('-')
                }),
            "id must be reverse-DNS"
        );
        ensure!(
            !manifest.name.trim().is_empty() && manifest.name.len() <= 200,
            "invalid name"
        );
        ensure!(
            !manifest.version.trim().is_empty() && manifest.version.len() <= 100,
            "invalid version"
        );
        ensure!(manifest.api_version == 1, "unsupported api_version");
        manifest.validate_capabilities()?;
        ensure!(
            manifest.idle_timeout_secs != Some(0),
            "idle timeout must be positive or null"
        );
        ensure!(
            manifest.idle_timeout_secs.unwrap_or(0) <= 86400,
            "idle timeout exceeds one day"
        );
        ensure!(
            !manifest.entrypoint.as_os_str().is_empty()
                && manifest
                    .entrypoint
                    .components()
                    .all(|c| matches!(c, Component::Normal(_))),
            "entrypoint must be a relative path without parent components"
        );
        let executable = directory
            .join(&manifest.entrypoint)
            .canonicalize()
            .context("entrypoint missing")?;
        if !executable.starts_with(&directory) || !executable.is_file() {
            bail!("entrypoint escapes plugin directory or is not a file");
        }
        Ok(Self {
            manifest,
            directory,
            executable,
        })
    }
}
