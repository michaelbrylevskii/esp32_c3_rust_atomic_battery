use super::constants::LOOP_DELAY_MS;
use super::effects::AppEffect;
use super::events::{from_nfc_event, from_storage_event, AppEvent};
use super::model::{AppState, ObservedTag};
use super::projections::{
    project_display, project_indicator, ColonProjection, DisplayProjection, IndicatorProjection,
};
use super::reducer::reduce;
use crate::errors::AppError;
use crate::hardware::AtomicMachineHardware;
use crate::storage::{AsyncAppStorage, AsyncStorageConfig};
use common::drivers::nfc_tag::async_nfc::{AsyncObservedTag, AsyncTagPayload};
use common::utils::atomic_tags::AtomicTag;
use esp_idf_svc::hal::delay::FreeRtos;
use log::{info, warn};
use std::time::Instant;

pub fn run() -> Result<(), AppError> {
    let hw = AtomicMachineHardware::take()?;
    let (storage, bootstrap) = AsyncAppStorage::take(AsyncStorageConfig::default())?;

    let mut last_switch_enabled = hw.switch.is_low();
    let initial_snapshot = hw.nfc.snapshot()?;
    let initial_observed_tag = decode_observed_tag(initial_snapshot.tag.as_ref());
    let mut last_nfc_generation = initial_snapshot.generation;

    let mut state = AppState::new(
        last_switch_enabled,
        bootstrap.consumption_per_sec,
        initial_observed_tag,
    );
    let mut last_display = None;
    let mut last_indicator = None;

    let now = Instant::now();
    for effect in reduce(&mut state, AppEvent::Tick { now }) {
        apply_effect(&hw, &storage, effect)?;
    }
    apply_projections(&hw, &state, now, &mut last_display, &mut last_indicator)?;

    loop {
        let now = Instant::now();
        let mut events = Vec::new();

        let switch_enabled = hw.switch.is_low();
        if switch_enabled != last_switch_enabled {
            last_switch_enabled = switch_enabled;
            if switch_enabled {
                info!("Switch enabled");
            } else {
                info!("Switch disabled");
            }
            events.push(AppEvent::SwitchChanged {
                enabled: switch_enabled,
                now,
            });
        }

        let snapshot = hw.nfc.snapshot()?;
        if snapshot.generation != last_nfc_generation {
            last_nfc_generation = snapshot.generation;
            let observed_tag = decode_observed_tag(snapshot.tag.as_ref());
            log_tag_change(observed_tag.as_ref());
            events.push(AppEvent::ObservedTagChanged { observed_tag, now });
        }

        for event in hw.nfc.drain_events()? {
            events.push(from_nfc_event(event, now));
        }

        for event in storage.drain_events()? {
            events.push(from_storage_event(event, now));
        }

        events.push(AppEvent::Tick { now });

        for event in events {
            for effect in reduce(&mut state, event) {
                apply_effect(&hw, &storage, effect)?;
            }
        }

        apply_projections(&hw, &state, now, &mut last_display, &mut last_indicator)?;
        FreeRtos::delay_ms(LOOP_DELAY_MS);
    }
}

fn apply_effect(
    hw: &AtomicMachineHardware<'_>,
    storage: &AsyncAppStorage,
    effect: AppEffect,
) -> Result<(), AppError> {
    match effect {
        AppEffect::RequestNextSessionId => storage.request_next_session_id()?,
        AppEffect::SaveConsumptionPerSec(value) => storage.enqueue_save_consumption(value)?,
        AppEffect::WriteBattery {
            expected_uid,
            battery,
        } => {
            let store = battery.to_store()?;
            hw.nfc
                .enqueue_write_kv_store_for_tag(&expected_uid, &store)?;
        }
    }

    Ok(())
}

fn apply_projections(
    hw: &AtomicMachineHardware<'_>,
    state: &AppState,
    now: Instant,
    last_display: &mut Option<DisplayProjection>,
    last_indicator: &mut Option<IndicatorProjection>,
) -> Result<(), AppError> {
    let display = project_display(state, now);
    if last_display.as_ref() != Some(&display) {
        apply_display_projection(hw, &display)?;
        *last_display = Some(display);
    }

    let indicator = project_indicator(state, now);
    if last_indicator.as_ref() != Some(&indicator) {
        apply_indicator_projection(hw, &indicator)?;
        *last_indicator = Some(indicator);
    }

    Ok(())
}

fn apply_display_projection(
    hw: &AtomicMachineHardware<'_>,
    projection: &DisplayProjection,
) -> Result<(), AppError> {
    match projection {
        DisplayProjection::Clear => {
            hw.display.set_colon(false)?;
            hw.display.clear()?;
        }
        DisplayProjection::Pair { left, right, colon } => {
            hw.display.show_int_pair(*left, *right)?;
            match colon {
                ColonProjection::StaticOn => hw.display.set_colon(true)?,
                ColonProjection::Pulse {
                    initial_on,
                    period,
                    on_duration,
                } => hw
                    .display
                    .start_colon_pulse(*initial_on, *period, *on_duration)?,
            }
        }
        DisplayProjection::Scroll {
            text,
            step_delay,
            cycles,
        } => {
            hw.display.set_colon(false)?;
            hw.display
                .start_scroll_text_cycles(text, *step_delay, *cycles)?;
        }
    }

    Ok(())
}

fn apply_indicator_projection(
    hw: &AtomicMachineHardware<'_>,
    projection: &IndicatorProjection,
) -> Result<(), AppError> {
    match projection {
        IndicatorProjection::Static(levels) => hw.indicator.set_levels(*levels)?,
        IndicatorProjection::Pattern(pattern) => hw.indicator.play_pattern(pattern.clone())?,
    }

    Ok(())
}

fn decode_observed_tag(observed: Option<&AsyncObservedTag>) -> Option<ObservedTag> {
    let observed = observed?;

    match &observed.payload {
        AsyncTagPayload::KvStore(store) => match AtomicTag::from_store(store) {
            Ok(AtomicTag::Battery(battery)) => Some(ObservedTag::Battery {
                uid: observed.info.uid.clone(),
                battery,
            }),
            Ok(AtomicTag::Service(service)) => Some(ObservedTag::Service {
                uid: observed.info.uid.clone(),
                service,
            }),
            Err(_) => Some(ObservedTag::Other {
                uid: observed.info.uid.clone(),
            }),
        },
        AsyncTagPayload::Empty | AsyncTagPayload::ReadError(_) => Some(ObservedTag::Other {
            uid: observed.info.uid.clone(),
        }),
    }
}

fn log_tag_change(observed_tag: Option<&ObservedTag>) {
    match observed_tag {
        Some(ObservedTag::Battery { uid, battery }) => info!(
            "Detected battery tag uid={} healthy={} dirty={} charge={}/{} session_id={}",
            encode_uid_hex(uid),
            battery.healthy,
            battery.dirty,
            battery.charge,
            battery.capacity,
            battery.session_id
        ),
        Some(ObservedTag::Service { uid, service }) => info!(
            "Detected service tag uid={} consumption_per_sec={}",
            encode_uid_hex(uid),
            service.consumption_per_sec
        ),
        Some(ObservedTag::Other { uid }) => {
            warn!("Detected unsupported NFC tag uid={}", encode_uid_hex(uid));
        }
        None => info!("Tag removed from reader"),
    }
}

fn encode_uid_hex(uid: &[u8]) -> String {
    let mut value = String::with_capacity(uid.len() * 2);
    for byte in uid {
        use core::fmt::Write as _;
        let _ = write!(&mut value, "{byte:02X}");
    }
    value
}
