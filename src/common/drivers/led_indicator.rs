//! Универсальная неблокирующая индикация для одного или нескольких светодиодов.
//!
//! Подробная документация на русском:
//! [docs/led_indicator.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/led_indicator.md)

use core::fmt;
use core::time::Duration;
use esp_idf_svc::hal::gpio::{Level, Output, PinDriver};
use esp_idf_svc::sys::EspError;
use std::array;
use std::io;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

const DEFAULT_WORKER_STACK_SIZE: usize = 4096;

/// Максимальный логический уровень яркости.
///
/// Значение `0` трактуется как полностью выключенный канал, `255` как полностью включенный.
pub const LEVEL_MAX: u8 = u8::MAX;

/// Полярность подключения отдельного светодиода.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedPolarity {
    /// Светодиод загорается при `HIGH`.
    ActiveHigh,
    /// Светодиод загорается при `LOW`.
    ActiveLow,
}

impl LedPolarity {
    fn to_gpio_level(self, level: u8) -> Level {
        let is_on = level > 0;
        match (self, is_on) {
            (LedPolarity::ActiveHigh, true) => Level::High,
            (LedPolarity::ActiveHigh, false) => Level::Low,
            (LedPolarity::ActiveLow, true) => Level::Low,
            (LedPolarity::ActiveLow, false) => Level::High,
        }
    }
}

/// Интерфейс backend'а, который умеет применять уровни яркости к группе каналов.
///
/// Модель уровней рассчитана на диапазон `0..=255`. Цифровой backend ниже
/// использует только факт `0 / non-zero`, а PWM backend сможет использовать
/// полную градацию без изменения внешнего API.
pub trait LedSink<const N: usize>: Send {
    /// Ошибка backend'а.
    type Error: fmt::Display + Send + 'static;

    /// Применяет логические уровни яркости к группе каналов.
    fn write_levels(&mut self, levels: [u8; N]) -> Result<(), Self::Error>;
}

/// Цифровой backend для группы GPIO-светодиодов.
///
/// Он не делает аппаратный PWM: любой уровень больше нуля считается состоянием "включено".
pub struct DigitalLedGroup<'d, const N: usize> {
    leds: [PinDriver<'d, Output>; N],
    polarities: [LedPolarity; N],
}

impl<'d, const N: usize> DigitalLedGroup<'d, N> {
    /// Создаёт цифровой backend из уже настроенных output-пинов.
    pub fn new(
        leds: [PinDriver<'d, Output>; N],
        polarities: [LedPolarity; N],
    ) -> Result<Self, EspError> {
        let mut group = Self { leds, polarities };
        group.write_levels([0; N])?;
        Ok(group)
    }
}

impl<'d, const N: usize> LedSink<N> for DigitalLedGroup<'d, N> {
    type Error = EspError;

    fn write_levels(&mut self, levels: [u8; N]) -> Result<(), Self::Error> {
        for ((led, polarity), level) in self
            .leds
            .iter_mut()
            .zip(self.polarities.iter())
            .zip(levels.iter())
        {
            led.set_level(polarity.to_gpio_level(*level))?;
        }
        Ok(())
    }
}

/// Настройки фонового LED worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsyncLedConfig {
    /// Период обслуживания фонового worker'а.
    pub worker_tick: Duration,
    /// Размер стека фонового worker'а.
    pub thread_stack_size: usize,
}

impl Default for AsyncLedConfig {
    fn default() -> Self {
        Self {
            worker_tick: Duration::from_millis(20),
            thread_stack_size: DEFAULT_WORKER_STACK_SIZE,
        }
    }
}

/// Режим повторения паттерна.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatMode {
    /// Выполнить паттерн один раз.
    Once,
    /// Выполнить паттерн указанное число раз.
    Times(u32),
    /// Крутить паттерн бесконечно.
    Forever,
}

/// Тип интерполяции между двумя наборами уровней.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Easing {
    /// Линейный переход.
    Linear,
}

/// Один шаг LED-паттерна.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedPatternStep<const N: usize> {
    /// Удерживать заданные уровни указанное время.
    Hold { levels: [u8; N], duration: Duration },
    /// Плавно перейти от одного набора уровней к другому.
    Transition {
        from: [u8; N],
        to: [u8; N],
        duration: Duration,
        easing: Easing,
    },
}

impl<const N: usize> LedPatternStep<N> {
    fn duration(&self) -> Duration {
        match self {
            LedPatternStep::Hold { duration, .. } => *duration,
            LedPatternStep::Transition { duration, .. } => *duration,
        }
    }

