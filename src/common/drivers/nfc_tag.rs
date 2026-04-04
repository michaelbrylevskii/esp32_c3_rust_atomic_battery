//! High-level обёртка над `pn532` для чтения и записи key-value данных на NFC Tag.
//!
//! Подробная документация на русском:
//! [docs/nfc_tag.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/nfc_tag.md)

use core::fmt::{self, Debug};
use core::time::Duration;
use std::string::String;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::vec::Vec;

use ndef::{Message, Payload, Record, RecordType};
use pn532::{requests::SAMMode, CountDown, Interface, Pn532, Request};

const USER_START_PAGE: u8 = 4;
const CAPABILITY_CONTAINER_PAGE: u8 = 3;
const PAGE_SIZE: usize = 4;
const READ_BLOCK_BYTES: usize = 16;
const READ_BLOCK_PAGES: u8 = 4;
const COMMAND_TIMEOUT: Duration = Duration::from_millis(100);
const INIT_TIMEOUT: Duration = Duration::from_millis(50);
const DEFAULT_INIT_STARTUP_DELAY: Duration = Duration::from_millis(200);
const DEFAULT_INIT_RETRY_DELAY: Duration = Duration::from_millis(200);
const DEFAULT_INIT_ATTEMPTS: usize = 5;
const TLV_NDEF_MESSAGE: u8 = 0x03;
const TLV_NULL: u8 = 0x00;
const TLV_TERMINATOR: u8 = 0xFE;
const TLV_LOCK_CONTROL: u8 = 0x01;
const TLV_MEMORY_CONTROL: u8 = 0x02;
const TLV_PROPRIETARY: u8 = 0xFD;
const KV_HEADER: &str = "KV1";

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

/// Поддерживаемые типы значений для key-value хранилища.
#[derive(Clone, Debug, PartialEq)]
pub enum KvValue {
    /// UTF-8 строка.
    Str(String),
    /// Беззнаковое 8-битное число.
    U8(u8),
    /// Беззнаковое 16-битное число.
    U16(u16),
    /// Беззнаковое 32-битное число.
    U32(u32),
    /// Беззнаковое 64-битное число.
    U64(u64),
    /// Знаковое 8-битное число.
    I8(i8),
    /// Знаковое 16-битное число.
    I16(i16),
    /// Знаковое 32-битное число.
    I32(i32),
    /// Знаковое 64-битное число.
    I64(i64),
    /// 32-битное число с плавающей точкой.
    ///
    /// Значение должно быть конечным: без `NaN`, `+inf`, `-inf`.
    F32(f32),
    /// 64-битное число с плавающей точкой.
    ///
    /// Значение должно быть конечным: без `NaN`, `+inf`, `-inf`.
    F64(f64),
    /// Булево значение.
    Bool(bool),
}

/// Одна запись key-value хранилища.
#[derive(Clone, Debug, PartialEq)]
pub struct KvEntry {
    /// ASCII-ключ.
    pub key: String,
    /// Значение ключа.
    pub value: KvValue,
}

/// Набор key-value записей, который сериализуется в NDEF Text Record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KvStore {
    entries: Vec<KvEntry>,
}

impl KvStore {
    /// Создаёт пустое key-value хранилище.
    pub fn new() -> Self {
        Self::default()
    }

    /// Возвращает все записи в порядке хранения.
    pub fn entries(&self) -> &[KvEntry] {
        &self.entries
    }

