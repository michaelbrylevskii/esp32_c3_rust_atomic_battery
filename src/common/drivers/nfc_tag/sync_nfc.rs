//! Синхронный high-level API для чтения и записи NFC-меток через `Pn532`.

use super::constants::{
    CAPABILITY_CONTAINER_MAGIC, CAPABILITY_CONTAINER_PAGE, CAPABILITY_TO_USER_BYTES_MULTIPLIER,
    COMMAND_TIMEOUT, DEFAULT_INIT_ATTEMPTS, DEFAULT_INIT_RETRY_DELAY, DEFAULT_INIT_STARTUP_DELAY,
    FIRMWARE_VERSION_RESPONSE_BYTES, INIT_TIMEOUT, PAGE_SIZE, POLL_TAG_EXPECTED_LEN,
    READ_BLOCK_BYTES, READ_BLOCK_PAGES, READ_BLOCK_RESPONSE_BYTES, USER_START_PAGE,
};
use super::format::{decode_text_record, encode_ndef_tlv, encode_text_record, extract_ndef_tlv};
use crate::utils::kv_store::{KvFormatError, KvStore};
use core::fmt::{self, Debug};
use core::time::Duration;
use pn532::{requests::SAMMode, CountDown, Interface, Pn532, Request};
use std::string::String;
use std::thread;
use std::vec::Vec;

/// Краткая информация о найденной NFC-метке.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagInfo {
    /// UID метки в том виде, как его вернул PN532.
    pub uid: Vec<u8>,
    /// Answer To Request, Type A.
    pub atqa: [u8; 2],
    /// Select Acknowledge.
    pub sak: u8,
}

/// Конфигурация высокоуровневой инициализации PN532.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NfcInitConfig {
    /// Пауза перед первой попыткой инициализации.
    pub startup_delay: Duration,
    /// Пауза между повторными попытками.
    pub retry_delay: Duration,
    /// Общее число попыток инициализации.
    pub attempts: usize,
}

impl Default for NfcInitConfig {
    fn default() -> Self {
        Self {
            startup_delay: DEFAULT_INIT_STARTUP_DELAY,
            retry_delay: DEFAULT_INIT_RETRY_DELAY,
            attempts: DEFAULT_INIT_ATTEMPTS,
        }
    }
}

/// Ошибки high-level NFC слоя.
#[derive(Debug)]
pub enum NfcError<E: Debug> {
    Pn532(pn532::Error<E>),
    Format(KvFormatError),
    InvalidResponse(&'static str),
    InvalidInitConfig(&'static str),
    TagStatus(u8),
    InvalidCapabilityContainer([u8; 4]),
    NoNdefMessage,
    UnsupportedTlv(u8),
    TlvLengthOutOfBounds,
    PayloadTooLarge { payload_len: usize, capacity: usize },
}

impl<E: Debug> From<pn532::Error<E>> for NfcError<E> {
    fn from(value: pn532::Error<E>) -> Self {
        NfcError::Pn532(value)
    }
}

impl<E: Debug> From<KvFormatError> for NfcError<E> {
    fn from(value: KvFormatError) -> Self {
        NfcError::Format(value)
    }
}

impl<E> fmt::Display for NfcError<E>
where
    E: Debug + fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NfcError::Pn532(err) => write!(f, "pn532 error: {err:?}"),
            NfcError::Format(err) => write!(f, "format error: {err}"),
            NfcError::InvalidResponse(reason) => write!(f, "invalid pn532 response: {reason}"),
            NfcError::InvalidInitConfig(reason) => {
                write!(f, "invalid NFC init config: {reason}")
            }
            NfcError::TagStatus(status) => write!(f, "ntag returned status 0x{status:02X}"),
            NfcError::InvalidCapabilityContainer(cc) => {
                write!(f, "invalid capability container: {cc:02X?}")
            }
            NfcError::NoNdefMessage => write!(f, "tag does not contain an NDEF message"),
            NfcError::UnsupportedTlv(tlv) => write!(f, "unsupported TLV 0x{tlv:02X}"),
            NfcError::TlvLengthOutOfBounds => write!(f, "TLV length exceeds tag capacity"),
            NfcError::PayloadTooLarge {
                payload_len,
                capacity,
            } => write!(
                f,
                "payload is too large for tag: {payload_len} bytes > {capacity} bytes"
            ),
        }
    }
}

