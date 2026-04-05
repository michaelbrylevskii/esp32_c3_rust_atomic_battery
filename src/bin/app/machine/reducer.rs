use super::constants::SERVICE_SCROLL_STEP;
use super::effects::AppEffect;
use super::events::AppEvent;
use super::model::{
    ActiveSession, AppState, BreakReason, MachinePhase, ObservedTag, ServiceFeedback,
};
use std::time::{Duration, Instant};

pub fn reduce(state: &mut AppState, event: AppEvent) -> Vec<AppEffect> {
    let now = event.now();
    let mut effects = Vec::new();

    match event {
        AppEvent::Tick { .. } => {}
        AppEvent::SwitchChanged { enabled, .. } => {
            state.switch_enabled = enabled;
        }
        AppEvent::ObservedTagChanged { observed_tag, .. } => {
            state.observed_tag = observed_tag;
            handle_observed_tag_changed(state, now, &mut effects);
        }
        AppEvent::NfcWriteFinished {
            expected_uid,
            result,
            ..
        } => {
            handle_nfc_write_finished(state, &expected_uid, result, now);
        }
        AppEvent::ConsumptionSaved { result, .. } => {
            state.consumption_save_in_flight = false;
            if result.is_err() {
                // Keep the in-memory value; a future service tag can trigger another save.
            }
        }
        AppEvent::SessionIdAllocated { result, .. } => {
            handle_session_id_allocated(state, result, now);
        }
    }

    reconcile(state, now, &mut effects);
    effects
}

fn handle_observed_tag_changed(state: &mut AppState, now: Instant, effects: &mut Vec<AppEffect>) {
    if let MachinePhase::Running(session) = &state.phase {
        let active_battery_present = matches!(
            state.observed_tag.as_ref(),
            Some(ObservedTag::Battery { uid, .. }) if *uid == session.battery_uid
        );

        if !active_battery_present {
            state.phase = MachinePhase::Idle;
            return;
        }
    }

    match (&state.phase, state.observed_tag.as_ref()) {
        (MachinePhase::Idle, Some(ObservedTag::Service { service, .. })) => {
            apply_service_feedback(state, service.consumption_per_sec, now, effects);
        }
        (MachinePhase::Idle, Some(ObservedTag::Battery { uid, battery })) if battery.dirty => {
            let mut broken_battery = battery.clone();
            broken_battery.mark_broken();
            state.phase = MachinePhase::Breaking {
                battery_uid: uid.clone(),
                broken_battery,
                reason: BreakReason::DirtyBatteryObserved,
                write_enqueued: false,
            };
        }
        (MachinePhase::Idle, Some(ObservedTag::Battery { uid, battery }))
            if state.switch_enabled && battery.is_usable() =>
        {
            let mut broken_battery = battery.clone();
            broken_battery.mark_broken();
            state.phase = MachinePhase::Breaking {
                battery_uid: uid.clone(),
                broken_battery,
                reason: BreakReason::HotPlugWhileEnabled,
                write_enqueued: false,
            };
        }
        (MachinePhase::AwaitingSessionId { battery_uid, .. }, Some(observed))
            if observed.uid() != battery_uid =>
        {
            state.phase = MachinePhase::Idle;
        }
        (MachinePhase::Opening { battery_uid, .. }, Some(observed))
            if observed.uid() != battery_uid =>
        {
            state.phase = MachinePhase::Idle;
        }
        (MachinePhase::Closing { battery_uid, .. }, Some(observed))
            if observed.uid() != battery_uid =>
        {
            state.phase = MachinePhase::Idle;
        }
        (MachinePhase::Breaking { battery_uid, .. }, Some(observed))
            if observed.uid() != battery_uid =>
        {
            state.phase = MachinePhase::Idle;
        }
        (
            MachinePhase::AwaitingSessionId { .. }
            | MachinePhase::Opening { .. }
            | MachinePhase::Closing { .. }
            | MachinePhase::Breaking { .. },
            None,
        ) => {
            state.phase = MachinePhase::Idle;
        }
        _ => {}
    }
}

