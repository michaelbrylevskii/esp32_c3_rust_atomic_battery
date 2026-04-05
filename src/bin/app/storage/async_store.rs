use super::sync_store::AppStorage;
use super::types::{AsyncStorageError, StorageBootstrap, DEFAULT_STORAGE_THREAD_STACK_SIZE};
use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};

/// Конфигурация фонового storage worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsyncStorageConfig {
    pub thread_stack_size: usize,
}

impl Default for AsyncStorageConfig {
    fn default() -> Self {
        Self {
            thread_stack_size: DEFAULT_STORAGE_THREAD_STACK_SIZE,
        }
    }
}

/// Событие завершения фоновой storage-команды.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AsyncStorageEvent {
    ConsumptionSaved {
        value: u32,
        result: Result<(), String>,
    },
    SessionIdAllocated(Result<u64, String>),
}

enum AsyncStorageCommand {
    SaveConsumption(u32),
    AllocateSessionId,
}

struct AsyncStorageState {
    events: VecDeque<AsyncStorageEvent>,
    shutdown: bool,
}

impl AsyncStorageState {
    fn new() -> Self {
        Self {
            events: VecDeque::new(),
            shutdown: false,
        }
    }
}

pub struct AsyncAppStorage {
    state: Arc<Mutex<AsyncStorageState>>,
    worker_error: Arc<Mutex<Option<String>>>,
    command_tx: mpsc::Sender<AsyncStorageCommand>,
    worker: Option<JoinHandle<()>>,
}

impl AsyncAppStorage {
    /// Создаёт storage worker и возвращает начальные настройки приложения.
    pub fn take(config: AsyncStorageConfig) -> Result<(Self, StorageBootstrap), AsyncStorageError> {
        let storage = AppStorage::take()?;
        let bootstrap = storage.load_bootstrap()?;
        let async_storage = Self::new(storage, config)?;
        Ok((async_storage, bootstrap))
    }

    pub fn new(storage: AppStorage, config: AsyncStorageConfig) -> Result<Self, AsyncStorageError> {
        if config.thread_stack_size == 0 {
            return Err(AsyncStorageError::InvalidThreadStackSize);
        }

        let state = Arc::new(Mutex::new(AsyncStorageState::new()));
        let worker_error = Arc::new(Mutex::new(None));
        let (command_tx, command_rx) = mpsc::channel();

        let worker_state = Arc::clone(&state);
        let worker_error_slot = Arc::clone(&worker_error);
        let worker = thread::Builder::new()
            .name("storage-worker".into())
            .stack_size(config.thread_stack_size)
            .spawn(move || {
                run_storage_worker(storage, worker_state, worker_error_slot, command_rx);
            })
            .map_err(AsyncStorageError::ThreadSpawn)?;

        Ok(Self {
            state,
            worker_error,
            command_tx,
            worker: Some(worker),
        })
    }

    /// Ставит в очередь сохранение `consumption_per_sec`.
    pub fn enqueue_save_consumption(&self, value: u32) -> Result<(), AsyncStorageError> {
        self.ensure_worker_alive()?;
        self.command_tx
            .send(AsyncStorageCommand::SaveConsumption(value))
            .map_err(|_| AsyncStorageError::WorkerStopped)
    }

    /// Запрашивает следующий `session_id`.
    pub fn request_next_session_id(&self) -> Result<(), AsyncStorageError> {
        self.ensure_worker_alive()?;
        self.command_tx
            .send(AsyncStorageCommand::AllocateSessionId)
            .map_err(|_| AsyncStorageError::WorkerStopped)
    }

    /// Возвращает и очищает накопившиеся события worker'а.
    pub fn drain_events(&self) -> Result<Vec<AsyncStorageEvent>, AsyncStorageError> {
        self.ensure_worker_alive()?;

        let mut guard = self
            .state
            .lock()
            .map_err(|_| AsyncStorageError::WorkerFailed("storage state mutex poisoned".into()))?;

        Ok(guard.events.drain(..).collect())
    }

    fn ensure_worker_alive(&self) -> Result<(), AsyncStorageError> {
        let worker_error = self
            .worker_error
            .lock()
            .map_err(|_| AsyncStorageError::WorkerFailed("storage error mutex poisoned".into()))?;

        if let Some(error) = worker_error.as_ref() {
            return Err(AsyncStorageError::WorkerFailed(error.clone()));
        }

        Ok(())
    }
}

impl Drop for AsyncAppStorage {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.shutdown = true;
        }

        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_storage_worker(
    storage: AppStorage,
    state: Arc<Mutex<AsyncStorageState>>,
    worker_error: Arc<Mutex<Option<String>>>,
    command_rx: mpsc::Receiver<AsyncStorageCommand>,
) {
    loop {
        let command = match command_rx.recv() {
            Ok(command) => command,
            Err(_) => return,
        };

        let shutdown = match state.lock() {
            Ok(guard) => guard.shutdown,
            Err(_) => {
                store_worker_error(&worker_error, "storage state mutex poisoned".into());
                return;
            }
        };

        if shutdown {
            return;
        }

        let event = match command {
            AsyncStorageCommand::SaveConsumption(value) => AsyncStorageEvent::ConsumptionSaved {
                value,
                result: storage
                    .save_consumption_per_sec(value)
                    .map_err(|err| err.to_string()),
            },
            AsyncStorageCommand::AllocateSessionId => AsyncStorageEvent::SessionIdAllocated(
                storage.next_session_id().map_err(|err| err.to_string()),
            ),
        };

        match state.lock() {
            Ok(mut guard) => guard.events.push_back(event),
            Err(_) => {
                store_worker_error(&worker_error, "storage state mutex poisoned".into());
                return;
            }
        }
    }
}

fn store_worker_error(slot: &Arc<Mutex<Option<String>>>, error: String) {
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(error);
    }
}
