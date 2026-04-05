# `nfc_tag`: high-level NFC-обёртка над PN532

## Что это такое

Модуль [`nfc_tag`](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/src/common/drivers/nfc_tag/mod.rs) это high-level слой поверх:

- `pn532` как транспорта и набора низкоуровневых команд
- NTAG / Type 2 Tag как памяти страницами
- TLV как контейнера NDEF-сообщения
- NDEF Text Record как формата полезной нагрузки

Прикладной key-value формат после рефакторинга вынесен отдельно:

- [`common::utils::kv_store`](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/src/common/utils/kv_store.rs)

Это разделение теперь явное:

- NFC driver отвечает за чтение/запись payload через PN532
- `KvStore` отвечает за текстовый формат `KV1`
- прикладные модели вроде battery/service tag строятся поверх `KvStore`

## Структура модуля

После рефакторинга `nfc_tag` разложен по подмодулям:

```text
src/common/drivers/nfc_tag/
  mod.rs
  constants.rs
  format.rs
  sync.rs
  async.rs
  esp_idf.rs
```

Что где лежит:

- `sync.rs` — синхронный high-level API над `Pn532`
- `async.rs` — async worker как обёртка над sync API
- `format.rs` — NDEF/TLV encode/decode
- `esp_idf.rs` — helper-конструкторы для `esp-idf-svc`
- `constants.rs` — именованные значения протокола и default-конфигов

## Что умеет модуль

- опрашивать Type A метку через PN532
- читать UID, ATQA, SAK
- читать пользовательскую область NTAG начиная с page 4
- находить NDEF TLV внутри памяти метки
- читать NDEF Text Record
- превращать payload в `KvStore`
- записывать `KvStore` обратно на метку
- держать async worker для постоянного опроса PN532
- давать `esp-idf` helper для быстрого создания reader'а через I2C

## Что именно хранится на метке

Сейчас NFC слой хранит прикладной payload как:

- одну строку UTF-8
- внутри одного NDEF Text Record
- в формате `KV1`

То есть это не произвольный NDEF-конструктор, а удобный application-specific storage для этого проекта.

## Основные NFC-типы

### `TagInfo`

Информация о найденной метке:

```rust
pub struct TagInfo {
    pub uid: Vec<u8>,
    pub atqa: [u8; 2],
    pub sak: u8,
}
```

### `NfcTag`

Синхронный high-level wrapper над `Pn532`.

Основные методы:

- `new(pn532)`
- `init()`
- `init_default()`
- `init_with_config(config)`
- `firmware_version()`
- `poll_tag(timeout)`
- `read_kv_store()`
- `write_kv_store(&store)`

### `NfcInitConfig`

Конфигурация retry-инициализации:

```rust
pub struct NfcInitConfig {
    pub startup_delay: Duration,
    pub retry_delay: Duration,
    pub attempts: usize,
}
```

Значения по умолчанию:

- `startup_delay = 200 ms`
- `retry_delay = 200 ms`
- `attempts = 5`

### `AsyncNfcTag`

Неблокирующая обёртка над `NfcTag`.

Она полезна, когда:

- основной цикл не должен блокироваться на `poll_tag()`
- NFC опрашивается постоянно
- UI и state machine должны жить независимо от PN532

Основные методы:

- `AsyncNfcTag::new(nfc, config)`
- `snapshot()`
- `write_kv_store_for_tag(expected_uid, store)`
- `last_worker_error()`

Как это работает:

- worker сам опрашивает `PN532`
- worker кэширует последнюю увиденную метку
- main loop читает только `snapshot()`
- запись идёт отдельной командой в тот же worker

### `AsyncNfcConfig`

Настройки async worker:

```rust
pub struct AsyncNfcConfig {
    pub poll_interval: Duration,
    pub poll_timeout: Duration,
    pub removal_debounce: Duration,
    pub thread_stack_size: usize,
}
```

Значения по умолчанию:

- `poll_interval = 30 ms`
- `poll_timeout = 40 ms`
- `removal_debounce = 1200 ms`
- `thread_stack_size = 8192`

