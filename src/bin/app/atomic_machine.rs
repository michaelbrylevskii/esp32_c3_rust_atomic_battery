use crate::errors::AppError;
use crate::hardware::AtomicMachineHardware;
use common::drivers::nfc_tag::{self, KvStore};
use esp_idf_svc::hal::delay::FreeRtos;
use log::info;
use std::time::Duration;

const BAT_SERVICE_KEY: &str = "service";
const BAT_HEALTH_KEY: &str = "health";
const BAT_CHARGE_KEY: &str = "charge";
const BAT_CONSUMPTION_KEY: &str = "consumption";

pub fn run() -> Result<(), AppError> {
    let mut hw = AtomicMachineHardware::take()?;

    let mut switch_enabled = false;
    let mut battery_healthy = false;
    let mut battery_has_charge = false;

    loop {
        info!("Loop begin");

        let battery_plugged = match read_nfc(&mut hw.nfc) {
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

        let switch_enabled_local = hw.switch.is_low();
        let switch_changed = switch_enabled_local != switch_enabled;
        if switch_changed {
            switch_enabled = switch_enabled_local;
        }

        if !battery_plugged {
            hw.red_led.set_high()?;
            if switch_changed && switch_enabled {
                hw.display
                    .start_scroll_text("no bat", Duration::from_millis(250))?;
            }
            if switch_changed && !switch_enabled {
                hw.display.clear()?;
            }
        } else {
            hw.red_led.set_low()?;
            hw.display.show_int_pair(15, 47)?;
        }

        FreeRtos::delay_ms(10);

        let _ = (&mut hw.board_led, &mut hw.green_led);
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
