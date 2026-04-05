//! Именованные значения протокола и default-конфигов для `nfc_tag`.

use core::time::Duration;

/// Первая пользовательская страница Type 2 Tag.
pub const USER_START_PAGE: u8 = 4;
/// Страница capability container.
pub const CAPABILITY_CONTAINER_PAGE: u8 = 3;
/// Размер одной страницы NTAG в байтах.
pub const PAGE_SIZE: usize = 4;
/// Размер одного блока `ntag_read` без status byte.
pub const READ_BLOCK_BYTES: usize = 16;
/// Сколько страниц читает одна команда `ntag_read`.
pub const READ_BLOCK_PAGES: u8 = 4;
/// Ожидаемый размер ответа `ntag_read`, включая status byte.
pub const READ_BLOCK_RESPONSE_BYTES: usize = 17;
/// Ожидаемый размер firmware response.
pub const FIRMWARE_VERSION_RESPONSE_BYTES: usize = 4;
/// Запас expected_len для `INLIST_ONE_ISO_A_TARGET`.
pub const POLL_TAG_EXPECTED_LEN: usize = 32;
/// Таймаут низкоуровневой команды чтения/записи NTAG.
pub const COMMAND_TIMEOUT: Duration = Duration::from_millis(100);
/// Таймаут для коротких init-команд PN532.
pub const INIT_TIMEOUT: Duration = Duration::from_millis(50);
/// Стартовая пауза перед первой init-попыткой.
pub const DEFAULT_INIT_STARTUP_DELAY: Duration = Duration::from_millis(200);
/// Пауза между retry инициализации.
pub const DEFAULT_INIT_RETRY_DELAY: Duration = Duration::from_millis(200);
/// Число попыток инициализации по умолчанию.
pub const DEFAULT_INIT_ATTEMPTS: usize = 5;

/// TLV type для NDEF message.
pub const TLV_NDEF_MESSAGE: u8 = 0x03;
/// TLV NULL.
pub const TLV_NULL: u8 = 0x00;
/// TLV terminator.
pub const TLV_TERMINATOR: u8 = 0xFE;
/// TLV lock control.
pub const TLV_LOCK_CONTROL: u8 = 0x01;
/// TLV memory control.
pub const TLV_MEMORY_CONTROL: u8 = 0x02;
/// Proprietary TLV.
pub const TLV_PROPRIETARY: u8 = 0xFD;

/// Магическое значение capability container у Type 2 Tag.
pub const CAPABILITY_CONTAINER_MAGIC: u8 = 0xE1;
/// Множитель перевода `cc[2]` в число пользовательских байт.
pub const CAPABILITY_TO_USER_BYTES_MULTIPLIER: usize = 8;

/// Период опроса async NFC worker по умолчанию.
pub const DEFAULT_ASYNC_POLL_INTERVAL: Duration = Duration::from_millis(30);
/// Таймаут одной попытки опроса async NFC worker.
pub const DEFAULT_ASYNC_POLL_TIMEOUT: Duration = Duration::from_millis(40);
/// Debounce перед признанием метки реально пропавшей.
pub const DEFAULT_ASYNC_REMOVAL_DEBOUNCE: Duration = Duration::from_millis(1200);
/// Размер стека async NFC worker по умолчанию.
pub const DEFAULT_ASYNC_THREAD_STACK_SIZE: usize = 8192;
