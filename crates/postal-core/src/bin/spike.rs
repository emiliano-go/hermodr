//! Phase 0 spike: prove what `whatsapp-rust` can do before committing to a rewrite.
//!
//! Goals, in order:
//!   1. Build against stable Rust (no nightly `simd` feature).
//!   2. Pair by QR against a real account.
//!   3. Refuse the initial history sync, then measure memory.
//!   4. Send and receive a text message.
//!
//! Run with: `cargo run -p postal-core --bin spike`
//!
//! Set `SPIKE_HISTORY=accept` to allow the initial history sync, so the
//! difference between the two modes can be measured.

use std::{env, time::Duration};

use anyhow::Result;
use postal_core::HistoryPolicy;
use qrcode::render::unicode;
use qrcode::QrCode;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::types::events::Event;

/// Pure Rust, no Go toolchain. `whatsapp-rust` persists protocol/crypto state
/// only; message history is never stored unless we choose to.
fn session_path() -> String {
    // `SqliteStore` takes a filesystem path (or a `sqlite://` URL); a bare
    // `sqlite:` prefix is treated as part of the filename.
    env::args()
        .nth(1)
        .unwrap_or_else(|| "spike.db".to_string())
}

/// Whether to accept the deep history sync.
///
/// Default is to refuse it: that is what made the v1 web client pull the
/// account's entire history into a WebKit heap.
fn accept_full_history() -> bool {
    matches!(
        env::var("SPIKE_HISTORY").as_deref(),
        Ok("accept") | Ok("1") | Ok("true")
    )
}

/// Reads this process's resident set size, so memory is reported rather than assumed.
fn rss_mb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

fn report_rss(phase: &str) {
    match rss_mb() {
        Some(mb) => println!("[spike] RSS after {phase}: {mb} MB"),
        None => println!("[spike] RSS after {phase}: unavailable"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let db = session_path();
    let full_history = accept_full_history();

    println!("[spike] session: {db}");
    println!(
        "[spike] deep history: {}",
        if full_history {
            "ACCEPTED (SPIKE_HISTORY set)"
        } else {
            "REFUSED (default): recent window and on-demand only"
        }
    );
    report_rss("startup");

    let policy = if full_history {
        HistoryPolicy::accept_everything()
    } else {
        HistoryPolicy::default()
    };

    let bot = Bot::builder()
        .with_backend(SqliteStore::new(&db).await?)
        .with_history_sync_admission(policy)
        .on_qr_code(|code, _timeout| async move {
            // Render as half-block Unicode so the QR is scannable straight from
            // the terminal, with no system dependency.
            match QrCode::new(code.as_bytes()) {
                Ok(qr) => {
                    let image = qr
                        .render::<unicode::Dense1x2>()
                        .dark_color(unicode::Dense1x2::Light)
                        .light_color(unicode::Dense1x2::Dark)
                        .quiet_zone(true)
                        .build();
                    println!("\n[spike] Scan to pair:\n\n{image}\n");
                }
                Err(e) => println!("[spike] failed to render QR: {e}\n{code}"),
            }
        })
        .on_connected(|_client| async {
            println!("[spike] connected");
            report_rss("connect");
        })
        .on_event_for(
            &[EventKind::Messages, EventKind::PairSuccess],
            |event, _client| async move {
                match event.as_ref() {
                    Event::Messages(batch) => {
                        for inbound in batch.messages.iter() {
                            let text = inbound
                                .message
                                .text_content()
                                .unwrap_or("<non-text>")
                                .to_string();
                            println!("[spike] message from {}: {text}", inbound.info.source.sender);
                        }
                    }
                    Event::PairSuccess(_) => println!("[spike] PAIRED successfully"),
                    _ => {}
                }
            },
        )
        .build()
        .await?;

    // Report growth while idle. With history refused these numbers should stay flat.
    tokio::spawn(async {
        for i in 1..=6 {
            tokio::time::sleep(Duration::from_secs(10)).await;
            report_rss(&format!("{}s idle", i * 10));
        }
    });

    bot.run().await;
    Ok(())
}
