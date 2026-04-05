use super::constants::{
    TLV_LOCK_CONTROL, TLV_MEMORY_CONTROL, TLV_NDEF_MESSAGE, TLV_NULL, TLV_PROPRIETARY,
    TLV_TERMINATOR,
};
use super::sync_nfc::NfcError;
use crate::utils::kv_store::KvFormatError;
use core::fmt::Debug;
use ndef::{Message, Payload, Record, RecordType};
use std::string::String;
use std::vec::Vec;

pub fn encode_text_record(text: &str) -> Result<Vec<u8>, KvFormatError> {
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

pub fn decode_text_record(bytes: &[u8]) -> Result<String, KvFormatError> {
    let message = Message::try_from(bytes).map_err(|_| KvFormatError::InvalidNdef)?;
    let Some(record) = message.records.first() else {
        return Err(KvFormatError::MissingTextRecord);
    };

    match &record.payload {
        Payload::RTD(RecordType::Text { txt, .. }) => Ok((*txt).to_owned()),
        _ => Err(KvFormatError::MissingTextRecord),
    }
}

pub fn encode_ndef_tlv(ndef_bytes: &[u8]) -> Vec<u8> {
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

pub fn extract_ndef_tlv<E: Debug>(data: &[u8]) -> Result<&[u8], NfcError<E>> {
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
    fn ndef_text_roundtrip() {
        let raw = encode_text_record("KV1\nname=S:Привет\\nмир").unwrap();
        let text = decode_text_record(&raw).unwrap();
        assert_eq!(text, "KV1\nname=S:Привет\\nмир");
    }

    #[test]
    fn tlv_roundtrip() {
        let tlv = encode_ndef_tlv(&[0xD1, 0x01, 0x05, 0x54, 0x02]);
        let ndef = extract_ndef_tlv::<core::convert::Infallible>(&tlv).unwrap();
        assert_eq!(ndef, &[0xD1, 0x01, 0x05, 0x54, 0x02]);
    }
}
