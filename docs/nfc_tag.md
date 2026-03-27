# `nfc_tag`: удобная обёртка для NFC-меток

## Что это такое

Модуль [`nfc_tag`](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/src/nfc_tag.rs) это high-level слой поверх:

- `pn532` как транспорта и набора низкоуровневых команд
- NTAG / Type 2 Tag как памяти страницами
- TLV как контейнера NDEF-сообщения
- NDEF Text Record как формата полезной нагрузки

Идея простая: в коде приложения не работать руками с `ntag_read(page)` и `ntag_write(page, data)`, а пользоваться более удобным API:

- `poll_tag()`
- `read_kv_store()`
- `write_kv_store()`

## Что умеет модуль

- Опрашивать метку через PN532
- Читать UID, ATQA, SAK
- Читать пользовательскую область NTAG начиная с page 4
- Находить NDEF TLV внутри памяти метки
- Читать NDEF Text Record
- Разбирать его в key-value набор
- Собирать key-value набор обратно
- Записывать его на метку

## Что именно хранится на метке

Сейчас модуль хранит весь набор key-value как **одну строку ASCII** внутри **одного NDEF Text Record**.

Это важный архитектурный выбор:

- снаружи это выглядит как key-value storage
- внутри это текстовый payload
- модуль полностью владеет своим NDEF-сообщением

То есть это не “произвольный NDEF-конструктор”, а именно удобное хранилище для вашего приложения.

## Поддерживаемые типы

Тип значения задаётся через `KvValue`:

- `Str(String)`
- `U8(u8)`
- `I8(i8)`
- `U4(u8)`
- `I4(i8)`
- `Bool(bool)`

Ограничения:

- `key` должен быть ASCII
- `key` не должен содержать `=`, `:`, `\n`, `\r`
- строковые значения должны быть ASCII
- строковые значения не должны содержать `\n`, `\r`
- `u4` допускает только `0..=15`
- `i4` допускает только `-8..=7`

## Внутренний формат key-value

Внутри NDEF Text Record хранится строка такого вида:

```text
KV1
name=S:ESP32-C3
counter=U8:42
temperature_offset=I8:-5
mode=U4:9
delta=I4:-3
enabled=B:1
```

Где:

- первая строка `KV1` это сигнатура формата
- далее каждая строка это `key=TYPE:value`

Типовые префиксы:

- `S` для строки
- `U8` для `u8`
- `I8` для `i8`
- `U4` для `u4`
- `I4` для `i4`
- `B` для boolean

Boolean хранится как:

- `B:0` -> `false`
- `B:1` -> `true`

## Ограничения текущей реализации

Важно понимать текущие границы модуля:

- модуль работает только с одним NDEF Text Record
- модуль не пытается сохранять чужие NDEF records
- при `write_kv_store()` пользовательская NDEF-область переписывается целиком
- если на метке уже лежит другой текстовый NDEF, но не в формате `KV1`, чтение вернёт `InvalidHeader`
- если на метке лежит нечто нестандартное или не-текстовое, чтение может вернуть ошибку формата или TLV/NDEF

Проще говоря: этот слой хорош как **свой формат хранения для своего проекта**, а не как универсальный редактор любых NFC-меток.

## Основные типы

### `TagInfo`

Информация о найденной метке:

```rust
pub struct TagInfo {
    pub uid: Vec<u8>,
    pub atqa: [u8; 2],
    pub sak: u8,
}
```

### `KvStore`

Основной контейнер key-value данных.

Полезные методы:

- `KvStore::new()`
- `insert_string(...)`
- `insert_u8(...)`
- `insert_i8(...)`
- `insert_u4(...)`
- `insert_i4(...)`
- `insert_bool(...)`
- `get(key)`
- `entries()`
- `to_text()`
- `from_text()`

### `NfcTag`

High-level обёртка над `Pn532`.

Основные методы:

- `new(pn532)`
- `init()`
- `init_default()`
- `init_with_config(config)`
- `firmware_version()`
- `poll_tag(timeout)`
- `read_kv_store()`
- `write_kv_store(&store)`

## Типичный сценарий использования

### 1. Создать `Pn532`

Сначала поднимается I2C, затем создаётся `Pn532`, затем поверх него `NfcTag`.

Пример из проекта:

```rust
let interface = I2CInterface { i2c };
let timer = StdTimer::new();
let pn532: Pn532<_, _, 64> = Pn532::new(interface, timer);
let mut nfc = NfcTag::new(pn532);
```

### 2. Инициализировать PN532

Минимальный низкоуровневый вариант:

```rust
nfc.init()?;
let fw = nfc.firmware_version()?;
info!("PN532 firmware raw: {:02X?}", fw);
```

Но в этом проекте удобнее пользоваться уже готовой инициализацией с задержкой и retry:

```rust
nfc.init_default()?;
let fw = nfc.firmware_version()?;
info!("PN532 firmware raw: {:02X?}", fw);
```

На реальном железе это оказалось полезно: иногда первый `sam_configuration` отдаёт timeout сразу после boot.

### 2.1. Кастомная конфигурация инициализации

```rust
use esp32_c3_rust_atomic_battery::nfc_tag::NfcInitConfig;
use std::time::Duration;

nfc.init_with_config(NfcInitConfig {
    startup_delay: Duration::from_millis(300),
    retry_delay: Duration::from_millis(150),
    attempts: 6,
})?;
```

Поля:

- `startup_delay` это пауза перед первой попыткой инициализации
- `retry_delay` это пауза между повторными попытками
- `attempts` это общее число попыток

