//! Удобная обёртка над TM1637 для 4-разрядного индикатора с двоеточием посередине.

use core::fmt;
use core::time::Duration;
use esp_idf_svc::hal::delay::Delay;
use esp_idf_svc::hal::gpio::{GpioError, Output, OutputPin, PinDriver};
use esp_idf_svc::sys::EspError;
use std::string::String;
use tm1637_embedded_hal::formatters;
use tm1637_embedded_hal::mappings::from_ascii_byte;
use tm1637_embedded_hal::tokens::Blocking;
use tm1637_embedded_hal::{Brightness, Error as TmError, TM1637Builder, TM1637};

const COLON_MASK: u8 = 0b1000_0000;
const DISPLAY_WIDTH: usize = 4;
const ERROR_TEXT: &str = "Error";

/// Выравнивание статического текста или числа на дисплее.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// Выравнивание влево.
    Left,
    /// Выравнивание вправо.
    Right,
}

/// Формат отображения целого числа.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntFormat {
    /// Выравнивание числа в четырёх знакоместах.
    pub align: Align,
    /// Показывать ли ведущие нули.
    ///
    /// Имеет смысл только при правом выравнивании.
    pub leading_zeros: bool,
}

impl Default for IntFormat {
    fn default() -> Self {
        Self {
            align: Align::Right,
            leading_zeros: false,
        }
    }
}

impl IntFormat {
    /// Формат по умолчанию: вправо, без ведущих нулей.
    pub fn new() -> Self {
        Self::default()
    }

    /// Выравнивание влево.
    pub fn left(mut self) -> Self {
        self.align = Align::Left;
        self
    }

    /// Выравнивание вправо.
    pub fn right(mut self) -> Self {
        self.align = Align::Right;
        self
    }

    /// Включить или выключить ведущие нули.
    pub fn leading_zeros(mut self, enabled: bool) -> Self {
        self.leading_zeros = enabled;
        self
    }
}

/// Настройки TM1637 helper-обёртки.
#[derive(Clone, Copy, Debug)]
pub struct DisplayConfig {
    /// Яркость индикатора.
    pub brightness: Brightness,
    /// Внутренний protocol delay библиотеки `tm1637-embedded-hal`.
    pub delay_us: u32,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            brightness: Brightness::L3,
            delay_us: 100,
        }
    }
}

/// Ошибки high-level слоя дисплея.
#[derive(Debug)]
pub enum DisplayError {
    /// Ошибка инициализации или конфигурации ESP-IDF GPIO.
    Esp(EspError),
    /// Ошибка драйвера TM1637.
    Driver(TmError<GpioError>),
    /// Число не помещается в четыре знакоместа.
    IntegerOutOfRange(i16),
    /// Значение минут должно быть в диапазоне `0..=99`.
    MinutesOutOfRange(u8),
    /// Значение секунд должно быть в диапазоне `0..=99`.
    SecondsOutOfRange(u8),
    /// Текст должен быть ASCII.
    NonAsciiText,
}

impl From<EspError> for DisplayError {
    fn from(value: EspError) -> Self {
        Self::Esp(value)
    }
}

impl From<TmError<GpioError>> for DisplayError {
    fn from(value: TmError<GpioError>) -> Self {
        Self::Driver(value)
    }
}

impl fmt::Display for DisplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DisplayError::Esp(err) => write!(f, "esp error: {err}"),
            DisplayError::Driver(err) => write!(f, "tm1637 driver error: {err:?}"),
            DisplayError::IntegerOutOfRange(value) => {
                write!(f, "integer {value} does not fit into 4 digits")
            }
            DisplayError::MinutesOutOfRange(value) => {
                write!(f, "minutes value {value} is out of range 0..=99")
            }
            DisplayError::SecondsOutOfRange(value) => {
                write!(f, "seconds value {value} is out of range 0..=99")
            }
            DisplayError::NonAsciiText => write!(f, "text must be ASCII"),
        }
    }
}

impl std::error::Error for DisplayError {}

/// Обёртка над 4-разрядным TM1637-дисплеем с физическим двоеточием между 2 и 3 разрядом.
///
/// Wrapper хранит текущее состояние кадра и отдельное состояние двоеточия,
/// поэтому двоеточие можно мигать независимо от последнего выведенного числа или текста.
pub struct SegmentDisplay4<'d> {
    display: TM1637<4, Blocking, PinDriver<'d, Output>, PinDriver<'d, Output>, Delay>,
    colon: bool,
    base_frame: [u8; DISPLAY_WIDTH],
}

impl<'d> SegmentDisplay4<'d> {
    /// Создаёт дисплей с настройками по умолчанию.
    pub fn new(clk: impl OutputPin + 'd, dio: impl OutputPin + 'd) -> Result<Self, DisplayError> {
        Self::with_config(clk, dio, DisplayConfig::default())
    }

    /// Создаёт дисплей с заданной конфигурацией brightness / protocol delay.
    pub fn with_config(
        clk: impl OutputPin + 'd,
        dio: impl OutputPin + 'd,
        config: DisplayConfig,
    ) -> Result<Self, DisplayError> {
        let clk = PinDriver::output(clk)?;
        let dio = PinDriver::output(dio)?;
        let delay: Delay = Default::default();

        let display = TM1637Builder::new(clk, dio, delay)
            .brightness(config.brightness)
            .delay_us(config.delay_us)
            .build_blocking::<4>();

        Ok(Self {
            display,
            colon: false,
            base_frame: [0; DISPLAY_WIDTH],
        })
    }

