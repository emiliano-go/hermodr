//! Exercises the [`Service`] against a real account.
//!
//! Phase 1 verification: pair (or reuse a session), receive and store messages,
//! enforce retention, and send a reply. Unlike the spike, this goes through the
//! service layer the UI will use.
//!
//! Run with: `cargo run -p hermodr-core --bin service-check`
//!
//! Set `SPIKE_SEND=<chat-jid>` to send a message to that chat once connected.

use std::{env, path::PathBuf, time::Duration};

use anyhow::Result;
use hermodr_core::{Retention, Service, ServiceConfig, ServiceEvent};

#[tokio::main]
async fn main() -> Result<()> {
    let data_dir = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("service-data"));

    // A deliberately short window so retention is observable within a run.
    let retention = Retention {
        max_age_hours: Some(24),
        max_messages_per_chat: Some(20),
    };

    let config = ServiceConfig {
        session_path: data_dir.join("session.db"),
        messages_path: data_dir.join("messages.db"),
        retention,
        accept_full_history: false,
        auto_download_media: true,
        media_dir: Some(data_dir.join("media")),
    };

    println!("[check] data dir: {}", data_dir.display());
    println!(
        "[check] retention: {:?}h, {:?} msgs/chat",
        retention.max_age_hours, retention.max_messages_per_chat
    );

    let (service, mut events) = Service::start(config).await?;

    println!("[check] listening for events (Ctrl-C to stop)\n");

    let send_target = env::var("SPIKE_SEND").ok();
    let mut sent = false;
    let mut stats = Stats::default();

    loop {
        let event = match tokio::time::timeout(Duration::from_secs(15), events.recv()).await {
            Ok(Ok(event)) => event,
            // Lagged: the consumer fell behind, which is not fatal.
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                println!("[check] dropped {n} events (consumer too slow)");
                continue;
            }
            Ok(Err(_)) => break,
            Err(_) => {
                report(&service, &stats);
                continue;
            }
        };

        match event {
            ServiceEvent::QrCode { .. } => {
                println!("[check] QR issued; scan it (run the spike binary to see the image)");
            }
            ServiceEvent::Connected => {
                println!("[check] connected");
                if let Some(chat) = &send_target {
                    if !sent {
                        match service.send_text(chat, "test from hermodr-core", Vec::new()).await {
                            Ok(()) => println!("[check] sent test message to {chat}"),
                            Err(e) => println!("[check] send failed: {e}"),
                        }
                        sent = true;
                    }
                }
            }
            ServiceEvent::Disconnected => println!("[check] disconnected"),
            ServiceEvent::Message { message } => {
                stats.received += 1;
                println!("[check] stored [{}] {}: {}", message.header.chat, message.header.sender, message.text);
            }
            ServiceEvent::RetentionApplied { removed } => {
                stats.pruned += removed;
                println!("[check] retention removed {removed} message(s)");
            }
            ServiceEvent::NamesUpdated { count } => {
                println!("[check] address book: {count} saved name(s)");
            }
            ServiceEvent::Syncing { pending, applied } => {
                println!("[check] syncing {applied}/{pending} message(s)");
            }
            ServiceEvent::InitialSyncComplete { messages, chats } => {
                println!("[check] initial sync complete: {messages} message(s), {chats} chat(s)");
            }
            ServiceEvent::Synced => println!("[check] synced"),
            _ => {}
        }
    }

    Ok(())
}

#[derive(Default)]
struct Stats {
    received: usize,
    pruned: usize,
}

fn report(service: &Service, stats: &Stats) {
    match service.chats() {
        Ok(chats) => {
            println!(
                "[check] {} chat(s), {} received, {} pruned",
                chats.len(),
                stats.received,
                stats.pruned
            );
            for chat in chats.iter().take(5) {
                println!(
                    "         {} ({} msgs): {}",
                    chat.chat, chat.message_count, chat.last_text
                );
            }
        }
        Err(e) => println!("[check] failed to read chats: {e}"),
    }
}
