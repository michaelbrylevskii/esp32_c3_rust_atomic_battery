use common::drivers::led_indicator::async_controller::{
    AsyncLedConfig, AsyncLedController, AsyncLedError,
};
use common::drivers::led_indicator::backend::LedPolarity;
use common::drivers::led_indicator::constants::LEVEL_MAX;
use common::drivers::led_indicator::digital_backend::DigitalLedGroup;
use common::drivers::led_indicator::easing::Easing;
use common::drivers::led_indicator::pattern::{LedPattern, RepeatMode};
use common::drivers::led_indicator::pwm_backend::{PwmLedError, PwmLedGroup};
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::PinDriver;
use esp_idf_svc::hal::ledc::config::{Resolution, TimerConfig};
use esp_idf_svc::hal::ledc::{LedcDriver, LedcTimerDriver};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::units::Hertz;
use esp_idf_svc::sys::EspError;
use log::error;
use std::fmt;
use std::time::Duration;

const PWM_FREQUENCY: Hertz = Hertz(5_000);
const STAGE_GAP_MS: u32 = 500;
const PWM_STEADY_HOLD_MS: u32 = 900;
const PWM_HALF_BRIGHT_HOLD_MS: u32 = 1200;
const PWM_BLINK_STEP_MS: u32 = 180;
const PWM_PULSE_HALF_MS: u32 = 400;
const PWM_CROSSFADE_MS: u32 = 1200;
const PWM_WAVE_STEP_MS: u32 = 420;
const BOARD_STEP_MS: u32 = 120;
const BOARD_HEARTBEAT_LONG_OFF_MS: u32 = 760;
const BOARD_BREATH_MS: u32 = 1600;

const RED_PIN_LEVELS: [u8; 2] = [LEVEL_MAX, 0];
const GREEN_PIN_LEVELS: [u8; 2] = [0, LEVEL_MAX];
const BOTH_ON_LEVELS: [u8; 2] = [LEVEL_MAX, LEVEL_MAX];
const HALF_BRIGHT_LEVELS: [u8; 2] = [LEVEL_MAX / 2, LEVEL_MAX / 2];
const OFF_LEVELS: [u8; 2] = [0, 0];
const BOARD_ON: [u8; 1] = [LEVEL_MAX];
const BOARD_OFF: [u8; 1] = [0];

#[derive(Debug)]
enum DemoError {
    Esp(EspError),
    Pwm(PwmLedError),
    AsyncLed(AsyncLedError),
}

impl From<EspError> for DemoError {
    fn from(value: EspError) -> Self {
        Self::Esp(value)
    }
}

impl From<PwmLedError> for DemoError {
    fn from(value: PwmLedError) -> Self {
        Self::Pwm(value)
    }
}

impl From<AsyncLedError> for DemoError {
    fn from(value: AsyncLedError) -> Self {
        Self::AsyncLed(value)
    }
}

impl fmt::Display for DemoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DemoError::Esp(err) => write!(f, "esp error: {err}"),
            DemoError::Pwm(err) => write!(f, "pwm error: {err}"),
            DemoError::AsyncLed(err) => write!(f, "async led error: {err}"),
        }
    }
}

impl std::error::Error for DemoError {}

fn custom_smoother_step(progress: f32) -> f32 {
    let clamped = progress.clamp(0.0, 1.0);
    clamped * clamped * clamped * (clamped * (clamped * 6.0 - 15.0) + 10.0)
}

fn board_heartbeat_pattern() -> LedPattern<1> {
    LedPattern::<1>::new()
        .hold(BOARD_ON, Duration::from_millis(u64::from(BOARD_STEP_MS)))
        .hold(
            BOARD_OFF,
            Duration::from_millis(u64::from(BOARD_STEP_MS * 2)),
        )
        .hold(BOARD_ON, Duration::from_millis(u64::from(BOARD_STEP_MS)))
        .hold(
            BOARD_OFF,
            Duration::from_millis(u64::from(BOARD_HEARTBEAT_LONG_OFF_MS)),
        )
        .repeat(RepeatMode::Forever)
}

fn board_attention_pattern() -> LedPattern<1> {
    LedPattern::alternate(
        BOARD_ON,
        BOARD_OFF,
        Duration::from_millis(u64::from(BOARD_STEP_MS)),
        4,
    )
}

fn board_breathe_pattern() -> LedPattern<1> {
    LedPattern::<1>::new()
        .transition(
            BOARD_OFF,
            BOARD_ON,
            Duration::from_millis(u64::from(BOARD_BREATH_MS)),
            Easing::Custom(custom_smoother_step),
        )
        .transition(
            BOARD_ON,
            BOARD_OFF,
            Duration::from_millis(u64::from(BOARD_BREATH_MS)),
            Easing::Custom(custom_smoother_step),
        )
        .repeat(RepeatMode::Forever)
}

fn sleep_stage(delay_ms: u32) {
    FreeRtos::delay_ms(delay_ms);
}

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Starting led_indicator_demo binary");

    if let Err(err) = run() {
        error!("Fatal error: {err:?}");
        loop {
            FreeRtos::delay_ms(1000);
        }
    }
}

