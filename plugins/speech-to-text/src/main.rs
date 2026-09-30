use anyhow::{ensure, Result};
use postal_plugins::TranscriptionRequest;
use serde::Deserialize;
use serde_json::json;
use tokio::{
    io::{AsyncWriteExt, BufReader},
    sync::{mpsc, watch},
    task::JoinSet,
};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Message {
    Hello {
        api_version: u32,
        capabilities: Vec<String>,
    },
    Transcribe {
        id: u64,
        #[serde(flatten)]
        request: TranscriptionRequest,
    },
    InstallModel {
        id: u64,
        #[serde(flatten)]
        request: postal_plugins::transcription::ModelDownload,
    },
    Cancel {
        id: u64,
    },
    Shutdown,
}

async fn output(value: serde_json::Value) -> Result<()> {
    let mut stdout = tokio::io::stdout();
    let mut bytes = serde_json::to_vec(&value)?;
    bytes.push(b'\n');
    stdout.write_all(&bytes).await?;
    stdout.flush().await?;
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--manifest"] {
        let mut manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json"))?;
        manifest["entrypoint"] = json!(if cfg!(windows) {
            "postal-stt.exe"
        } else {
            "postal-stt"
        });
        println!("{}", serde_json::to_string_pretty(&manifest)?);
        return Ok(());
    }
    if !args.is_empty() {
        ensure!(
            args.len() == 5 && args[0] == "--install-model",
            "usage: postal-stt --install-model HTTPS_URL SHA256 DATA_DIRECTORY FILENAME"
        );
        postal_stt::install_model(&args[1], &args[2], std::path::Path::new(&args[3]), &args[4])
            .await?;
        return Ok(());
    }
    let (sender, mut input) = mpsc::channel(1);
    let reader = tokio::spawn(async move {
        let mut stdin = BufReader::new(tokio::io::stdin());
        while let Ok(Some(line)) = postal_stt::line(&mut stdin).await {
            if sender.send(line).await.is_err() {
                break;
            }
        }
    });
    let mut ready = false;
    let mut active: Option<(u64, watch::Sender<bool>)> = None;
    let mut tasks: JoinSet<(u64, bool, Result<serde_json::Value>)> = JoinSet::new();
    let mut closing = false;
    loop {
        tokio::select! {
            Some(result) = tasks.join_next(), if !tasks.is_empty() => {
                let (id, model, result) = result?;
                active = None;
                match result {
                    Ok(reply) => output(reply).await?,
                    Err(error) => output(json!({"type":if model{"model_error"}else{"transcribe_error"},"id":id,"message":error.to_string()})).await?,
                }
                if closing { break; }
            }
            incoming = input.recv(), if !closing => {
                let Some(line) = incoming else {
                    if let Some((_,cancel)) = &active { let _ = cancel.send(true); closing = true; }
                    else { break; }
                    continue;
                };
                let message: Message = match serde_json::from_slice(&line) {
                    Ok(message) => message,
                    Err(_) => { output(json!({"type":"log","level":"warn","message":"invalid or oversized request"})).await?; continue; }
                };
                match message {
                    Message::Hello { api_version:1, capabilities } if !ready && capabilities == ["transcribe"] => {
                        ready = true; output(json!({"type":"ready","name":"Speech to Text"})).await?;
                    }
                    Message::Shutdown => {
                        if let Some((_,cancel)) = &active { let _ = cancel.send(true); closing = true; }
                        else { break; }
                    }
                    Message::Cancel { id } if ready => {
                        if let Some((current,cancel)) = &active { if *current == id { let _ = cancel.send(true); } }
                    }
                    Message::Transcribe { id, request } if ready => {
                        if active.is_some() { output(json!({"type":"transcribe_error","id":id,"message":"request already active"})).await?; continue; }
                        let (cancel, cancelled) = watch::channel(false); active = Some((id,cancel));
                        tasks.spawn(async move { let result=postal_stt::transcribe_with_cancel(request,cancelled).await.map(|t|json!({"type":"transcript","id":id,"provider":t.provider,"text":t.text,"language":t.language})); (id,false,result) });
                    }
                    Message::InstallModel {id,request} if ready => {
                        if active.is_some(){output(json!({"type":"model_error","id":id,"message":"request already active"})).await?;continue;}
                        let (cancel,mut cancelled)=watch::channel(false);active=Some((id,cancel));
                        tasks.spawn(async move {
                            let result=async {request.validate()?;tokio::select! {
                                _=cancelled.changed()=>Err(anyhow::anyhow!("model installation cancelled")),
                                result=postal_stt::install_model(&request.url,&request.sha256,&request.data_directory,&request.filename)=>result.map(|_|json!({"type":"model_installed","id":id,"filename":request.filename})),
                            }}.await; (id,true,result)
                        });
                    }
                    Message::Transcribe { id, .. } => output(json!({"type":"transcribe_error","id":id,"message":"transcribe handshake required"})).await?,
                    _ => output(json!({"type":"log","level":"warn","message":"unsupported handshake or message"})).await?,
                }
            }
        }
    }
    reader.abort();
    Ok(())
}
