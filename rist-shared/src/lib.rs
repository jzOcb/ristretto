//! Shared IPC types, protocol helpers, and localization utilities.

rust_i18n::i18n!("locales", fallback = "en");

pub mod i18n;
pub mod protocol;
pub mod types;

pub use protocol::{decode_frame, decode_frame_async, encode_frame, encode_frame_async};
pub use types::*;

