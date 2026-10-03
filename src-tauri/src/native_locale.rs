use std::sync::{Mutex, OnceLock};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use crate::command_error::{CommandError, CommandResult};

#[derive(Clone, Copy, Default)]
enum Language { #[default] English, Arabic }

#[derive(Default)]
pub(crate) struct NativeLocale(Mutex<Language>);

fn catalog(language: Language) -> &'static Value {
    static EN: OnceLock<Value> = OnceLock::new();
    static AR: OnceLock<Value> = OnceLock::new();
    match language {
        Language::English => EN.get_or_init(|| serde_json::from_str(include_str!("../../src/lib/i18n/locales/en.json")).expect("valid English catalog")),
        Language::Arabic => AR.get_or_init(|| serde_json::from_str(include_str!("../../src/lib/i18n/locales/ar.json")).expect("valid Arabic catalog")),
    }
}

fn label(selected: &Value, source: &Value, code: &str) -> String {
    selected.get(code).and_then(Value::as_str).filter(|text| !text.trim().is_empty())
        .or_else(|| source.get(code).and_then(Value::as_str))
        .or_else(|| source.get("locale.text_unavailable").and_then(Value::as_str))
        .unwrap_or("Postal").to_owned()
}

pub(crate) fn english_text(code: &str) -> String {
    label(catalog(Language::English), catalog(Language::English), code)
}

pub(crate) fn text(app: &AppHandle, code: &str) -> String {
    let language = app.try_state::<NativeLocale>().map(|state| *state.0.lock().unwrap()).unwrap_or_default();
    label(catalog(language), catalog(Language::English), code)
}

#[tauri::command]
pub(crate) fn set_native_locale(app: AppHandle, language: String) -> CommandResult<()> {
    let language = match language.as_str() {
        "en" => Language::English, "ar" => Language::Arabic,
        _ => return Err(CommandError::code("error.locale_invalid")),
    };
    *app.state::<NativeLocale>().0.lock().unwrap() = language;
    #[cfg(desktop)]
    crate::tray::refresh_labels(&app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_labels_share_source_catalog_and_never_show_missing_keys() {
        let english = catalog(Language::English); let arabic = catalog(Language::Arabic);
        assert_eq!(label(arabic, english, "locale.language"), "اللغة");
        assert_eq!(label(&serde_json::json!({}), english, "locale.language"), "Language");
        assert_eq!(label(arabic, english, "unknown.native.key"), english_text("locale.text_unavailable"));
        assert!(!label(arabic, english, "__proto__").contains("__proto__"));
    }
}
