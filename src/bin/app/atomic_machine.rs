use crate::errors::AppError;
use crate::hardware::AtomicMachineHardware;
use crate::storage::{ActiveSessionRecord, AppStorage};
use common::drivers::nfc_tag::{self, NfcError, TagInfo};
use common::utils::atomic_tags::{AtomicTag, BatteryTag, ServiceTag};
use esp_idf_svc::hal::delay::FreeRtos;
use log::{info, warn};
use std::time::{Duration, Instant};

const LOOP_DELAY_MS: u32 = 50;
const READ_TAG_TIMEOUT: Duration = Duration::from_millis(100);
const CHARGE_PERSIST_INTERVAL: Duration = Duration::from_secs(60);
const NO_BAT_SCROLL_STEP: Duration = Duration::from_millis(250);
const ERROR_SCROLL_STEP: Duration = Duration::from_millis(250);
const SERVICE_SCROLL_STEP: Duration = Duration::from_millis(150);
const SERVICE_BLINK_STEP: Duration = Duration::from_millis(120);

pub fn run() -> Result<(), AppError> {
    let mut hw = AtomicMachineHardware::take()?;
    let storage = AppStorage::take()?;

    let mut consumption_per_sec = storage.load_consumption_per_sec()?;
    let mut pending_resume = storage.load_active_session()?;
    let mut runtime_fault_session: Option<ActiveSessionRecord> = None;
    let mut active_session: Option<ActiveBatterySession> = None;

    let mut switch_enabled = hw.switch.is_low();
    let mut last_seen_tag_uid_hex: Option<String> = None;
    let mut hot_plug_guard_armed = false;
    let mut display_state = DisplayState::Clear;
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

        let mut detected = read_atomic_tag(&mut hw.nfc);
        let current_tag_uid_hex = detected.as_ref().map(|tag| encode_uid_hex(&tag.info.uid));
        let tag_changed = current_tag_uid_hex != last_seen_tag_uid_hex;

        if tag_changed {
            match detected.as_ref() {
                Some(DetectedTag {
                    info,
                    tag: AtomicTag::Battery(battery),
                }) => info!(
                    "Detected battery tag uid={} healthy={} dirty={} charge={}/{} session_id={}",
                    encode_uid_hex(&info.uid),
                    battery.healthy,
                    battery.dirty,
                    battery.charge,
                    battery.capacity,
                    battery.session_id
                ),
                Some(DetectedTag {
                    info,
                    tag: AtomicTag::Service(service),
                }) => info!(
                    "Detected service tag uid={} consumption_per_sec={}",
                    encode_uid_hex(&info.uid),
                    service.consumption_per_sec
                ),
                None => {
                    if last_seen_tag_uid_hex.is_some() {
                        info!("Tag removed from reader");
                    }
                }
            }
        }

        if let Some(session) = active_session.as_ref() {
            let active_battery_present = battery_view_from_detected(detected.as_ref())
                .map(|(info, _)| info.uid == session.battery_uid)
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
                        persist_active_battery(&mut hw.nfc, session)?;
                    }

                    if session.battery.charge == 0 {
                        close_session_cleanly(&storage, &mut hw.nfc, &mut session.battery)?;
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
                        // Keep the in-memory state authoritative while session is active.
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
                        break_detected_battery(&mut hw.nfc, battery, info)?;
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
                        &mut hw,
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
                    break_detected_battery(&mut hw.nfc, battery, info)?;
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
                    break_detected_battery(&mut hw.nfc, battery, info)?;
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
                    write_battery(&mut hw.nfc, battery)?;

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
                        close_session_cleanly(&storage, &mut hw.nfc, &mut battery_to_close)?;
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

        apply_led_state(
            &mut hw,
            now,
            feedback.as_ref(),
            active_session.as_ref(),
            detected.as_ref(),
            switch_enabled_local,
            pending_resume.as_ref(),
            runtime_fault_session.is_none(),
        )?;

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
    Counter { minutes: u8, seconds: u8 },
    NoBattery,
    Error,
    ServiceMessage(String),
}

#[derive(Clone, Debug)]
struct ServiceFeedback {
    message: String,
    started_at: Instant,
    ends_at: Instant,
}

fn read_atomic_tag(nfc: &mut nfc_tag::esp_idf::EspNfcTag<'_>) -> Option<DetectedTag> {
    match nfc.poll_tag(READ_TAG_TIMEOUT) {
        Ok(Some(info)) => match nfc.read_kv_store() {
            Ok(store) => match AtomicTag::from_store(&store) {
                Ok(tag) => Some(DetectedTag { info, tag }),
                Err(err) => {
                    warn!("Ignoring unsupported/invalid NFC payload: {err}");
                    None
                }
            },
            Err(NfcError::NoNdefMessage) => None,
            Err(err) => {
                warn!("Failed to read NFC store: {err}");
                None
            }
        },
        Ok(None) => None,
        Err(err) => {
            warn!("Failed to poll NFC tag: {err}");
            None
        }
    }
}

fn apply_service_tag(
    storage: &AppStorage,
    hw: &mut AtomicMachineHardware<'_>,
    feedback: &mut Option<ServiceFeedback>,
    consumption_per_sec: &mut u32,
    service: &ServiceTag,
    now: Instant,
) -> Result<(), AppError> {
    *consumption_per_sec = service.consumption_per_sec;
    storage.save_consumption_per_sec(service.consumption_per_sec)?;

    let message = format!("rate {}", service.consumption_per_sec);
    hw.display
        .start_scroll_text(&message, SERVICE_SCROLL_STEP)?;

    let windows = message.len().saturating_add(5);
    let duration = SERVICE_SCROLL_STEP
        .checked_mul(windows as u32)
        .unwrap_or(Duration::from_secs(3))
        + Duration::from_millis(200);

    *feedback = Some(ServiceFeedback {
        message,
        started_at: now,
        ends_at: now + duration,
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
    nfc: &mut nfc_tag::esp_idf::EspNfcTag<'_>,
    session: &mut ActiveBatterySession,
) -> Result<(), AppError> {
    write_battery(nfc, &session.battery)?;
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
    nfc: &mut nfc_tag::esp_idf::EspNfcTag<'_>,
    battery: &mut BatteryTag,
) -> Result<(), AppError> {
    battery.close_session();
    write_battery(nfc, battery)?;
    storage.clear_active_session()?;
    Ok(())
}

fn break_detected_battery(
    nfc: &mut nfc_tag::esp_idf::EspNfcTag<'_>,
    battery: &mut BatteryTag,
    info: &TagInfo,
) -> Result<(), AppError> {
    if battery.healthy {
        warn!("Breaking battery {:02X?}", info.uid);
        battery.mark_broken();
        write_battery(nfc, battery)?;
    }

    Ok(())
}

fn write_battery(
    nfc: &mut nfc_tag::esp_idf::EspNfcTag<'_>,
    battery: &BatteryTag,
) -> Result<(), AppError> {
    let store = battery.to_store()?;
    nfc.write_kv_store(&store)?;
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
        let (minutes, seconds) = remaining_time_mmss(&session.battery, consumption_per_sec);
        if switch_enabled && session.battery.healthy && session.battery.charge > 0 {
            return DisplayState::Counter { minutes, seconds };
        }

        return DisplayState::Counter { minutes, seconds };
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
        return DisplayState::Counter { minutes, seconds };
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
        DisplayState::Clear => hw.display.clear()?,
        DisplayState::Counter { minutes, seconds } => {
            hw.display.show_int_pair(*minutes, *seconds)?;
        }
        DisplayState::NoBattery => {
            hw.display.start_scroll_text("no bat", NO_BAT_SCROLL_STEP)?;
        }
        DisplayState::Error => {
            hw.display.start_scroll_error(ERROR_SCROLL_STEP)?;
        }
        DisplayState::ServiceMessage(message) => {
            hw.display.start_scroll_text(message, SERVICE_SCROLL_STEP)?;
        }
    }

    Ok(())
}

fn apply_led_state(
    hw: &mut AtomicMachineHardware<'_>,
    now: Instant,
    feedback: Option<&ServiceFeedback>,
    active_session: Option<&ActiveBatterySession>,
    detected: Option<&DetectedTag>,
    switch_enabled: bool,
    pending_resume: Option<&ActiveSessionRecord>,
    can_resume_pending_session: bool,
) -> Result<(), AppError> {
    if let Some(active_feedback) = feedback {
        let elapsed = now.duration_since(active_feedback.started_at).as_millis();
        let phase = (elapsed / SERVICE_BLINK_STEP.as_millis()) % 2 == 0;

        if phase {
            hw.red_led.set_high()?;
            hw.green_led.set_low()?;
        } else {
            hw.red_led.set_low()?;
            hw.green_led.set_high()?;
        }

        return Ok(());
    }

    if let Some(session) = active_session {
        if switch_enabled && session.battery.healthy && session.battery.charge > 0 {
            hw.red_led.set_low()?;
            hw.green_led.set_high()?;
        } else {
            hw.red_led.set_high()?;
            hw.green_led.set_low()?;
        }

        return Ok(());
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
            hw.red_led.set_high()?;
            hw.green_led.set_low()?;
            return Ok(());
        }

        if switch_enabled && battery.charge > 0 {
            hw.red_led.set_low()?;
            hw.green_led.set_high()?;
        } else {
            hw.red_led.set_high()?;
            hw.green_led.set_low()?;
        }

        return Ok(());
    }

    hw.red_led.set_high()?;
    hw.green_led.set_low()?;
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

fn matches_active_session_record(
    info: &TagInfo,
    battery: &BatteryTag,
    record: &ActiveSessionRecord,
) -> bool {
    encode_uid_hex(&info.uid) == record.battery_uid_hex && battery.session_id == record.session_id
}
