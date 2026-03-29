//! Удобная обёртка над TM1637 для 4-разрядного индикатора с двоеточием посередине.
//!
//! Подробная документация на русском:
//! [docs/segment_display.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/segment_display.md)

use core::fmt;
use core::time::Duration;
use esp_idf_svc::hal::delay::Delay;
use esp_idf_svc::hal::gpio::{GpioError, Output, OutputPin, PinDriver};
use esp_idf_svc::sys::EspError;
use std::io;
use std::string::String;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;
use tm1637_embedded_hal::formatters;
use tm1637_embedded_hal::mappings::from_ascii_byte;
use tm1637_embedded_hal::tokens::Blocking;
use tm1637_embedded_hal::{Brightness, Error as TmError, TM1637Builder, TM1637};

const COLON_MASK: u8 = 0b1000_0000;
const DISPLAY_WIDTH: usize = 4;
const ERROR_TEXT: &str = "Error";
const DEFAULT_WORKER_STACK_SIZE: usize = 4096;

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

/// Настройки фоновой задачи дисплея.
///
/// `worker_tick` определяет, как часто фоновая задача пересчитывает бегущую строку
/// и мигание двоеточия. Это не шаг анимации, а внутренний период обслуживания.
#[derive(Clone, Copy, Debug)]
pub struct AsyncDisplayConfig {
    /// Период обслуживания фоновой задачи.
    pub worker_tick: Duration,
    /// Размер стека для фоновой задачи.
    pub thread_stack_size: usize,
}

impl Default for AsyncDisplayConfig {
    fn default() -> Self {
        Self {
            worker_tick: Duration::from_millis(20),
            thread_stack_size: DEFAULT_WORKER_STACK_SIZE,
        }
    }
}

/// Ошибки неблокирующей обёртки дисплея.
#[derive(Debug)]
pub enum AsyncDisplayError {
    /// Ошибка low-level слоя дисплея.
    Display(DisplayError),
    /// Не удалось запустить фоновую задачу.
    ThreadSpawn(io::Error),
    /// Фоновая задача уже остановлена.
    WorkerStopped,
    /// Фоновая задача завершилась с ошибкой.
    WorkerFailed(String),
    /// `worker_tick` должен быть больше нуля.
    InvalidWorkerTick,
    /// Интервал анимации должен быть больше нуля.
    InvalidAnimationDelay,
}

impl From<DisplayError> for AsyncDisplayError {
    fn from(value: DisplayError) -> Self {
        Self::Display(value)
    }
}

impl fmt::Display for AsyncDisplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsyncDisplayError::Display(err) => write!(f, "display error: {err}"),
            AsyncDisplayError::ThreadSpawn(err) => {
                write!(f, "failed to spawn display worker thread: {err}")
            }
            AsyncDisplayError::WorkerStopped => write!(f, "display worker thread has stopped"),
            AsyncDisplayError::WorkerFailed(err) => {
                write!(f, "display worker thread failed: {err}")
            }
            AsyncDisplayError::InvalidWorkerTick => {
                write!(f, "display worker tick must be greater than zero")
            }
            AsyncDisplayError::InvalidAnimationDelay => {
                write!(f, "animation delay must be greater than zero")
            }
        }
    }
}

impl std::error::Error for AsyncDisplayError {}

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

#[derive(Clone)]
enum BufferedContent {
    Static([u8; DISPLAY_WIDTH]),
    Scroll {
        source: Vec<u8>,
        step_delay: Duration,
    },
}

#[derive(Clone, Copy)]
enum BufferedColonMode {
    Static(bool),
    Blink {
        initial_on: bool,
        interval: Duration,
    },
}

#[derive(Clone)]
struct BufferedState {
    content: BufferedContent,
    content_generation: u64,
    colon: BufferedColonMode,
    colon_generation: u64,
    brightness: Brightness,
    brightness_generation: u64,
    shutdown: bool,
}

