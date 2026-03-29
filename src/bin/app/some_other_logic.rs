use crate::errors::AppError;
use common::drivers::nfc_tag::{self, KvStore};
use common::drivers::segment_display::AsyncSegmentDisplay4;
use esp_idf_svc::hal::gpio::Pull;
use esp_idf_svc::hal::{
    delay::FreeRtos,
    gpio::{Level, PinDriver},
    peripherals::Peripherals,
};
use log::info;
use std::time::Duration;

const BAT_SERVICE_KEY: &str = "service";
const BAT_HEALTH_KEY: &str = "health";
const BAT_CHARGE_KEY: &str = "charge";
const BAT_CONSUMPTION_KEY: &str = "consumption";

pub fn run() -> Result<(), AppError> {
    let p = Peripherals::take()?;

    let mut board_led_pin = PinDriver::output(p.pins.gpio8)?;
    board_led_pin.set_level(Level::Low)?;

    let mut red_led_pin = PinDriver::output(p.pins.gpio0)?;
    red_led_pin.set_level(Level::High)?;

    let mut green_led_pin = PinDriver::output(p.pins.gpio1)?;
    green_led_pin.set_level(Level::Low)?;

    let switch_pin = PinDriver::input(p.pins.gpio10, Pull::Up)?;

    let mut nfc = nfc_tag::esp_idf::new_default(
        p.i2c0,
        p.pins.gpio3, // SDA
        p.pins.gpio4, // SCL
    )?;
    nfc.init_default()?;

    let display = AsyncSegmentDisplay4::new(
        p.pins.gpio5, // CLK
        p.pins.gpio6, // DIO
    )?;

    display.clear()?;

    let mut switch_enabled = false;
    let mut battery_healthy = false;
    let mut battery_has_charge = false;

    loop {
        info!("Loop begin");

        let battery_plugged = match read_nfc(&mut nfc) {
            Some(_battery_data) => {
                let _ = (
                    BAT_SERVICE_KEY,
                    BAT_HEALTH_KEY,
                    BAT_CHARGE_KEY,
                    BAT_CONSUMPTION_KEY,
                );
                let _ = (&mut battery_healthy, &mut battery_has_charge);
                true
            }
            None => false,
        };

        let switch_enabled_local = switch_pin.is_low();
        let switch_changed = switch_enabled_local != switch_enabled;
        if switch_changed {
            switch_enabled = switch_enabled_local;
        }

        if !battery_plugged {
            red_led_pin.set_high()?;
            if switch_changed && switch_enabled {
                display.start_scroll_text("no bat", Duration::from_millis(250))?;
            }
            if switch_changed && !switch_enabled {
                display.clear()?;
            }
        } else {
            red_led_pin.set_low()?;
            display.show_int_pair(15, 47)?;
        }

        FreeRtos::delay_ms(10);

        let _ = (&mut board_led_pin, &mut green_led_pin);
    }
}

fn read_nfc(nfc: &mut nfc_tag::esp_idf::EspNfcTag<'_>) -> Option<KvStore> {
    match nfc.poll_tag(Duration::from_millis(1000)) {
        Ok(Some(tag)) => {
            info!(
                "Tag UID: {:02X?}, ATQA={:02X?}, SAK=0x{:02X}",
                tag.uid, tag.atqa, tag.sak
            );

            match nfc.read_kv_store() {
                Ok(store) => {
                    info!("Tag key-value data: {:?}", store.entries());
                    Some(store)
                }
                Err(_) => None,
            }
        }
        Ok(None) => None,
        Err(_) => None,
    }
}
