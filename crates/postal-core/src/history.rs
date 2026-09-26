//! History-sync policy.
//!
//! WhatsApp offers the whole account history during pairing. On this account
//! that is over 20 GB, and on the v1 web client it was pulled straight into a
//! WebKit heap. Here the decision is made before any of it is downloaded.
//!
//! The admission hook only sees *chunk metadata*, never message timestamps, so
//! the policy works at the granularity WhatsApp itself provides: it keeps the
//! recent window (which is where current conversations live) and refuses the
//! deep `FULL` history. Anything older that is actually needed is fetched later
//! on demand, when a chat is opened.

use whatsapp_rust::{
    HistorySyncAdmission, HistorySyncDecision, HistorySyncMetadata,
};

/// History sync types, from `HistorySyncType` in `whatsapp.proto`.
///
/// These are the raw `i32` values the admission hook reports.
mod sync_type {
    pub const INITIAL_BOOTSTRAP: i32 = 0;
    pub const INITIAL_STATUS_V3: i32 = 1;
    pub const FULL: i32 = 2;
    pub const RECENT: i32 = 3;
    pub const PUSH_NAME: i32 = 4;
    pub const NON_BLOCKING_DATA: i32 = 5;
    pub const ON_DEMAND: i32 = 6;
}

/// Decides which history-sync chunks are accepted during pairing.
#[derive(Debug, Clone, Default)]
pub struct HistoryPolicy {
    /// Accept the deep `FULL` sync. Defaults to `false`: that is the 20 GB case.
    pub accept_full_history: bool,
}

impl HistoryPolicy {
    /// Keep everything, matching the default WhatsApp Web behaviour.
    pub fn accept_everything() -> Self {
        Self {
            accept_full_history: true,
        }
    }
}

impl HistoryPolicy {
    /// Classifies a chunk from its sync type.
    ///
    /// Split out from [`HistorySyncAdmission::decide`] because
    /// `HistorySyncMetadata` is `#[non_exhaustive]` and cannot be constructed
    /// outside the library, which would otherwise make the policy untestable.
    fn classify(&self, sync_type: Option<i32>) -> HistorySyncDecision {
        // A missing sync type means the chunk cannot be classified; refusing is
        // the safer default because the rejection is permanent, and an unknown
        // chunk is more likely to be bulk history than live traffic.
        let Some(sync_type) = sync_type else {
            return HistorySyncDecision::RejectAndAcknowledge;
        };

        match sync_type {
            // The recent window: current conversations, and what we want.
            sync_type::RECENT => HistorySyncDecision::Accept,

            // Deep history. Refused by default.
            sync_type::FULL => {
                if self.accept_full_history {
                    HistorySyncDecision::Accept
                } else {
                    HistorySyncDecision::RejectAndAcknowledge
                }
            }

            // Contacts, push names, and similar small metadata: needed to make
            // the chat list usable, and negligible in size.
            sync_type::PUSH_NAME | sync_type::NON_BLOCKING_DATA => HistorySyncDecision::Accept,

            // The initial bootstrap carries account setup, not bulk messages.
            sync_type::INITIAL_BOOTSTRAP | sync_type::INITIAL_STATUS_V3 => {
                HistorySyncDecision::Accept
            }

            // Fetching an old conversation the user opened. Always allowed:
            // this is the mechanism that replaces bulk history.
            sync_type::ON_DEMAND => HistorySyncDecision::Accept,

            // Unknown/newer types: refuse rather than risk pulling bulk data.
            _ => HistorySyncDecision::RejectAndAcknowledge,
        }
    }
}

impl HistorySyncAdmission for HistoryPolicy {
    fn decide(&self, metadata: &HistorySyncMetadata<'_>) -> HistorySyncDecision {
        self.classify(metadata.sync_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decide(policy: &HistoryPolicy, sync_type: i32) -> HistorySyncDecision {
        policy.classify(Some(sync_type))
    }

    #[test]
    fn refuses_bulk_history_by_default() {
        let policy = HistoryPolicy::default();
        assert_eq!(
            decide(&policy, sync_type::FULL),
            HistorySyncDecision::RejectAndAcknowledge
        );
    }

    #[test]
    fn accepts_recent_window() {
        assert_eq!(
            decide(&HistoryPolicy::default(), sync_type::RECENT),
            HistorySyncDecision::Accept
        );
    }

    #[test]
    fn accepts_metadata_and_on_demand() {
        let policy = HistoryPolicy::default();
        for sync_type in [
            sync_type::PUSH_NAME,
            sync_type::NON_BLOCKING_DATA,
            sync_type::ON_DEMAND,
        ] {
            assert_eq!(decide(&policy, sync_type), HistorySyncDecision::Accept);
        }
    }

    #[test]
    fn opt_in_accepts_bulk_history() {
        assert_eq!(
            decide(&HistoryPolicy::accept_everything(), sync_type::FULL),
            HistorySyncDecision::Accept
        );
    }

    #[test]
    fn refuses_unclassified_chunks() {
        assert_eq!(
            HistoryPolicy::default().classify(None),
            HistorySyncDecision::RejectAndAcknowledge
        );
    }
}
