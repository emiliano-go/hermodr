//! History admission rejects deep or unknown chunks before downloading them.

use whatsapp_rust::{
    waproto::whatsapp::history_sync::HistorySyncType, HistorySyncAdmission, HistorySyncDecision,
    HistorySyncMetadata,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryKind {
    Bootstrap,
    Status,
    Full,
    Recent,
    PushNames,
    NonBlocking,
    OnDemand,
    Unknown,
}

impl From<Option<i32>> for HistoryKind {
    fn from(raw: Option<i32>) -> Self {
        match raw {
            Some(n) if n == HistorySyncType::INITIAL_BOOTSTRAP as i32 => Self::Bootstrap,
            Some(n) if n == HistorySyncType::INITIAL_STATUS_V3 as i32 => Self::Status,
            Some(n) if n == HistorySyncType::FULL as i32 => Self::Full,
            Some(n) if n == HistorySyncType::RECENT as i32 => Self::Recent,
            Some(n) if n == HistorySyncType::PUSH_NAME as i32 => Self::PushNames,
            Some(n) if n == HistorySyncType::NON_BLOCKING_DATA as i32 => Self::NonBlocking,
            Some(n) if n == HistorySyncType::ON_DEMAND as i32 => Self::OnDemand,
            _ => Self::Unknown,
        }
    }
}

/// Decides which history-sync chunks are accepted during pairing.
#[derive(Debug, Clone, Default)]
pub struct HistoryPolicy {
    /// Accept deep FULL history; off by default.
    pub accept_full_history: bool,
}

impl HistoryPolicy {
    pub fn accept_everything() -> Self {
        Self {
            accept_full_history: true,
        }
    }

    fn classify(&self, kind: HistoryKind) -> HistorySyncDecision {
        match kind {
            HistoryKind::Unknown => HistorySyncDecision::RejectAndAcknowledge,
            HistoryKind::Full if !self.accept_full_history => {
                HistorySyncDecision::RejectAndAcknowledge
            }
            HistoryKind::Bootstrap
            | HistoryKind::Status
            | HistoryKind::Full
            | HistoryKind::Recent
            | HistoryKind::PushNames
            | HistoryKind::NonBlocking
            | HistoryKind::OnDemand => HistorySyncDecision::Accept,
        }
    }
}

impl HistorySyncAdmission for HistoryPolicy {
    fn decide(&self, metadata: &HistorySyncMetadata<'_>) -> HistorySyncDecision {
        self.classify(metadata.sync_type.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_kinds_retain_their_admission_policy() {
        let cases = [
            (HistorySyncType::INITIAL_BOOTSTRAP, HistoryKind::Bootstrap),
            (HistorySyncType::INITIAL_STATUS_V3, HistoryKind::Status),
            (HistorySyncType::FULL, HistoryKind::Full),
            (HistorySyncType::RECENT, HistoryKind::Recent),
            (HistorySyncType::PUSH_NAME, HistoryKind::PushNames),
            (HistorySyncType::NON_BLOCKING_DATA, HistoryKind::NonBlocking),
            (HistorySyncType::ON_DEMAND, HistoryKind::OnDemand),
        ];
        for (wire, kind) in cases {
            let adapted = HistoryKind::from(Some(wire as i32));
            assert_eq!(adapted, kind);
            assert_eq!(
                HistoryPolicy::default().classify(adapted),
                if kind == HistoryKind::Full {
                    HistorySyncDecision::RejectAndAcknowledge
                } else {
                    HistorySyncDecision::Accept
                },
                "{kind:?}",
            );
            assert_eq!(
                HistoryPolicy::accept_everything().classify(adapted),
                HistorySyncDecision::Accept,
                "{kind:?}",
            );
        }
    }

    #[test]
    fn unknown_and_missing_kinds_are_rejected_even_with_full_history() {
        for raw in [None, Some(-1), Some(7), Some(i32::MAX)] {
            let kind = HistoryKind::from(raw);
            assert_eq!(kind, HistoryKind::Unknown);
            for policy in [HistoryPolicy::default(), HistoryPolicy::accept_everything()] {
                assert_eq!(
                    policy.classify(kind),
                    HistorySyncDecision::RejectAndAcknowledge
                );
            }
        }
    }
}
