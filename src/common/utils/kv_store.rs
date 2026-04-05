//! Утилитарное key-value хранилище для прикладных payload'ов проекта.
//!
//! Подробная документация на русском:
//! [docs/kv_store.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/kv_store.md)

use core::fmt;
use std::string::String;
use std::vec::Vec;

const KV_HEADER: &str = "KV1";

/// Поддерживаемые типы значений для прикладного key-value хранилища.
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
    F32(f32),
    /// 64-битное число с плавающей точкой.
    F64(f64),
    /// Булево значение.
    Bool(bool),
}

/// Одна key-value запись.
#[derive(Clone, Debug, PartialEq)]
pub struct KvEntry {
    /// ASCII-ключ.
    pub key: String,
    /// Значение ключа.
    pub value: KvValue,
}

/// Удобный контейнер для сериализуемого key-value набора.
///
/// Внутренний текстовый формат сейчас называется `KV1` и используется как
/// прикладной payload в NFC-части проекта, но сам контейнер не зависит от PN532,
/// NDEF или конкретного транспорта.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KvStore {
    entries: Vec<KvEntry>,
}

impl KvStore {
    /// Создаёт пустое хранилище.
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

    /// Сериализует хранилище во внутренний формат `KV1`.
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

    /// Парсит текст `KV1` обратно в `KvStore`.
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

/// Ошибки формата `KV1`.
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

/// Проверяет ключ на совместимость с форматом `KV1`.
pub fn validate_key(key: &str) -> Result<(), KvFormatError> {
    if key.is_empty() || !key.is_ascii() {
        return Err(KvFormatError::InvalidKey);
    }

    if key.contains('=') || key.contains(':') || key.contains('\n') || key.contains('\r') {
        return Err(KvFormatError::InvalidKey);
    }

    Ok(())
}

/// Проверяет значение на совместимость с форматом `KV1`.
pub fn validate_value(value: &KvValue) -> Result<(), KvFormatError> {
    match value {
        KvValue::F32(number) if !number.is_finite() => Err(KvFormatError::NonFiniteFloat("f32")),
        KvValue::F64(number) if !number.is_finite() => Err(KvFormatError::NonFiniteFloat("f64")),
        _ => Ok(()),
    }
}

/// Экранирует UTF-8 строку для хранения внутри одной строки `KV1`.
pub fn escape_string_value(value: &str) -> String {
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

/// Разворачивает escape-последовательности `KV1` обратно в UTF-8 строку.
pub fn unescape_string_value(value: &str) -> Result<String, KvFormatError> {
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
}