impl BufferedState {
    fn new(brightness: Brightness) -> Self {
        Self {
            content: BufferedContent::Static([0; DISPLAY_WIDTH]),
            content_generation: 0,
            colon: BufferedColonMode::Static(false),
            colon_generation: 0,
            brightness,
            brightness_generation: 0,
            shutdown: false,
        }
    }
}

/// Неблокирующая обёртка над `SegmentDisplay4`.
///
/// Все вызовы `show_*` и анимации только обновляют внутренний буфер состояния.
/// Реальный вывод на TM1637 выполняется фоновой задачей, поэтому основной код
/// может продолжать работать, пока дисплей крутит строку или мигает двоеточием.
pub struct AsyncSegmentDisplay4 {
    state: Arc<Mutex<BufferedState>>,
    worker_error: Arc<Mutex<Option<String>>>,
    worker: Option<JoinHandle<()>>,
}

impl AsyncSegmentDisplay4 {
    /// Создаёт дисплей с настройками по умолчанию и сразу запускает фоновую задачу.
    pub fn new(
        clk: impl OutputPin + Send + 'static,
        dio: impl OutputPin + Send + 'static,
    ) -> Result<Self, AsyncDisplayError> {
        Self::with_config(
            clk,
            dio,
            DisplayConfig::default(),
            AsyncDisplayConfig::default(),
        )
    }

    /// Создаёт дисплей с отдельными настройками low-level драйвера и фоновой задачи.
    pub fn with_config(
        clk: impl OutputPin + Send + 'static,
        dio: impl OutputPin + Send + 'static,
        display_config: DisplayConfig,
        async_config: AsyncDisplayConfig,
    ) -> Result<Self, AsyncDisplayError> {
        if async_config.worker_tick.is_zero() {
            return Err(AsyncDisplayError::InvalidWorkerTick);
        }

        let mut display = SegmentDisplay4::with_config(clk, dio, display_config)?;
        display.init()?;

        let state = Arc::new(Mutex::new(BufferedState::new(display_config.brightness)));
        let worker_error = Arc::new(Mutex::new(None));

        let worker_state = Arc::clone(&state);
        let worker_error_slot = Arc::clone(&worker_error);
        let worker = thread::Builder::new()
            .name("tm1637-worker".into())
            .stack_size(async_config.thread_stack_size)
            .spawn(move || {
                run_display_worker(
                    display,
                    worker_state,
                    worker_error_slot,
                    async_config.worker_tick,
                );
            })
            .map_err(AsyncDisplayError::ThreadSpawn)?;

        Ok(Self {
            state,
            worker_error,
            worker: Some(worker),
        })
    }

    /// Очищает буфер дисплея.
    pub fn clear(&self) -> Result<(), AsyncDisplayError> {
        self.update_content(BufferedContent::Static([0; DISPLAY_WIDTH]))
    }

    /// Меняет яркость дисплея.
    pub fn set_brightness(&self, brightness: Brightness) -> Result<(), AsyncDisplayError> {
        self.with_state(|state| {
            state.brightness = brightness;
            state.brightness_generation = state.brightness_generation.wrapping_add(1);
        })
    }

    /// Явно включает или выключает двоеточие.
    ///
    /// Вызов переводит двоеточие в статический режим и отключает мигание.
    pub fn set_colon(&self, enabled: bool) -> Result<(), AsyncDisplayError> {
        self.with_state(|state| {
            state.colon = BufferedColonMode::Static(enabled);
            state.colon_generation = state.colon_generation.wrapping_add(1);
        })
    }

    /// Инвертирует двоеточие и переводит его в статический режим.
    pub fn toggle_colon(&self) -> Result<(), AsyncDisplayError> {
        self.with_state(|state| {
            let enabled = match state.colon {
                BufferedColonMode::Static(enabled) => enabled,
                BufferedColonMode::Blink { initial_on, .. } => initial_on,
            };
            state.colon = BufferedColonMode::Static(!enabled);
            state.colon_generation = state.colon_generation.wrapping_add(1);
        })
    }