    /// Возвращает значение по ключу, если оно есть.
    pub fn get(&self, key: &str) -> Option<&KvValue> {
        self.entries
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| &entry.value)
    }

    /// Вставляет или обновляет запись.
    ///
    /// Проверяет ключ и значение на соответствие ограничениям формата.
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: KvValue,
    ) -> Result<&mut Self, KvFormatError> {
        let key = key.into();
        validate_key(&key)?;
        validate_value(&value)?;

        if let Some(existing) = self.entries.iter_mut().find(|entry| entry.key == key) {
            existing.value = value;
        } else {
            self.entries.push(KvEntry { key, value });
        }

        Ok(self)
    }

    /// Удобный helper для строкового значения.
    pub fn insert_string(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<&mut Self, KvFormatError> {
        self.insert(key, KvValue::Str(value.into()))
    }

    /// Удобный helper для `u8`.
    pub fn insert_u8(
        &mut self,
        key: impl Into<String>,
        value: u8,
    ) -> Result<&mut Self, KvFormatError> {
        self.insert(key, KvValue::U8(value))
    }

    /// Удобный helper для `u16`.
    pub fn insert_u16(
        &mut self,
        key: impl Into<String>,
        value: u16,
    ) -> Result<&mut Self, KvFormatError> {
        self.insert(key, KvValue::U16(value))
    }

    /// Удобный helper для `u32`.
    pub fn insert_u32(
        &mut self,
        key: impl Into<String>,
        value: u32,
    ) -> Result<&mut Self, KvFormatError> {
        self.insert(key, KvValue::U32(value))
    }

    /// Удобный helper для `u64`.
    pub fn insert_u64(
        &mut self,
        key: impl Into<String>,
        value: u64,
    ) -> Result<&mut Self, KvFormatError> {
        self.insert(key, KvValue::U64(value))
    }

    /// Удобный helper для `i8`.
    pub fn insert_i8(
        &mut self,
        key: impl Into<String>,
        value: i8,
    ) -> Result<&mut Self, KvFormatError> {
        self.insert(key, KvValue::I8(value))
    }

    /// Удобный helper для `i16`.
    pub fn insert_i16(
        &mut self,
        key: impl Into<String>,
        value: i16,
    ) -> Result<&mut Self, KvFormatError> {
        self.insert(key, KvValue::I16(value))
    }

    /// Удобный helper для `i32`.
    pub fn insert_i32(
        &mut self,
        key: impl Into<String>,
        value: i32,
    ) -> Result<&mut Self, KvFormatError> {
        self.insert(key, KvValue::I32(value))
    }

    /// Удобный helper для `i64`.
    pub fn insert_i64(
        &mut self,
        key: impl Into<String>,
        value: i64,
    ) -> Result<&mut Self, KvFormatError> {
        self.insert(key, KvValue::I64(value))
    }

    /// Удобный helper для `f32`.
    pub fn insert_f32(
        &mut self,
        key: impl Into<String>,
        value: f32,
    ) -> Result<&mut Self, KvFormatError> {
        self.insert(key, KvValue::F32(value))
    }

    /// Удобный helper для `f64`.
    pub fn insert_f64(
        &mut self,
        key: impl Into<String>,
        value: f64,
    ) -> Result<&mut Self, KvFormatError> {
        self.insert(key, KvValue::F64(value))
    }

    /// Удобный helper для `bool`.
    pub fn insert_bool(
        &mut self,
        key: impl Into<String>,
        value: bool,
    ) -> Result<&mut Self, KvFormatError> {
        self.insert(key, KvValue::Bool(value))
    }

    /// Сериализует хранилище в внутренний текстовый формат `KV1`.
    ///
    /// Этот текст потом кладётся в NDEF Text Record.
    pub fn to_text(&self) -> Result<String, KvFormatError> {
        let mut text = String::from(KV_HEADER);

        for entry in &self.entries {
            validate_key(&entry.key)?;
            validate_value(&entry.value)?;
            text.push('\n');
            text.push_str(&entry.key);
            text.push('=');
            text.push_str(entry.value.type_tag());
            text.push(':');
            text.push_str(&entry.value.to_storage_value());
        }

        Ok(text)
    }

    /// Парсит внутренний текстовый формат `KV1` обратно в `KvStore`.
    pub fn from_text(text: &str) -> Result<Self, KvFormatError> {
        let mut lines = text.lines();
        let Some(header) = lines.next() else {
            return Err(KvFormatError::MissingHeader);
        };

        if header != KV_HEADER {
            return Err(KvFormatError::InvalidHeader);
        }

        let mut store = KvStore::new();
        for line in lines {
            if line.is_empty() {
                continue;
            }

            let (key, rest) = line
                .split_once('=')
                .ok_or(KvFormatError::InvalidEntryFormat)?;
            let (type_tag, raw_value) = rest
                .split_once(':')
                .ok_or(KvFormatError::InvalidEntryFormat)?;
            let value = KvValue::from_parts(type_tag, raw_value)?;
            store.insert(key.to_owned(), value)?;
        }

        Ok(store)
    }
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

impl KvValue {
    fn type_tag(&self) -> &'static str {
        match self {
            KvValue::Str(_) => "S",
            KvValue::U8(_) => "U8",
            KvValue::U16(_) => "U16",
            KvValue::U32(_) => "U32",
            KvValue::U64(_) => "U64",
            KvValue::I8(_) => "I8",
            KvValue::I16(_) => "I16",
            KvValue::I32(_) => "I32",
            KvValue::I64(_) => "I64",
            KvValue::F32(_) => "F32",
            KvValue::F64(_) => "F64",
            KvValue::Bool(_) => "B",
        }
    }

    fn to_storage_value(&self) -> String {
        match self {
            KvValue::Str(value) => escape_string_value(value),
            KvValue::U8(value) => value.to_string(),
            KvValue::U16(value) => value.to_string(),
            KvValue::U32(value) => value.to_string(),
            KvValue::U64(value) => value.to_string(),
            KvValue::I8(value) => value.to_string(),
            KvValue::I16(value) => value.to_string(),
            KvValue::I32(value) => value.to_string(),
            KvValue::I64(value) => value.to_string(),
            KvValue::F32(value) => value.to_string(),
            KvValue::F64(value) => value.to_string(),
            KvValue::Bool(value) => {
                if *value {
                    String::from("1")
                } else {
                    String::from("0")
                }
            }
        }
    }

    fn from_parts(type_tag: &str, raw_value: &str) -> Result<Self, KvFormatError> {
        match type_tag {
            "S" => Ok(KvValue::Str(unescape_string_value(raw_value)?)),
            "U8" => raw_value
                .parse::<u8>()
                .map(KvValue::U8)
                .map_err(|_| KvFormatError::InvalidNumber),
            "U16" => raw_value
                .parse::<u16>()
                .map(KvValue::U16)
                .map_err(|_| KvFormatError::InvalidNumber),
            "U32" => raw_value
                .parse::<u32>()
                .map(KvValue::U32)
                .map_err(|_| KvFormatError::InvalidNumber),
            "U64" => raw_value
                .parse::<u64>()
                .map(KvValue::U64)
                .map_err(|_| KvFormatError::InvalidNumber),
            "I8" => raw_value
                .parse::<i8>()
                .map(KvValue::I8)
                .map_err(|_| KvFormatError::InvalidNumber),
            "I16" => raw_value
                .parse::<i16>()
                .map(KvValue::I16)
                .map_err(|_| KvFormatError::InvalidNumber),
            "I32" => raw_value
                .parse::<i32>()
                .map(KvValue::I32)
                .map_err(|_| KvFormatError::InvalidNumber),
            "I64" => raw_value
                .parse::<i64>()
                .map(KvValue::I64)
                .map_err(|_| KvFormatError::InvalidNumber),
            "F32" => {
                let value = raw_value
                    .parse::<f32>()
                    .map_err(|_| KvFormatError::InvalidNumber)?;
                if !value.is_finite() {
                    return Err(KvFormatError::NonFiniteFloat("f32"));
                }
                Ok(KvValue::F32(value))
            }
            "F64" => {
                let value = raw_value
                    .parse::<f64>()
                    .map_err(|_| KvFormatError::InvalidNumber)?;
                if !value.is_finite() {
                    return Err(KvFormatError::NonFiniteFloat("f64"));
                }
                Ok(KvValue::F64(value))
            }
            "B" => match raw_value {
                "0" => Ok(KvValue::Bool(false)),
                "1" => Ok(KvValue::Bool(true)),
                _ => Err(KvFormatError::InvalidBoolean),
            },
            _ => Err(KvFormatError::InvalidTypeTag),
        }
    }
}

