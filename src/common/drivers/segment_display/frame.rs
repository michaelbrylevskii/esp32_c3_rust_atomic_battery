use super::constants::{COLON_MASK, DISPLAY_WIDTH};
use super::types::{Align, DisplayError, IntFormat};
use std::string::String;
use tm1637_embedded_hal::formatters;
use tm1637_embedded_hal::mappings::from_ascii_byte;

pub(crate) fn format_int(
    value: i16,
    format: IntFormat,
) -> Result<[u8; DISPLAY_WIDTH], DisplayError> {
    if !(-999..=9999).contains(&value) {
        return Err(DisplayError::IntegerOutOfRange(value));
    }

    let text = if format.leading_zeros && matches!(format.align, Align::Right) {
        if value < 0 {
            format!("-{:0>3}", value.unsigned_abs())
        } else {
            format!("{:0>4}", value)
        }
    } else {
        match format.align {
            Align::Left => format!("{value:<4}"),
            Align::Right => format!("{value:>4}"),
        }
    };

    Ok(text_to_segments(&text))
}

pub(crate) fn format_int_pair_frame(
    left: u8,
    right: u8,
) -> Result<[u8; DISPLAY_WIDTH], DisplayError> {
    if left > 99 {
        return Err(DisplayError::PairLeftOutOfRange(left));
    }
    if right > 99 {
        return Err(DisplayError::PairRightOutOfRange(right));
    }

    Ok(formatters::clock_to_4digits(left, right, false))
}

pub(crate) fn format_text_frame(
    text: &str,
    align: Align,
) -> Result<[u8; DISPLAY_WIDTH], DisplayError> {
    if !text.is_ascii() {
        return Err(DisplayError::NonAsciiText);
    }

    let trimmed: String = text.chars().take(DISPLAY_WIDTH).collect();
    let padded = match align {
        Align::Left => format!("{trimmed:<4}"),
        Align::Right => format!("{trimmed:>4}"),
    };

    Ok(text_to_segments(&padded))
}

pub(crate) fn build_scroll_source(text: &str) -> Result<Vec<u8>, DisplayError> {
    if !text.is_ascii() {
        return Err(DisplayError::NonAsciiText);
    }

    let mut source = Vec::with_capacity(text.len() + DISPLAY_WIDTH * 2);
    source.extend_from_slice(b"    ");
    source.extend_from_slice(text.as_bytes());
    source.extend_from_slice(b"    ");
    Ok(source)
}

pub(crate) fn scroll_window_frame(source: &[u8], offset: usize) -> [u8; DISPLAY_WIDTH] {
    let mut frame = [0u8; DISPLAY_WIDTH];
    for (index, byte) in source[offset..offset + DISPLAY_WIDTH].iter().enumerate() {
        frame[index] = from_ascii_byte(*byte);
    }
    frame
}

pub(crate) fn apply_colon(frame: &mut [u8; DISPLAY_WIDTH], enabled: bool) {
    if enabled {
        frame[1] |= COLON_MASK;
    } else {
        frame[1] &= !COLON_MASK;
    }
}

fn text_to_segments(text: &str) -> [u8; DISPLAY_WIDTH] {
    let mut frame = [0u8; DISPLAY_WIDTH];
    for (index, byte) in text.as_bytes().iter().take(DISPLAY_WIDTH).enumerate() {
        frame[index] = from_ascii_byte(*byte);
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_right_aligned_integer() {
        let frame = format_int(42, IntFormat::new()).unwrap();
        assert_eq!(frame, text_to_segments("  42"));
    }

    #[test]
    fn formats_pair_frame() {
        let frame = format_int_pair_frame(12, 34).unwrap();
        assert_eq!(frame, formatters::clock_to_4digits(12, 34, false));
    }

    #[test]
    fn builds_scroll_source_with_padding() {
        let source = build_scroll_source("AB").unwrap();
        assert_eq!(&source[..4], b"    ");
        assert_eq!(&source[4..6], b"AB");
        assert_eq!(&source[source.len() - 4..], b"    ");
    }
}
