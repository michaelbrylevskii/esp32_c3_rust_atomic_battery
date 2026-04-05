//! Async worker-обёртка над sync `NfcTag`.

use super::constants::{
    DEFAULT_ASYNC_POLL_INTERVAL, DEFAULT_ASYNC_POLL_TIMEOUT, DEFAULT_ASYNC_REMOVAL_DEBOUNCE,
    DEFAULT_ASYNC_THREAD_STACK_SIZE,
};
use super::sync_nfc::{NfcError, NfcTag, TagInfo};
use crate::utils::kv_store::KvStore;
use core::fmt::{self, Debug};
use core::time::Duration;
use pn532::{CountDown, Interface};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

/// Настройки фонового NFC worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsyncNfcConfig {
    /// Частота итераций worker-потока.
    pub poll_interval: Duration,
    /// Таймаут одной попытки `poll_tag`.
    pub poll_timeout: Duration,
    /// Сколько держать последнюю метку после единичных промахов чтения.
    pub removal_debounce: Duration,
    /// Размер стека фонового потока.
    pub thread_stack_size: usize,
}

impl Default for AsyncNfcConfig {
    fn default() -> Self {
        Self {
            poll_interval: DEFAULT_ASYNC_POLL_INTERVAL,
            poll_timeout: DEFAULT_ASYNC_POLL_TIMEOUT,
            removal_debounce: DEFAULT_ASYNC_REMOVAL_DEBOUNCE,
            thread_stack_size: DEFAULT_ASYNC_THREAD_STACK_SIZE,
        }
    }
}

/// Результат чтения payload у текущей метки.
#[derive(Clone, Debug, PartialEq)]
pub enum AsyncTagPayload {
    KvStore(KvStore),
    Empty,
    ReadError(String),
}

/// Снэпшот последней увиденной метки в фоне.
#[derive(Clone, Debug, PartialEq)]
pub struct AsyncObservedTag {
    pub info: TagInfo,
    pub payload: AsyncTagPayload,
}

/// Состояние, которое возвращает async NFC worker.
#[derive(Clone, Debug, PartialEq)]
pub struct AsyncNfcSnapshot {
    pub generation: u64,
    pub tag: Option<AsyncObservedTag>,
}

/// Ошибки async NFC worker.
#[derive(Debug)]
pub enum AsyncNfcError {
    ThreadSpawn(std::io::Error),
    WorkerStopped,
    WorkerFailed(String),
    InvalidWorkerConfig(&'static str),
    CommandFailed(String),
}

impl fmt::Display for AsyncNfcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsyncNfcError::ThreadSpawn(err) => write!(f, "failed to spawn NFC worker: {err}"),
            AsyncNfcError::WorkerStopped => write!(f, "NFC worker has stopped"),
            AsyncNfcError::WorkerFailed(err) => write!(f, "NFC worker failed: {err}"),
            AsyncNfcError::InvalidWorkerConfig(err) => {
                write!(f, "invalid NFC worker config: {err}")
            }
            AsyncNfcError::CommandFailed(err) => write!(f, "NFC command failed: {err}"),
        }
    }
}

impl std::error::Error for AsyncNfcError {}

#[derive(Clone)]
struct AsyncNfcState {
    snapshot: AsyncNfcSnapshot,
    shutdown: bool,
}

impl AsyncNfcState {
    fn new() -> Self {
        Self {
            snapshot: AsyncNfcSnapshot {
                generation: 0,
                tag: None,
            },
            shutdown: false,
        }
    }
}

enum AsyncNfcCommand {
    WriteKvStore {
        expected_uid: Vec<u8>,
        store: KvStore,
        reply: mpsc::Sender<Result<(), String>>,
    },
}

/// Async-обёртка над sync `NfcTag`.
///
/// Worker-поток сам опрашивает PN532 и кэширует последнюю увиденную метку,
/// а основной код читает только snapshot и отправляет редкие команды записи.
pub struct AsyncNfcTag<I, T, const N: usize>
where
    I: Interface + Send + 'static,
    I::Error: Debug + fmt::Display + Send + 'static,
    T: CountDown<Time = Duration> + Send + 'static,
{
    state: Arc<Mutex<AsyncNfcState>>,
    worker_error: Arc<Mutex<Option<String>>>,
    command_tx: mpsc::Sender<AsyncNfcCommand>,
    worker: Option<JoinHandle<()>>,
    _marker: core::marker::PhantomData<(I, T)>,
}