Значения по умолчанию в `init_default()`:

- `startup_delay = 200 ms`
- `retry_delay = 200 ms`
- `attempts = 5`

### 3. Ждать метку

```rust
match nfc.poll_tag(Duration::from_millis(1000))? {
    Some(tag) => {
        info!(
            "Tag UID: {:02X?}, ATQA={:02X?}, SAK=0x{:02X}",
            tag.uid, tag.atqa, tag.sak
        );
    }
    None => {
        // Метки нет
    }
}
```

`None` это нормальный случай: просто в поле нет метки.

## Пример: записать набор key-value

```rust
let mut store = KvStore::new();
store.insert_string("name", "ESP32-C3")?;
store.insert_u8("counter", 42)?;
store.insert_i8("temperature_offset", -5)?;
store.insert_u4("mode", 9)?;
store.insert_i4("delta", -3)?;
store.insert_bool("enabled", true)?;

nfc.write_kv_store(&store)?;
```

## Пример: прочитать набор key-value

```rust
let store = nfc.read_kv_store()?;

for entry in store.entries() {
    info!("{entry:?}");
}
```

## Пример: получить конкретное значение

```rust
if let Some(value) = store.get("enabled") {
    info!("enabled = {:?}", value);
}
```

Если нужно строго вытащить конкретный тип, сейчас это делается через `match`:

```rust
match store.get("counter") {
    Some(KvValue::U8(value)) => info!("counter = {value}"),
    Some(other) => warn!("counter has unexpected type: {:?}", other),
    None => warn!("counter is missing"),
}
```

## Пример полного цикла read/write/read-back

Ниже логика, близкая к той, что сейчас используется в `test_reader()`:

```rust
let mut demo_store = KvStore::new();
demo_store.insert_string("name", "ESP32-C3")?;
demo_store.insert_u8("counter", 42)?;
demo_store.insert_i8("temperature_offset", -5)?;
demo_store.insert_u4("mode", 9)?;
demo_store.insert_i4("delta", -3)?;
demo_store.insert_bool("enabled", true)?;

if let Some(tag) = nfc.poll_tag(Duration::from_millis(1000))? {
    info!("Found tag: {:02X?}", tag.uid);

    match nfc.read_kv_store() {
        Ok(store) => info!("Current tag data: {:?}", store.entries()),
        Err(err) => warn!("Current tag data cannot be read yet: {err}"),
    }

    nfc.write_kv_store(&demo_store)?;

    let read_back = nfc.read_kv_store()?;
    if read_back == demo_store {
        info!("read-back matches written data");
    } else {
        warn!("read-back differs from written data");
    }
}
```

## Что делает `test_reader()` в проекте

Текущая демонстрация использования находится в [src/main.rs](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/src/main.rs#L142).

Сейчас там:

- поднимается I2C на `GPIO3` / `GPIO4`
- создаётся `NfcTag`
- вызывается `init_default()`
- печатается firmware version
- собирается demo `KvStore`
- в цикле опрашивается метка
- при обнаружении печатается содержимое key-value
- при `WRITE_DEMO = true` делается запись и read-back проверка

Безопасный режим для повседневной работы:

```rust
const WRITE_DEMO: bool = false;
```

Если нужно один раз переписать метку тестовыми данными:

```rust
const WRITE_DEMO: bool = true;
```

После проверки лучше вернуть обратно `false`, чтобы код не перезаписывал каждую поднесённую метку.

## Какие ошибки можно ожидать

### `NfcError::NoNdefMessage`

На метке нет NDEF TLV. Это нормально для пустой или неинициализированной метки.

### `KvFormatError::InvalidHeader`

На метке есть текстовый NDEF, но это не наш формат `KV1`.

Пример: метка ранее была записана через NFC Tools как обычный Text Record вроде `Hello, NFC!`.

### `NfcError::PayloadTooLarge`

Собранный payload не помещается в пользовательскую область метки.

Это ограничение зависит от ёмкости конкретной NTAG.

### `NfcError::TagStatus(...)`

PN532/NTAG вернули статус ошибки при чтении или записи страницы.

### `NfcError::InvalidResponse(...)`

Низкоуровневый ответ от PN532/NTAG пришёл не в том формате, который ожидался.

## Практические замечания

- Для PN532 по I2C нужны внешние pull-up резисторы на SDA/SCL
- Модуль PN532 должен быть аппаратно переведён в I2C mode
- В этом проекте `expected_len = 32` для `INLIST_ONE_ISO_A_TARGET` оказался безопаснее коротких значений
- Для `ntag_read(page)` ожидается `17` байт: `status + 16 data bytes`
- На некоторых стартах PN532 может не ответить на первый `sam_configuration`, поэтому retry на init оказался полезен

## Когда эту обёртку стоит расширять

Следующие логичные улучшения:

- typed getters: `get_u8("counter")`, `get_bool("enabled")`
- удаление ключей
- обновление значения по месту через удобный API
- поддержка нескольких NDEF records
- отдельный бинарный формат вместо текстового
- поддержка non-ASCII строк, если это понадобится

## Кратко

Если совсем коротко, пользоваться модулем нужно так:

1. Создать `NfcTag`
2. Вызвать `init()`
3. Ждать метку через `poll_tag()`
4. Читать через `read_kv_store()`
5. Записывать через `write_kv_store()`

Для текущего проекта это уже рабочий и проверенный на железе слой поверх PN532.
