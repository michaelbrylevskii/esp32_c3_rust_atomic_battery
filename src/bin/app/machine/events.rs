use super::model::ObservedTag;
use crate::storage::AsyncStorageEvent;
use common::drivers::nfc_tag::async_nfc::AsyncNfcEvent;
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub enum AppEvent {
    Tick {
        now: Instant,
    },
    SwitchChanged {
        enabled: bool,
        now: Instant,
    },
    ObservedTagChanged {
        observed_tag: Option<ObservedTag>,
        now: Instant,
    },
    NfcWriteFinished {
        expected_uid: Vec<u8>,
        result: Result<(), String>,
        now: Instant,
    },
    ConsumptionSaved {
        value: u32,
        result: Result<(), String>,
        now: Instant,
    },
    SessionIdAllocated {
        result: Result<u64, String>,
        now: Instant,
    },
}

impl AppEvent {
    pub fn now(&self) -> Instant {
        match self {
            AppEvent::Tick { now }
            | AppEvent::SwitchChanged { now, .. }
            | AppEvent::ObservedTagChanged { now, .. }
            | AppEvent::NfcWriteFinished { now, .. }
            | AppEvent::ConsumptionSaved { now, .. }
            | AppEvent::SessionIdAllocated { now, .. } => *now,
        }
    }
}

pub fn from_nfc_event(event: AsyncNfcEvent, now: Instant) -> AppEvent {
    match event {
        AsyncNfcEvent::WriteFinished {
            expected_uid,
            result,
            ..
        } => AppEvent::NfcWriteFinished {
            expected_uid,
            result,
            now,
        },
    }
}

pub fn from_storage_event(event: AsyncStorageEvent, now: Instant) -> AppEvent {
    match event {
        AsyncStorageEvent::ConsumptionSaved { value, result } => {
            AppEvent::ConsumptionSaved { value, result, now }
        }
        AsyncStorageEvent::SessionIdAllocated(result) => {
            AppEvent::SessionIdAllocated { result, now }
        }
    }
}
