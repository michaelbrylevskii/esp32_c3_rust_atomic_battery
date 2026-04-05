use crate::errors::AppError;
use common::drivers::led_indicator::async_controller::{AsyncLedConfig, AsyncLedController};
use common::drivers::led_indicator::backend::LedPolarity;
use common::drivers::led_indicator::digital_backend::DigitalLedGroup;
use common::drivers::nfc_tag;
use common::drivers::nfc_tag::async_nfc::{AsyncNfcConfig, AsyncNfcTag};
use common::drivers::segment_display::AsyncSegmentDisplay4;
use esp_idf_svc::hal::gpio::{Input, Output};
use esp_idf_svc::hal::{
    gpio::{Level, PinDriver, Pull},
    peripherals::Peripherals,
};

pub struct AtomicMachineHardware<'d> {
    _board_led: PinDriver<'d, Output>,
    pub switch: PinDriver<'d, Input>,
    pub nfc: nfc_tag::esp_idf::AsyncEspNfcTag<'static>,
    pub display: AsyncSegmentDisplay4,
    pub indicator: AsyncLedController<2>,
}

impl AtomicMachineHardware<'static> {
    pub fn take() -> Result<Self, AppError> {
        let p = Peripherals::take()?;

        let board_led_pin = p.pins.gpio8.degrade_output();
        let red_led_pin = p.pins.gpio0.degrade_output();
        let green_led_pin = p.pins.gpio1.degrade_output();
        let switch_pin = p.pins.gpio10.degrade_input();

        let nfc_i2c = p.i2c0;
        let nfc_i2c_sda_pin = p.pins.gpio3;
        let nfc_i2c_scl_pin = p.pins.gpio4;

        let display_clk_pin = p.pins.gpio5;
        let display_dio_pin = p.pins.gpio6;

        let mut board_led = PinDriver::output(board_led_pin)?;
        board_led.set_level(Level::Low)?;

        let red_led = PinDriver::output(red_led_pin)?;
        let green_led = PinDriver::output(green_led_pin)?;

        let switch = PinDriver::input(switch_pin, Pull::Up)?;

        let mut nfc = nfc_tag::esp_idf::new_default(nfc_i2c, nfc_i2c_sda_pin, nfc_i2c_scl_pin)?;
        nfc.init_default()?;
        let nfc = AsyncNfcTag::new(nfc, AsyncNfcConfig::default())?;

        let display = AsyncSegmentDisplay4::new(display_clk_pin, display_dio_pin)?;
        display.clear()?;

        let led_group = DigitalLedGroup::new(
            [red_led, green_led],
            [LedPolarity::ActiveHigh, LedPolarity::ActiveHigh],
        )?;
        let indicator = AsyncLedController::new(led_group, AsyncLedConfig::default())?;

        Ok(Self {
            _board_led: board_led,
            switch,
            nfc,
            display,
            indicator,
        })
    }
}
