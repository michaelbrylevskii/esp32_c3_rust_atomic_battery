use super::backend::LedSink;
use super::constants::{DEFAULT_WORKER_STACK_SIZE, DEFAULT_WORKER_TICK};
use super::pattern::LedPattern;
use core::fmt;
use core::time::Duration;
use std::io;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

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
            worker_tick: DEFAULT_WORKER_TICK,
            thread_stack_size: DEFAULT_WORKER_STACK_SIZE,
        }
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

        let levels = match &snapshot.mode {
            BufferedLedMode::Static(levels) => *levels,
            BufferedLedMode::Pattern(pattern) => {
                pattern.sample_levels(now.duration_since(mode_started))
            }
        };

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

fn store_led_worker_error(slot: &Arc<Mutex<Option<String>>>, error: String) {
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(error);
    }
}