fn run() -> Result<(), DemoError> {
    let peripherals = Peripherals::take()?;

    let board_led = PinDriver::output(peripherals.pins.gpio8.degrade_output())?;
    let board_group = DigitalLedGroup::new([board_led], [LedPolarity::ActiveLow])?;
    let board_indicator = AsyncLedController::<1>::new(board_group, AsyncLedConfig::default())?;

    let timer = LedcTimerDriver::new(
        peripherals.ledc.timer0,
        &TimerConfig::default()
            .frequency(PWM_FREQUENCY)
            .resolution(Resolution::Bits8),
    )?;

    let red = LedcDriver::new(peripherals.ledc.channel0, &timer, peripherals.pins.gpio0)?;
    let green = LedcDriver::new(peripherals.ledc.channel1, &timer, peripherals.pins.gpio1)?;

    let group = PwmLedGroup::new(
        timer,
        [red, green],
        [LedPolarity::ActiveHigh, LedPolarity::ActiveHigh],
    )?;

    let indicator = AsyncLedController::<2>::new(group, AsyncLedConfig::default())?;

    loop {
        log::info!("Stage: heartbeat + static levels");
        board_indicator.play_pattern(board_heartbeat_pattern())?;

        indicator.set_levels(RED_PIN_LEVELS)?;
        sleep_stage(PWM_STEADY_HOLD_MS);

        indicator.set_levels(GREEN_PIN_LEVELS)?;
        sleep_stage(PWM_STEADY_HOLD_MS);

        indicator.set_levels(BOTH_ON_LEVELS)?;
        sleep_stage(PWM_STEADY_HOLD_MS);

        indicator.set_levels(HALF_BRIGHT_LEVELS)?;
        sleep_stage(PWM_HALF_BRIGHT_HOLD_MS);
        sleep_stage(STAGE_GAP_MS);

        log::info!("Stage: attention blink + alternating colors");
        board_indicator.play_pattern(board_attention_pattern())?;
        indicator.play_pattern(LedPattern::blink(
            RED_PIN_LEVELS,
            OFF_LEVELS,
            Duration::from_millis(u64::from(PWM_BLINK_STEP_MS)),
            Duration::from_millis(u64::from(PWM_BLINK_STEP_MS)),
            3,
        ))?;
        sleep_stage(1400);

        indicator.play_pattern(LedPattern::alternate(
            RED_PIN_LEVELS,
            GREEN_PIN_LEVELS,
            Duration::from_millis(u64::from(PWM_BLINK_STEP_MS)),
            3,
        ))?;
        sleep_stage(1400);
        sleep_stage(STAGE_GAP_MS);

        log::info!("Stage: board breathe + pwm pulse");
        board_indicator.play_pattern(board_breathe_pattern())?;
        indicator.play_pattern(LedPattern::pulse(
            BOTH_ON_LEVELS,
            Duration::from_millis(u64::from(PWM_PULSE_HALF_MS)),
            Duration::from_millis(u64::from(PWM_PULSE_HALF_MS)),
            2,
        ))?;
        sleep_stage(1800);
        sleep_stage(STAGE_GAP_MS);

        log::info!("Stage: eased crossfade");
        indicator.play_pattern(
            LedPattern::<2>::new()
                .transition(
                    RED_PIN_LEVELS,
                    GREEN_PIN_LEVELS,
                    Duration::from_millis(u64::from(PWM_CROSSFADE_MS)),
                    Easing::EaseInOutSine,
                )
                .transition(
                    GREEN_PIN_LEVELS,
                    RED_PIN_LEVELS,
                    Duration::from_millis(u64::from(PWM_CROSSFADE_MS)),
                    Easing::EaseInOutSine,
                )
                .repeat(RepeatMode::Times(2))
                .final_levels(OFF_LEVELS),
        )?;
        sleep_stage(5200);
        sleep_stage(STAGE_GAP_MS);

        log::info!("Stage: linear and eased wave sequence");
        board_indicator.play_pattern(
            LedPattern::<1>::new()
                .transition(
                    BOARD_OFF,
                    BOARD_ON,
                    Duration::from_millis(700),
                    Easing::EaseOutCubic,
                )
                .transition(
                    BOARD_ON,
                    BOARD_OFF,
                    Duration::from_millis(700),
                    Easing::EaseInCubic,
                )
                .repeat(RepeatMode::Times(2))
                .final_levels(BOARD_OFF),
        )?;
        indicator.play_pattern(
            LedPattern::<2>::new()
                .transition_linear(
                    OFF_LEVELS,
                    RED_PIN_LEVELS,
                    Duration::from_millis(u64::from(PWM_WAVE_STEP_MS)),
                )
                .transition(
                    RED_PIN_LEVELS,
                    BOTH_ON_LEVELS,
                    Duration::from_millis(u64::from(PWM_WAVE_STEP_MS)),
                    Easing::EaseOutQuad,
                )
                .transition(
                    BOTH_ON_LEVELS,
                    GREEN_PIN_LEVELS,
                    Duration::from_millis(u64::from(PWM_WAVE_STEP_MS)),
                    Easing::EaseInOutQuad,
                )
                .transition(
                    GREEN_PIN_LEVELS,
                    OFF_LEVELS,
                    Duration::from_millis(u64::from(PWM_WAVE_STEP_MS)),
                    Easing::EaseInQuad,
                )
                .repeat(RepeatMode::Times(2))
                .final_levels(OFF_LEVELS),
        )?;
        sleep_stage(4200);
        sleep_stage(STAGE_GAP_MS);

        log::info!("Stage: custom easing finale");
        board_indicator.play_pattern(board_breathe_pattern())?;
        indicator.play_pattern(
            LedPattern::<2>::new()
                .transition(
                    OFF_LEVELS,
                    BOTH_ON_LEVELS,
                    Duration::from_millis(900),
                    Easing::Custom(custom_smoother_step),
                )
                .hold(BOTH_ON_LEVELS, Duration::from_millis(260))
                .transition(
                    BOTH_ON_LEVELS,
                    OFF_LEVELS,
                    Duration::from_millis(900),
                    Easing::Custom(custom_smoother_step),
                )
                .repeat(RepeatMode::Times(2))
                .final_levels(OFF_LEVELS),
        )?;
        sleep_stage(2700);

        indicator.turn_off()?;
        board_indicator.turn_off()?;
        sleep_stage(600);
    }
}
