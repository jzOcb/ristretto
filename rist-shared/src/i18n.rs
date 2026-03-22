//! Localization helpers and locale loading.

use std::sync::OnceLock;

static LOCALE: OnceLock<String> = OnceLock::new();

/// Returns the preferred locale based on `RISTRETTO_LANG`, `LC_ALL`, or `LANG`.
#[must_use]
pub fn preferred_locale() -> String {
    for key in ["RISTRETTO_LANG", "LC_ALL", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                let normalized = value.split('.').next().unwrap_or(&value).replace('_', "-");
                if normalized.eq_ignore_ascii_case("zh-tw")
                    || normalized.eq_ignore_ascii_case("zh-hk")
                {
                    return "zh-TW".to_owned();
                }
                if normalized.starts_with("zh") {
                    return "zh-CN".to_owned();
                }
                return "en".to_owned();
            }
        }
    }
    "en".to_owned()
}

/// Initializes the process-wide locale once.
pub fn init_locale() {
    let locale = LOCALE.get_or_init(preferred_locale);
    rust_i18n::set_locale(locale);
}

/// Translates the provided key using the preferred locale.
#[must_use]
pub fn tr(key: &str) -> String {
    init_locale();
    rust_i18n::t!(key).into_owned()
}
