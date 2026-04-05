use super::constants::{DEFAULT_WORKER_STACK_SIZE, DEFAULT_WORKER_TICK};
use core::fmt;
use core::time::Duration;
use esp_idf_svc::hal::gpio::GpioError;
use esp_idf_svc::sys::EspError;
use std::io;
use tm1637_embedded_hal::{Brightness, Error as TmError};

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
    /// Левое значение пары должно быть в диапазоне `0..=99`.
    PairLeftOutOfRange(u8),
    /// Правое значение пары должно быть в диапазоне `0..=99`.
    PairRightOutOfRange(u8),
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
            DisplayError::PairLeftOutOfRange(value) => {
                write!(f, "left pair value {value} is out of range 0..=99")
            }
            DisplayError::PairRightOutOfRange(value) => {
                write!(f, "right pair value {value} is out of range 0..=99")
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
            worker_tick: DEFAULT_WORKER_TICK,
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