    /// Запускает независимое мигание двоеточия.
    pub fn start_colon_blink(
        &self,
        initial_on: bool,
        interval: Duration,
    ) -> Result<(), AsyncDisplayError> {
        if interval.is_zero() {
            return Err(AsyncDisplayError::InvalidAnimationDelay);
        }

        self.with_state(|state| {
            state.colon = BufferedColonMode::Blink {
                initial_on,
                interval,
            };
            state.colon_generation = state.colon_generation.wrapping_add(1);
        })
    }

    /// Останавливает мигание двоеточия и фиксирует его в выбранном состоянии.
    pub fn stop_colon_blink(&self, enabled: bool) -> Result<(), AsyncDisplayError> {
        self.set_colon(enabled)
    }

    /// Показывает целое число в четырёх знакоместах.
    pub fn show_int(&self, value: i16, format: IntFormat) -> Result<(), AsyncDisplayError> {
        self.update_content(BufferedContent::Static(format_int(value, format)?))
    }

    /// Показывает время в виде `MM:SS`.
    pub fn show_mmss(&self, minutes: u8, seconds: u8) -> Result<(), AsyncDisplayError> {
        let frame = formatters::clock_to_4digits(minutes, seconds, false);
        if minutes > 99 {
            return Err(DisplayError::MinutesOutOfRange(minutes).into());
        }
        if seconds > 99 {
            return Err(DisplayError::SecondsOutOfRange(seconds).into());
        }

        self.update_content(BufferedContent::Static(frame))
    }

    /// Показывает короткий ASCII-текст, обрезая его до четырёх символов.
    pub fn show_text(&self, text: &str, align: Align) -> Result<(), AsyncDisplayError> {
        self.update_content(BufferedContent::Static(format_text_frame(text, align)?))
    }

    /// Показывает максимально похожее на `ERROR` статическое сообщение.
    pub fn show_error(&self) -> Result<(), AsyncDisplayError> {
        self.show_text("Erro", Align::Left)
    }

    /// Включает непрерывную бегущую строку.
    ///
    /// Анимация крутится до тех пор, пока не будет вызван другой `show_*`
    /// или `start_scroll_text(...)` с новым сообщением.
    pub fn start_scroll_text(
        &self,
        text: &str,
        step_delay: Duration,
    ) -> Result<(), AsyncDisplayError> {
        if step_delay.is_zero() {
            return Err(AsyncDisplayError::InvalidAnimationDelay);
        }

        self.update_content(BufferedContent::Scroll {
            source: build_scroll_source(text)?,
            step_delay,
        })
    }

    /// Включает непрерывную бегущую строку с сообщением `Error`.
    pub fn start_scroll_error(&self, step_delay: Duration) -> Result<(), AsyncDisplayError> {
        self.start_scroll_text(ERROR_TEXT, step_delay)
    }