impl<I, T, const N: usize> AsyncNfcTag<I, T, N>
where
    I: Interface + Send + 'static,
    I::Error: Debug + fmt::Display + Send + 'static,
    T: CountDown<Time = Duration> + Send + 'static,
{
    /// Создаёт async worker поверх уже инициализированного sync NFC wrapper.
    pub fn new(mut nfc: NfcTag<I, T, N>, config: AsyncNfcConfig) -> Result<Self, AsyncNfcError> {
        if config.poll_interval.is_zero() {
            return Err(AsyncNfcError::InvalidWorkerConfig(
                "poll_interval must be greater than zero",
            ));
        }
        if config.poll_timeout.is_zero() {
            return Err(AsyncNfcError::InvalidWorkerConfig(
                "poll_timeout must be greater than zero",
            ));
        }
        if config.removal_debounce.is_zero() {
            return Err(AsyncNfcError::InvalidWorkerConfig(
                "removal_debounce must be greater than zero",
            ));
        }

        let state = Arc::new(Mutex::new(AsyncNfcState::new()));
        let worker_error = Arc::new(Mutex::new(None));
        let (command_tx, command_rx) = mpsc::channel();

        let worker_state = Arc::clone(&state);
        let worker_error_slot = Arc::clone(&worker_error);
        let worker = thread::Builder::new()
            .name("pn532-worker".into())
            .stack_size(config.thread_stack_size)
            .spawn(move || {
                run_async_nfc_worker(
                    &mut nfc,
                    worker_state,
                    worker_error_slot,
                    command_rx,
                    config,
                );
            })
            .map_err(AsyncNfcError::ThreadSpawn)?;

        Ok(Self {
            state,
            worker_error,
            command_tx,
            worker: Some(worker),
            _marker: core::marker::PhantomData,
        })
    }

    /// Возвращает текущий снэпшот NFC worker'а.
    pub fn snapshot(&self) -> Result<AsyncNfcSnapshot, AsyncNfcError> {
        self.ensure_worker_alive()?;
        let guard = self
            .state
            .lock()
            .map_err(|_| AsyncNfcError::WorkerFailed("NFC state mutex poisoned".into()))?;
        Ok(guard.snapshot.clone())
    }

    /// Записывает `KvStore` только если на ридере всё ещё та же самая метка.
    pub fn write_kv_store_for_tag(
        &self,
        expected_uid: &[u8],
        store: &KvStore,
    ) -> Result<(), AsyncNfcError> {
        self.ensure_worker_alive()?;

        let (reply_tx, reply_rx) = mpsc::channel();
        self.command_tx
            .send(AsyncNfcCommand::WriteKvStore {
                expected_uid: expected_uid.to_vec(),
                store: store.clone(),
                reply: reply_tx,
            })
            .map_err(|_| AsyncNfcError::WorkerStopped)?;

        reply_rx
            .recv()
            .map_err(|_| AsyncNfcError::WorkerStopped)?
            .map_err(AsyncNfcError::CommandFailed)
    }

    /// Возвращает последнюю фатальную ошибку worker'а, если она была.
    pub fn last_worker_error(&self) -> Option<String> {
        self.worker_error
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn ensure_worker_alive(&self) -> Result<(), AsyncNfcError> {
        let worker_error = self
            .worker_error
            .lock()
            .map_err(|_| AsyncNfcError::WorkerFailed("NFC error mutex poisoned".into()))?;

        if let Some(error) = worker_error.as_ref() {
            return Err(AsyncNfcError::WorkerFailed(error.clone()));
        }

        Ok(())
    }
}

impl<I, T, const N: usize> Drop for AsyncNfcTag<I, T, N>
where
    I: Interface + Send + 'static,
    I::Error: Debug + fmt::Display + Send + 'static,
    T: CountDown<Time = Duration> + Send + 'static,
{
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.shutdown = true;
        }

        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_async_nfc_worker<I, T, const N: usize>(
    nfc: &mut NfcTag<I, T, N>,
    state: Arc<Mutex<AsyncNfcState>>,
    worker_error: Arc<Mutex<Option<String>>>,
    command_rx: mpsc::Receiver<AsyncNfcCommand>,
    config: AsyncNfcConfig,
) where
    I: Interface + Send + 'static,
    I::Error: Debug + fmt::Display + Send + 'static,
    T: CountDown<Time = Duration> + Send + 'static,
{
    let mut missing_since: Option<std::time::Instant> = None;

    loop {
        let shutdown = match state.lock() {
            Ok(guard) => guard.shutdown,
            Err(_) => {
                store_async_nfc_worker_error(&worker_error, "NFC state mutex poisoned".into());
                return;
            }
        };

        if shutdown {
            return;
        }

        while let Ok(command) = command_rx.try_recv() {
            if let Err(err) = process_async_nfc_command(nfc, &state, command) {
                store_async_nfc_worker_error(&worker_error, err);
                return;
            }
        }

        match nfc.poll_tag(config.poll_timeout) {
            Ok(Some(info)) => {
                missing_since = None;
                let should_reread = match state.lock() {
                    Ok(guard) => guard
                        .snapshot
                        .tag
                        .as_ref()
                        .map(|tag| tag.info.uid != info.uid)
                        .unwrap_or(true),
                    Err(_) => {
                        store_async_nfc_worker_error(
                            &worker_error,
                            "NFC state mutex poisoned".into(),
                        );
                        return;
                    }
                };

                if should_reread {
                    let payload = match nfc.read_kv_store() {
                        Ok(store) => AsyncTagPayload::KvStore(store),
                        Err(NfcError::NoNdefMessage) => AsyncTagPayload::Empty,
                        Err(err) => AsyncTagPayload::ReadError(err.to_string()),
                    };

                    let observed = AsyncObservedTag { info, payload };
                    if let Ok(mut guard) = state.lock() {
                        guard.snapshot.generation = guard.snapshot.generation.wrapping_add(1);
                        guard.snapshot.tag = Some(observed);
                    } else {
                        store_async_nfc_worker_error(
                            &worker_error,
                            "NFC state mutex poisoned".into(),
                        );
                        return;
                    }
                }
            }
            Ok(None) => {
                let now = std::time::Instant::now();
                let should_clear = match missing_since {
                    Some(started_at) => now.duration_since(started_at) >= config.removal_debounce,
                    None => {
                        missing_since = Some(now);
                        false
                    }
                };

                if should_clear {
                    if let Ok(mut guard) = state.lock() {
                        if guard.snapshot.tag.is_some() {
                            guard.snapshot.generation = guard.snapshot.generation.wrapping_add(1);
                            guard.snapshot.tag = None;
                        }
                    } else {
                        store_async_nfc_worker_error(
                            &worker_error,
                            "NFC state mutex poisoned".into(),
                        );
                        return;
                    }
                }
            }
            Err(err) => {
                if !matches!(
                    err,
                    NfcError::Pn532(pn532::Error::TimeoutAck)
                        | NfcError::Pn532(pn532::Error::TimeoutResponse)
                ) {
                    // Keep the last good snapshot after transient transport errors instead of
                    // treating them as an instant "tag removed" event.
                }
            }
        }

        thread::sleep(config.poll_interval);
    }
}

fn process_async_nfc_command<I, T, const N: usize>(
    nfc: &mut NfcTag<I, T, N>,
    state: &Arc<Mutex<AsyncNfcState>>,
    command: AsyncNfcCommand,
) -> Result<(), String>
where
    I: Interface + Send + 'static,
    I::Error: Debug + fmt::Display + Send + 'static,
    T: CountDown<Time = Duration> + Send + 'static,
{
    match command {
        AsyncNfcCommand::WriteKvStore {
            expected_uid,
            store,
            reply,
        } => {
            let current_uid = state
                .lock()
                .map_err(|_| "NFC state mutex poisoned".to_owned())?
                .snapshot
                .tag
                .as_ref()
                .map(|tag| tag.info.uid.clone());

            let result = match current_uid {
                Some(uid) if uid == expected_uid => nfc
                    .write_kv_store(&store)
                    .map_err(|err| err.to_string())
                    .map(|()| {
                        if let Ok(mut guard) = state.lock() {
                            if let Some(tag) = guard.snapshot.tag.as_mut() {
                                tag.payload = AsyncTagPayload::KvStore(store.clone());
                            }
                            guard.snapshot.generation = guard.snapshot.generation.wrapping_add(1);
                        }
                    }),
                Some(_) => Err("expected NFC tag is no longer present on the reader".to_owned()),
                None => Err("no NFC tag is currently present on the reader".to_owned()),
            };

            let _ = reply.send(result.map(|()| ()));
            Ok(())
        }
    }
}

fn store_async_nfc_worker_error(slot: &Arc<Mutex<Option<String>>>, error: String) {
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(error);
    }
}
