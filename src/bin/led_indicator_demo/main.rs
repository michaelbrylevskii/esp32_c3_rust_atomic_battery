use common::drivers::led_indicator::async_controller::{
    AsyncLedConfig, AsyncLedController, AsyncLedError,
};
use common::drivers::led_indicator::backend::LedPolarity;
use common::drivers::led_indicator::constants::LEVEL_MAX;
use common::drivers::led_indicator::easing::Easing;
use common::drivers::led_indicator::pattern::{LedPattern, RepeatMode};
use common::drivers::led_indicator::pwm_backend::{PwmLedError, PwmLedGroup};
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::ledc::config::{Resolution, TimerConfig};
use esp_idf_svc::hal::ledc::{LedcDriver, LedcTimerDriver};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::units::Hertz;
use esp_idf_svc::sys::EspError;
use log::error;
use std::fmt;
use std::time::Duration;

const PWM_FREQUENCY: Hertz = Hertz(5_000);
const RED_PIN_LEVELS: [u8; 2] = [LEVEL_MAX, 0];
const GREEN_PIN_LEVELS: [u8; 2] = [0, LEVEL_MAX];
const HALF_BRIGHT_LEVELS: [u8; 2] = [LEVEL_MAX / 2, LEVEL_MAX / 2];

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
        indicator.set_levels(RED_PIN_LEVELS)?;
        FreeRtos::delay_ms(900);

        indicator.set_levels(GREEN_PIN_LEVELS)?;
        FreeRtos::delay_ms(900);

        indicator.set_levels(HALF_BRIGHT_LEVELS)?;
        FreeRtos::delay_ms(1200);

        indicator.play_pattern(LedPattern::blink(
            RED_PIN_LEVELS,
            [0, 0],
            Duration::from_millis(180),
            Duration::from_millis(180),
            3,
        ))?;
        FreeRtos::delay_ms(1400);

        indicator.play_pattern(LedPattern::alternate(
            RED_PIN_LEVELS,
            GREEN_PIN_LEVELS,
            Duration::from_millis(180),
            3,
        ))?;
        FreeRtos::delay_ms(1400);

        indicator.play_pattern(LedPattern::pulse(
            RED_PIN_LEVELS,
            Duration::from_millis(400),
            Duration::from_millis(400),
            2,
        ))?;
        FreeRtos::delay_ms(1800);

        indicator.play_pattern(
            LedPattern::<2>::new()
                .transition_with_easing(
                    RED_PIN_LEVELS,
                    GREEN_PIN_LEVELS,
                    Duration::from_millis(1200),
                    Easing::EaseInOutSine,
                )
                .transition_with_easing(
                    GREEN_PIN_LEVELS,
                    RED_PIN_LEVELS,
                    Duration::from_millis(1200),
                    Easing::EaseInOutSine,
                )
                .repeat(RepeatMode::Times(2))
                .final_levels([0, 0]),
        )?;
        FreeRtos::delay_ms(5200);

        indicator.turn_off()?;
        FreeRtos::delay_ms(600);
    }
}
