use super::constants::{
    ERROR_SCROLL_STEP, GREEN_ONLY_LEVELS, NO_BATTERY_TEXT, NO_BAT_SCROLL_STEP, RED_ONLY_LEVELS,
    SERVICE_FEEDBACK_BLINK_CYCLES, SERVICE_SCROLL_STEP,
};
use super::model::{
    active_colon_timing, pair_from_charge, pair_from_remaining_seconds, AppState, MachinePhase,
    ObservedTag,
};
use common::drivers::led_indicator::pattern::LedPattern;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColonProjection {
    StaticOn,
    Pulse {
        initial_on: bool,
        period: Duration,
        on_duration: Duration,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisplayProjection {
    Clear,
    Pair {
        left: u8,
        right: u8,
        colon: ColonProjection,
    },
    Scroll {
        text: String,
        step_delay: Duration,
        cycles: Option<u32>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum IndicatorProjection {
    Static([u8; 2]),
    Pattern(LedPattern<2>),
}

pub fn project_display(state: &AppState, now: std::time::Instant) -> DisplayProjection {
    if let Some(feedback) = state.service_feedback.as_ref() {
        return DisplayProjection::Scroll {
            text: feedback.message.clone(),
            step_delay: SERVICE_SCROLL_STEP,
            cycles: Some(1),
        };
    }

    match &state.phase {
        MachinePhase::AwaitingSessionId {
            battery,
            started_at,
            ..
        } => {
            let elapsed_ms = now.duration_since(*started_at).as_millis();
            let consumed = (u128::from(state.consumption_per_sec) * elapsed_ms / 1000)
                .min(u128::from(u64::MAX)) as u64;
            let charge = battery.charge.saturating_sub(consumed);
            let (left, right) = pair_from_charge(charge, state.consumption_per_sec);
            let (period, on_duration) = active_colon_timing();
            DisplayProjection::Pair {
                left,
                right,
                colon: ColonProjection::Pulse {
                    initial_on: true,
                    period,
                    on_duration,
                },
            }
        }
        MachinePhase::Opening {
            opened_battery,
            opened_at: _,
            ..
        } if state.switch_enabled => {
            let (left, right) = pair_from_remaining_seconds(
                opened_battery.remaining_seconds(state.consumption_per_sec),
            );
            let (period, on_duration) = active_colon_timing();
            DisplayProjection::Pair {
                left,
                right,
                colon: ColonProjection::Pulse {
                    initial_on: true,
                    period,
                    on_duration,
                },
            }
        }
        MachinePhase::Running(session) => {
            let (left, right) = session.current_pair(now, state.consumption_per_sec);
            let (period, on_duration) = active_colon_timing();
            DisplayProjection::Pair {
                left,
                right,
                colon: ColonProjection::Pulse {
                    initial_on: true,
                    period,
                    on_duration,
                },
            }
        }
        MachinePhase::Opening { opened_battery, .. }
        | MachinePhase::Closing {
            closed_battery: opened_battery,
            ..
        } => {
            let (left, right) = pair_from_remaining_seconds(
                opened_battery.remaining_seconds(state.consumption_per_sec),
            );
            DisplayProjection::Pair {
                left,
                right,
                colon: ColonProjection::StaticOn,
            }
        }
        MachinePhase::Breaking { .. } => DisplayProjection::Scroll {
            text: "Error".into(),
            step_delay: ERROR_SCROLL_STEP,
            cycles: None,
        },
        _ => match state.observed_tag.as_ref() {
            Some(ObservedTag::Battery { battery, .. }) if !battery.healthy || battery.dirty => {
                DisplayProjection::Scroll {
                    text: "Error".into(),
                    step_delay: ERROR_SCROLL_STEP,
                    cycles: None,
                }
            }
            Some(ObservedTag::Battery { battery, .. }) => {
                let (left, right) = pair_from_remaining_seconds(
                    battery.remaining_seconds(state.consumption_per_sec),
                );
                DisplayProjection::Pair {
                    left,
                    right,
                    colon: ColonProjection::StaticOn,
                }
            }
            _ if state.switch_enabled => DisplayProjection::Scroll {
                text: NO_BATTERY_TEXT.into(),
                step_delay: NO_BAT_SCROLL_STEP,
                cycles: None,
            },
            _ => DisplayProjection::Clear,
        },
    }
}

pub fn project_indicator(state: &AppState, now: std::time::Instant) -> IndicatorProjection {
    if let Some(feedback) = state.service_feedback.as_ref() {
        return IndicatorProjection::Pattern(
            LedPattern::alternate(
                RED_ONLY_LEVELS,
                GREEN_ONLY_LEVELS,
                feedback.blink_interval,
                SERVICE_FEEDBACK_BLINK_CYCLES,
            )
            .final_levels(RED_ONLY_LEVELS),
        );
    }

    match &state.phase {
        MachinePhase::AwaitingSessionId { .. } | MachinePhase::Opening { .. }
            if state.switch_enabled =>
        {
            IndicatorProjection::Static(GREEN_ONLY_LEVELS)
        }
        MachinePhase::Running(session)
            if session.current_charge(now, state.consumption_per_sec) > 0 =>
        {
            IndicatorProjection::Static(GREEN_ONLY_LEVELS)
        }
        _ => IndicatorProjection::Static(RED_ONLY_LEVELS),
    }
}