    fn terminal_levels(&self) -> [u8; N] {
        match self {
            LedPatternStep::Hold { levels, .. } => *levels,
            LedPatternStep::Transition { to, .. } => *to,
        }
    }
}

/// Последовательность LED-шагов, которую выполняет async worker.
///
/// Паттерн хранит уровни для всех каналов сразу. За счёт этого один и тот же
/// механизм подходит и для одиночного LED, и для пары `red/green`, и для
/// более сложных multi-channel сценариев.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedPattern<const N: usize> {
    steps: Vec<LedPatternStep<N>>,
    repeat: RepeatMode,
    final_levels: Option<[u8; N]>,
}

impl<const N: usize> LedPattern<N> {
    /// Создаёт пустой паттерн.
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            repeat: RepeatMode::Once,
            final_levels: None,
        }
    }

    /// Устанавливает режим повторения паттерна.
    pub fn repeat(mut self, repeat: RepeatMode) -> Self {
        self.repeat = repeat;
        self
    }

    /// Устанавливает финальные уровни после завершения конечного паттерна.
    pub fn final_levels(mut self, levels: [u8; N]) -> Self {
        self.final_levels = Some(levels);
        self
    }

    /// Добавляет шаг удержания.
    pub fn hold(mut self, levels: [u8; N], duration: Duration) -> Self {
        self.steps.push(LedPatternStep::Hold { levels, duration });
        self
    }

    /// Добавляет шаг линейного перехода.
    pub fn transition(mut self, from: [u8; N], to: [u8; N], duration: Duration) -> Self {
        self.steps.push(LedPatternStep::Transition {
            from,
            to,
            duration,
            easing: Easing::Linear,
        });
        self
    }

    /// Возвращает паттерн простого мигания между двумя состояниями.
    pub fn blink(
        on_levels: [u8; N],
        off_levels: [u8; N],
        on_duration: Duration,
        off_duration: Duration,
        times: u32,
    ) -> Self {
        Self::new()
            .hold(on_levels, on_duration)
            .hold(off_levels, off_duration)
            .repeat(RepeatMode::Times(times.max(1)))
            .final_levels(off_levels)
    }

    /// Возвращает паттерн попеременного переключения между двумя кадрами.
    pub fn alternate(
        first_levels: [u8; N],
        second_levels: [u8; N],
        phase_duration: Duration,
        cycles: u32,
    ) -> Self {
        Self::blink(
            first_levels,
            second_levels,
            phase_duration,
            phase_duration,
            cycles,
        )
    }

    /// Возвращает паттерн "пульсации" с линейным нарастанием и спадом.
    pub fn pulse(peak_levels: [u8; N], rise: Duration, fall: Duration, cycles: u32) -> Self {
        Self::new()
            .transition([0; N], peak_levels, rise)
            .transition(peak_levels, [0; N], fall)
            .repeat(RepeatMode::Times(cycles.max(1)))
            .final_levels([0; N])
    }

    fn validate(&self) -> Result<(), AsyncLedError> {
        if self.steps.is_empty() {
            return Err(AsyncLedError::EmptyPattern);
        }

        if self.steps.iter().any(|step| step.duration().is_zero()) {
            return Err(AsyncLedError::InvalidStepDuration);
        }

        Ok(())
    }

    fn total_duration(&self) -> Duration {
        self.steps
            .iter()
            .fold(Duration::ZERO, |sum, step| sum + step.duration())
    }

    fn default_final_levels(&self) -> [u8; N] {
        self.final_levels.unwrap_or_else(|| {
            self.steps
                .last()
                .map(|step| step.terminal_levels())
                .unwrap_or([0; N])
        })
    }
}

impl<const N: usize> Default for LedPattern<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Ошибки async LED слоя.
#[derive(Debug)]
pub enum AsyncLedError {
    /// Не удалось запустить фоновый поток.
    ThreadSpawn(io::Error),
    /// Фоновый поток уже остановился.
    WorkerStopped,
    /// Фоновый поток завершился с ошибкой.
    WorkerFailed(String),
    /// Период фонового worker'а должен быть больше нуля.
    InvalidWorkerTick,
    /// Паттерн не содержит шагов.
    EmptyPattern,
    /// Длительность шага должна быть больше нуля.
    InvalidStepDuration,
}

