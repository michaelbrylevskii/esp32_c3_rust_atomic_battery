# `kv_store`: прикладной key-value формат проекта

## Что это такое

Модуль [`kv_store`](../src/common/utils/kv_store.rs) это утилитарный слой для хранения типизированных key-value данных.

Он используется как текстовый прикладной формат `KV1`.

Роли слоёв:

- NFC driver отвечает за чтение и запись payload
- `KvStore` отвечает за прикладной текстовый формат `KV1`
- прикладные модели вроде battery/service tag строятся уже поверх `KvStore`

## Что умеет модуль

- хранить key-value записи
- валидировать ключи и значения
- сериализовать данные в текстовый формат `KV1`
- парсить `KV1` обратно
- поддерживать типы `String`, integer, float и `bool`

## Основные типы

### `KvValue`

Поддерживаемые типы значений:

- `Str(String)`
- `U8(u8)`
- `U16(u16)`
- `U32(u32)`
- `U64(u64)`
- `I8(i8)`
- `I16(i16)`
- `I32(i32)`
- `I64(i64)`
- `F32(f32)`
- `F64(f64)`
- `Bool(bool)`

### `KvStore`

Основной контейнер.

Полезные методы:

- `KvStore::new()`
- `insert_string(...)`
- `insert_u8(...)`
- `insert_u16(...)`
- `insert_u32(...)`
- `insert_u64(...)`
- `insert_i8(...)`
- `insert_i16(...)`
- `insert_i32(...)`
- `insert_i64(...)`
- `insert_f32(...)`
- `insert_f64(...)`
- `insert_bool(...)`
- `get(key)`
- `entries()`
- `to_text()`
- `from_text()`

### `KvFormatError`

Ошибки формата `KV1`:

- ошибки заголовка
- ошибки парсинга строки
- ошибки типа
- ошибки числа / boolean
- ошибки escape-последовательностей
- ошибки `NDEF Text Record`, если этот формат используется как payload в NFC-части

## Формат `KV1`

Внутренний вид данных:

```text
KV1
name=S:Привет,\nESP32-C3
counter=U8:42
limit=U16:1024
serial=U32:123456
energy=U64:9876543210
temperature_offset=I8:-5
bias=I16:-32000
temp_raw=I32:-123456
distance=I64:-9876543210
soc=F32:98.5
voltage=F64:12.625
enabled=B:1
```

Где:

- первая строка `KV1` это сигнатура формата
- дальше каждая строка это `key=TYPE:value`
- `String` хранится как UTF-8 с escape-последовательностями

## Ограничения

- ключ должен быть ASCII
- ключ не должен содержать `=`, `:`, `\n`, `\r`
- строки значений поддерживают UTF-8
- для строк используются `\\`, `\n`, `\r`, `\t`
- `f32` и `f64` должны быть конечными
- `NaN`, `+inf`, `-inf` запрещены

## Пример

```rust
use common::utils::kv_store::{KvStore, KvValue};

let mut store = KvStore::new();
store.insert_string("name", "Привет,\nESP32-C3")?;
store.insert_u32("rate", 1500)?;
store.insert_bool("enabled", true)?;

let text = store.to_text()?;
let decoded = KvStore::from_text(&text)?;

assert_eq!(decoded.get("rate"), Some(&KvValue::U32(1500)));
```
