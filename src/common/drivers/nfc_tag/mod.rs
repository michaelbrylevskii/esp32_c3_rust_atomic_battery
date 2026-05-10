//! High-level NFC-обёртка над `pn532` и Type 2 Tag.
//!
//! Подробная документация на русском:
//! [docs/nfc_tag.md](../../../../docs/nfc_tag.md)

pub mod async_nfc;
pub mod constants;
pub mod esp_idf;
mod format;
pub mod sync_nfc;
