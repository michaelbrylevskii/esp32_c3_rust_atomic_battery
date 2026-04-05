use common::drivers::led_indicator::AsyncLedError;
use common::drivers::nfc_tag::{AsyncNfcError, NfcError};
use common::drivers::segment_display::{AsyncDisplayError, DisplayError};
use common::utils::atomic_tags::AtomicTagError;
use common::utils::kv_store::KvFormatError;
use esp_idf_svc::hal::i2c::I2cError;
use esp_idf_svc::sys::EspError;
use std::fmt;

#[derive(Debug)]
pub enum AppError {
    Esp(EspError),
    Kv(KvFormatError),
    Nfc(NfcError<I2cError>),
    AsyncNfc(AsyncNfcError),
    Display(DisplayError),
    AsyncDisplay(AsyncDisplayError),
    AsyncLed(AsyncLedError),
    Tag(AtomicTagError),
}

impl From<EspError> for AppError {
    fn from(value: EspError) -> Self {
        Self::Esp(value)
    }
}

impl From<KvFormatError> for AppError {
    fn from(value: KvFormatError) -> Self {
        Self::Kv(value)
    }
}

impl From<AtomicTagError> for AppError {
    fn from(value: AtomicTagError) -> Self {
        Self::Tag(value)
    }
}

impl From<NfcError<I2cError>> for AppError {
    fn from(value: NfcError<I2cError>) -> Self {
        Self::Nfc(value)
    }
}

impl From<AsyncNfcError> for AppError {
    fn from(value: AsyncNfcError) -> Self {
        Self::AsyncNfc(value)
    }
}

impl From<DisplayError> for AppError {
    fn from(value: DisplayError) -> Self {
        Self::Display(value)
    }
}

impl From<AsyncDisplayError> for AppError {
    fn from(value: AsyncDisplayError) -> Self {
        Self::AsyncDisplay(value)
    }
}

impl From<AsyncLedError> for AppError {
    fn from(value: AsyncLedError) -> Self {
        Self::AsyncLed(value)
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Esp(err) => write!(f, "esp error: {err}"),
            AppError::Kv(err) => write!(f, "kv format error: {err}"),
            AppError::Nfc(err) => write!(f, "nfc error: {err}"),
            AppError::AsyncNfc(err) => write!(f, "async nfc error: {err}"),
            AppError::Display(err) => write!(f, "display error: {err}"),
            AppError::AsyncDisplay(err) => write!(f, "async display error: {err}"),
            AppError::AsyncLed(err) => write!(f, "async led error: {err}"),
            AppError::Tag(err) => write!(f, "tag error: {err}"),
        }
    }
}

impl std::error::Error for AppError {}
