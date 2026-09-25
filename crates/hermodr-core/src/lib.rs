//! Core of the Hermóðr native client.
//!
//! Hermóðr talks the WhatsApp multi-device protocol directly instead of
//! embedding WhatsApp Web. That removes the webview entirely and, more
//! importantly, makes history sync a decision this program gets to make.

pub mod history;
pub mod ogg;
pub mod service;
pub mod store;

use anyhow::Result;

pub use history::HistoryPolicy;
pub use service::{
    AdminReport, GroupInfo, InviteInfo, UserProfile, Participant, Profile, SearchResult, SendOptions, Service, ServiceConfig,
    ServiceEvent, VoiceNote,
};
pub use store::{
    ChatMarks, ChatRetention, ChatSummary, MessageStore, NewEvent, Retention, StoredMessage,
    ViewOnce,
};

/// Renders a pairing code as an SVG string for the UI to display.
///
/// Done here rather than in the frontend so the QR encoder is shared with the
/// terminal spike and the UI needs no JavaScript QR dependency.
pub fn qr_svg(data: &str) -> Result<String> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(data.as_bytes())?;
    Ok(code
        .render::<svg::Color<'_>>()
        .min_dimensions(240, 240)
        .dark_color(svg::Color("#e4e4e7"))
        .light_color(svg::Color("#111214"))
        .build())
}