fn handle_session_id_allocated(state: &mut AppState, result: Result<u64, String>, now: Instant) {
    let MachinePhase::AwaitingSessionId {
        battery_uid,
        battery,
        request_enqueued: _,
    } = &state.phase
    else {
        return;
    };

    let Some(ObservedTag::Battery {
        uid,
        battery: observed,
    }) = state.observed_tag.as_ref()
    else {
        state.phase = MachinePhase::Idle;
        return;
    };

    if *uid != *battery_uid || observed != battery || !state.switch_enabled || !battery.is_usable()
    {
        state.phase = MachinePhase::Idle;
        return;
    }

    match result {
        Ok(session_id) => {
            let mut opened_battery = battery.clone();
            opened_battery.open_session(session_id);
            state.phase = MachinePhase::Opening {
                battery_uid: battery_uid.clone(),
                opened_battery,
                opened_at: now,
                write_enqueued: false,
            };
        }
        Err(_) => {
            state.phase = MachinePhase::Idle;
        }
    }
}

fn handle_nfc_write_finished(
    state: &mut AppState,
    expected_uid: &[u8],
    result: Result<(), String>,
    now: Instant,
) {
    match &mut state.phase {
        MachinePhase::Opening {
            battery_uid,
            opened_battery,
            opened_at,
            write_enqueued,
        } if battery_uid.as_slice() == expected_uid => match result {
            Ok(()) => {
                let session = ActiveSession::new(battery_uid.clone(), opened_battery, *opened_at);
                state.phase = MachinePhase::Running(session);
            }
            Err(_) => {
                *write_enqueued = false;
                if !matches!(
                    state.observed_tag.as_ref(),
                    Some(ObservedTag::Battery { uid, .. }) if uid == battery_uid
                ) {
                    state.phase = MachinePhase::Idle;
                }
            }
        },
        MachinePhase::Running(session) if session.battery_uid.as_slice() == expected_uid => {
            if let Some(in_flight) = session.persist_write_in_flight.as_ref() {
                match result {
                    Ok(()) => session.complete_persist(now, in_flight.charge),
                    Err(_) => session.clear_persist_request(),
                }
            }
        }
        MachinePhase::Closing {
            battery_uid,
            write_enqueued,
            ..
        } if battery_uid.as_slice() == expected_uid => match result {
            Ok(()) => state.phase = MachinePhase::Idle,
            Err(_) => {
                *write_enqueued = false;
                if !matches!(
                    state.observed_tag.as_ref(),
                    Some(ObservedTag::Battery { uid, .. }) if uid == battery_uid
                ) {
                    state.phase = MachinePhase::Idle;
                }
            }
        },
        MachinePhase::Breaking {
            battery_uid,
            write_enqueued,
            ..
        } if battery_uid.as_slice() == expected_uid => match result {
            Ok(()) => state.phase = MachinePhase::Idle,
            Err(_) => {
                *write_enqueued = false;
                if !matches!(
                    state.observed_tag.as_ref(),
                    Some(ObservedTag::Battery { uid, .. }) if uid == battery_uid
                ) {
                    state.phase = MachinePhase::Idle;
                }
            }
        },
        _ => {}
    }
}

