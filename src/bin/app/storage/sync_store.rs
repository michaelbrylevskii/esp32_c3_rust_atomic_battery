use super::types::{
    StorageBootstrap, StorageError, DEFAULT_CONSUMPTION_PER_SEC, KEY_CONSUMPTION_PER_SEC,
    KEY_SESSION_COUNTER, NVS_NAMESPACE,
};
use esp_idf_svc::nvs::{EspDefaultNvs, EspDefaultNvsPartition};

pub struct AppStorage {
    nvs: EspDefaultNvs,
}

impl AppStorage {
    pub fn take() -> Result<Self, StorageError> {
        let partition = EspDefaultNvsPartition::take()?;
        let nvs = EspDefaultNvs::new(partition, NVS_NAMESPACE, true)?;
        Ok(Self { nvs })
    }

    pub fn load_bootstrap(&self) -> Result<StorageBootstrap, StorageError> {
        Ok(StorageBootstrap {
            consumption_per_sec: self.load_consumption_per_sec()?,
        })
    }

    pub fn load_consumption_per_sec(&self) -> Result<u32, StorageError> {
        let value = self
            .nvs
            .get_u32(KEY_CONSUMPTION_PER_SEC)?
            .unwrap_or(DEFAULT_CONSUMPTION_PER_SEC);

        if value == 0 {
            return Err(StorageError::InvalidConsumptionPerSec(value));
        }

        Ok(value)
    }

    pub fn save_consumption_per_sec(&self, value: u32) -> Result<(), StorageError> {
        if value == 0 {
            return Err(StorageError::InvalidConsumptionPerSec(value));
        }

        self.nvs.set_u32(KEY_CONSUMPTION_PER_SEC, value)?;
        Ok(())
    }

    pub fn next_session_id(&self) -> Result<u64, StorageError> {
        let next_value = self
            .nvs
            .get_u64(KEY_SESSION_COUNTER)?
            .unwrap_or(0)
            .wrapping_add(1);
        self.nvs.set_u64(KEY_SESSION_COUNTER, next_value)?;
        Ok(next_value)
    }
}
