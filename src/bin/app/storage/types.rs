use core::fmt;
use std::io;

pub const NVS_NAMESPACE: &str = "atomic_app";
pub const KEY_CONSUMPTION_PER_SEC: &str = "cons_per_sec";
pub const KEY_SESSION_COUNTER: &str = "session_ctr";
pub const DEFAULT_CONSUMPTION_PER_SEC: u32 = 1000;
pub const DEFAULT_STORAGE_THREAD_STACK_SIZE: usize = 6 * 1024;

/// Начальное состояние, которое приложение читает из NVS перед запуском runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageBootstrap {
    pub consumption_per_sec: u32,
}

/// Ошибки синхронного NVS backend'а приложения.
#[derive(Debug)]
pub enum StorageError {
    Esp(esp_idf_svc::sys::EspError),
    InvalidConsumptionPerSec(u32),
}

impl From<esp_idf_svc::sys::EspError> for StorageError {
    fn from(value: esp_idf_svc::sys::EspError) -> Self {
        Self::Esp(value)
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::Esp(err) => write!(f, "storage esp error: {err}"),
            StorageError::InvalidConsumptionPerSec(value) => {
                write!(f, "invalid consumption_per_sec value: {value}")
            }
        }
    }
}

impl std::error::Error for StorageError {}

/// Ошибки фоновой async-обёртки над NVS backend'ом.
#[derive(Debug)]
pub enum AsyncStorageError {
    Storage(StorageError),
    ThreadSpawn(io::Error),
    WorkerStopped,
    WorkerFailed(String),
    InvalidThreadStackSize,
}

impl From<StorageError> for AsyncStorageError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl fmt::Display for AsyncStorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsyncStorageError::Storage(err) => write!(f, "storage error: {err}"),
            AsyncStorageError::ThreadSpawn(err) => {
                write!(f, "failed to spawn storage worker thread: {err}")
            }
            AsyncStorageError::WorkerStopped => write!(f, "storage worker thread has stopped"),
            AsyncStorageError::WorkerFailed(err) => {
                write!(f, "storage worker thread failed: {err}")
            }
            AsyncStorageError::InvalidThreadStackSize => {
                write!(f, "storage worker stack size must be greater than zero")
            }
        }
    }
}

impl std::error::Error for AsyncStorageError {}
