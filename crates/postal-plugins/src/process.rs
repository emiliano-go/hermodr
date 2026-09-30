use crate::{
    manifest::Plugin,
    protocol::{self, Reply},
};
use anyhow::{bail, Context, Result};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::mpsc,
    task::JoinHandle,
};

pub(crate) struct Session {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<Vec<u8>>,
    stdout_task: JoinHandle<()>,
    stderr_task: JoinHandle<()>,
    id: String,
    capabilities: Vec<String>,
}

impl Session {
    pub fn spawn(plugin: &Plugin) -> Result<Self> {
        // Revalidate paths at each spawn, including after an idle unload.
        let current = Plugin::load(&plugin.directory)?;
        anyhow::ensure!(
            current.executable == plugin.executable
                && serde_json::to_value(&current.manifest)?
                    == serde_json::to_value(&plugin.manifest)?,
            "plugin changed; restart Postal and review its manifest"
        );
        let mut command = Command::new(&plugin.executable);
        command
            .current_dir(&plugin.directory)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            command.creation_flags(0x08000000);
            for key in ["SystemRoot", "WINDIR"] {
                if let Some(value) = std::env::var_os(key) {
                    command.env(key, value);
                }
            }
        }
        let mut child = command.spawn().context("spawn plugin")?;
        let input = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut stderr = BufReader::new(child.stderr.take().unwrap());
        let (sender, output) = mpsc::channel(16);
        let stdout_task = tokio::spawn(async move {
            while let Ok(Some(line)) = protocol::line(&mut stdout).await {
                if sender.send(line).await.is_err() {
                    break;
                }
            }
        });
        let id = plugin.manifest.id.clone();
        let stderr_id = id.clone();
        let stderr_task = tokio::spawn(async move {
            while let Ok(Some(line)) = protocol::line(&mut stderr).await {
                log::warn!(
                    "plugin {stderr_id} stderr: {}",
                    String::from_utf8_lossy(&line).escape_debug()
                );
            }
        });
        Ok(Self {
            child,
            input,
            output,
            stdout_task,
            stderr_task,
            id,
            capabilities: plugin.manifest.capabilities.clone(),
        })
    }

    pub async fn handshake(&mut self) -> Result<()> {
        let mut hello = serde_json::to_vec(&protocol::HostMessage::<serde_json::Value>::Hello {
            api_version: 1,
            capabilities: self.capabilities.clone(),
        })?;
        hello.push(b'\n');
        self.write(&hello).await?;
        loop {
            if let Reply::Ready { name } = self.next().await? {
                anyhow::ensure!(!name.trim().is_empty(), "empty ready name");
                return Ok(());
            }
        }
    }

    async fn write(&mut self, data: &[u8]) -> Result<()> {
        self.input.write_all(data).await?;
        self.input.flush().await?;
        Ok(())
    }

    pub async fn next(&mut self) -> Result<Reply> {
        loop {
            let data = tokio::select! {
                data = self.output.recv() => data.context("plugin closed stdout")?,
                status = self.child.wait() => bail!("plugin exited: {}", status?),
            };
            match serde_json::from_slice(&data) {
                Ok(Reply::Log { level, message }) => {
                    log::info!(
                        "plugin {} [{}]: {}",
                        self.id,
                        level.escape_debug(),
                        message.escape_debug()
                    );
                }
                Ok(Reply::Call { id }) => {
                    let mut response =
                        serde_json::to_vec(&protocol::HostMessage::<serde_json::Value>::Error {
                            id,
                            error: "host methods are not available in v1".into(),
                        })?;
                    response.push(b'\n');
                    self.write(&response).await?;
                }
                Ok(Reply::Event {}) => {
                    log::warn!("plugin {}: UI events are not supported", self.id)
                }
                Ok(reply) => return Ok(reply),
                Err(error) => log::warn!(
                    "plugin {}: invalid or oversized JSON line: {error}",
                    self.id
                ),
            }
        }
    }

    pub async fn deliver(
        &mut self,
        id: u64,
        bytes: &[u8],
        kind: Option<&crate::RequestKind>,
    ) -> Result<serde_json::Value> {
        self.write(bytes).await?;
        loop {
            match self.next().await? {
                Reply::Ack { seq } if kind.is_none() && seq == id => {
                    return Ok(serde_json::Value::Null)
                }
                Reply::Transcript {
                    id: reply_id,
                    provider: actual,
                    text,
                    language,
                } if matches!(kind, Some(crate::RequestKind::Transcribe(_))) && reply_id == id => {
                    anyhow::ensure!(
                        matches!(kind,Some(crate::RequestKind::Transcribe(provider)) if provider==&actual),
                        "transcript provider mismatch"
                    );
                    let transcript = crate::Transcript {
                        provider: actual,
                        text,
                        language,
                    };
                    transcript.validate()?;
                    return Ok(serde_json::to_value(transcript)?);
                }
                Reply::ModelInstalled {
                    id: reply_id,
                    filename,
                } if matches!(kind, Some(crate::RequestKind::InstallModel(_)))
                    && reply_id == id =>
                {
                    anyhow::ensure!(
                        matches!(kind,Some(crate::RequestKind::InstallModel(expected)) if expected==&filename),
                        "installed model filename mismatch"
                    );
                    return Ok(serde_json::Value::Null);
                }
                Reply::TranscribeError {
                    id: reply_id,
                    message,
                }
                | Reply::ModelError {
                    id: reply_id,
                    message,
                } if kind.is_some() && reply_id == id => {
                    anyhow::ensure!(message.len() <= 4096, "oversized transcription error");
                    bail!("transcription failed: {message}");
                }
                _ => log::warn!(
                    "plugin {}: unexpected reply while awaiting request {}",
                    self.id,
                    id
                ),
            }
        }
    }

    pub async fn cancel(&mut self, id: u64) -> Result<()> {
        self.write(format!("{{\"type\":\"cancel\",\"id\":{id}}}\n").as_bytes())
            .await
    }

    pub async fn shutdown(&mut self) {
        let graceful = async {
            self.write(b"{\"type\":\"shutdown\"}\n").await?;
            self.child.wait().await?;
            Ok::<_, anyhow::Error>(())
        };
        if !matches!(
            tokio::time::timeout(Duration::from_secs(2), graceful).await,
            Ok(Ok(()))
        ) {
            let _ = self.child.kill().await;
            let _ = self.child.wait().await;
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stdout_task.abort();
        self.stderr_task.abort();
    }
}