`removal_debounce` важен для живого железа: он защищает от ложного “метка пропала” при кратковременных сбоях PN532.

### `AsyncNfcSnapshot`

Снимок текущего состояния async worker'а:

```rust
pub struct AsyncNfcSnapshot {
    pub generation: u64,
    pub tag: Option<AsyncObservedTag>,
}
```

Где:

- `generation` увеличивается при смене наблюдаемого состояния
- `tag = None` означает, что метки в поле сейчас нет

### `AsyncObservedTag`

Информация о последней увиденной метке:

- `info: TagInfo`
- `payload: AsyncTagPayload`

### `AsyncTagPayload`

Payload хранится в трёх вариантах:

- `KvStore(store)` — на метке корректный payload текущего приложения
- `Empty` — метка есть, но NDEF payload не найден
- `ReadError(text)` — UID прочитан, но содержимое не удалось прочитать или разобрать

### `nfc_tag::esp_idf`

Вспомогательный слой для проектов на `esp-idf-svc`.

Полезные элементы:

- `esp_idf::StdTimer`
- `esp_idf::EspNfcTag`
- `esp_idf::AsyncEspNfcTag`
- `esp_idf::new_with_driver(i2c_driver)`
- `esp_idf::new(i2c, sda, scl, baudrate)`
- `esp_idf::new_default(i2c, sda, scl)`
- `esp_idf::new_async_default(i2c, sda, scl, worker_config)`

## Типичный сценарий: sync API

### 1. Создать `NfcTag` для `esp-idf`

```rust
let mut nfc = common::drivers::nfc_tag::esp_idf::new_default(
    p.i2c0,
    p.pins.gpio3,
    p.pins.gpio4,
)?;
```

### 2. Инициализировать PN532

```rust
nfc.init_default()?;
let fw = nfc.firmware_version()?;
info!("PN532 firmware raw: {:02X?}", fw);
```

### 3. Прочитать payload как `KvStore`

```rust
use common::utils::kv_store::{KvStore, KvValue};

if let Some(tag) = nfc.poll_tag(Duration::from_millis(1000))? {
    let store = nfc.read_kv_store()?;
    info!("UID = {:02X?}", tag.uid);

    match store.get("counter") {
        Some(KvValue::U8(value)) => info!("counter = {value}"),
        _ => info!("counter is missing"),
    }
}
```

### 4. Записать `KvStore` на метку

```rust
use common::utils::kv_store::KvStore;

let mut store = KvStore::new();
store.insert_string("name", "ESP32-C3")?;
store.insert_u32("consumption_per_sec", 1500)?;
store.insert_bool("enabled", true)?;

nfc.write_kv_store(&store)?;
```

## Типичный сценарий: async API

```rust
use common::drivers::nfc_tag::{AsyncNfcConfig, AsyncNfcTag};

let mut sync_nfc = common::drivers::nfc_tag::esp_idf::new_default(
    p.i2c0,
    p.pins.gpio3,
    p.pins.gpio4,
)?;
sync_nfc.init_default()?;

let async_nfc = AsyncNfcTag::new(sync_nfc, AsyncNfcConfig::default())?;

let snapshot = async_nfc.snapshot()?;
if let Some(tag) = snapshot.tag {
    info!("UID = {:02X?}", tag.info.uid);
}
```

## Ограничения текущей реализации

- модуль работает только с одним NDEF Text Record
- модуль не пытается сохранять чужие NDEF records
- при `write_kv_store()` пользовательская NDEF-область переписывается целиком
- если на метке лежит другой текстовый NDEF, но не в формате `KV1`, разбор payload вернёт ошибку формата
- если на метке лежит нестандартный payload, async snapshot может вернуть `ReadError(...)`

Проще говоря: это хороший storage-слой для своего проекта, а не универсальный редактор любых NFC-меток.

## Связанные документы

- KV-формат: [docs/kv_store.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/kv_store.md)
- LED indicator: [docs/led_indicator.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/led_indicator.md)
- Segment display: [docs/segment_display.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/segment_display.md)