fn reconcile(state: &mut AppState, now: Instant, effects: &mut Vec<AppEffect>) {
    if matches!(state.service_feedback.as_ref(), Some(feedback) if now >= feedback.ends_at) {
        state.service_feedback = None;
    }

    if let MachinePhase::Running(session) = &state.phase {
        if !matches!(
            state.observed_tag.as_ref(),
            Some(ObservedTag::Battery { uid, .. }) if *uid == session.battery_uid
        ) {
            state.phase = MachinePhase::Idle;
        }
    }

    match &mut state.phase {
        MachinePhase::Idle => {
            if let Some(ObservedTag::Battery { uid, battery }) = state.observed_tag.as_ref() {
                if battery.dirty {
                    let mut broken_battery = battery.clone();
                    broken_battery.mark_broken();
                    state.phase = MachinePhase::Breaking {
                        battery_uid: uid.clone(),
                        broken_battery,
                        reason: BreakReason::DirtyBatteryObserved,
                        write_enqueued: false,
                    };
                    reconcile(state, now, effects);
                    return;
                }
            }

            if state.switch_enabled {
                if let Some(ObservedTag::Battery { uid, battery }) = state.observed_tag.as_ref() {
                    if battery.is_usable() {
                        state.phase = MachinePhase::AwaitingSessionId {
                            battery_uid: uid.clone(),
                            battery: battery.clone(),
                            request_enqueued: false,
                        };
                        reconcile(state, now, effects);
                    }
                }
            }
        }
        MachinePhase::AwaitingSessionId {
            request_enqueued, ..
        } => {
            if !state.switch_enabled {
                state.phase = MachinePhase::Idle;
                return;
            }

            if !*request_enqueued {
                effects.push(AppEffect::RequestNextSessionId);
                *request_enqueued = true;
            }
        }
        MachinePhase::Opening {
            battery_uid,
            opened_battery,
            write_enqueued,
            ..
        } => {
            if !matches!(
                state.observed_tag.as_ref(),
                Some(ObservedTag::Battery { uid, .. }) if uid == battery_uid
            ) {
                state.phase = MachinePhase::Idle;
                return;
            }

            if !*write_enqueued {
                effects.push(AppEffect::WriteBattery {
                    expected_uid: battery_uid.clone(),
                    battery: opened_battery.clone(),
                });
                *write_enqueued = true;
            }
        }
        MachinePhase::Running(session) => {
            let current_charge = session.current_charge(now, state.consumption_per_sec);

            if !state.switch_enabled || current_charge == 0 {
                let mut closed_battery = session.current_battery(now, state.consumption_per_sec);
                closed_battery.close_session();
                state.phase = MachinePhase::Closing {
                    battery_uid: session.battery_uid.clone(),
                    closed_battery,
                    write_enqueued: false,
                };
                reconcile(state, now, effects);
                return;
            }

            if session.should_persist(now, state.consumption_per_sec) {
                let battery = session.current_battery(now, state.consumption_per_sec);
                session.mark_persist_started(now, battery.charge);
                effects.push(AppEffect::WriteBattery {
                    expected_uid: session.battery_uid.clone(),
                    battery,
                });
            }
        }
        MachinePhase::Closing {
            battery_uid,
            closed_battery,
            write_enqueued,
        } => {
            if !matches!(
                state.observed_tag.as_ref(),
                Some(ObservedTag::Battery { uid, .. }) if uid == battery_uid
            ) {
                state.phase = MachinePhase::Idle;
                return;
            }

            if !*write_enqueued {
                effects.push(AppEffect::WriteBattery {
                    expected_uid: battery_uid.clone(),
                    battery: closed_battery.clone(),
                });
                *write_enqueued = true;
            }
        }
        MachinePhase::Breaking {
            battery_uid,
            broken_battery,
            write_enqueued,
            ..
        } => {
            if !matches!(
                state.observed_tag.as_ref(),
                Some(ObservedTag::Battery { uid, .. }) if uid == battery_uid
            ) {
                state.phase = MachinePhase::Idle;
                return;
            }

            if !*write_enqueued {
                effects.push(AppEffect::WriteBattery {
                    expected_uid: battery_uid.clone(),
                    battery: broken_battery.clone(),
                });
                *write_enqueued = true;
            }
        }
    }
}

fn apply_service_feedback(
    state: &mut AppState,
    consumption_per_sec: u32,
    now: Instant,
    effects: &mut Vec<AppEffect>,
) {
    state.consumption_per_sec = consumption_per_sec;

    let message = consumption_per_sec.to_string();
    let duration = single_scroll_duration(&message, SERVICE_SCROLL_STEP);
    state.service_feedback = Some(ServiceFeedback {
        message,
        ends_at: now + duration,
        blink_interval: feedback_blink_interval(duration),
    });

    if !state.consumption_save_in_flight {
        effects.push(AppEffect::SaveConsumptionPerSec(consumption_per_sec));
        state.consumption_save_in_flight = true;
    }
}

fn single_scroll_duration(message: &str, step_delay: Duration) -> Duration {
    step_delay
        .checked_mul(message.len().saturating_add(5) as u32)
        .unwrap_or(Duration::from_secs(2))
}

fn feedback_blink_interval(duration: Duration) -> Duration {
    let phase_count = u128::from(super::constants::SERVICE_FEEDBACK_BLINK_CYCLES.saturating_mul(2));
    let interval_ms = (duration.as_millis() / phase_count).max(1);
    Duration::from_millis(interval_ms as u64)
}
