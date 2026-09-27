//! Groups and communities: names, members, invites and admin tools.

use super::*;

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

impl Service {
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
        let due: Vec<String> = {
            let backoff = self.subject_backoff.lock().unwrap();
            self.store
                .chats()?
                .into_iter()
                .filter(|c| c.display_name.is_none() && c.chat.ends_with("@g.us"))
                .map(|c| c.chat)
                .filter(|chat| backoff.get(chat).map_or(true, |(retry_at, _)| *retry_at <= now))
                .collect()
        };
        let mut resolved = 0;
        for chat in due {
            if let Some(subject) = fetch_group_subject(&self.client, &chat).await {
                self.store.set_name(&chat, &subject)?;
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
    pub async fn group_info(&self, chat: &str) -> Result<GroupInfo> {
        if !chat.ends_with("@g.us") {
            return Ok(GroupInfo::default());
        }
        if let Some(info) = self.group_cache.lock().unwrap().get(chat).cloned() {
            return Ok(info);
        }
        let jid: Jid = chat.parse()?;
        let mut metadata = self
            .client
            .groups()
            .fetch_metadata(&jid)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        // The server often omits the phone number on LID participants, so fill
        // it from the client's LID/PN cache before naming them.
        self.client
            .groups()
            .resolve_participant_addresses(&mut metadata)
            .await;

        let mut seen = std::collections::HashSet::new();
        let mut participants = Vec::new();
        for member in &metadata.participants {
            let mention = member.jid.to_non_ad().to_string();
            if !seen.insert(mention.clone()) {
                continue;
            }
            remember_lid_pn(&self.store, &member.jid, member.phone_number.as_ref().or(member.lid.as_ref()));
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
            let name = candidates
                .into_iter()
                .flatten()
                .filter_map(|j| self.store.name_for(&j.to_string()).ok().flatten())
                .find(|n| !is_placeholder_name(n))
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
        // Group metadata rarely carries usernames; one usync query fills them in,
        // and gives members we only know by number a username or business name.
        let jids: Vec<Jid> = participants.iter().filter_map(|p| p.jid.parse().ok()).collect();
        if let Ok(infos) = self.client.contacts().get_user_info(&jids).await {
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
        participants.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        // Whether we are an admin, matched in whichever form the group lists us.
        let own: Vec<String> = [self.client.pn(), self.client.lid()]
            .into_iter()
            .flatten()
            .map(|j| j.to_non_ad().to_string())
            .collect();
        let admin = metadata.participants.iter().any(|m| {
            m.is_admin()
                && [Some(&m.jid), m.phone_number.as_ref(), m.lid.as_ref()]
                    .into_iter()
                    .flatten()
                    .any(|j| own.contains(&j.to_non_ad().to_string()))
        });
        let parent = metadata.parent_group_jid.as_ref().map(|j| j.to_string());
        let parent_name = match &parent {
            Some(jid) => match self.store.name_for(jid).ok().flatten() {
                Some(name) => Some(name),
                None => self.group_overviews().await.into_iter().find(|(id, _)| id == jid).map(|(_, s)| s),
            },
            None => None,
        };
        // The creator may have left, so a member's name comes first, then anything known.
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
        let owner = member
            .map(|p| p.name.clone())
            .or_else(|| {
                creator
                    .iter()
                    .filter_map(|j| self.store.name_for(j).ok().flatten())
                    .find(|n| !is_placeholder_name(n))
            })
            .or_else(|| metadata.creator_username.clone())
            .or_else(|| metadata.creator_pn.as_ref().map(|j| format!("+{}", j.user)));
        let community = metadata.is_parent_group;
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
            community,
            announcements: metadata.is_default_sub_group,
            parent,
            parent_name,
            admin,
            can_send: !community && (!metadata.is_announcement || admin),
        };
        self.group_cache
            .lock()
            .unwrap()
            .insert(chat.to_string(), info.clone());
        Ok(info)
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
        // Already a member when the chat is here.
        let joined = self.store.chats()?.iter().any(|c| c.chat == jid);
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
        Ok((joined.group_jid().to_string(), pending))
    }

    /// Every group the account is in, fetched once and cached.
    pub(super) async fn participating(&self) -> Vec<whatsapp_rust::GroupOverview> {
        {
            let cache = self.groups_cache.lock().unwrap();
            if !cache.is_empty() {
                return cache.clone();
            }
        }
        match self.client.groups().list_participating().await {
            Ok(groups) => {
                *self.groups_cache.lock().unwrap() = groups.clone();
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
        Ok(reported
            .reports
            .into_iter()
            .map(|report| AdminReport {
                message: self.store.message(chat, &report.message_id).ok(),
                reporters: report
                    .reporters
                    .into_iter()
                    .map(|r| (r.phone_number.unwrap_or(r.jid).to_non_ad().to_string(), r.timestamp))
                    .collect(),
                id: report.message_id,
            })
            .collect())
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
        Ok(())
    }
}
