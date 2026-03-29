use crate::errors::AppError;
use common::drivers::nfc_tag;
use common::drivers::segment_display::AsyncSegmentDisplay4;
use esp_idf_svc::hal::gpio::{Input, Output};
use esp_idf_svc::hal::{
    gpio::{Level, PinDriver, Pull},
    peripherals::Peripherals,
};

pub struct AtomicMachineHardware<'d> {
    pub board_led: PinDriver<'d, Output>,
    pub red_led: PinDriver<'d, Output>,
    pub green_led: PinDriver<'d, Output>,
    pub switch: PinDriver<'d, Input>,
    pub nfc: nfc_tag::esp_idf::EspNfcTag<'d>,
    pub display: AsyncSegmentDisplay4,
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

        let mut red_led = PinDriver::output(red_led_pin)?;
        red_led.set_level(Level::High)?;

        let mut green_led = PinDriver::output(green_led_pin)?;
        green_led.set_level(Level::Low)?;

        let switch = PinDriver::input(switch_pin, Pull::Up)?;

        let mut nfc = nfc_tag::esp_idf::new_default(nfc_i2c, nfc_i2c_sda_pin, nfc_i2c_scl_pin)?;
        nfc.init_default()?;

        let display = AsyncSegmentDisplay4::new(display_clk_pin, display_dio_pin)?;
        display.clear()?;

        Ok(Self {
            board_led,
            red_led,
            green_led,
            switch,
            nfc,
            display,
        })
    }
}
