//! High-level NFC-обёртка над `pn532` и Type 2 Tag.
//!
//! Подробная документация на русском:
//! [docs/nfc_tag.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/nfc_tag.md)

pub mod r#async;
pub mod constants;
pub mod esp_idf;
mod format;
pub mod sync;

pub use r#async::{
    AsyncNfcConfig, AsyncNfcError, AsyncNfcSnapshot, AsyncNfcTag, AsyncObservedTag, AsyncTagPayload,
};
pub use sync::{NfcError, NfcInitConfig, NfcTag, TagInfo};
