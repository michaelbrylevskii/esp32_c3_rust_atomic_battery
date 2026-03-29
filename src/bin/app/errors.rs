use common::drivers::nfc_tag::{KvFormatError, NfcError};
use common::drivers::segment_display::{AsyncDisplayError, DisplayError};
use esp_idf_svc::hal::i2c::I2cError;
use esp_idf_svc::sys::EspError;
use std::fmt;

#[derive(Debug)]
pub enum AppError {
    Esp(EspError),
    Kv(KvFormatError),
    Nfc(NfcError<I2cError>),
    Display(DisplayError),
    AsyncDisplay(AsyncDisplayError),
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

impl From<NfcError<I2cError>> for AppError {
    fn from(value: NfcError<I2cError>) -> Self {
        Self::Nfc(value)
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

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Esp(err) => write!(f, "esp error: {err}"),
            AppError::Kv(err) => write!(f, "kv format error: {err}"),
            AppError::Nfc(err) => write!(f, "nfc error: {err}"),
            AppError::Display(err) => write!(f, "display error: {err}"),
            AppError::AsyncDisplay(err) => write!(f, "async display error: {err}"),
        }
    }
}

impl std::error::Error for AppError {}
