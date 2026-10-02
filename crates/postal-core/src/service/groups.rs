//! Groups and communities: names, members, invites and admin tools.

use super::*;
use crate::store::group_audit::GroupAuditKind as AuditKind;
use whatsapp_rust::wacore::iq::groups::ParticipantChangeResponse;
use whatsapp_rust::wacore::types::wire_enums::MemberAddMode;

/// Fetches a group's subject over the `w:g2` namespace.
///
/// Group names are not carried on incoming messages, and the library exposes no
/// typed accessor for them in this version, so the query is issued directly.
pub(super) async fn fetch_group_subject(client: &Client, group: &str) -> Option<String> {
    let jid: Jid = group.parse().ok()?;
    let node = NodeBuilder::new("iq")
        .attr("type", "get")
        .attr("xmlns", "w:g2")
        .attr("to", jid)
        .children([NodeBuilder::new("query")
            .attr("request", "interactive")
            .build()])
        .build();

    let response = client
        .send_iq_node(node, Some(Duration::from_secs(15)))
        .await
        .ok()?;

    // The subject lives on the <group> child of the response.
    let group_node = response.get().get_optional_child_by_tag(&["group"])?;
    group_node
        .attrs()
        .optional_string("subject")
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

impl WhatsAppService {
    /// Resolves display names for chats that do not have one yet.
    ///
    /// Only groups need a query: a one-to-one chat is named after its contact,
    /// whose name arrives with the message itself. Returns how many were
    /// resolved, so the caller can refresh only when something changed.
    /// A failed group waits before its next query, twice as long each time up to
    /// half an hour; overlapping calls return 0 at once.
    pub async fn resolve_missing_names(&self) -> Result<usize> {
        if self.resolving.swap(true, Ordering::SeqCst) {
            return Ok(0);
        }
        let result = self.resolve_untried_groups().await;
        self.resolving.store(false, Ordering::SeqCst);
        result
    }

    async fn resolve_untried_groups(&self) -> Result<usize> {
        let now = std::time::Instant::now();
        let chats = self.store.chats().await?;
        let due: Vec<String> = {
            let backoff = self.subject_backoff.lock().unwrap();
            chats
                .into_iter()
                .filter(|c| c.display_name.is_none() && c.chat.ends_with("@g.us"))
                .map(|c| c.chat)
                .filter(|chat| backoff.get(chat).map_or(true, |(retry_at, _)| *retry_at <= now))
                .collect()
        };
        let mut resolved = 0;
        for chat in due {
            if let Some(subject) = fetch_group_subject(&self.client, &chat).await {
                self.store.set_name(&chat, &subject).await?;
                self.subject_backoff.lock().unwrap().remove(&chat);
                resolved += 1;
            } else {
                let mut backoff = self.subject_backoff.lock().unwrap();
                let wait = backoff
                    .get(&chat)
                    .map_or(Duration::from_secs(30), |(_, last)| (*last * 2).min(Duration::from_secs(30 * 60)));
                backoff.insert(chat, (std::time::Instant::now() + wait, wait));
            }
        }
        Ok(resolved)
    }

    /// Members of a group chat, for mention autocomplete.
    pub async fn participants(&self, chat: &str) -> Result<Vec<Participant>> {
        Ok(self.group_info(chat).await?.participants)
    }

    /// Everything the group info sidebar needs.
    /// Everything the group info sidebar needs.
    pub async fn group_info(&self, chat: &str) -> Result<GroupInfo> {
        if !chat.ends_with("@g.us") {
            return Ok(GroupInfo::default());
        }
        if let Some(info) = self.group_cache.lock().unwrap().get(chat).cloned() {
            return Ok(info);
        }
        let metadata = self.fetch_group_metadata(chat).await?;
        let participants = self.group_participants(&metadata).await;
        let admin = self.is_group_admin(&metadata);
        let parent = metadata.parent_group_jid.as_ref().map(|j| j.to_string());
        let parent_name = self.group_parent_name(parent.as_deref()).await;
        let (owner, owner_jid) = self.group_owner(&metadata, &participants).await;
        let info = GroupInfo {
            subject: metadata.subject.clone(),
            description: metadata.description.clone(),
            created_at: metadata.creation_time,
            owner,
            owner_jid,
            participants,
            allow_admin_reports: metadata.allow_admin_reports,
            announce: metadata.is_announcement,
            locked: metadata.is_locked,
            community: metadata.is_parent_group,
            announcements: metadata.is_default_sub_group,
            parent,
            parent_name,
            admin,
            can_send: !metadata.is_parent_group && (!metadata.is_announcement || admin),
            members_can_add: metadata.member_add_mode == Some(MemberAddMode::AllMemberAdd),
        };
        self.record_group_profile_snapshot(chat, &info).await.logged();
        self.group_cache.lock().unwrap().insert(chat.to_string(), info.clone());
        Ok(info)
    }

    /// The group's metadata with the participant phone numbers the server
    /// omits filled in from the client's LID/PN cache.
    async fn fetch_group_metadata(&self, chat: &str) -> Result<whatsapp_rust::GroupMetadata> {
        let jid: Jid = chat.parse()?;
        let mut metadata = self
            .client
            .groups()
            .fetch_metadata(&jid)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.client
            .groups()
            .resolve_participant_addresses(&mut metadata)
            .await;
        Ok(metadata)
    }

    /// The member list: one entry per address form, named from whatever is
    /// known, enriched with usync usernames, sorted by name.
    async fn group_participants(&self, metadata: &whatsapp_rust::GroupMetadata) -> Vec<Participant> {
        let mut seen = std::collections::HashSet::new();
        let mut participants = Vec::new();
        for member in &metadata.participants {
            let mention = member.jid.to_non_ad().to_string();
            if !seen.insert(mention.clone()) {
                continue;
            }
            remember_lid_pn(&self.store, &member.jid, member.phone_number.as_ref().or(member.lid.as_ref())).await;
            let candidates = [
                member.phone_number.as_ref(),
                member.lid.as_ref(),
                Some(&member.jid),
            ];
            // Phone number without the server, so it reads as a number.
            let number = member
                .phone_number
                .as_ref()
                .map(|j| j.to_non_ad().to_string())
                .or_else(|| mention.ends_with("@s.whatsapp.net").then(|| mention.clone()))
                .map(|j| j.split('@').next().unwrap_or(&j).to_string());
            let username = member.username.as_ref().map(|u| u.to_string());
            if let Some(username) = &username {
                if self.store.set_username(&mention, username).await.observed() == Some(true) { let _ = self.events.send(ServiceEvent::NamesUpdated { count: 1 }); }
            }
            // WhatsApp's masked number for a member whose phone is hidden
            // ("+598∙∙∙∙∙27"). Only a label of last resort; never stored as a
            // name, or it would overwrite the member's real push name.
            let masked = member
                .details
                .as_ref()
                .and_then(|d| d.display_name.as_ref())
                .map(|n| n.to_string())
                .filter(|n| !n.trim().is_empty());
            // A name someone can read: saved/push name, then username, then the
            // phone number, and only last the masked number or the LID.
            // A placeholder under one form must not hide a real name stored
            // under the other.
            let candidates: Vec<String> = candidates.into_iter().flatten().map(|j| j.to_string()).collect();
            let name = first_stored_name(&self.store, &candidates).await.map(|(_, name)| name)
                .or_else(|| username.clone())
                .or_else(|| number.clone())
                .or(masked)
                .unwrap_or_else(|| mention.split('@').next().unwrap_or(&mention).to_string());
            let label = member
                .details
                .as_ref()
                .and_then(|d| d.participant_label.as_ref())
                .map(|l| l.to_string())
                .filter(|l| !l.is_empty());
            participants.push(Participant {
                jid: mention.clone(),
                name,
                admin: member.is_admin(),
                owner: member.is_super_admin(),
                number,
                username,
                label,
            });
        }
        self.enrich_usernames(&mut participants).await;
        participants.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        participants
    }

    /// Group metadata rarely carries usernames; bounded usync queries fill them
    /// in, and give members we only know by number a username or business name.
    async fn enrich_usernames(&self, participants: &mut [Participant]) {
        let jids: Vec<Jid> = participants.iter().filter_map(|p| p.jid.parse().ok()).collect();
        let Ok(infos) = self.user_info(&jids).await else { return };
        let numeric = |n: &str| is_placeholder_name(n);
        for p in participants.iter_mut() {
            let user = p.jid.split('@').next().unwrap_or_default();
            let Some(info) = infos.values().find(|i| {
                i.jid.user == user
                    || i.lid.as_ref().is_some_and(|l| l.user == user)
                    || p.number.as_deref() == Some(i.jid.user.as_str())
            }) else {
                continue;
            };
            if p.username.is_none() {
                p.username = info.username.as_ref().map(|u| u.to_string());
            }
            if let Some(username) = &p.username {
                if self.store.set_username(&p.jid, username).await.observed() == Some(true) { let _ = self.events.send(ServiceEvent::NamesUpdated { count: 1 }); }
            }
            if numeric(&p.name) {
                if let Some(better) = info
                    .verified_name
                    .as_ref()
                    .and_then(|v| v.name.clone())
                    .or_else(|| p.username.clone())
                {
                    p.name = better;
                }
            }
        }
    }

    /// Whether we are an admin, matched in whichever form the group lists us.
    pub(super) fn is_group_admin(&self, metadata: &whatsapp_rust::GroupMetadata) -> bool {
        let own: Vec<String> = [self.client.pn(), self.client.lid()]
            .into_iter()
            .flatten()
            .map(|j| j.to_non_ad().to_string())
            .collect();
        metadata.participants.iter().any(|m| {
            m.is_admin()
                && [Some(&m.jid), m.phone_number.as_ref(), m.lid.as_ref()]
                    .into_iter()
                    .flatten()
                    .any(|j| own.contains(&j.to_non_ad().to_string()))
        })
    }

    /// The parent community's name, from the store or the group list.
    async fn group_parent_name(&self, parent: Option<&str>) -> Option<String> {
        let jid = parent?;
        match self.store.name_for(jid).await.observed().flatten() {
            Some(name) => Some(name),
            None => self
                .group_overviews()
                .await
                .into_iter()
                .find(|(id, _)| id.as_str() == jid)
                .map(|(_, subject)| subject),
        }
    }

    /// Who created the group, preferring the member entry so a name the group
    /// still lists wins over anything learned elsewhere.
    async fn group_owner(
        &self,
        metadata: &whatsapp_rust::GroupMetadata,
        participants: &[Participant],
    ) -> (Option<String>, Option<String>) {
        let creator: Vec<String> = [metadata.creator.as_ref(), metadata.creator_pn.as_ref()]
            .into_iter()
            .flatten()
            .map(|j| j.to_non_ad().to_string())
            .collect();
        let member = metadata
            .participants
            .iter()
            .find(|m| {
                [Some(&m.jid), m.phone_number.as_ref(), m.lid.as_ref()]
                    .into_iter()
                    .flatten()
                    .any(|j| creator.contains(&j.to_non_ad().to_string()))
            })
            .and_then(|m| {
                let jid = m.jid.to_non_ad().to_string();
                participants.iter().find(|p| p.jid == jid)
            });
        let owner_jid = member.map(|p| p.jid.clone()).or_else(|| creator.first().cloned());
        let named_owner = if member.is_none() {
            first_stored_name(&self.store, &creator).await.map(|(_, name)| name)
        } else {
            None
        };
        let owner = member
            .map(|p| p.name.clone())
            .or(named_owner)
            .or_else(|| metadata.creator_username.clone())
            .or_else(|| metadata.creator_pn.as_ref().map(|j| format!("+{}", j.user)));
        (owner, owner_jid)
    }

    /// Adds participants, returning the server's answer for each.
    pub async fn add_group_participants(&self, chat: &str, jids: &[String]) -> Result<Vec<ParticipantChange>> {
        let group: Jid = chat.parse()?;
        let participants = parse_jids(jids)?;
        let previous = self.group_cache.lock().unwrap().get(chat).cloned();
        let results = self
            .client
            .groups()
            .add_participants(&group, &participants)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.audit_participant_changes(chat, &results, AuditKind::Join, previous.as_ref()).await;
        self.after_group_change(chat);
        Ok(results.iter().map(change_of).collect())
    }

    /// Removes participants; a community's parent group also removes them from
    /// its subgroups, which is what the phone's own UI does.
    pub async fn remove_group_participants(&self, chat: &str, jids: &[String]) -> Result<Vec<ParticipantChange>> {
        let group: Jid = chat.parse()?;
        let participants = parse_jids(jids)?;
        let groups = self.client.groups();
        let previous = self.group_info(chat).await.ok();
        let linked = previous.as_ref().is_some_and(|info| info.community);
        let results = if linked {
            groups.remove_participants_including_linked_groups(&group, &participants).await
        } else {
            groups.remove_participants(&group, &participants).await
        }
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.audit_participant_changes(chat, &results, AuditKind::Remove, previous.as_ref()).await;
        self.after_group_change(chat);
        Ok(results.iter().map(change_of).collect())
    }

    /// Gives participants admin rights.
    pub async fn promote_group_participants(&self, chat: &str, jids: &[String]) -> Result<Vec<ParticipantChange>> {
        let group: Jid = chat.parse()?;
        let participants = parse_jids(jids)?;
        let previous = self.group_cache.lock().unwrap().get(chat).cloned();
        let results = self
            .client
            .groups()
            .promote_participants(&group, &participants)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.audit_participant_changes(chat, &results, AuditKind::Promote, previous.as_ref()).await;
        self.after_group_change(chat);
        Ok(results.iter().map(change_of).collect())
    }

    /// Takes admin rights back.
    pub async fn demote_group_participants(&self, chat: &str, jids: &[String]) -> Result<Vec<ParticipantChange>> {
        let group: Jid = chat.parse()?;
        let participants = parse_jids(jids)?;
        let previous = self.group_cache.lock().unwrap().get(chat).cloned();
        let results = self
            .client
            .groups()
            .demote_participants(&group, &participants)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.audit_participant_changes(chat, &results, AuditKind::Demote, previous.as_ref()).await;
        self.after_group_change(chat);
        Ok(results.iter().map(change_of).collect())
    }

    /// Sets whether members, or only admins, may add people.
    pub async fn set_members_can_add(&self, chat: &str, allow: bool) -> Result<()> {
        let group: Jid = chat.parse()?;
        let mode = if allow { MemberAddMode::AllMemberAdd } else { MemberAddMode::AdminAdd };
        self.client
            .groups()
            .set_member_add_mode(&group, mode)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.audit_local_group_change(chat, AuditKind::MemberAddMode, None, None, None, None,
            Some(if allow { "all_member_add" } else { "admin_add" })).await.logged();
        self.after_group_change(chat);
        Ok(())
    }

    /// Drops the cached roster and tells the UI, so an open panel reloads
    /// without waiting for the server's own notification.
    pub(super) fn after_group_change(&self, chat: &str) {
        self.group_cache.lock().unwrap().remove(chat);
        let _ = self.events.send(ServiceEvent::GroupChanged { chat: chat.to_string() });
    }

    async fn audit_participant_changes(&self, chat: &str, results: &[ParticipantChangeResponse], kind: AuditKind, previous: Option<&GroupInfo>) {
        for result in results {
            let change = change_of(result);
            if !change.ok || change.pending { continue; }
            let (old, new) = {
                let member = previous.and_then(|group| group.participants.iter().find(|member| member.jid == change.jid));
                match kind {
                    AuditKind::Join => (member.map(|_| "present"), "present"),
                    AuditKind::Remove => (member.map(|_| "present"), "absent"),
                    AuditKind::Promote | AuditKind::Demote => (member.map(|member| if member.owner { "owner" } else if member.admin { "admin" } else { "member" }),
                        if kind == AuditKind::Promote { "admin" } else { "member" }),
                    _ => continue,
                }
            };
            self.audit_local_group_change(chat, kind, Some(&change.jid), None, None, old, Some(new)).await.logged();
        }
    }

    /// A group invite link's group, without joining it.
    pub async fn invite_info(&self, link: &str) -> Result<InviteInfo> {
        let group = self
            .client
            .groups()
            .get_invite_info(link)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let jid = group.id.to_string();
        let joined = self.client.groups().list_participating().await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?
            .iter().any(|group| group.id.to_string() == jid);
        let picture = self.avatar(&jid, false).await.ok().flatten();
        Ok(InviteInfo {
            size: group.size.unwrap_or(group.participants.len() as u32),
            subject: group.subject,
            description: group.description.filter(|d| !d.trim().is_empty()),
            created_at: group.creation_time,
            approval: group.membership_approval,
            community: group.is_parent_group,
            joined,
            picture,
            jid,
        })
    }

    /// Joins through an invite link; `pending` when an admin has to approve.
    pub async fn join_invite(&self, link: &str) -> Result<(String, bool)> {
        use whatsapp_rust::JoinGroupResult;
        let joined = self
            .client
            .groups()
            .join_with_invite_code(link)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let pending = matches!(joined, JoinGroupResult::PendingApproval(_));
        if !pending { self.after_group_change(&joined.group_jid().to_string()); }
        Ok((joined.group_jid().to_string(), pending))
    }

    /// Every group the account is in, refreshed after group-update events.
    pub(super) async fn participating(&self) -> Vec<whatsapp_rust::GroupOverview> {
        {
            let cache = self.groups_cache.lock().unwrap();
            if let Some(groups) = cache.as_ref() {
                return groups.clone();
            }
        }
        match self.client.groups().list_participating().await {
            Ok(groups) => {
                let rows = groups.iter().map(|group| {
                    use whatsapp_rust::{GroupHierarchy, SubgroupKind};
                    let (parent, community, announcements) = match &group.hierarchy {
                        GroupHierarchy::Community => (None, true, false),
                        GroupHierarchy::Subgroup { parent, kind } => (Some(parent.to_non_ad().to_string()), false, *kind == SubgroupKind::Announcement),
                        _ => (None, false, false),
                    };
                    CachedSpaceGroup { jid: group.id.to_non_ad().to_string(), subject: group.subject.clone(), parent, community, announcements }
                }).collect::<Vec<_>>();
                self.store.run(move |store| store.cache_space_groups(&rows)).await.logged();
                *self.groups_cache.lock().unwrap() = Some(groups.clone());
                groups
            }
            Err(e) => {
                log::warn!("could not list groups: {e}");
                Vec::new()
            }
        }
    }

    /// Every group the account is in, as `(jid, subject)`.
    pub(super) async fn group_overviews(&self) -> Vec<(String, String)> {
        self.participating()
            .await
            .into_iter()
            .filter_map(|g| g.subject.map(|s| (g.id.to_string(), s)))
            .collect()
    }

    /// Community parents and subgroups among the account's groups; plain groups are left out.
    pub async fn group_kinds(&self) -> std::collections::HashMap<String, GroupKind> {
        use whatsapp_rust::{GroupHierarchy, SubgroupKind};
        self.participating()
            .await
            .into_iter()
            .filter_map(|g| {
                let kind = match g.hierarchy {
                    GroupHierarchy::Community => GroupKind { community: true, announcements: false, parent: None },
                    GroupHierarchy::Subgroup { parent, kind } => GroupKind {
                        community: false,
                        announcements: kind == SubgroupKind::Announcement,
                        parent: Some(parent.to_string()),
                    },
                    _ => return None,
                };
                Some((g.id.to_string(), kind))
            })
            .collect()
    }

    /// Reports a group message to the group's admins.
    pub async fn report_to_admins(&self, chat: &str, id: &str) -> Result<()> {
        let jid: Jid = chat.parse()?;
        self.client
            .groups()
            .report_messages_to_admins(jid, &[id.to_string()])
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Messages members reported to this group's admins. Only admins may ask.
    pub async fn admin_reports(&self, chat: &str) -> Result<Vec<AdminReport>> {
        let jid: Jid = chat.parse()?;
        let reported = self
            .client
            .groups()
            .get_reported_messages(jid)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let mut reports = Vec::with_capacity(reported.reports.len());
        for report in reported.reports {
            reports.push(AdminReport {
                message: self.store.message(chat, &report.message_id).await.observed(),
                reporters: report
                    .reporters
                    .into_iter()
                    .map(|r| (r.phone_number.unwrap_or(r.jid).to_non_ad().to_string(), r.timestamp))
                    .collect(),
                id: report.message_id,
            });
        }
        Ok(reports)
    }

    /// Lets members report messages to the admins, or stops them.
    pub async fn set_allow_admin_reports(&self, chat: &str, allow: bool) -> Result<()> {
        let jid: Jid = chat.parse()?;
        self.client
            .groups()
            .set_allow_admin_reports(jid, allow)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        if let Some(info) = self.group_cache.lock().unwrap().get_mut(chat) {
            info.allow_admin_reports = allow;
        }
        Ok(())
    }

    /// Sets our own tag in a group; empty clears it.
    pub async fn set_member_label(&self, chat: &str, label: &str) -> Result<()> {
        let jid: Jid = chat.parse()?;
        let own_jid = self.client.pn().or_else(|| self.client.lid()).map(|jid| jid.to_non_ad().to_string());
        let old = self.group_cache.lock().unwrap().get(chat).and_then(|group| own_jid.as_ref()
            .and_then(|own| group.participants.iter().find(|member| &member.jid == own))).and_then(|member| member.label.clone());
        self.client
            .groups()
            .update_member_label(jid, label)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let own: Vec<String> = [self.client.pn(), self.client.lid()]
            .into_iter()
            .flatten()
            .map(|j| j.to_non_ad().to_string())
            .collect();
        if let Some(info) = self.group_cache.lock().unwrap().get_mut(chat) {
            if let Some(p) = info.participants.iter_mut().find(|p| own.contains(&p.jid)) {
                p.label = (!label.is_empty()).then(|| label.to_string());
            }
        }
        self.audit_local_group_change(chat, AuditKind::MemberTag, own_jid.as_deref(), None, None,
            old.as_deref(), Some(label)).await.logged();
        Ok(())
    }
}

/// Parses the addresses a command sends; one bad address fails the call
/// rather than silently dropping whoever it named.
pub(super) fn parse_jids(jids: &[String]) -> Result<Vec<Jid>> {
    jids.iter()
        .map(|jid| jid.parse::<Jid>().map_err(|e| anyhow::anyhow!("bad participant address {jid}: {e}")))
        .collect()
}

/// The UI shape of one server answer.
pub(super) fn change_of(response: &ParticipantChangeResponse) -> ParticipantChange {
    participant_change(
        response.jid.to_string(),
        response.status.clone(),
        response.error.clone(),
        response.add_request.is_some() && response.is_ok(),
    )
}

#[cfg(test)]
mod participant_tests {
    use super::*;

    #[test]
    fn changes_report_ok_refusal_and_pending() {
        let ok = participant_change("1@s".into(), Some("200".into()), None, false);
        assert!(ok.ok && !ok.pending && ok.code.as_deref() == Some("200"));

        let refused = participant_change("2@s".into(), Some("409".into()), Some("conflict".into()), false);
        assert!(!refused.ok);
        assert_eq!(refused.error.as_deref(), Some("conflict"));

        let pending = participant_change("3@s".into(), Some("200".into()), None, true);
        assert!(pending.ok && pending.pending);
    }

    #[test]
    fn addresses_are_parsed_or_the_call_fails() {
        let good = parse_jids(&["59899000000@s.whatsapp.net".into()]).unwrap();
        assert_eq!(good.len(), 1);
        assert!(parse_jids(&["not a jid".into()]).is_err());
    }
}