    /// Инициализирует дисплей: очищает его и применяет уровень яркости.
    pub fn init(&mut self) -> Result<(), DisplayError> {
        self.display.init()?;
        self.base_frame = [0; DISPLAY_WIDTH];
        self.render_current()
    }

    /// Очищает дисплей и сбрасывает сохранённый кадр.
    pub fn clear(&mut self) -> Result<(), DisplayError> {
        self.base_frame = [0; DISPLAY_WIDTH];
        self.render_current()
    }

    /// Меняет яркость дисплея.
    pub fn set_brightness(&mut self, brightness: Brightness) -> Result<(), DisplayError> {
        self.display.set_brightness(brightness)?;
        Ok(())
    }

    /// Возвращает текущее состояние двоеточия.
    pub fn colon(&self) -> bool {
        self.colon
    }

    /// Явно включает или выключает двоеточие и сразу перерисовывает текущий кадр.
    pub fn set_colon(&mut self, enabled: bool) -> Result<(), DisplayError> {
        self.colon = enabled;
        self.render_current()
    }

    /// Инвертирует состояние двоеточия и сразу перерисовывает текущий кадр.
    pub fn toggle_colon(&mut self) -> Result<(), DisplayError> {
        self.colon = !self.colon;
        self.render_current()
    }

    /// Показывает целое число в четырёх знакоместах.
    pub fn show_int(&mut self, value: i16, format: IntFormat) -> Result<(), DisplayError> {
        self.base_frame = format_int(value, format)?;
        self.render_current()
    }

    /// Показывает время в виде `MM:SS`.
    ///
    /// Физическое двоеточие управляется текущим состоянием `colon`.
    pub fn show_mmss(&mut self, minutes: u8, seconds: u8) -> Result<(), DisplayError> {
        if minutes > 99 {
            return Err(DisplayError::MinutesOutOfRange(minutes));
        }
        if seconds > 99 {
            return Err(DisplayError::SecondsOutOfRange(seconds));
        }

        self.base_frame = formatters::clock_to_4digits(minutes, seconds, false);
        self.render_current()
    }

    /// Показывает короткий ASCII-текст, обрезая его до четырёх символов.
    pub fn show_text(&mut self, text: &str, align: Align) -> Result<(), DisplayError> {
        self.base_frame = format_text_frame(text, align)?;
        self.render_current()
    }

    /// Показывает максимально похожее на `ERROR` статическое сообщение.
    ///
    /// Для 4 разрядов это `Erro`.
    pub fn show_error(&mut self) -> Result<(), DisplayError> {
        self.show_text("Erro", Align::Left)
    }

    /// Один раз прокручивает ASCII-текст по индикатору как бегущую строку.
    pub fn scroll_text_once(
        &mut self,
        text: &str,
        step_delay: Duration,
    ) -> Result<(), DisplayError> {
        let source = build_scroll_source(text)?;
        for offset in 0..=source.len() - DISPLAY_WIDTH {
            let mut frame = [0u8; DISPLAY_WIDTH];
            for (index, byte) in source[offset..offset + DISPLAY_WIDTH].iter().enumerate() {
                frame[index] = from_ascii_byte(*byte);
            }

            self.base_frame = frame;
            self.render_current()?;
            self.display
                .delay_mut()
                .delay_ms(step_delay.as_millis() as u32);
        }

        Ok(())
    }

    /// Один раз прокручивает сообщение `Error`.
    pub fn scroll_error_once(&mut self, step_delay: Duration) -> Result<(), DisplayError> {
        self.scroll_text_once(ERROR_TEXT, step_delay)
    }

    fn render_current(&mut self) -> Result<(), DisplayError> {
        let mut frame = self.base_frame;
        apply_colon(&mut frame, self.colon);
        self.display.display_slice(0, &frame)?;
        Ok(())
    }
}

fn format_int(value: i16, format: IntFormat) -> Result<[u8; DISPLAY_WIDTH], DisplayError> {
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

fn format_text_frame(text: &str, align: Align) -> Result<[u8; DISPLAY_WIDTH], DisplayError> {
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

fn build_scroll_source(text: &str) -> Result<Vec<u8>, DisplayError> {
    if !text.is_ascii() {
        return Err(DisplayError::NonAsciiText);
    }

    let mut source = Vec::with_capacity(text.len() + DISPLAY_WIDTH * 2);
    source.extend_from_slice(b"    ");
    source.extend_from_slice(text.as_bytes());
    source.extend_from_slice(b"    ");
    Ok(source)
}

fn text_to_segments(text: &str) -> [u8; DISPLAY_WIDTH] {
    let mut frame = [0u8; DISPLAY_WIDTH];
    for (index, byte) in text.as_bytes().iter().take(DISPLAY_WIDTH).enumerate() {
        frame[index] = from_ascii_byte(*byte);
    }
    frame
}

fn apply_colon(frame: &mut [u8; DISPLAY_WIDTH], enabled: bool) {
    if enabled {
        frame[1] |= COLON_MASK;
    } else {
        frame[1] &= !COLON_MASK;
    }
}