impl fmt::Display for AsyncLedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsyncLedError::ThreadSpawn(err) => {
                write!(f, "failed to spawn LED worker thread: {err}")
            }
            AsyncLedError::WorkerStopped => write!(f, "LED worker thread has stopped"),
            AsyncLedError::WorkerFailed(err) => write!(f, "LED worker thread failed: {err}"),
            AsyncLedError::InvalidWorkerTick => {
                write!(f, "LED worker tick must be greater than zero")
            }
            AsyncLedError::EmptyPattern => write!(f, "LED pattern must contain at least one step"),
            AsyncLedError::InvalidStepDuration => {
                write!(f, "LED pattern step duration must be greater than zero")
            }
        }
    }
}

impl std::error::Error for AsyncLedError {}

#[derive(Clone)]
enum BufferedLedMode<const N: usize> {
    Static([u8; N]),
    Pattern(LedPattern<N>),
}

#[derive(Clone)]
struct BufferedLedState<const N: usize> {
    mode: BufferedLedMode<N>,
    generation: u64,
    shutdown: bool,
}

impl<const N: usize> BufferedLedState<N> {
    fn new() -> Self {
        Self {
            mode: BufferedLedMode::Static([0; N]),
            generation: 0,
            shutdown: false,
        }
    }
}

/// Неблокирующий контроллер для группы LED-каналов.
///
/// Он принимает либо статические уровни, либо полноценные паттерны. Сам worker
/// живёт в отдельном потоке и не блокирует основную state machine приложения.
pub struct AsyncLedController<const N: usize> {
    state: Arc<Mutex<BufferedLedState<N>>>,
    worker_error: Arc<Mutex<Option<String>>>,
    worker: Option<JoinHandle<()>>,
}

impl<const N: usize> AsyncLedController<N> {
    /// Создаёт async LED controller поверх произвольного backend'а.
    pub fn new<S>(mut sink: S, config: AsyncLedConfig) -> Result<Self, AsyncLedError>
    where
        S: LedSink<N> + 'static,
    {
        if config.worker_tick.is_zero() {
            return Err(AsyncLedError::InvalidWorkerTick);
        }

        sink.write_levels([0; N])
            .map_err(|err| AsyncLedError::WorkerFailed(err.to_string()))?;

        let state = Arc::new(Mutex::new(BufferedLedState::new()));
        let worker_error = Arc::new(Mutex::new(None));

        let worker_state = Arc::clone(&state);
        let worker_error_slot = Arc::clone(&worker_error);
        let worker = thread::Builder::new()
            .name("led-worker".into())
            .stack_size(config.thread_stack_size)
            .spawn(move || {
                run_led_worker(sink, worker_state, worker_error_slot, config.worker_tick);
            })
            .map_err(AsyncLedError::ThreadSpawn)?;

        Ok(Self {
            state,
            worker_error,
            worker: Some(worker),
        })
    }

    /// Явно устанавливает уровни всех каналов.
    pub fn set_levels(&self, levels: [u8; N]) -> Result<(), AsyncLedError> {
        self.with_state(|state| {
            state.mode = BufferedLedMode::Static(levels);
            state.generation = state.generation.wrapping_add(1);
        })
    }

    /// Выключает все каналы.
    pub fn turn_off(&self) -> Result<(), AsyncLedError> {
        self.set_levels([0; N])
    }

    /// Запускает заданный паттерн.
    pub fn play_pattern(&self, pattern: LedPattern<N>) -> Result<(), AsyncLedError> {
        pattern.validate()?;
        self.with_state(|state| {
            state.mode = BufferedLedMode::Pattern(pattern);
            state.generation = state.generation.wrapping_add(1);
        })
    }

