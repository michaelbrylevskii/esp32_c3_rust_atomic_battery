use crate::app::AppError;
use crate::drivers::nfc_tag::{self, KvStore, NfcError};
use crate::drivers::segment_display::{Align, AsyncSegmentDisplay4, IntFormat};
use esp_idf_svc::hal::{delay::FreeRtos, peripherals::Peripherals};
use log::{error, info, warn};
use std::time::Duration;

const WRITE_DEMO: bool = false;

pub fn display_demo() -> Result<(), AppError> {
    let peripherals = Peripherals::take()?;

    let display = AsyncSegmentDisplay4::new(
        peripherals.pins.gpio5, // CLK
        peripherals.pins.gpio6, // DIO
    )?;

    loop {
        display.show_int(42, IntFormat::new().right())?;
        FreeRtos::delay_ms(1000);

        display.show_int(42, IntFormat::new().right().leading_zeros(true))?;
        FreeRtos::delay_ms(1000);

        display.show_int(42, IntFormat::new().left())?;
        FreeRtos::delay_ms(1000);

        display.set_colon(true)?;
        display.show_int_pair(12, 34)?;
        display.start_colon_blink(true, Duration::from_millis(500))?;
        FreeRtos::delay_ms(3000);
        display.stop_colon_blink(false)?;
        display.show_error()?;
        FreeRtos::delay_ms(1200);

        display.show_text("AbCd", Align::Left)?;
        FreeRtos::delay_ms(1200);

        display.start_scroll_error(Duration::from_millis(250))?;
        FreeRtos::delay_ms(3000);
        display.clear()?;
        FreeRtos::delay_ms(500);
    }
}

pub fn nfc_demo() -> Result<(), AppError> {
    let p = Peripherals::take()?;

    let mut nfc = nfc_tag::esp_idf::new_default(
        p.i2c0,
        p.pins.gpio3, // SDA
        p.pins.gpio4, // SCL
    )?;

    nfc.init_default()?;

    let fw = nfc.firmware_version()?;
    info!("PN532 firmware raw: {:02X?}", fw);

    let mut demo_store = KvStore::new();
    demo_store.insert_string("name", "Привет,\nESP32-C3")?;
    demo_store.insert_u8("counter", 42)?;
    demo_store.insert_u16("limit", 1024)?;
    demo_store.insert_u32("serial", 123_456)?;
    demo_store.insert_u64("energy", 9_876_543_210)?;
    demo_store.insert_i8("temperature_offset", -5)?;
    demo_store.insert_i16("bias", -32_000)?;
    demo_store.insert_i32("temp_raw", -123_456)?;
    demo_store.insert_i64("distance", -9_876_543_210)?;
    demo_store.insert_f32("soc", 98.5)?;
    demo_store.insert_f64("voltage", 12.625)?;
    demo_store.insert_bool("enabled", true)?;

    let mut wrote_once = false;

    loop {
        match nfc.poll_tag(Duration::from_millis(1000)) {
            Ok(Some(tag)) => {
                info!(
                    "Tag UID: {:02X?}, ATQA={:02X?}, SAK=0x{:02X}",
                    tag.uid, tag.atqa, tag.sak
                );

                match nfc.read_kv_store() {
                    Ok(store) => info!("Tag key-value data: {:?}", store.entries()),
                    Err(NfcError::NoNdefMessage) => {
                        info!("Tag is empty or does not contain NDEF yet")
                    }
                    Err(err) => warn!("read_kv_store error: {err}"),
                }

                if WRITE_DEMO && !wrote_once {
                    match nfc.write_kv_store(&demo_store) {
                        Ok(()) => {
                            info!("write_kv_store OK: {:?}", demo_store.entries());
                            match nfc.read_kv_store() {
                                Ok(read_back) => {
                                    info!("read-back after write: {:?}", read_back.entries());
                                    if read_back == demo_store {
                                        info!("read-back matches written data");
                                        wrote_once = true;
                                    } else {
                                        warn!("read-back differs from written data");
                                    }
                                }
                                Err(err) => warn!("read-back error after write: {err}"),
                            }
                        }
                        Err(err) => warn!("write_kv_store error: {err}"),
                    }
                }
                FreeRtos::delay_ms(300);
            }
            Ok(None) => {}
            Err(err) => {
                error!("PN532 error: {err}");
                FreeRtos::delay_ms(200);
            }
        }
    }
}
