use ts_rs::TypeVisitor;

pub fn visit_wire_types(visitor: &mut impl TypeVisitor) {
    macro_rules! roots {
        ($($ty:ty),* $(,)?) => { $(visitor.visit::<$ty>();)* };
    }
    roots!(
        crate::service::ServiceEvent, crate::service::Profile, crate::service::GroupInfo,
        crate::service::ParticipantChange, crate::service::SearchResult, crate::service::GroupKind,
        crate::service::InviteInfo, crate::service::UserProfile, crate::service::AdminReport,
        crate::service::GroupHistoryOffer, crate::service::GroupHistoryResult,
        crate::service::GroupMemberAddResult, crate::store::scheduled::ScheduledMessage,
        crate::service::GroupJoinRequest, crate::store::contact_identity::ContactIdentity,
        crate::service::LinkedDevice, crate::store::gallery::GalleryPage, crate::store::gallery::GalleryFilter,
        crate::store::transcription::StoredTranscript,
        crate::store::media_policy::MediaAutoDownload, crate::store::media_policy::MediaAutoDownloadOverrides,
        crate::service::BlockedContact, crate::service::GroupCreateResult,
        crate::service::GroupSettings, crate::service::GroupSettingChange,
        crate::service::BooleanProp, crate::service::StickerLibrary, crate::service::StickerResyncReport,
        crate::service::StorageReport, crate::service::StorageCleanup, crate::service::StorageOrder,
        crate::service::CleanupResult, crate::store::StoredMessage, crate::store::ChatSummary,
        crate::store::MessageReceipt, crate::store::ChatRetention, crate::store::DiskRetention,
        crate::store::MessageCursor, crate::store::MessagePage, crate::store::MessagePageDirection,
        crate::store::ChatMarks, crate::store::archive::ArchiveReport,
    );
    crate::store::archive::visit_wire_types(visitor);
}
