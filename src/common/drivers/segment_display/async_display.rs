use super::constants::{DISPLAY_WIDTH, SCROLL_ERROR_TEXT, STATIC_ERROR_TEXT};
use super::frame::{build_scroll_source, format_int, format_int_pair_frame, format_text_frame};
use super::sync_display::SegmentDisplay4;
use super::types::{Align, AsyncDisplayConfig, AsyncDisplayError, DisplayConfig, IntFormat};
use super::worker::run_display_worker;
use core::time::Duration;
use esp_idf_svc::hal::gpio::OutputPin;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use tm1637_embedded_hal::Brightness;

#[derive(Clone)]
pub(super) enum BufferedContent {
    Static([u8; DISPLAY_WIDTH]),
    Countdown {
        initial_total_seconds: u32,
        step_period: Duration,
    },
    Scroll {
        source: Vec<u8>,
        step_delay: Duration,
        cycles: Option<u32>,
    },
}

#[derive(Clone, Copy)]
pub(super) enum BufferedColonMode {
    Static(bool),
    Blink {
        initial_on: bool,
        interval: Duration,
    },
    Pulse {
        initial_on: bool,
        period: Duration,
        on_duration: Duration,
    },
}

#[derive(Clone)]
pub(super) struct BufferedState {
    pub(super) content: BufferedContent,
    pub(super) content_generation: u64,
    pub(super) colon: BufferedColonMode,
    pub(super) colon_generation: u64,
    pub(super) brightness: Brightness,
    pub(super) brightness_generation: u64,
    pub(super) shutdown: bool,
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
                BufferedColonMode::Pulse { initial_on, .. } => initial_on,
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

    /// Показывает пару целых чисел в виде `NN:NN`.
    pub fn show_int_pair(&self, left: u8, right: u8) -> Result<(), AsyncDisplayError> {
        self.update_content(BufferedContent::Static(format_int_pair_frame(left, right)?))
    }

    /// Показывает автономный countdown в формате `MM:SS`.
    ///
    /// Значение пересчитывается фоновой задачей без участия основного цикла.
    pub fn start_countdown(
        &self,
        initial_total_seconds: u32,
        step_period: Duration,
    ) -> Result<(), AsyncDisplayError> {
        if step_period.is_zero() {
            return Err(AsyncDisplayError::InvalidAnimationDelay);
        }

        self.update_content(BufferedContent::Countdown {
            initial_total_seconds,
            step_period,
        })
    }

    /// Показывает короткий ASCII-текст, обрезая его до четырёх символов.
    pub fn show_text(&self, text: &str, align: Align) -> Result<(), AsyncDisplayError> {
        self.update_content(BufferedContent::Static(format_text_frame(text, align)?))
    }

    /// Показывает максимально похожее на `ERROR` статическое сообщение.
    pub fn show_error(&self) -> Result<(), AsyncDisplayError> {
        self.show_text(STATIC_ERROR_TEXT, Align::Left)
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
        self.start_scroll_text_cycles(text, step_delay, None)
    }

    /// Включает бегущую строку с заданным числом полных циклов.
    pub fn start_scroll_text_cycles(
        &self,
        text: &str,
        step_delay: Duration,
        cycles: Option<u32>,
    ) -> Result<(), AsyncDisplayError> {
        if step_delay.is_zero() {
            return Err(AsyncDisplayError::InvalidAnimationDelay);
        }

        self.update_content(BufferedContent::Scroll {
            source: build_scroll_source(text)?,
            step_delay,
            cycles,
        })
    }

    /// Включает непрерывную бегущую строку с сообщением `Error`.
    pub fn start_scroll_error(&self, step_delay: Duration) -> Result<(), AsyncDisplayError> {
        self.start_scroll_text(SCROLL_ERROR_TEXT, step_delay)
    }

    /// Запускает синхронный импульс двоеточия с заданным полным периодом.
    pub fn start_colon_pulse(
        &self,
        initial_on: bool,
        period: Duration,
        on_duration: Duration,
    ) -> Result<(), AsyncDisplayError> {
        if period.is_zero() || on_duration.is_zero() || on_duration > period {
            return Err(AsyncDisplayError::InvalidAnimationDelay);
        }

        self.with_state(|state| {
            state.colon = BufferedColonMode::Pulse {
                initial_on,
                period,
                on_duration,
            };
            state.colon_generation = state.colon_generation.wrapping_add(1);
        })
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

    fn with_state(&self, update: impl FnOnce(&mut BufferedState)) -> Result<(), AsyncDisplayError> {
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