/// Ошибки формата key-value и NDEF Text payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KvFormatError {
    MissingHeader,
    InvalidHeader,
    InvalidEntryFormat,
    InvalidTypeTag,
    InvalidNumber,
    InvalidBoolean,
    InvalidKey,
    InvalidEscapeSequence,
    TrailingEscape,
    NonFiniteFloat(&'static str),
    InvalidNdef,
    MissingTextRecord,
    MessageTooLarge,
}

impl fmt::Display for KvFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for KvFormatError {}

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

/// Настройки фонового NFC worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsyncNfcConfig {
    /// Частота итераций worker-потока.
    pub poll_interval: Duration,
    /// Таймаут одной попытки `poll_tag`.
    pub poll_timeout: Duration,
    /// Сколько держать последнюю метку после единичных промахов опроса.
    pub removal_debounce: Duration,
    /// Размер стека фонового потока.
    pub thread_stack_size: usize,
}

impl Default for AsyncNfcConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(30),
            poll_timeout: Duration::from_millis(40),
            removal_debounce: Duration::from_millis(1200),
            thread_stack_size: 8192,
        }
    }
}

/// Результат чтения payload у текущей метки.
#[derive(Clone, Debug, PartialEq)]
pub enum AsyncTagPayload {
    KvStore(KvStore),
    Empty,
    ReadError(String),
}

/// Снэпшот последней увиденной метки в фоне.
#[derive(Clone, Debug, PartialEq)]
pub struct AsyncObservedTag {
    pub info: TagInfo,
    pub payload: AsyncTagPayload,
}

/// Состояние, которое возвращает async NFC worker.
#[derive(Clone, Debug, PartialEq)]
pub struct AsyncNfcSnapshot {
    pub generation: u64,
    pub tag: Option<AsyncObservedTag>,
}

#[derive(Debug)]
pub enum AsyncNfcError {
    ThreadSpawn(std::io::Error),
    WorkerStopped,
    WorkerFailed(String),
    InvalidWorkerConfig(&'static str),
    CommandFailed(String),
}

impl fmt::Display for AsyncNfcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsyncNfcError::ThreadSpawn(err) => write!(f, "failed to spawn NFC worker: {err}"),
            AsyncNfcError::WorkerStopped => write!(f, "NFC worker has stopped"),
            AsyncNfcError::WorkerFailed(err) => write!(f, "NFC worker failed: {err}"),
            AsyncNfcError::InvalidWorkerConfig(err) => {
                write!(f, "invalid NFC worker config: {err}")
            }
            AsyncNfcError::CommandFailed(err) => write!(f, "NFC command failed: {err}"),
        }
    }
}

impl std::error::Error for AsyncNfcError {}

#[derive(Clone)]
struct AsyncNfcState {
    snapshot: AsyncNfcSnapshot,
    shutdown: bool,
}

