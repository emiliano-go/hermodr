use std::{any::TypeId, collections::{BTreeMap, HashSet}};
use ts_rs::{Config, TS, TypeVisitor};

struct Types {
    config: Config,
    seen: HashSet<TypeId>,
    declarations: BTreeMap<String, String>,
}

impl TypeVisitor for Types {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        if !self.seen.insert(TypeId::of::<T>()) { return; }
        if T::output_path().is_some() {
            let declaration = format!("export {}\n", T::decl(&self.config));
            if let Some(previous) = self.declarations.insert(T::ident(&self.config), declaration.clone()) {
                assert_eq!(previous, declaration, "wire type name collision");
            }
        }
        T::visit_dependencies(self);
    }
}

pub fn wire_types() -> String {
    let mut types = Types {
        config: Config::default().with_large_int("number"),
        seen: HashSet::new(), declarations: BTreeMap::new(),
    };
    postal_core::wire::visit_wire_types(&mut types);
    postal_plugins::visit_wire_types(&mut types);
    macro_rules! roots {
        ($($ty:ty),* $(,)?) => { $(types.visit::<$ty>();)* };
    }
    roots!(
        crate::settings::UiSettings, crate::account_store::AccountsView,
        crate::connection::ConnectionState, crate::connection::OnceState,
        crate::chats::ChatSettings, crate::groups::Joined, crate::plugins::PluginsView,
        crate::messages::Target, crate::media_actions::MediaAction, crate::polls::EventForm,
    );
    format!("// Generated from Rust Serde DTOs. Run pnpm generate:wire.\n{}", types.declarations.values().cloned().collect::<String>())
}

pub fn wire_fixture() -> String {
    use postal_core::{ServiceEvent, StoredMessage};
    let mut message = StoredMessage::default();
    message.history_shareable = true;
    message.header.chat = "synthetic@invalid".into();
    message.header.id = "fixture".into();
    message.header.from_me = true;
    message.local.sort_order = 7;
    message.media.kind = Some("image".into());
    message.media.locator = Some(vec![1, 2, 3]);
    message.quote.id = Some("quoted".into());
    message.quote.locator = Some(vec![4, 5, 6]);
    message.link.url = Some("https://example.invalid/".into());
    let event = ServiceEvent::MessageHint {
        chat: message.header.chat.clone(), id: message.header.id.clone(),
        sender: "synthetic@invalid".into(), from_me: true, fresh: false,
    };
    let plugin = postal_plugins::wire_fixture(serde_json::to_value(&event).unwrap());
    let archive = postal_core::store::archive::ArchiveReport {
        directory: "synthetic".into(), messages: 7, attachments: 1, missing_attachments: 0,
    };
    let fixture = serde_json::json!({"message": message, "event": event,
        "settings": crate::settings::UiSettings::default(), "plugin": plugin, "archive": archive});
    format!("// Generated synthetic Serde fixture. Run pnpm generate:wire.\n\
        import type {{ StoredMessage, ServiceEvent, UiSettings, HostMessage, ArchiveReport }} from './wire';\n\
        export const fixture = {} satisfies {{ message: StoredMessage; event: ServiceEvent; settings: UiSettings; plugin: HostMessage<ServiceEvent>; archive: ArchiveReport }};\n",
        serde_json::to_string_pretty(&fixture).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_wire_matches_rust() {
        assert_eq!(wire_types().replace("\r\n", "\n"), include_str!("../../src/lib/utils/wire.ts").replace("\r\n", "\n"), "run pnpm generate:wire");
        assert_eq!(wire_fixture().replace("\r\n", "\n"), include_str!("../../src/lib/utils/wire.fixture.ts").replace("\r\n", "\n"), "run pnpm generate:wire");
    }
}