    /// Возвращает последнюю фатальную ошибку фонового потока, если она была.
    pub fn last_worker_error(&self) -> Option<String> {
        self.worker_error
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn with_state(
        &self,
        update: impl FnOnce(&mut BufferedLedState<N>),
    ) -> Result<(), AsyncLedError> {
        self.ensure_worker_alive()?;

        let mut state = self
            .state
            .lock()
            .map_err(|_| AsyncLedError::WorkerFailed("LED state mutex poisoned".into()))?;

        if state.shutdown {
            return Err(AsyncLedError::WorkerStopped);
        }

        update(&mut state);
        Ok(())
    }

    fn ensure_worker_alive(&self) -> Result<(), AsyncLedError> {
        let worker_error = self
            .worker_error
            .lock()
            .map_err(|_| AsyncLedError::WorkerFailed("LED error mutex poisoned".into()))?;

        if let Some(error) = worker_error.as_ref() {
            return Err(AsyncLedError::WorkerFailed(error.clone()));
        }

        Ok(())
    }
}

impl<const N: usize> Drop for AsyncLedController<N> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.shutdown = true;
        }

        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_led_worker<S, const N: usize>(
    mut sink: S,
    state: Arc<Mutex<BufferedLedState<N>>>,
    worker_error: Arc<Mutex<Option<String>>>,
    worker_tick: Duration,
) where
    S: LedSink<N>,
{
    let mut generation = 0u64;
    let mut mode_started = Instant::now();
    let mut last_levels = [u8::MAX; N];

    loop {
        let snapshot = match state.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                store_led_worker_error(&worker_error, "LED state mutex poisoned".into());
                return;
            }
        };

        if snapshot.shutdown {
            return;
        }

        let now = Instant::now();
        if snapshot.generation != generation {
            generation = snapshot.generation;
            mode_started = now;
        }

        let levels = levels_from_mode(&snapshot.mode, mode_started, now);
        if levels != last_levels {
            if let Err(err) = sink.write_levels(levels) {
                store_led_worker_error(&worker_error, format!("failed to apply LED levels: {err}"));
                return;
            }
            last_levels = levels;
        }

        thread::sleep(worker_tick);
    }
}

fn levels_from_mode<const N: usize>(
    mode: &BufferedLedMode<N>,
    started_at: Instant,
    now: Instant,
) -> [u8; N] {
    match mode {
        BufferedLedMode::Static(levels) => *levels,
        BufferedLedMode::Pattern(pattern) => levels_from_pattern(pattern, started_at, now),
    }
}

fn levels_from_pattern<const N: usize>(
    pattern: &LedPattern<N>,
    started_at: Instant,
    now: Instant,
) -> [u8; N] {
    if pattern.steps.is_empty() {
        return [0; N];
    }

    let cycle_duration_ms = pattern.total_duration().as_millis();
    if cycle_duration_ms == 0 {
        return pattern.default_final_levels();
    }

    let elapsed_ms = now.duration_since(started_at).as_millis();
    let cycle_elapsed_ms = match pattern.repeat {
        RepeatMode::Forever => elapsed_ms % cycle_duration_ms,
        RepeatMode::Once => {
            if elapsed_ms >= cycle_duration_ms {
                return pattern.default_final_levels();
            }
            elapsed_ms
        }
        RepeatMode::Times(times) => {
            let repeats = u128::from(times.max(1));
            let total_duration_ms = cycle_duration_ms.saturating_mul(repeats);
            if elapsed_ms >= total_duration_ms {
                return pattern.default_final_levels();
            }
            elapsed_ms % cycle_duration_ms
        }
    };

    let mut accumulated_ms = 0u128;
    for step in &pattern.steps {
        let step_duration_ms = step.duration().as_millis();
        if cycle_elapsed_ms < accumulated_ms.saturating_add(step_duration_ms) {
            let step_elapsed_ms = cycle_elapsed_ms.saturating_sub(accumulated_ms);
            return levels_from_step(step, step_elapsed_ms);
        }
        accumulated_ms = accumulated_ms.saturating_add(step_duration_ms);
    }

    pattern.default_final_levels()
}

fn levels_from_step<const N: usize>(step: &LedPatternStep<N>, elapsed_ms: u128) -> [u8; N] {
    match step {
        LedPatternStep::Hold { levels, .. } => *levels,
        LedPatternStep::Transition {
            from,
            to,
            duration,
            easing,
        } => interpolate_levels(*from, *to, elapsed_ms, duration.as_millis(), *easing),
    }
}

fn interpolate_levels<const N: usize>(
    from: [u8; N],
    to: [u8; N],
    elapsed_ms: u128,
    duration_ms: u128,
    easing: Easing,
) -> [u8; N] {
    if duration_ms == 0 {
        return to;
    }

    match easing {
        Easing::Linear => array::from_fn(|index| {
            let from_level = i32::from(from[index]);
            let to_level = i32::from(to[index]);
            let delta = to_level - from_level;
            let progress = elapsed_ms.min(duration_ms) as i128;
            let value =
                i128::from(from_level) + (i128::from(delta) * progress) / duration_ms as i128;
            value.clamp(i128::from(u8::MIN), i128::from(u8::MAX)) as u8
        }),
    }
}

fn store_led_worker_error(slot: &Arc<Mutex<Option<String>>>, error: String) {
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(error);
    }
}
