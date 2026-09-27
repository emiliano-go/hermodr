use std::{collections::HashMap, future::Future};

use anyhow::Result;
use tokio::sync::Semaphore;
use whatsapp_rust::{prelude::Jid, wacore::iq::usync::UserInfo};

use super::WhatsAppService;

const BATCH_SIZE: usize = 100;
pub(super) const MAX_REQUESTS: usize = 2;

impl WhatsAppService {
    pub(super) async fn user_info(&self, jids: &[Jid]) -> Result<HashMap<Jid, UserInfo>> {
        query_chunks(jids, &self.user_info_slots, |chunk| async move {
            self.client
                .contacts()
                .get_user_info(&chunk)
                .await
                .map_err(Into::into)
        })
        .await
    }
}

async fn query_chunks<T, F, Fut>(
    jids: &[Jid],
    slots: &Semaphore,
    mut fetch: F,
) -> Result<HashMap<Jid, T>>
where
    F: FnMut(Vec<Jid>) -> Fut,
    Fut: Future<Output = Result<HashMap<Jid, T>>>,
{
    let mut result = HashMap::new();
    for chunk in jids.chunks(BATCH_SIZE) {
        let _permit = slots.acquire().await?;
        result.extend(fetch(chunk.to_vec()).await?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn large_overlapping_lookups_are_chunked_and_share_the_limit() {
        let jids: Vec<Jid> = (0..1001)
            .map(|i| format!("{i}@s.whatsapp.net").parse().unwrap())
            .collect();
        let slots = Semaphore::new(MAX_REQUESTS);
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let calls = AtomicUsize::new(0);
        let fetch = |chunk: Vec<Jid>| {
            let (active, peak, calls) = (&active, &peak, &calls);
            async move {
                assert!(!chunk.is_empty() && chunk.len() <= BATCH_SIZE);
                calls.fetch_add(1, Ordering::SeqCst);
                let running = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(running, Ordering::SeqCst);
                tokio::task::yield_now().await;
                active.fetch_sub(1, Ordering::SeqCst);
                Ok(chunk
                    .into_iter()
                    .map(|jid| (jid.clone(), jid.to_string()))
                    .collect())
            }
        };
        let results = tokio::join!(
            query_chunks(&jids, &slots, fetch),
            query_chunks(&jids, &slots, fetch),
            query_chunks(&jids, &slots, fetch),
        );
        for result in [results.0, results.1, results.2] {
            let result = result.unwrap();
            assert_eq!(result.len(), jids.len());
            for jid in &jids {
                assert_eq!(result.get(jid), Some(&jid.to_string()));
            }
        }
        assert_eq!(calls.load(Ordering::SeqCst), 33);
        assert_eq!(peak.load(Ordering::SeqCst), MAX_REQUESTS);
        assert_eq!(slots.available_permits(), MAX_REQUESTS);
    }

    #[tokio::test]
    async fn empty_queries_skip_network_and_failures_release_capacity() {
        let slots = Semaphore::new(MAX_REQUESTS);
        let empty = query_chunks::<(), _, _>(&[], &slots, |_| async { panic!("empty request") })
            .await
            .unwrap();
        assert!(empty.is_empty());
        let jids = vec!["1@s.whatsapp.net".parse().unwrap(); BATCH_SIZE + 1];
        let mut calls = 0;
        let result = query_chunks::<(), _, _>(&jids, &slots, |_| {
            calls += 1;
            let call = calls;
            async move {
                anyhow::ensure!(call != 2, "offline");
                Ok(HashMap::new())
            }
        })
        .await;
        assert!(result.unwrap_err().to_string().contains("offline"));
        assert_eq!(calls, 2);
        assert_eq!(slots.available_permits(), MAX_REQUESTS);
    }
}