impl AsyncNfcState {
    fn new() -> Self {
        Self {
            snapshot: AsyncNfcSnapshot {
                generation: 0,
                tag: None,
            },
            shutdown: false,
        }
    }
}

enum AsyncNfcCommand {
    WriteKvStore {
        expected_uid: Vec<u8>,
        store: KvStore,
        reply: mpsc::Sender<Result<(), String>>,
    },
}

/// Неблокирующая обёртка над `NfcTag`.
///
/// Worker-поток сам опрашивает PN532 и кэширует последнюю увиденную метку,
/// а основной код читает только snapshot и отправляет редкие команды записи.
pub struct AsyncNfcTag<I, T, const N: usize>
where
    I: Interface + Send + 'static,
    I::Error: Debug + fmt::Display + Send + 'static,
    T: CountDown<Time = Duration> + Send + 'static,
{
    state: Arc<Mutex<AsyncNfcState>>,
    worker_error: Arc<Mutex<Option<String>>>,
    command_tx: mpsc::Sender<AsyncNfcCommand>,
    worker: Option<JoinHandle<()>>,
    _marker: core::marker::PhantomData<(I, T)>,
}

impl<I, T, const N: usize> AsyncNfcTag<I, T, N>
where
    I: Interface + Send + 'static,
    I::Error: Debug + fmt::Display + Send + 'static,
    T: CountDown<Time = Duration> + Send + 'static,
{
    pub fn new(mut nfc: NfcTag<I, T, N>, config: AsyncNfcConfig) -> Result<Self, AsyncNfcError> {
        if config.poll_interval.is_zero() {
            return Err(AsyncNfcError::InvalidWorkerConfig(
                "poll_interval must be greater than zero",
            ));
        }
        if config.poll_timeout.is_zero() {
            return Err(AsyncNfcError::InvalidWorkerConfig(
                "poll_timeout must be greater than zero",
            ));
        }
        if config.removal_debounce.is_zero() {
            return Err(AsyncNfcError::InvalidWorkerConfig(
                "removal_debounce must be greater than zero",
            ));
        }

        let state = Arc::new(Mutex::new(AsyncNfcState::new()));
        let worker_error = Arc::new(Mutex::new(None));
        let (command_tx, command_rx) = mpsc::channel();

        let worker_state = Arc::clone(&state);
        let worker_error_slot = Arc::clone(&worker_error);
        let worker = thread::Builder::new()
            .name("pn532-worker".into())
            .stack_size(config.thread_stack_size)
            .spawn(move || {
                run_async_nfc_worker(
                    &mut nfc,
                    worker_state,
                    worker_error_slot,
                    command_rx,
                    config,
                );
            })
            .map_err(AsyncNfcError::ThreadSpawn)?;

        Ok(Self {
            state,
            worker_error,
            command_tx,
            worker: Some(worker),
            _marker: core::marker::PhantomData,
        })
    }

    pub fn snapshot(&self) -> Result<AsyncNfcSnapshot, AsyncNfcError> {
        self.ensure_worker_alive()?;
        let guard = self
            .state
            .lock()
            .map_err(|_| AsyncNfcError::WorkerFailed("NFC state mutex poisoned".into()))?;
        Ok(guard.snapshot.clone())
    }

    pub fn write_kv_store_for_tag(
        &self,
        expected_uid: &[u8],
        store: &KvStore,
    ) -> Result<(), AsyncNfcError> {
        self.ensure_worker_alive()?;

        let (reply_tx, reply_rx) = mpsc::channel();
        self.command_tx
            .send(AsyncNfcCommand::WriteKvStore {
                expected_uid: expected_uid.to_vec(),
                store: store.clone(),
                reply: reply_tx,
            })
            .map_err(|_| AsyncNfcError::WorkerStopped)?;

        reply_rx
            .recv()
            .map_err(|_| AsyncNfcError::WorkerStopped)?
            .map_err(AsyncNfcError::CommandFailed)
    }

    pub fn last_worker_error(&self) -> Option<String> {
        self.worker_error
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn ensure_worker_alive(&self) -> Result<(), AsyncNfcError> {
        let worker_error = self
            .worker_error
            .lock()
            .map_err(|_| AsyncNfcError::WorkerFailed("NFC error mutex poisoned".into()))?;

        if let Some(error) = worker_error.as_ref() {
            return Err(AsyncNfcError::WorkerFailed(error.clone()));
        }

        Ok(())
    }
}

impl<I, T, const N: usize> Drop for AsyncNfcTag<I, T, N>
where
    I: Interface + Send + 'static,
    I::Error: Debug + fmt::Display + Send + 'static,
    T: CountDown<Time = Duration> + Send + 'static,
{
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.shutdown = true;
        }

        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Вспомогательный слой для быстрого подключения `NfcTag` в проектах на `esp-idf-svc`.
pub mod esp_idf {
    use super::{AsyncNfcConfig, AsyncNfcTag, NfcTag};
    use core::convert::Infallible;
    use core::time::Duration;
    use esp_idf_svc::hal::gpio::{InputPin, OutputPin};
    use esp_idf_svc::hal::i2c::{I2c, I2cConfig, I2cDriver};
    use esp_idf_svc::hal::units::Hertz;
    use esp_idf_svc::sys::EspError;
    use pn532::{i2c::I2CInterface, CountDown, Pn532};
    use std::time::Instant;

    /// Стандартная скорость I2C для PN532 helper-конструктора.
    pub const DEFAULT_BAUDRATE: Hertz = Hertz(100_000);

    /// Готовый тип `NfcTag` для `esp-idf` c `I2cDriver` и стандартным таймером.
    pub type EspNfcTag<'d> = NfcTag<I2CInterface<I2cDriver<'d>>, StdTimer, 64>;
    /// Async-тип `NfcTag` для `esp-idf` с тем же low-level transport.
    pub type AsyncEspNfcTag<'d> = AsyncNfcTag<I2CInterface<I2cDriver<'d>>, StdTimer, 64>;

    /// Таймер для `pn532::CountDown`, реализованный через `std::time::Instant`.
    #[derive(Debug)]
    pub struct StdTimer {
        deadline: Option<Instant>,
    }

    impl StdTimer {
        /// Создаёт таймер для PN532.
        pub fn new() -> Self {
            Self { deadline: None }
        }
    }

    impl Default for StdTimer {
        fn default() -> Self {
            Self::new()
        }
    }

    impl CountDown for StdTimer {
        type Time = Duration;

        fn start<T>(&mut self, count: T)
        where
            T: Into<Self::Time>,
        {
            self.deadline = Some(Instant::now() + count.into());
        }

        fn wait(&mut self) -> pn532::nb::Result<(), Infallible> {
            match self.deadline {
                Some(deadline) if Instant::now() >= deadline => Ok(()),
                _ => Err(pn532::nb::Error::WouldBlock),
            }
        }
    }

    /// Создаёт `NfcTag` из уже готового `I2cDriver`.
    pub fn new_with_driver<'d>(i2c: I2cDriver<'d>) -> EspNfcTag<'d> {
        let interface = I2CInterface { i2c };
        let timer = StdTimer::new();
        let pn532 = Pn532::new(interface, timer);
        NfcTag::new(pn532)
    }

    /// Создаёт `NfcTag`, сам поднимая `I2cDriver` с указанной скоростью шины.
    pub fn new<'d, I2C>(
        i2c: I2C,
        sda: impl InputPin + OutputPin + 'd,
        scl: impl InputPin + OutputPin + 'd,
        baudrate: Hertz,
    ) -> Result<EspNfcTag<'d>, EspError>
    where
        I2C: I2c + 'd,
    {
        let config = I2cConfig::new().baudrate(baudrate.into());
        let driver = I2cDriver::new(i2c, sda, scl, &config)?;
        Ok(new_with_driver(driver))
    }

    /// Создаёт `NfcTag` со стандартной скоростью I2C `100 kHz`.
    pub fn new_default<'d, I2C>(
        i2c: I2C,
        sda: impl InputPin + OutputPin + 'd,
        scl: impl InputPin + OutputPin + 'd,
    ) -> Result<EspNfcTag<'d>, EspError>
    where
        I2C: I2c + 'd,
    {
        new(i2c, sda, scl, DEFAULT_BAUDRATE)
    }

    /// Создаёт и сразу запускает async NFC worker для `esp-idf`.
    pub fn new_async_default<I2C>(
        i2c: I2C,
        sda: impl InputPin + OutputPin + 'static,
        scl: impl InputPin + OutputPin + 'static,
        worker_config: AsyncNfcConfig,
    ) -> Result<AsyncEspNfcTag<'static>, super::AsyncNfcError>
    where
        I2C: I2c + 'static,
    {
        let nfc = new_default(i2c, sda, scl)
            .map_err(|err| super::AsyncNfcError::CommandFailed(err.to_string()))?;
        AsyncNfcTag::new(nfc, worker_config)
    }
}

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
    ///
    /// Этот метод не делает retry и не добавляет стартовую задержку.
    pub fn init(&mut self) -> Result<(), NfcError<I::Error>> {
        self.pn532.process(
            &Request::sam_configuration(SAMMode::Normal, false),
            0,
            INIT_TIMEOUT,
        )?;
        Ok(())
    }

    /// Инициализирует PN532 с параметрами по умолчанию.
    ///
    /// Под капотом добавляет стартовую паузу и несколько попыток `init()`.
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
        let response = self
            .pn532
            .process(&Request::GET_FIRMWARE_VERSION, 4, INIT_TIMEOUT)?;

        if response.len() < 4 {
            return Err(NfcError::InvalidResponse(
                "firmware version is shorter than 4 bytes",
            ));
        }

        Ok(response[..4].try_into().expect("slice length checked"))
    }

    /// Опрашивает поле и возвращает найденную Type A метку.
    ///
    /// Возвращает `Ok(None)`, если метки в поле нет.
    pub fn poll_tag(&mut self, timeout: Duration) -> Result<Option<TagInfo>, NfcError<I::Error>> {
        match self
            .pn532
            .process(&Request::INLIST_ONE_ISO_A_TARGET, 32, timeout)
        {
            Ok(response) => Ok(Some(parse_tag_info(response)?)),
            Err(pn532::Error::TimeoutAck) | Err(pn532::Error::TimeoutResponse) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Читает key-value данные с метки.
    ///
    /// Метод ожидает, что на метке лежит NDEF Text Record в формате `KV1`.
    pub fn read_kv_store(&mut self) -> Result<KvStore, NfcError<I::Error>> {
        let text = self.read_text_payload()?;
        Ok(KvStore::from_text(&text)?)
    }

    /// Записывает key-value данные на метку.
    ///
    /// Текущая реализация переписывает всю пользовательскую NDEF-область целиком.
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

        if cc[0] != 0xE1 || cc[2] == 0 {
            return Err(NfcError::InvalidCapabilityContainer(cc));
        }

        Ok(cc[2] as usize * 8)
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
        let response = self
            .pn532
            .process(&Request::ntag_read(start_page), 17, COMMAND_TIMEOUT)?;

        if response.len() < 17 {
            return Err(NfcError::InvalidResponse(
                "ntag_read returned fewer than 17 bytes",
            ));
        }

        if response[0] != 0x00 {
            return Err(NfcError::TagStatus(response[0]));
        }

        let mut data = [0u8; READ_BLOCK_BYTES];
        data.copy_from_slice(&response[1..17]);
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

fn run_async_nfc_worker<I, T, const N: usize>(
    nfc: &mut NfcTag<I, T, N>,
    state: Arc<Mutex<AsyncNfcState>>,
    worker_error: Arc<Mutex<Option<String>>>,
    command_rx: mpsc::Receiver<AsyncNfcCommand>,
    config: AsyncNfcConfig,
) where
    I: Interface + Send + 'static,
    I::Error: Debug + fmt::Display + Send + 'static,
    T: CountDown<Time = Duration> + Send + 'static,
{
    let mut missing_since: Option<std::time::Instant> = None;

    loop {
        let shutdown = match state.lock() {
            Ok(guard) => guard.shutdown,
            Err(_) => {
                store_async_nfc_worker_error(&worker_error, "NFC state mutex poisoned".into());
                return;
            }
        };

        if shutdown {
            return;
        }

        while let Ok(command) = command_rx.try_recv() {
            if let Err(err) = process_async_nfc_command(nfc, &state, command) {
                store_async_nfc_worker_error(&worker_error, err);
                return;
            }
        }

        match nfc.poll_tag(config.poll_timeout) {
            Ok(Some(info)) => {
                missing_since = None;
                let should_reread = match state.lock() {
                    Ok(guard) => guard
                        .snapshot
                        .tag
                        .as_ref()
                        .map(|tag| tag.info.uid != info.uid)
                        .unwrap_or(true),
                    Err(_) => {
                        store_async_nfc_worker_error(
                            &worker_error,
                            "NFC state mutex poisoned".into(),
                        );
                        return;
                    }
                };

                if should_reread {
                    let payload = match nfc.read_kv_store() {
                        Ok(store) => AsyncTagPayload::KvStore(store),
                        Err(NfcError::NoNdefMessage) => AsyncTagPayload::Empty,
                        Err(err) => AsyncTagPayload::ReadError(err.to_string()),
                    };

                    let observed = AsyncObservedTag { info, payload };
                    if let Ok(mut guard) = state.lock() {
                        guard.snapshot.generation = guard.snapshot.generation.wrapping_add(1);
                        guard.snapshot.tag = Some(observed);
                    } else {
                        store_async_nfc_worker_error(
                            &worker_error,
                            "NFC state mutex poisoned".into(),
                        );
                        return;
                    }
                }
            }
            Ok(None) => {
                let now = std::time::Instant::now();
                let should_clear = match missing_since {
                    Some(started_at) => now.duration_since(started_at) >= config.removal_debounce,
                    None => {
                        missing_since = Some(now);
                        false
                    }
                };

                if should_clear {
                    if let Ok(mut guard) = state.lock() {
                        if guard.snapshot.tag.is_some() {
                            guard.snapshot.generation = guard.snapshot.generation.wrapping_add(1);
                            guard.snapshot.tag = None;
                        }
                    } else {
                        store_async_nfc_worker_error(
                            &worker_error,
                            "NFC state mutex poisoned".into(),
                        );
                        return;
                    }
                }
            }
            Err(err) => {
                if !matches!(
                    err,
                    NfcError::Pn532(pn532::Error::TimeoutAck)
                        | NfcError::Pn532(pn532::Error::TimeoutResponse)
                ) {
                    // Keep the last good snapshot after transient transport errors instead of
                    // treating them as an instant "tag removed" event.
                }
            }
        }

        thread::sleep(config.poll_interval);
    }
}

fn process_async_nfc_command<I, T, const N: usize>(
    nfc: &mut NfcTag<I, T, N>,
    state: &Arc<Mutex<AsyncNfcState>>,
    command: AsyncNfcCommand,
) -> Result<(), String>
where
    I: Interface + Send + 'static,
    I::Error: Debug + fmt::Display + Send + 'static,
    T: CountDown<Time = Duration> + Send + 'static,
{
    match command {
        AsyncNfcCommand::WriteKvStore {
            expected_uid,
            store,
            reply,
        } => {
            let current_uid = state
                .lock()
                .map_err(|_| "NFC state mutex poisoned".to_owned())?
                .snapshot
                .tag
                .as_ref()
                .map(|tag| tag.info.uid.clone());

            let result = match current_uid {
                Some(uid) if uid == expected_uid => nfc
                    .write_kv_store(&store)
                    .map_err(|err| err.to_string())
                    .map(|()| {
                        if let Ok(mut guard) = state.lock() {
                            if let Some(tag) = guard.snapshot.tag.as_mut() {
                                tag.payload = AsyncTagPayload::KvStore(store.clone());
                            }
                            guard.snapshot.generation = guard.snapshot.generation.wrapping_add(1);
                        }
                    }),
                Some(_) => Err("expected NFC tag is no longer present on the reader".to_owned()),
                None => Err("no NFC tag is currently present on the reader".to_owned()),
            };

            let _ = reply.send(result.map(|()| ()));
            Ok(())
        }
    }
}

fn store_async_nfc_worker_error(slot: &Arc<Mutex<Option<String>>>, error: String) {
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(error);
    }
}

fn parse_tag_info<E: Debug>(response: &[u8]) -> Result<TagInfo, NfcError<E>> {
    if response.len() < 6 {
        return Err(NfcError::InvalidResponse(
            "InListPassiveTarget response is shorter than 6 bytes",
        ));
    }

    if response[0] == 0 {
        return Err(NfcError::InvalidResponse("no targets reported in response"));
    }

    let uid_len = response[5] as usize;
    let uid_start = 6;
    let uid_end = uid_start + uid_len;

    if response.len() < uid_end {
        return Err(NfcError::InvalidResponse(
            "UID length exceeds response size",
        ));
    }

    Ok(TagInfo {
        uid: response[uid_start..uid_end].to_vec(),
        atqa: [response[2], response[3]],
        sak: response[4],
    })
}

fn validate_key(key: &str) -> Result<(), KvFormatError> {
    if key.is_empty() || !key.is_ascii() {
        return Err(KvFormatError::InvalidKey);
    }

    if key.contains('=') || key.contains(':') || key.contains('\n') || key.contains('\r') {
        return Err(KvFormatError::InvalidKey);
    }

    Ok(())
}

fn validate_value(value: &KvValue) -> Result<(), KvFormatError> {
    match value {
        KvValue::F32(number) if !number.is_finite() => Err(KvFormatError::NonFiniteFloat("f32")),
        KvValue::F64(number) if !number.is_finite() => Err(KvFormatError::NonFiniteFloat("f64")),
        _ => Ok(()),
    }
}

fn encode_text_record(text: &str) -> Result<Vec<u8>, KvFormatError> {
    let mut message = Message::default();
    let mut record = Record::new(
        None,
        Payload::RTD(RecordType::Text {
            enc: "en",
            txt: text,
        }),
    );
    message
        .append_record(&mut record)
        .map_err(|_| KvFormatError::MessageTooLarge)?;

    let bytes = message
        .to_vec()
        .map_err(|_| KvFormatError::MessageTooLarge)?;
    Ok(bytes.as_slice().to_vec())
}

fn decode_text_record(bytes: &[u8]) -> Result<String, KvFormatError> {
    let message = Message::try_from(bytes).map_err(|_| KvFormatError::InvalidNdef)?;
    let Some(record) = message.records.first() else {
        return Err(KvFormatError::MissingTextRecord);
    };

    match &record.payload {
        Payload::RTD(RecordType::Text { txt, .. }) => Ok((*txt).to_owned()),
        _ => Err(KvFormatError::MissingTextRecord),
    }
}

fn escape_string_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }

    escaped
}

