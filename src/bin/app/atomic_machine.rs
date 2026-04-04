use crate::errors::AppError;
use crate::hardware::AtomicMachineHardware;
use crate::storage::{ActiveSessionRecord, AppStorage};
use common::drivers::led_indicator::{LedPattern, LEVEL_MAX};
use common::drivers::nfc_tag::{self, AsyncObservedTag, AsyncTagPayload, TagInfo};
use common::utils::atomic_tags::{AtomicTag, BatteryTag, ServiceTag};
use esp_idf_svc::hal::delay::FreeRtos;
use log::{info, warn};
use std::time::{Duration, Instant};

const LOOP_DELAY_MS: u32 = 50;
const CHARGE_PERSIST_INTERVAL: Duration = Duration::from_secs(60);
const NO_BAT_SCROLL_STEP: Duration = Duration::from_millis(250);
const ERROR_SCROLL_STEP: Duration = Duration::from_millis(250);
const SERVICE_SCROLL_STEP: Duration = Duration::from_millis(150);
const ACTIVE_COUNTDOWN_STEP_PERIOD: Duration = Duration::from_secs(1);
const SERVICE_FEEDBACK_BLINK_CYCLES: u32 = 3;
const RED_ONLY_LEVELS: [u8; 2] = [LEVEL_MAX, 0];
const GREEN_ONLY_LEVELS: [u8; 2] = [0, LEVEL_MAX];

