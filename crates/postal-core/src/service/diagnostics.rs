use super::WhatsAppService;
use serde::Serialize;
use whatsapp_rust::wacore::iq::abprops::{self, AbDefault, AbProp, AbPropType};

pub(super) fn boolean_props() -> impl Iterator<Item = AbProp> {
    abprops::ALL.iter().flat_map(|registry| registry.iter().copied())
        .filter(|prop| prop.value_type == AbPropType::Bool)
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct BooleanProp {
    pub name: &'static str,
    pub code: u32,
    pub default: bool,
    pub value: Option<bool>,
}

impl WhatsAppService {
    /// Read watched boolean flags without fetching or changing server state.
    pub async fn boolean_props(&self) -> Vec<BooleanProp> {
        let mut values = Vec::new();
        for prop in boolean_props() {
            values.push(BooleanProp {
                name: prop.name,
                code: prop.code,
                default: matches!(prop.default, AbDefault::Bool(true)),
                value: self.client.ab_prop_enabled(prop).await,
            });
        }
        values.sort_unstable_by_key(|prop| prop.name);
        values
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whatsapp_rust::wacore::store::ab_props::AbPropsCache;

    #[tokio::test]
    async fn watched_catalog_preserves_unknown_and_tracks_full_and_delta_updates() {
        let props: Vec<_> = boolean_props().collect();
        assert!(!props.is_empty());
        assert!(props.iter().all(|prop| matches!(prop.default, AbDefault::Bool(_))));
        let cache = AbPropsCache::new();
        cache.watch_many(&props).await;
        let watched = cache.interest().await;
        assert!(props.iter().all(|prop| watched.contains(&prop.code)));
        let prop = props[0];
        assert_eq!(cache.get_bool(prop).await, None);
        cache.apply_props(false, [(prop.code, "true".into())].into_iter()).await;
        assert_eq!(cache.get_bool(prop).await, Some(true));
        cache.apply_props(true, [(prop.code, "0".into())].into_iter()).await;
        assert_eq!(cache.get_bool(prop).await, Some(false));
        cache.apply_props(false, std::iter::empty()).await;
        assert_eq!(cache.get_bool(prop).await, None);
    }
}
