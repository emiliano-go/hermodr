//! Reports what a reply that quotes a view-once actually carries, so the
//! platform's behaviour can be measured instead of assumed.
//!
//! WhatsApp sends a linked device a view-once with its media key but no address
//! to fetch the bytes from, so it can never be downloaded on its own. A reply
//! that quotes one is the only place a copy could arrive, and whether that copy
//! is complete decides if a quoted view-once can ever be taken back. This walks
//! the stored quotes and says, for each, whether a copy is present and whether
//! it has somewhere to fetch from.
//!
//! Usage: quote-probe <path to messages.db> [--all]
//!
//! Only quotes received by a build that records the payload have one; rows
//! written before that are reported as having no copy. The app need not be
//! running, but a payload only appears for a message delivered while a recording
//! build is running: open a chat and load older messages, or wait for a new
//! reply quoting a view-once.
use postal_core::store::MessageStore;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or(
        "usage: quote-probe <path to messages.db> [--all]\n\
         that file is the account's messages.db, under the app's data directory",
    )?;
    let show_all = args.any(|a| a == "--all");
    let store = MessageStore::open(
        std::path::Path::new(&path),
        postal_core::store::Retention::unlimited(),
    )?;

    let (mut total, mut mine, mut with_copy, mut fetchable) = (0u32, 0u32, 0u32, 0u32);
    for q in store.view_once_quotes()? {
        total += 1;
        let author_is_me = q.sender.as_deref() == Some("@me");
        if author_is_me {
            mine += 1;
        }
        // A direct_path, when there is one, is a URL. The mimetype is the only
        // other text in a media submessage, so this is enough to tell a
        // fetchable copy from a key with no location.
        let (copy, has_url) = match &q.locator {
            None => (false, false),
            Some(bytes) => {
                let text = String::from_utf8_lossy(bytes).to_lowercase();
                (true, text.contains("whatsapp.net") || text.contains("mmg."))
            }
        };
        if copy {
            with_copy += 1;
        }
        if has_url {
            fetchable += 1;
        }
        if !has_url && !show_all {
            continue;
        }
        if with_copy <= 25 {
            println!(
                "reply {} in {}\n  quotes {} written by {} ({})\n  gate {} · copy {} · CDN address {}\n  quoted row: kind {} · own locator {}",
                &q.id[..q.id.len().min(24)],
                q.chat,
                q.quoted.as_deref().unwrap_or("-"),
                q.sender.as_deref().unwrap_or("-"),
                if author_is_me { "you" } else { "them" },
                if q.allowed { "open" } else { "closed" },
                match &q.locator {
                    None => "none".to_string(),
                    Some(b) => format!("{} B", b.len()),
                },
                if has_url { "YES" } else { "no" },
                q.quoted_kind.as_deref().unwrap_or("-"),
                if q.quoted_stored { "yes" } else { "no" },
            );
        }
    }

    println!("\nreplies quoting a view-once: {total}");
    println!("  quoting one you sent:           {mine}");
    println!("  with a copy recorded:           {with_copy}");
    println!("  copy carrying a CDN address:    {fetchable}");
    if with_copy == 0 {
        println!(
            "\nNo copy yet, so nothing is settled. A copy is only recorded for a message\n\
             delivered while a recording build runs: open a chat, load older messages,\n\
             or wait for a new reply quoting a view-once, then run this again."
        );
    } else if fetchable == 0 {
        println!(
            "\nEvery copy is a key with no address, exactly like the view-once itself.\n\
             A quoted view-once cannot be downloaded from this device, and the recovery\n\
             path cannot work for anyone."
        );
    } else {
        println!(
            "\nA quoted view-once does carry a fetchable copy. The gate is then the only\n\
             thing deciding who may take it, and the recovery path works."
        );
    }
    Ok(())
}