pub fn run() -> Result<(), AppError> {
    let hw = AtomicMachineHardware::take()?;
    let storage = AppStorage::take()?;

    let mut consumption_per_sec = storage.load_consumption_per_sec()?;
    let mut pending_resume = storage.load_active_session()?;
    let mut runtime_fault_session: Option<ActiveSessionRecord> = None;
    let mut active_session: Option<ActiveBatterySession> = None;

    let mut switch_enabled = hw.switch.is_low();
    let mut last_seen_tag_uid_hex: Option<String> = None;
    let mut hot_plug_guard_armed = false;
    let mut display_state = DisplayState::Clear;
    let mut indicator_state: Option<IndicatorState> = None;
    let mut feedback: Option<ServiceFeedback> = None;

    loop {
        let now = Instant::now();
        let switch_enabled_local = hw.switch.is_low();
        let switch_turned_on = switch_enabled_local && !switch_enabled;
        let switch_turned_off = !switch_enabled_local && switch_enabled;
        switch_enabled = switch_enabled_local;

        if switch_turned_on {
            info!("Switch enabled");
        } else if switch_turned_off {
            info!("Switch disabled");
        }

        let snapshot = hw.nfc.snapshot()?;
        let current_tag_uid_hex = snapshot
            .tag
            .as_ref()
            .map(|tag| encode_uid_hex(&tag.info.uid));
        let tag_changed = current_tag_uid_hex != last_seen_tag_uid_hex;
        let mut detected = decode_detected_tag(snapshot.tag.as_ref());

        if tag_changed {
            log_tag_change(
                snapshot.tag.as_ref(),
                detected.as_ref(),
                last_seen_tag_uid_hex.is_some(),
            );
        }

        if let Some(session) = active_session.as_ref() {
            let active_battery_present = snapshot
                .tag
                .as_ref()
                .map(|tag| tag.info.uid == session.battery_uid)
                .unwrap_or(false);

            if !active_battery_present {
                warn!("Active battery disappeared before clean shutdown");
                runtime_fault_session = Some(ActiveSessionRecord {
                    battery_uid_hex: encode_uid_hex(&session.battery_uid),
                    session_id: session.battery.session_id,
                });
                active_session = None;
                pending_resume = storage.load_active_session()?;
            }
        }

        if let Some(session) = active_session.as_mut() {
            if let Some((info, battery)) = battery_view_from_detected(detected.as_ref()) {
                if info.uid == session.battery_uid {
                    update_active_session_charge(session, consumption_per_sec, now);

                    if now.duration_since(session.last_persist_at) >= CHARGE_PERSIST_INTERVAL {
                        persist_active_battery(&hw.nfc, session)?;
                    }

                    if session.battery.charge == 0 {
                        close_session_cleanly(
                            &storage,
                            &hw.nfc,
                            &session.battery_uid,
                            &mut session.battery,
                        )?;
                        active_session = None;
                        pending_resume = None;
                        runtime_fault_session = None;

                        if let Some(DetectedTag {
                            tag: AtomicTag::Battery(current_battery),
                            ..
                        }) = detected.as_mut()
                        {
                            current_battery.charge = 0;
                            current_battery.close_session();
                        }
                    } else if battery.dirty != session.battery.dirty
                        || battery.session_id != session.battery.session_id
                        || battery.charge != session.battery.charge
                    {
                        if let Some(DetectedTag {
                            tag: AtomicTag::Battery(current_battery),
                            ..
                        }) = detected.as_mut()
                        {
                            *current_battery = session.battery.clone();
                        }
                    }
                }
            }
        }

        if active_session.is_none() {
            if let Some(persisted) = pending_resume.as_ref() {
                if let Some(DetectedTag {
                    info,
                    tag: AtomicTag::Battery(battery),
                }) = detected.as_mut()
                {
                    let resume_candidate = runtime_fault_session.is_none()
                        && matches_active_session_record(info, battery, persisted)
                        && battery.healthy
                        && battery.dirty;

                    if resume_candidate {
                        if switch_enabled_local {
                            info!(
                                "Resuming active battery session after reboot uid={} session_id={}",
                                encode_uid_hex(&info.uid),
                                battery.session_id
                            );
                            active_session = Some(ActiveBatterySession::resume(
                                info.uid.clone(),
                                battery.clone(),
                                now,
                            ));
                            pending_resume = None;
                            runtime_fault_session = None;
                        }
                    } else if switch_enabled_local && tag_changed {
                        warn!(
                            "Pending session mismatch while switch is enabled, breaking detected battery uid={}",
                            encode_uid_hex(&info.uid)
                        );
                        break_detected_battery(&hw.nfc, battery, info)?;
                        storage.clear_active_session()?;
                        pending_resume = None;
                        runtime_fault_session = None;
                    }
                }
            }
        }

        if active_session.is_none() && pending_resume.is_none() {
            if let Some(DetectedTag {
                tag: AtomicTag::Service(service),
                ..
            }) = detected.as_ref()
            {
                if tag_changed {
                    apply_service_tag(
                        &storage,
                        &mut feedback,
                        &mut consumption_per_sec,
                        service,
                        now,
                    )?;
                }
            }
        }

        if let Some(DetectedTag {
            info,
            tag: AtomicTag::Battery(battery),
        }) = detected.as_mut()
        {
            if active_session.is_none() {
                let resume_candidate = pending_resume
                    .as_ref()
                    .map(|record| {
                        runtime_fault_session.is_none()
                            && matches_active_session_record(info, battery, record)
                            && battery.healthy
                            && battery.dirty
                    })
                    .unwrap_or(false);

                if battery.dirty && tag_changed && !resume_candidate {
                    warn!(
                        "Dirty battery inserted outside resumable session uid={}",
                        encode_uid_hex(&info.uid)
                    );
                    break_detected_battery(&hw.nfc, battery, info)?;
                    if pending_resume.is_some() || runtime_fault_session.is_some() {
                        storage.clear_active_session()?;
                        pending_resume = None;
                        runtime_fault_session = None;
                    }
                } else if hot_plug_guard_armed
                    && switch_enabled_local
                    && tag_changed
                    && !resume_candidate
                {
                    warn!(
                        "Battery inserted while switch is already enabled uid={}",
                        encode_uid_hex(&info.uid)
                    );
                    break_detected_battery(&hw.nfc, battery, info)?;
                    if pending_resume.is_some() || runtime_fault_session.is_some() {
                        storage.clear_active_session()?;
                        pending_resume = None;
                        runtime_fault_session = None;
                    }
                }
            }
        }

        if switch_turned_on && active_session.is_none() {
            if let Some(persisted) = pending_resume.as_ref() {
                if let Some(DetectedTag {
                    info,
                    tag: AtomicTag::Battery(battery),
                }) = detected.as_ref()
                {
                    if runtime_fault_session.is_none()
                        && matches_active_session_record(info, battery, persisted)
                        && battery.healthy
                        && battery.dirty
                    {
                        info!(
                            "Resuming active session on switch-on uid={} session_id={}",
                            encode_uid_hex(&info.uid),
                            battery.session_id
                        );
                        active_session = Some(ActiveBatterySession::resume(
                            info.uid.clone(),
                            battery.clone(),
                            now,
                        ));
                        pending_resume = None;
                        runtime_fault_session = None;
                    }
                }
            } else if let Some(DetectedTag {
                info,
                tag: AtomicTag::Battery(battery),
            }) = detected.as_mut()
            {
                if battery.is_usable() {
                    let session_id = storage.next_session_id()?;
                    battery.open_session(session_id);
                    write_battery(&hw.nfc, &info.uid, battery)?;

                    let battery_uid_hex = encode_uid_hex(&info.uid);
                    storage.save_active_session(&battery_uid_hex, session_id)?;
                    info!(
                        "Started new battery session uid={} session_id={} charge={}/{} consumption_per_sec={}",
                        battery_uid_hex,
                        session_id,
                        battery.charge,
                        battery.capacity,
                        consumption_per_sec
                    );
                    active_session = Some(ActiveBatterySession::start(
                        info.uid.clone(),
                        battery.clone(),
                        now,
                    ));
                }
            }
        }

        if switch_turned_off {
            if let Some(session) = active_session.take() {
                if let Some(DetectedTag {
                    info,
                    tag: AtomicTag::Battery(battery),
                }) = detected.as_mut()
                {
                    if info.uid == session.battery_uid {
                        let mut battery_to_close = session.battery.clone();
                        close_session_cleanly(&storage, &hw.nfc, &info.uid, &mut battery_to_close)?;
                        let remaining_charge = battery_to_close.charge;
                        let capacity = battery_to_close.capacity;
                        *battery = battery_to_close;
                        info!(
                            "Closed battery session cleanly uid={} remaining_charge={}/{}",
                            encode_uid_hex(&info.uid),
                            remaining_charge,
                            capacity
                        );
                        pending_resume = None;
                        runtime_fault_session = None;
                    } else {
                        warn!("Switch disabled, but active battery is no longer on reader");
                        pending_resume = storage.load_active_session()?;
                    }
                } else {
                    warn!("Switch disabled, but no battery is currently readable");
                    pending_resume = storage.load_active_session()?;
                }
            }
        }

        if let Some(active_feedback) = feedback.as_ref() {
            if now >= active_feedback.ends_at {
                feedback = None;
            }
        }

        let desired_indicator = resolve_indicator_state(
            feedback.as_ref(),
            active_session.as_ref(),
            detected.as_ref(),
            switch_enabled_local,
            pending_resume.as_ref(),
            runtime_fault_session.is_none(),
        );
        if indicator_state.as_ref() != Some(&desired_indicator) {
            apply_indicator_state(&hw, &desired_indicator)?;
            indicator_state = Some(desired_indicator);
        }

        let desired_display = if let Some(active_feedback) = feedback.as_ref() {
            DisplayState::ServiceMessage(active_feedback.message.clone())
        } else {
            resolve_display_state(
                active_session.as_ref(),
                detected.as_ref(),
                switch_enabled_local,
                consumption_per_sec,
                pending_resume.as_ref(),
                runtime_fault_session.is_none(),
            )
        };

        if desired_display != display_state {
            apply_display_state(&hw, &desired_display)?;
            display_state = desired_display;
        }

        last_seen_tag_uid_hex = current_tag_uid_hex;
        hot_plug_guard_armed = true;

        FreeRtos::delay_ms(LOOP_DELAY_MS);
    }
}

