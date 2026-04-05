use super::constants::{ACTIVE_COLON_ON_DURATION, ACTIVE_COLON_PERIOD, CHARGE_PERSIST_INTERVAL};
use common::utils::atomic_tags::{BatteryTag, ServiceTag};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObservedTag {
    Battery { uid: Vec<u8>, battery: BatteryTag },
    Service { uid: Vec<u8>, service: ServiceTag },
    Other { uid: Vec<u8> },
}

impl ObservedTag {
    pub fn uid(&self) -> &[u8] {
        match self {
            ObservedTag::Battery { uid, .. }
            | ObservedTag::Service { uid, .. }
            | ObservedTag::Other { uid } => uid,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakReason {
    HotPlugWhileEnabled,
    DirtyBatteryObserved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceFeedback {
    pub message: String,
    pub ends_at: Instant,
    pub blink_interval: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistWrite {
    pub charge: u64,
    pub requested_at: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveSession {
    pub battery_uid: Vec<u8>,
    pub session_id: u64,
    pub capacity: u64,
    pub charge_at_open: u64,
    pub opened_at: Instant,
    pub last_persist_at: Instant,
    pub last_persisted_charge: u64,
    pub persist_write_in_flight: Option<PersistWrite>,
}

impl ActiveSession {
    pub fn new(battery_uid: Vec<u8>, battery: &BatteryTag, opened_at: Instant) -> Self {
        Self {
            battery_uid,
            session_id: battery.session_id,
            capacity: battery.capacity,
            charge_at_open: battery.charge,
            opened_at,
            last_persist_at: opened_at,
            last_persisted_charge: battery.charge,
            persist_write_in_flight: None,
        }
    }

    pub fn current_charge(&self, now: Instant, consumption_per_sec: u32) -> u64 {
        if consumption_per_sec == 0 {
            return self.charge_at_open;
        }

        let elapsed_ms = now.duration_since(self.opened_at).as_millis();
        let consumed =
            (u128::from(consumption_per_sec) * elapsed_ms / 1000).min(u128::from(u64::MAX)) as u64;

        self.charge_at_open.saturating_sub(consumed)
    }

    pub fn current_battery(&self, now: Instant, consumption_per_sec: u32) -> BatteryTag {
        BatteryTag {
            capacity: self.capacity,
            charge: self.current_charge(now, consumption_per_sec),
            healthy: true,
            dirty: true,
            session_id: self.session_id,
        }
    }

    pub fn current_pair(&self, now: Instant, consumption_per_sec: u32) -> (u8, u8) {
        let remaining_seconds = self
            .current_battery(now, consumption_per_sec)
            .remaining_seconds(consumption_per_sec);
        pair_from_remaining_seconds(remaining_seconds)
    }

    pub fn should_persist(&self, now: Instant, consumption_per_sec: u32) -> bool {
        if self.persist_write_in_flight.is_some() {
            return false;
        }

        if now.duration_since(self.last_persist_at) < CHARGE_PERSIST_INTERVAL {
            return false;
        }

        self.current_charge(now, consumption_per_sec) != self.last_persisted_charge
    }

    pub fn mark_persist_started(&mut self, now: Instant, charge: u64) {
        self.persist_write_in_flight = Some(PersistWrite {
            charge,
            requested_at: now,
        });
    }

    pub fn complete_persist(&mut self, completed_at: Instant, charge: u64) {
        self.last_persisted_charge = charge;
        self.last_persist_at = completed_at;
        self.persist_write_in_flight = None;
    }

    pub fn clear_persist_request(&mut self) {
        self.persist_write_in_flight = None;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachinePhase {
    Idle,
    AwaitingSessionId {
        battery_uid: Vec<u8>,
        battery: BatteryTag,
        request_enqueued: bool,
    },
    Opening {
        battery_uid: Vec<u8>,
        opened_battery: BatteryTag,
        opened_at: Instant,
        write_enqueued: bool,
    },
    Running(ActiveSession),
    Closing {
        battery_uid: Vec<u8>,
        closed_battery: BatteryTag,
        write_enqueued: bool,
    },
    Breaking {
        battery_uid: Vec<u8>,
        broken_battery: BatteryTag,
        reason: BreakReason,
        write_enqueued: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppState {
    pub switch_enabled: bool,
    pub consumption_per_sec: u32,
    pub observed_tag: Option<ObservedTag>,
    pub phase: MachinePhase,
    pub service_feedback: Option<ServiceFeedback>,
    pub consumption_save_in_flight: bool,
}

impl AppState {
    pub fn new(
        switch_enabled: bool,
        consumption_per_sec: u32,
        observed_tag: Option<ObservedTag>,
    ) -> Self {
        Self {
            switch_enabled,
            consumption_per_sec,
            observed_tag,
            phase: MachinePhase::Idle,
            service_feedback: None,
            consumption_save_in_flight: false,
        }
    }
}

pub fn pair_from_remaining_seconds(remaining_seconds: u64) -> (u8, u8) {
    let capped_seconds = remaining_seconds.min((99 * 60 + 59) as u64);
    let minutes = (capped_seconds / 60) as u8;
    let seconds = (capped_seconds % 60) as u8;
    (minutes, seconds)
}

pub fn active_colon_timing() -> (Duration, Duration) {
    (ACTIVE_COLON_PERIOD, ACTIVE_COLON_ON_DURATION)
}