    /// Возвращает последнюю фатальную ошибку фоновой задачи, если она была.
    pub fn last_worker_error(&self) -> Option<String> {
        self.worker_error
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn update_content(&self, content: BufferedContent) -> Result<(), AsyncDisplayError> {
        self.with_state(|state| {
            state.content = content;
            state.content_generation = state.content_generation.wrapping_add(1);
        })
    }

    fn with_state(
        &self,
        update: impl FnOnce(&mut BufferedState),
    ) -> Result<(), AsyncDisplayError> {
        self.ensure_worker_alive()?;

        let mut state = self
            .state
            .lock()
            .map_err(|_| AsyncDisplayError::WorkerFailed("display state mutex poisoned".into()))?;

        if state.shutdown {
            return Err(AsyncDisplayError::WorkerStopped);
        }

        update(&mut state);
        Ok(())
    }

    fn ensure_worker_alive(&self) -> Result<(), AsyncDisplayError> {
        let worker_error = self
            .worker_error
            .lock()
            .map_err(|_| AsyncDisplayError::WorkerFailed("display error mutex poisoned".into()))?;

        if let Some(error) = worker_error.as_ref() {
            return Err(AsyncDisplayError::WorkerFailed(error.clone()));
        }

        Ok(())
    }
}

impl Drop for AsyncSegmentDisplay4 {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.shutdown = true;
        }

        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
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

fn run_display_worker(
    mut display: SegmentDisplay4<'static>,
    state: Arc<Mutex<BufferedState>>,
    worker_error: Arc<Mutex<Option<String>>>,
    worker_tick: Duration,
) {
    let mut content_generation = 0u64;
    let mut colon_generation = 0u64;
    let mut brightness_generation = 0u64;
    let mut content_started = Instant::now();
    let mut colon_started = Instant::now();
    let mut last_rendered_frame = [u8::MAX; DISPLAY_WIDTH];

    loop {
        let snapshot = match state.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                store_worker_error(
                    &worker_error,
                    "display worker state mutex poisoned".to_owned(),
                );
                return;
            }
        };

        if snapshot.shutdown {
            return;
        }

        let now = Instant::now();

        if snapshot.content_generation != content_generation {
            content_generation = snapshot.content_generation;
            content_started = now;
        }

        if snapshot.colon_generation != colon_generation {
            colon_generation = snapshot.colon_generation;
            colon_started = now;
        }

        if snapshot.brightness_generation != brightness_generation {
            if let Err(err) = display.set_brightness(snapshot.brightness) {
                store_worker_error(
                    &worker_error,
                    format!("failed to apply display brightness: {err}"),
                );
                return;
            }

            brightness_generation = snapshot.brightness_generation;
        }

        let mut frame = frame_from_content(&snapshot.content, content_started, now);
        apply_colon(
            &mut frame,
            colon_is_on(snapshot.colon, colon_started, now),
        );

        if frame != last_rendered_frame {
            if let Err(err) = display.display.display_slice(0, &frame) {
                store_worker_error(
                    &worker_error,
                    format!("failed to render display frame: {err:?}"),
                );
                return;
            }

            last_rendered_frame = frame;
        }

        thread::sleep(worker_tick);
    }
}

fn frame_from_content(
    content: &BufferedContent,
    content_started: Instant,
    now: Instant,
) -> [u8; DISPLAY_WIDTH] {
    match content {
        BufferedContent::Static(frame) => *frame,
        BufferedContent::Scroll { source, step_delay } => {
            let windows = source.len().saturating_sub(DISPLAY_WIDTH) + 1;
            let offset = if windows <= 1 {
                0
            } else {
                animation_steps(now, content_started, *step_delay) % windows
            };

            let mut frame = [0u8; DISPLAY_WIDTH];
            for (index, byte) in source[offset..offset + DISPLAY_WIDTH].iter().enumerate() {
                frame[index] = from_ascii_byte(*byte);
            }
            frame
        }
    }
}

fn colon_is_on(mode: BufferedColonMode, colon_started: Instant, now: Instant) -> bool {
    match mode {
        BufferedColonMode::Static(enabled) => enabled,
        BufferedColonMode::Blink {
            initial_on,
            interval,
        } => {
            if animation_steps(now, colon_started, interval) % 2 == 0 {
                initial_on
            } else {
                !initial_on
            }
        }
    }
}

fn animation_steps(now: Instant, started_at: Instant, step_delay: Duration) -> usize {
    let step_millis = step_delay.as_millis();
    if step_millis == 0 {
        return 0;
    }

    (now.duration_since(started_at).as_millis() / step_millis) as usize
}

fn store_worker_error(worker_error: &Arc<Mutex<Option<String>>>, error: String) {
    if let Ok(mut slot) = worker_error.lock() {
        *slot = Some(error);
    }
}