#[derive(Clone, Debug)]
struct DetectedTag {
    info: TagInfo,
    tag: AtomicTag,
}

#[derive(Clone, Debug)]
struct ActiveBatterySession {
    battery_uid: Vec<u8>,
    battery: BatteryTag,
    last_tick_at: Instant,
    last_persist_at: Instant,
    consumption_millis_accumulator: u128,
}

impl ActiveBatterySession {
    fn start(battery_uid: Vec<u8>, battery: BatteryTag, now: Instant) -> Self {
        Self {
            battery_uid,
            battery,
            last_tick_at: now,
            last_persist_at: now,
            consumption_millis_accumulator: 0,
        }
    }

    fn resume(battery_uid: Vec<u8>, battery: BatteryTag, now: Instant) -> Self {
        Self::start(battery_uid, battery, now)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DisplayState {
    Clear,
    StaticCounter { minutes: u8, seconds: u8 },
    ActiveCountdown { total_seconds: u32 },
    NoBattery,
    Error,
    ServiceMessage(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum IndicatorState {
    Red,
    Green,
    ServiceFeedback { interval: Duration, cycles: u32 },
}

#[derive(Clone, Debug)]
struct ServiceFeedback {
    message: String,
    ends_at: Instant,
    blink_interval: Duration,
}

fn decode_detected_tag(observed: Option<&AsyncObservedTag>) -> Option<DetectedTag> {
    let observed = observed?;
    let store = match &observed.payload {
        AsyncTagPayload::KvStore(store) => store,
        AsyncTagPayload::Empty | AsyncTagPayload::ReadError(_) => return None,
    };

    match AtomicTag::from_store(store) {
        Ok(tag) => Some(DetectedTag {
            info: observed.info.clone(),
            tag,
        }),
        Err(_) => None,
    }
}

fn log_tag_change(
    observed: Option<&AsyncObservedTag>,
    detected: Option<&DetectedTag>,
    had_previous_tag: bool,
) {
    match (observed, detected) {
        (
            Some(_),
            Some(DetectedTag {
                info,
                tag: AtomicTag::Battery(battery),
            }),
        ) => info!(
            "Detected battery tag uid={} healthy={} dirty={} charge={}/{} session_id={}",
            encode_uid_hex(&info.uid),
            battery.healthy,
            battery.dirty,
            battery.charge,
            battery.capacity,
            battery.session_id
        ),
        (
            Some(_),
            Some(DetectedTag {
                info,
                tag: AtomicTag::Service(service),
            }),
        ) => info!(
            "Detected service tag uid={} consumption_per_sec={}",
            encode_uid_hex(&info.uid),
            service.consumption_per_sec
        ),
        (Some(observed), None) => match &observed.payload {
            AsyncTagPayload::Empty => {
                info!(
                    "Detected NFC tag uid={} without NDEF payload",
                    encode_uid_hex(&observed.info.uid)
                );
            }
            AsyncTagPayload::ReadError(err) => {
                warn!(
                    "Detected NFC tag uid={} but failed to read payload: {}",
                    encode_uid_hex(&observed.info.uid),
                    err
                );
            }
            AsyncTagPayload::KvStore(_) => {
                warn!(
                    "Detected NFC tag uid={} with unsupported application payload",
                    encode_uid_hex(&observed.info.uid)
                );
            }
        },
        (None, _) if had_previous_tag => info!("Tag removed from reader"),
        (None, _) => {}
    }
}

fn apply_service_tag(
    storage: &AppStorage,
    feedback: &mut Option<ServiceFeedback>,
    consumption_per_sec: &mut u32,
    service: &ServiceTag,
    now: Instant,
) -> Result<(), AppError> {
    *consumption_per_sec = service.consumption_per_sec;
    storage.save_consumption_per_sec(service.consumption_per_sec)?;

    let message = service.consumption_per_sec.to_string();
    let duration = single_scroll_duration(&message, SERVICE_SCROLL_STEP);
    let blink_interval = feedback_blink_interval(duration);

    *feedback = Some(ServiceFeedback {
        message,
        ends_at: now + duration,
        blink_interval,
    });

    info!(
        "Updated consumption_per_sec to {}",
        service.consumption_per_sec
    );

    Ok(())
}

fn update_active_session_charge(
    session: &mut ActiveBatterySession,
    consumption_per_sec: u32,
    now: Instant,
) {
    let elapsed_ms = now.duration_since(session.last_tick_at).as_millis();
    session.last_tick_at = now;

    session.consumption_millis_accumulator = session
        .consumption_millis_accumulator
        .saturating_add(u128::from(consumption_per_sec) * elapsed_ms);

    let units_to_consume = (session.consumption_millis_accumulator / 1000) as u64;
    session.consumption_millis_accumulator %= 1000;

    if units_to_consume > 0 {
        session.battery.consume(units_to_consume);
    }
}

fn persist_active_battery(
    nfc: &nfc_tag::esp_idf::AsyncEspNfcTag<'_>,
    session: &mut ActiveBatterySession,
) -> Result<(), AppError> {
    write_battery(nfc, &session.battery_uid, &session.battery)?;
    session.last_persist_at = Instant::now();
    info!(
        "Persisted active battery charge uid={} charge={}/{} session_id={}",
        encode_uid_hex(&session.battery_uid),
        session.battery.charge,
        session.battery.capacity,
        session.battery.session_id
    );
    Ok(())
}

fn close_session_cleanly(
    storage: &AppStorage,
    nfc: &nfc_tag::esp_idf::AsyncEspNfcTag<'_>,
    battery_uid: &[u8],
    battery: &mut BatteryTag,
) -> Result<(), AppError> {
    battery.close_session();
    write_battery(nfc, battery_uid, battery)?;
    storage.clear_active_session()?;
    Ok(())
}

fn break_detected_battery(
    nfc: &nfc_tag::esp_idf::AsyncEspNfcTag<'_>,
    battery: &mut BatteryTag,
    info: &TagInfo,
) -> Result<(), AppError> {
    if battery.healthy {
        warn!("Breaking battery {:02X?}", info.uid);
        battery.mark_broken();
        write_battery(nfc, &info.uid, battery)?;
    }

    Ok(())
}

fn write_battery(
    nfc: &nfc_tag::esp_idf::AsyncEspNfcTag<'_>,
    battery_uid: &[u8],
    battery: &BatteryTag,
) -> Result<(), AppError> {
    let store = battery.to_store()?;
    nfc.write_kv_store_for_tag(battery_uid, &store)?;
    Ok(())
}

fn battery_view_from_detected(detected: Option<&DetectedTag>) -> Option<(&TagInfo, &BatteryTag)> {
    let detected = detected?;

    match &detected.tag {
        AtomicTag::Battery(battery) => Some((&detected.info, battery)),
        AtomicTag::Service(_) => None,
    }
}

fn resolve_display_state(
    active_session: Option<&ActiveBatterySession>,
    detected: Option<&DetectedTag>,
    switch_enabled: bool,
    consumption_per_sec: u32,
    pending_resume: Option<&ActiveSessionRecord>,
    can_resume_pending_session: bool,
) -> DisplayState {
    if let Some(session) = active_session {
        return DisplayState::ActiveCountdown {
            total_seconds: countdown_total_seconds(&session.battery, consumption_per_sec),
        };
    }

    if let Some(DetectedTag {
        info,
        tag: AtomicTag::Battery(battery),
    }) = detected
    {
        let resumed_battery = pending_resume
            .map(|record| {
                can_resume_pending_session && matches_active_session_record(info, battery, record)
            })
            .unwrap_or(false);

        if !battery.healthy || (battery.dirty && !resumed_battery) {
            return DisplayState::Error;
        }

        let (minutes, seconds) = remaining_time_mmss(battery, consumption_per_sec);
        return DisplayState::StaticCounter { minutes, seconds };
    }

    if switch_enabled {
        DisplayState::NoBattery
    } else {
        DisplayState::Clear
    }
}

fn apply_display_state(
    hw: &AtomicMachineHardware<'_>,
    display_state: &DisplayState,
) -> Result<(), AppError> {
    match display_state {
        DisplayState::Clear => {
            hw.display.stop_colon_blink(false)?;
            hw.display.clear()?;
        }
        DisplayState::StaticCounter { minutes, seconds } => {
            hw.display.show_int_pair(*minutes, *seconds)?;
            hw.display.stop_colon_blink(true)?;
        }
        DisplayState::ActiveCountdown { total_seconds } => {
            hw.display
                .start_countdown(*total_seconds, ACTIVE_COUNTDOWN_STEP_PERIOD)?;
            hw.display.start_colon_pulse(
                true,
                ACTIVE_COUNTDOWN_STEP_PERIOD,
                Duration::from_millis((ACTIVE_COUNTDOWN_STEP_PERIOD.as_millis() / 2).max(1) as u64),
            )?;
        }
        DisplayState::NoBattery => {
            hw.display.stop_colon_blink(false)?;
            hw.display.start_scroll_text("no bat", NO_BAT_SCROLL_STEP)?;
        }
        DisplayState::Error => {
            hw.display.stop_colon_blink(false)?;
            hw.display.start_scroll_error(ERROR_SCROLL_STEP)?;
        }
        DisplayState::ServiceMessage(message) => {
            hw.display.stop_colon_blink(false)?;
            hw.display
                .start_scroll_text_cycles(message, SERVICE_SCROLL_STEP, Some(1))?;
        }
    }

    Ok(())
}

fn resolve_indicator_state(
    feedback: Option<&ServiceFeedback>,
    active_session: Option<&ActiveBatterySession>,
    detected: Option<&DetectedTag>,
    switch_enabled: bool,
    pending_resume: Option<&ActiveSessionRecord>,
    can_resume_pending_session: bool,
) -> IndicatorState {
    if let Some(active_feedback) = feedback {
        return IndicatorState::ServiceFeedback {
            interval: active_feedback.blink_interval,
            cycles: SERVICE_FEEDBACK_BLINK_CYCLES,
        };
    }

    if let Some(session) = active_session {
        if switch_enabled && session.battery.healthy && session.battery.charge > 0 {
            return IndicatorState::Green;
        }
        return IndicatorState::Red;
    }

    if let Some(DetectedTag {
        info,
        tag: AtomicTag::Battery(battery),
    }) = detected
    {
        let resumed_battery = pending_resume
            .map(|record| {
                can_resume_pending_session && matches_active_session_record(info, battery, record)
            })
            .unwrap_or(false);

        if !battery.healthy || (battery.dirty && !resumed_battery) {
            return IndicatorState::Red;
        }

        if switch_enabled && battery.charge > 0 {
            IndicatorState::Green
        } else {
            IndicatorState::Red
        }
    } else {
        IndicatorState::Red
    }
}

fn apply_indicator_state(
    hw: &AtomicMachineHardware<'_>,
    indicator_state: &IndicatorState,
) -> Result<(), AppError> {
    match indicator_state {
        IndicatorState::Red => hw.indicator.set_levels(RED_ONLY_LEVELS)?,
        IndicatorState::Green => hw.indicator.set_levels(GREEN_ONLY_LEVELS)?,
        IndicatorState::ServiceFeedback { interval, cycles } => {
            let pattern =
                LedPattern::alternate(RED_ONLY_LEVELS, GREEN_ONLY_LEVELS, *interval, *cycles)
                    .final_levels(RED_ONLY_LEVELS);
            hw.indicator.play_pattern(pattern)?;
        }
    }

    Ok(())
}

fn remaining_time_mmss(battery: &BatteryTag, consumption_per_sec: u32) -> (u8, u8) {
    let remaining_seconds = battery.remaining_seconds(consumption_per_sec);
    let capped_seconds = remaining_seconds.min((99 * 60 + 59) as u64);
    let minutes = (capped_seconds / 60) as u8;
    let seconds = (capped_seconds % 60) as u8;
    (minutes, seconds)
}

fn encode_uid_hex(uid: &[u8]) -> String {
    let mut value = String::with_capacity(uid.len() * 2);
    for byte in uid {
        use core::fmt::Write as _;
        let _ = write!(&mut value, "{byte:02X}");
    }
    value
}

fn single_scroll_duration(message: &str, step_delay: Duration) -> Duration {
    step_delay
        .checked_mul(message.len().saturating_add(5) as u32)
        .unwrap_or(Duration::from_secs(2))
}

fn feedback_blink_interval(duration: Duration) -> Duration {
    let phase_count = u128::from(SERVICE_FEEDBACK_BLINK_CYCLES.saturating_mul(2));
    let interval_ms = (duration.as_millis() / phase_count).max(1);
    Duration::from_millis(interval_ms as u64)
}

fn countdown_total_seconds(battery: &BatteryTag, consumption_per_sec: u32) -> u32 {
    battery
        .remaining_seconds(consumption_per_sec)
        .min((99 * 60 + 59) as u64) as u32
}

fn matches_active_session_record(
    info: &TagInfo,
    battery: &BatteryTag,
    record: &ActiveSessionRecord,
) -> bool {
    encode_uid_hex(&info.uid) == record.battery_uid_hex && battery.session_id == record.session_id
}
