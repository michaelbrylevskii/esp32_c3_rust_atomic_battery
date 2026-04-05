use super::constants::{DISPLAY_WIDTH, SCROLL_ERROR_TEXT, STATIC_ERROR_TEXT};
use super::frame::{
    apply_colon, build_scroll_source, format_int, format_int_pair_frame, format_text_frame,
    scroll_window_frame,
};
use super::types::{Align, DisplayConfig, DisplayError, IntFormat};
use core::time::Duration;
use esp_idf_svc::hal::delay::Delay;
use esp_idf_svc::hal::gpio::{Output, OutputPin, PinDriver};
use tm1637_embedded_hal::tokens::Blocking;
use tm1637_embedded_hal::{Brightness, TM1637Builder, TM1637};

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

    /// Показывает пару целых чисел в виде `NN:NN`.
    ///
    /// Физическое двоеточие управляется текущим состоянием `colon`.
    pub fn show_int_pair(&mut self, left: u8, right: u8) -> Result<(), DisplayError> {
        self.base_frame = format_int_pair_frame(left, right)?;
        self.render_current()
    }

    /// Показывает короткий ASCII-текст, обрезая его до четырёх символов.
    pub fn show_text(&mut self, text: &str, align: Align) -> Result<(), DisplayError> {
        self.base_frame = format_text_frame(text, align)?;
        self.render_current()
    }

    /// Показывает максимально похожее на `ERROR` статическое сообщение.
    pub fn show_error(&mut self) -> Result<(), DisplayError> {
        self.show_text(STATIC_ERROR_TEXT, Align::Left)
    }

    /// Один раз прокручивает ASCII-текст по индикатору как бегущую строку.
    pub fn scroll_text_once(
        &mut self,
        text: &str,
        step_delay: Duration,
    ) -> Result<(), DisplayError> {
        let source = build_scroll_source(text)?;
        for offset in 0..=source.len() - DISPLAY_WIDTH {
            self.base_frame = scroll_window_frame(&source, offset);
            self.render_current()?;
            self.display
                .delay_mut()
                .delay_ms(step_delay.as_millis() as u32);
        }

        Ok(())
    }

    /// Один раз прокручивает сообщение `Error`.
    pub fn scroll_error_once(&mut self, step_delay: Duration) -> Result<(), DisplayError> {
        self.scroll_text_once(SCROLL_ERROR_TEXT, step_delay)
    }

    pub(crate) fn render_frame(&mut self, frame: [u8; DISPLAY_WIDTH]) -> Result<(), DisplayError> {
        self.display.display_slice(0, &frame)?;
        Ok(())
    }

    fn render_current(&mut self) -> Result<(), DisplayError> {
        let mut frame = self.base_frame;
        apply_colon(&mut frame, self.colon);
        self.render_frame(frame)
    }
}