fn unescape_string_value(value: &str) -> Result<String, KvFormatError> {
    let mut unescaped = String::with_capacity(value.len());
    let mut chars = value.chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            unescaped.push(ch);
            continue;
        }

        let Some(escaped) = chars.next() else {
            return Err(KvFormatError::TrailingEscape);
        };

        match escaped {
            '\\' => unescaped.push('\\'),
            'n' => unescaped.push('\n'),
            'r' => unescaped.push('\r'),
            't' => unescaped.push('\t'),
            _ => return Err(KvFormatError::InvalidEscapeSequence),
        }
    }

    Ok(unescaped)
}

fn encode_ndef_tlv(ndef_bytes: &[u8]) -> Vec<u8> {
    let mut tlv = Vec::with_capacity(ndef_bytes.len() + 4);
    tlv.push(TLV_NDEF_MESSAGE);

    if ndef_bytes.len() < 0xFF {
        tlv.push(ndef_bytes.len() as u8);
    } else {
        tlv.push(0xFF);
        tlv.push(((ndef_bytes.len() >> 8) & 0xFF) as u8);
        tlv.push((ndef_bytes.len() & 0xFF) as u8);
    }

    tlv.extend_from_slice(ndef_bytes);
    tlv.push(TLV_TERMINATOR);
    tlv
}

