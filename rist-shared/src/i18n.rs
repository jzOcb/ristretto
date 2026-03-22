//! Localization helpers and locale loading.

/// Returns the preferred locale based on `RISTRETTO_LANG`, `LC_ALL`, or `LANG`.
#[must_use]
pub fn preferred_locale() -> String {
    for key in ["RISTRETTO_LANG", "LC_ALL", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                if value.starts_with("zh") {
                    return "zh-CN".to_owned();
                }
                return "en".to_owned();
            }
        }
    }
    "en".to_owned()
}

/// Translates the provided key using the preferred locale.
#[must_use]
pub fn tr(key: &str) -> String {
    rust_i18n::set_locale(&preferred_locale());
    rust_i18n::t!(key).into_owned()
}