impl<E> std::error::Error for NfcError<E> where E: Debug + fmt::Display {}

/// High-level обёртка над `Pn532` для работы с key-value данными на NFC-метке.
pub struct NfcTag<I, T, const N: usize>
where
    I: Interface,
    T: CountDown<Time = Duration>,
{
    pn532: Pn532<I, T, N>,
}

impl<I, T, const N: usize> NfcTag<I, T, N>
where
    I: Interface,
    T: CountDown<Time = Duration>,
{
    /// Создаёт high-level NFC wrapper поверх уже созданного `Pn532`.
    pub fn new(pn532: Pn532<I, T, N>) -> Self {
        Self { pn532 }
    }

    /// Выполняет одну низкоуровневую попытку инициализации PN532 через `SAMConfiguration`.
    pub fn init(&mut self) -> Result<(), NfcError<I::Error>> {
        self.pn532.process(
            &Request::sam_configuration(SAMMode::Normal, false),
            0,
            INIT_TIMEOUT,
        )?;
        Ok(())
    }

    /// Инициализирует PN532 с параметрами по умолчанию.
    pub fn init_default(&mut self) -> Result<(), NfcError<I::Error>> {
        self.init_with_config(NfcInitConfig::default())
    }

    /// Инициализирует PN532 с заданной конфигурацией задержек и retry.
    pub fn init_with_config(&mut self, config: NfcInitConfig) -> Result<(), NfcError<I::Error>> {
        if config.attempts == 0 {
            return Err(NfcError::InvalidInitConfig(
                "attempts must be greater than zero",
            ));
        }

        thread::sleep(config.startup_delay);

        let mut last_error = None;
        for attempt in 0..config.attempts {
            match self.init() {
                Ok(()) => return Ok(()),
                Err(err) => {
                    last_error = Some(err);

                    if attempt + 1 < config.attempts {
                        thread::sleep(config.retry_delay);
                    }
                }
            }
        }

        Err(last_error.expect("attempts > 0 ensures at least one init attempt"))
    }

    /// Читает версию firmware PN532.
    pub fn firmware_version(&mut self) -> Result<[u8; 4], NfcError<I::Error>> {
        let response = self.pn532.process(
            &Request::GET_FIRMWARE_VERSION,
            FIRMWARE_VERSION_RESPONSE_BYTES,
            INIT_TIMEOUT,
        )?;

        if response.len() < FIRMWARE_VERSION_RESPONSE_BYTES {
            return Err(NfcError::InvalidResponse(
                "firmware version is shorter than 4 bytes",
            ));
        }

        Ok(response[..FIRMWARE_VERSION_RESPONSE_BYTES]
            .try_into()
            .expect("slice length checked"))
    }

    /// Опрашивает поле и возвращает найденную Type A метку.
    pub fn poll_tag(&mut self, timeout: Duration) -> Result<Option<TagInfo>, NfcError<I::Error>> {
        match self.pn532.process(
            &Request::INLIST_ONE_ISO_A_TARGET,
            POLL_TAG_EXPECTED_LEN,
            timeout,
        ) {
            Ok(response) => Ok(Some(parse_tag_info(response)?)),
            Err(pn532::Error::TimeoutAck) | Err(pn532::Error::TimeoutResponse) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Читает key-value данные с метки.
    pub fn read_kv_store(&mut self) -> Result<KvStore, NfcError<I::Error>> {
        let text = self.read_text_payload()?;
        Ok(KvStore::from_text(&text)?)
    }

    /// Записывает key-value данные на метку.
    pub fn write_kv_store(&mut self, store: &KvStore) -> Result<(), NfcError<I::Error>> {
        let text = store.to_text()?;
        self.write_text_payload(&text)
    }

    fn read_text_payload(&mut self) -> Result<String, NfcError<I::Error>> {
        let user_data = self.read_user_area()?;
        let ndef_bytes = extract_ndef_tlv(&user_data)?;
        decode_text_record(ndef_bytes).map_err(Into::into)
    }

    fn write_text_payload(&mut self, text: &str) -> Result<(), NfcError<I::Error>> {
        let capacity = self.read_user_capacity()?;
        let ndef_bytes = encode_text_record(text)?;
        let tlv = encode_ndef_tlv(&ndef_bytes);

        if tlv.len() > capacity {
            return Err(NfcError::PayloadTooLarge {
                payload_len: tlv.len(),
                capacity,
            });
        }

        let mut user_data = vec![0u8; capacity];
        user_data[..tlv.len()].copy_from_slice(&tlv);
        self.write_user_area(&user_data)
    }

    fn read_user_capacity(&mut self) -> Result<usize, NfcError<I::Error>> {
        let cc_block = self.read_four_pages(CAPABILITY_CONTAINER_PAGE)?;
        let cc = [cc_block[0], cc_block[1], cc_block[2], cc_block[3]];

        if cc[0] != CAPABILITY_CONTAINER_MAGIC || cc[2] == 0 {
            return Err(NfcError::InvalidCapabilityContainer(cc));
        }

        Ok(cc[2] as usize * CAPABILITY_TO_USER_BYTES_MULTIPLIER)
    }

    fn read_user_area(&mut self) -> Result<Vec<u8>, NfcError<I::Error>> {
        let capacity = self.read_user_capacity()?;
        let block_count = capacity.div_ceil(READ_BLOCK_BYTES);
        let mut data = Vec::with_capacity(block_count * READ_BLOCK_BYTES);

        for block_index in 0..block_count {
            let page = USER_START_PAGE + (block_index as u8 * READ_BLOCK_PAGES);
            let bytes = self.read_four_pages(page)?;
            data.extend_from_slice(&bytes);
        }

        data.truncate(capacity);
        Ok(data)
    }

    fn write_user_area(&mut self, user_data: &[u8]) -> Result<(), NfcError<I::Error>> {
        if user_data.len() % PAGE_SIZE != 0 {
            return Err(NfcError::InvalidResponse(
                "user area length is not page-aligned",
            ));
        }

        for (page_offset, chunk) in user_data.chunks(PAGE_SIZE).enumerate() {
            let page = USER_START_PAGE + page_offset as u8;
            let page_bytes: [u8; PAGE_SIZE] = chunk
                .try_into()
                .expect("chunks are always PAGE_SIZE bytes long");
            self.write_page(page, &page_bytes)?;
        }

        Ok(())
    }

    fn read_four_pages(
        &mut self,
        start_page: u8,
    ) -> Result<[u8; READ_BLOCK_BYTES], NfcError<I::Error>> {
        let response = self.pn532.process(
            &Request::ntag_read(start_page),
            READ_BLOCK_RESPONSE_BYTES,
            COMMAND_TIMEOUT,
        )?;

        if response.len() < READ_BLOCK_RESPONSE_BYTES {
            return Err(NfcError::InvalidResponse(
                "ntag_read returned fewer than 17 bytes",
            ));
        }

        if response[0] != 0x00 {
            return Err(NfcError::TagStatus(response[0]));
        }

        let mut data = [0u8; READ_BLOCK_BYTES];
        data.copy_from_slice(&response[1..READ_BLOCK_RESPONSE_BYTES]);
        Ok(data)
    }

    fn write_page(&mut self, page: u8, bytes: &[u8; PAGE_SIZE]) -> Result<(), NfcError<I::Error>> {
        let response = self
            .pn532
            .process(&Request::ntag_write(page, bytes), 1, COMMAND_TIMEOUT)?;

        if response.is_empty() {
            return Err(NfcError::InvalidResponse(
                "ntag_write returned an empty response",
            ));
        }

        if response[0] != 0x00 {
            return Err(NfcError::TagStatus(response[0]));
        }

        Ok(())
    }
}

pub(crate) fn parse_tag_info<E: Debug>(response: &[u8]) -> Result<TagInfo, NfcError<E>> {
    if response.len() < 6 {
        return Err(NfcError::InvalidResponse(
            "InListPassiveTarget response is shorter than 6 bytes",
        ));
    }

    if response[0] == 0 {
        return Err(NfcError::InvalidResponse(
            "InListPassiveTarget reports zero targets",
        ));
    }

    let uid_len = response[5] as usize;
    let expected_len = 6 + uid_len;
    if response.len() < expected_len {
        return Err(NfcError::InvalidResponse(
            "InListPassiveTarget response is shorter than UID length",
        ));
    }

    Ok(TagInfo {
        atqa: [response[2], response[3]],
        sak: response[4],
        uid: response[6..6 + uid_len].to_vec(),
    })
}