fn extract_ndef_tlv<E: Debug>(data: &[u8]) -> Result<&[u8], NfcError<E>> {
    let mut index = 0;

    while index < data.len() {
        match data[index] {
            TLV_NULL => {
                index += 1;
            }
            TLV_TERMINATOR => return Err(NfcError::NoNdefMessage),
            TLV_NDEF_MESSAGE => {
                let (value_start, value_len) = parse_tlv_length(data, index + 1)?;
                let value_end = value_start + value_len;
                if value_end > data.len() {
                    return Err(NfcError::TlvLengthOutOfBounds);
                }
                return Ok(&data[value_start..value_end]);
            }
            TLV_LOCK_CONTROL | TLV_MEMORY_CONTROL | TLV_PROPRIETARY => {
                let (value_start, value_len) = parse_tlv_length(data, index + 1)?;
                index = value_start + value_len;
            }
            tlv => return Err(NfcError::UnsupportedTlv(tlv)),
        }
    }

    Err(NfcError::NoNdefMessage)
}

fn parse_tlv_length<E: Debug>(
    data: &[u8],
    length_index: usize,
) -> Result<(usize, usize), NfcError<E>> {
    if length_index >= data.len() {
        return Err(NfcError::TlvLengthOutOfBounds);
    }

    if data[length_index] != 0xFF {
        return Ok((length_index + 1, data[length_index] as usize));
    }

    if length_index + 2 >= data.len() {
        return Err(NfcError::TlvLengthOutOfBounds);
    }

    let value_len = u16::from_be_bytes([data[length_index + 1], data[length_index + 2]]) as usize;
    Ok((length_index + 3, value_len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_store_text_roundtrip() {
        let mut store = KvStore::new();
        store.insert_string("name", "Привет,\nESP32-C3").unwrap();
        store.insert_u8("count", 42).unwrap();
        store.insert_u16("limit", 1024).unwrap();
        store.insert_u32("serial", 123_456).unwrap();
        store.insert_u64("energy", 9_876_543_210).unwrap();
        store.insert_i8("offset", -12).unwrap();
        store.insert_i16("bias", -32000).unwrap();
        store.insert_i32("temp_raw", -123_456).unwrap();
        store.insert_i64("distance", -9_876_543_210).unwrap();
        store.insert_f32("soc", 98.5).unwrap();
        store.insert_f64("voltage", 12.625).unwrap();
        store.insert_bool("enabled", true).unwrap();

        let text = store.to_text().unwrap();
        let decoded = KvStore::from_text(&text).unwrap();

        assert_eq!(decoded, store);
    }

    #[test]
    fn kv_store_rejects_non_finite_floats() {
        let mut store = KvStore::new();
        assert_eq!(
            store.insert_f32("bad32", f32::NAN).unwrap_err(),
            KvFormatError::NonFiniteFloat("f32")
        );
        assert_eq!(
            store.insert_f64("bad64", f64::INFINITY).unwrap_err(),
            KvFormatError::NonFiniteFloat("f64")
        );
        assert_eq!(
            KvStore::from_text("KV1\nvalue=F32:NaN").unwrap_err(),
            KvFormatError::NonFiniteFloat("f32")
        );
    }

    #[test]
    fn ndef_text_roundtrip() {
        let raw = encode_text_record("KV1\nname=S:Привет\\nмир").unwrap();
        let text = decode_text_record(&raw).unwrap();
        assert_eq!(text, "KV1\nname=S:Привет\\nмир");
    }

    #[test]
    fn string_escape_roundtrip() {
        let original = "Первая строка\nВторая строка\r\nПуть: C:\\tmp\tok";
        let escaped = escape_string_value(original);
        assert_eq!(unescape_string_value(&escaped).unwrap(), original);
    }

    #[test]
    fn string_escape_rejects_invalid_sequences() {
        assert_eq!(
            unescape_string_value("bad\\x").unwrap_err(),
            KvFormatError::InvalidEscapeSequence
        );
        assert_eq!(
            unescape_string_value("bad\\").unwrap_err(),
            KvFormatError::TrailingEscape
        );
    }

    #[test]
    fn tlv_roundtrip() {
        let tlv = encode_ndef_tlv(&[0xD1, 0x01, 0x05, 0x54, 0x02]);
        let ndef = extract_ndef_tlv::<core::convert::Infallible>(&tlv).unwrap();
        assert_eq!(ndef, &[0xD1, 0x01, 0x05, 0x54, 0x02]);
    }
}
